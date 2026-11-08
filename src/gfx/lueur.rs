//! **LES LUEURS** — DEMANDE EXPLICITE (2026-10-14) : « superposer un léger effet de
//! dégradé de blanc vers noir partant d'un cercle blanc à la couleur des barres,
//! et pouvoir placer son centre où je le souhaite sur l'interface de l'app, pour le
//! mode visible dans les captures ». Puis, le même jour : « pouvoir éditer l'effet,
//! ajouter des éléments, régler le flou, dans un dash » (`win32::cef_lueurs`), et
//! « animer les lueurs avec des chemins programmés, des allers et retours et des
//! rotations du fond », et « en faire apparaître et disparaître avec un effet de
//! fondu » (voir `Element::apparition`). Puis : qu'une lueur « apparaisse en fondu
//! quand on l'ajoute et disparaisse en fondu » quand on la retire (voir `ajouter`,
//! `retirer`, `FONDU_AJOUT`), et « des cycles de couleur » (`Element::etapes`).
//! Enfin : « des segments, des droites, des rectangles et des cercles » avec leur
//! « inclinaison », et « des effets de rotation sur les éléments » (`Figure`,
//! `Element::inclinaison`, `Element::rotation_propre`).
//!
//! **UNE FIGURE N'EST QU'UNE AUTRE DISTANCE** : le shader mesure, dans le repère
//! tourné de l'inclinaison, une distance NORMALISÉE (0 au cœur, 1 au bord) — au
//! centre pour un cercle, au rectangle, au segment (une gélule) ou à la droite ;
//! le fondu (`flou`) et l'intensité s'appliquent ensuite à l'identique.
//!
//! Des dégradés RADIAUX posés sur tous les fonds (barres, coins, panneaux) : chaque
//! élément est un disque de sa couleur (blanc par défaut) qui se fond dans la couleur
//! des barres à son `rayon` ; le `flou` règle la largeur du fondu (0 : un disque net
//! au bord, 1 : un fondu depuis le centre). Au-delà de tous les disques, un
//! `assombrissement` léger vers le noir. Évalués au pixel ÉCRAN, comme le dégradé
//! animé (`gfx::degrade`) : une seule scène pour toute l'interface.
//!
//! **L'ANIMATION EST CALCULÉE ICI, À CHAQUE FRAME** (`constantes`), et le shader ne
//! reçoit que des positions : chaque élément suit son `Mouvement` (aller-retour ou
//! boucle sur son chemin, orbite autour de son centre), puis toute la scène tourne
//! autour du `centre` de rotation. Un disque étant rond, tourner les centres suffit
//! à faire tourner le fond.
//!
//! ⚠️ **MODE CAPTURABLE SEULEMENT** (`capture_visibility`) : en mode transparent, les
//! fonds sont le bureau flouté, et une tache blanche dessus n'aurait pas de sens.
//!
//! Positions et rayons en FRACTIONS de l'écran PRINCIPAL (origine `(0, 0)` en
//! coordonnées écran) : le réglage survit à un changement de résolution, et un
//! centre hors de `[0, 1]` reste possible pour atteindre un écran secondaire.
//!
//! **PERSISTANCE** : `lueurs.txt`, une ligne d'en-tête
//! `active,assombrissement,rotation,centre x,centre y[,opacité]`, puis une ligne par élément
//! `x,y,rayon,intensite,flou,r,g,b,mouvement,periode,orbite[,px,py]*`, suivie
//! éventuellement de `;apparition,cycle,presence,fondu,decalage`, puis de
//! `;cycle de couleur,duree[,r,g,b]*`, puis de
//! `;figure,inclinaison,longueur,rotation propre`, puis de
//! `;attente,disparition,absence` (voir `Pause`), puis de
//! `;dégradé[,r,g,b]*` (voir `Degrade`), puis de `;image` (l'empreinte, voir
//! `gfx::lueur_images`), puis de `;défilement,teinte` (le dégradé ANIMÉ). ⚠️ **APRÈS UN
//! `;`** : le chemin est de longueur variable, et rien ne pourrait se ranger après
//! lui sans séparateur. Les champs d'animation sont FACULTATIFS : un fichier
//! d'avant l'animation se relit tel quel.

use std::cell::{Cell, RefCell};
use std::f32::consts::TAU;
use std::sync::OnceLock;
use std::time::Instant;

use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

/// Le nombre d'éléments que le shader sait dessiner (taille des tableaux du cbuffer).
/// ⭐ 8 → 20 : DEMANDE EXPLICITE (2026-10-14) : « pouvoir utiliser 20 lueurs à la
/// fois ». ⚠️ Les tableaux du `cbuffer` (`quad_renderer`) portent ce nombre EN DUR.
pub const MAX_ELEMENTS: usize = 20;
/// Le nombre de couleurs d'un dégradé, EN PLUS de la couleur principale.
pub const MAX_ARRETS: usize = 4;
/// Le nombre de points d'un chemin, EN PLUS du centre de l'élément.
pub const MAX_POINTS: usize = 6;
/// Le nombre de couleurs d'un cycle, EN PLUS de la couleur principale.
pub const MAX_ETAPES: usize = 7;
/// La durée, en secondes, du fondu d'une lueur ajoutée ou retirée — et de la case
/// « Lueurs dessinées » qu'on coche ou décoche.
pub const FONDU_AJOUT: f64 = 0.8;

/// Comment un élément se déplace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mouvement {
    Fixe,
    /// Du centre au dernier point du chemin, puis retour — adouci aux deux bouts.
    AllerRetour,
    /// Le chemin fermé, parcouru à vitesse constante.
    Boucle,
    /// Un cercle de rayon `orbite` autour du centre.
    Orbite,
    /// **L'ALLER SIMPLE** (DEMANDE EXPLICITE, 2026-10-14 : « déplacement puis
    /// disparition puis réapparition au point de départ, minuterie avant le
    /// mouvement ») : la lueur apparaît au départ en fondu, y attend `attente`,
    /// parcourt le chemin en `periode` (adouci aux deux bouts), s'éteint au bout en
    /// `disparition`, reste éteinte `absence`, puis recommence.
    Trajet,
}

impl Mouvement {
    /// ⛔ Le CODE est le rang : on n'ajoute qu'À LA FIN, sinon les fichiers
    /// enregistrés changeraient de mouvement.
    pub const TOUS: [(Mouvement, &'static str); 5] = [
        (Mouvement::Fixe, "fixe"),
        (Mouvement::AllerRetour, "aller-retour"),
        (Mouvement::Boucle, "boucle"),
        (Mouvement::Orbite, "orbite"),
        (Mouvement::Trajet, "trajet"),
    ];

    pub fn nom(self) -> &'static str {
        Self::TOUS.iter().find(|(m, _)| *m == self).map_or("fixe", |(_, n)| n)
    }

    pub fn depuis_nom(nom: &str) -> Option<Mouvement> {
        Self::TOUS.iter().find(|(_, n)| *n == nom).map(|(m, _)| *m)
    }

    fn code(self) -> u8 {
        Self::TOUS.iter().position(|(m, _)| *m == self).unwrap_or(0) as u8
    }

    fn depuis_code(code: f32) -> Mouvement {
        Self::TOUS.get(code as usize).filter(|_| code >= 0.0).map_or(Mouvement::Fixe, |(m, _)| *m)
    }
}

/// La forme d'une lueur. Le CODE (rang dans `TOUTES`) est celui que lit le shader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Figure {
    Cercle,
    /// Demi-largeur `longueur`, demi-hauteur `rayon`.
    Rectangle,
    /// Une gélule : demi-longueur `longueur`, demi-épaisseur `rayon`.
    Segment,
    /// Une droite infinie, de demi-épaisseur `rayon`.
    Droite,
    /// ⭐ Une IMAGE importée (DEMANDE EXPLICITE, 2026-10-14), dans le rectangle de
    /// demi-largeur `longueur` et de demi-hauteur `rayon` — voir `gfx::lueur_images`.
    Image,
}

impl Figure {
    pub const TOUTES: [(Figure, &'static str); 5] = [
        (Figure::Cercle, "cercle"),
        (Figure::Rectangle, "rectangle"),
        (Figure::Segment, "segment"),
        (Figure::Droite, "droite"),
        (Figure::Image, "image"),
    ];

    pub fn nom(self) -> &'static str {
        Self::TOUTES.iter().find(|(f, _)| *f == self).map_or("cercle", |(_, n)| n)
    }

    pub fn depuis_nom(nom: &str) -> Option<Figure> {
        Self::TOUTES.iter().find(|(_, n)| *n == nom).map(|(f, _)| *f)
    }

    fn code(self) -> u8 {
        Self::TOUTES.iter().position(|(f, _)| *f == self).unwrap_or(0) as u8
    }

    fn depuis_code(code: f32) -> Figure {
        Self::TOUTES.get(code as usize).filter(|_| code >= 0.0).map_or(Figure::Cercle, |(f, _)| *f)
    }
}

/// ⭐ **LE DÉGRADÉ DANS LA FORME** — DEMANDE EXPLICITE (2026-10-14) : « faire des
/// formes qui soient composées de dégradés ». La couleur de la lueur va de sa
/// couleur principale à chacune de ses `arrets`, le long :
/// - de son grand axe (`Lineaire`), tourné avec la figure — au travers d'une
///   droite, qui n'a pas de bout ;
/// - du centre vers le bord (`Radial`) ;
/// - d'un tour autour du centre (`Angulaire`), qui revient à la première couleur.
///
/// Le fondu du bord (`flou`) et l'intensité s'appliquent ENSUITE, comme pour une
/// lueur d'une seule couleur. Le CODE est le rang : on n'ajoute qu'à la fin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Degrade {
    Aucun,
    Lineaire,
    Radial,
    Angulaire,
}

impl Degrade {
    pub const TOUS: [(Degrade, &'static str); 4] =
        [(Degrade::Aucun, "aucun"), (Degrade::Lineaire, "lineaire"), (Degrade::Radial, "radial"), (Degrade::Angulaire, "angulaire")];

    pub fn nom(self) -> &'static str {
        Self::TOUS.iter().find(|(d, _)| *d == self).map_or("aucun", |(_, n)| n)
    }

    pub fn depuis_nom(nom: &str) -> Option<Degrade> {
        Self::TOUS.iter().find(|(_, n)| *n == nom).map(|(d, _)| *d)
    }

    fn code(self) -> u8 {
        Self::TOUS.iter().position(|(d, _)| *d == self).unwrap_or(0) as u8
    }

    fn depuis_code(code: f32) -> Degrade {
        Self::TOUS.get(code as usize).filter(|_| code >= 0.0).map_or(Degrade::Aucun, |(d, _)| *d)
    }
}

/// Un disque de lumière — ou une autre `Figure`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Element {
    /// Centre, en fractions de l'écran principal — le DÉPART du chemin.
    pub x: f32,
    pub y: f32,
    /// Rayon, en fraction de la LARGEUR de l'écran principal.
    pub rayon: f32,
    /// 0..1 : force du mélange vers `couleur` au centre.
    pub intensite: f32,
    /// 0..1 : largeur du fondu, du bord (0) au centre (1).
    pub flou: f32,
    /// 0..1 par canal.
    pub couleur: [f32; 3],
    pub mouvement: Mouvement,
    /// Secondes pour un tour (aller ET retour, boucle, orbite).
    pub periode: f32,
    /// Rayon de l'orbite, en fraction de la LARGEUR de l'écran principal.
    pub orbite: f32,
    /// Les points du chemin après le centre — seuls les `nb_points` premiers comptent.
    pub points: [[f32; 2]; MAX_POINTS],
    pub nb_points: usize,
    /// **APPARAÎT ET DISPARAÎT EN FONDU** — un cycle de `cycle` secondes : visible
    /// pendant la fraction `presence`, dont la fraction `fondu` est passée à
    /// s'allumer puis à s'éteindre (moitié chacun), éteinte le reste du temps.
    /// `decalage` (0..1) décale le cycle : plusieurs lueurs peuvent se relayer.
    pub apparition: bool,
    pub cycle: f32,
    pub presence: f32,
    pub fondu: f32,
    pub decalage: f32,
    /// **LE CYCLE DE COULEURS** : la lueur passe de `couleur` à chacune des
    /// `nb_etapes` premières `etapes`, puis revient à `couleur` — un tour en
    /// `duree_couleur` secondes, fondu d'une couleur à la suivante.
    pub cycle_couleur: bool,
    pub duree_couleur: f32,
    pub etapes: [[f32; 3]; MAX_ETAPES],
    pub nb_etapes: usize,
    /// L'instant (horloge de `maintenant`) de son AJOUT — le fondu d'entrée. Ni
    /// enregistré ni relu : une lueur chargée au démarrage est déjà là.
    pub nee_a: f64,
    /// Les ANCRES des trois horloges (mouvement, apparition, couleurs), dans
    /// l'ordre de `Duree` — voir `phase`. Ni enregistrées ni relues.
    pub ancres: [Ancre; 3],
    pub figure: Figure,
    /// En DEGRÉS, sens des aiguilles d'une montre (l'axe y de l'écran descend).
    pub inclinaison: f32,
    /// Demi-largeur d'un rectangle, demi-longueur d'un segment — en fraction de la
    /// LARGEUR de l'écran principal.
    pub longueur: f32,
    /// Tours par minute de la figure sur elle-même (négatif : sens inverse).
    pub rotation_propre: f32,
    /// L'ancre de cette rotation (voir `Ancre`) — phase en TOURS.
    pub ancre_rotation_propre: Ancre,
    /// Secondes d'arrêt au départ avant chaque mouvement (aller-retour, boucle,
    /// aller simple) — la MINUTERIE.
    pub attente: f32,
    /// Aller simple : secondes du fondu de disparition au bout, et de réapparition
    /// au départ (0 : d'un coup).
    pub disparition: f32,
    /// Aller simple : secondes passées éteinte entre la disparition et la
    /// réapparition.
    pub absence: f32,
    /// Le dégradé de la forme (voir `Degrade`) et ses couleurs après la principale
    /// — seules les `nb_arrets` premières comptent.
    pub degrade: Degrade,
    pub arrets: [[f32; 3]; MAX_ARRETS],
    pub nb_arrets: usize,
    /// L'image d'une `Figure::Image` — son empreinte dans la bibliothèque (voir
    /// `gfx::lueur_images`) ; 0 : aucune.
    pub image: u32,
    /// ⭐ **LE DÉGRADÉ ANIMÉ** — DEMANDE EXPLICITE (2026-10-14) : « animer le
    /// dégradé dans le temps, faire évoluer son hue dans le temps ».
    /// `defilement` : tours par minute du dégradé qui DÉFILE dans la forme (il
    /// boucle alors, revenant à sa première couleur) ; `teinte` : tours par minute
    /// de la roue des teintes, appliqués à TOUTES ses couleurs. 0 : immobile.
    pub defilement: f32,
    pub teinte: f32,
    /// Leurs ancres (voir `Ancre`) — phase en TOURS. Ni enregistrées ni relues.
    pub ancre_defilement: Ancre,
    pub ancre_teinte: Ancre,
}

/// ⛔ **UNE PHASE NE SE CALCULE PAS `t / durée`** — défaut SIGNALÉ (2026-10-14) :
/// « un bug sur la durée du cycle des boucles ». Avec `t` compté depuis le
/// lancement, changer la durée change `t / durée` de CENTAINES de tours après une
/// heure : la lueur sautait ailleurs sur son chemin, et à chaque cran du curseur.
/// La phase repart donc de l'endroit où elle était (`phase`), à l'instant du
/// changement (`instant`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ancre {
    pub instant: f64,
    pub phase: f64,
}

impl Ancre {
    /// La phase (en tours) à l'instant `t`, pour une durée de tour `duree`.
    fn phase(self, t: f64, duree: f32) -> f64 {
        self.phase + (t - self.instant) / f64::from(duree.max(PERIODE.0))
    }

    /// L'ancre qui CONTINUE la phase actuelle quand la durée passe à une autre.
    fn reancree(self, t: f64, ancienne_duree: f32) -> Ancre {
        Ancre { instant: t, phase: self.phase(t, ancienne_duree) }
    }
}

/// Les temps d'arrêt d'un mouvement (voir `Element::attente`), réglables sans
/// saut (`changer_pause`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pause {
    Attente,
    Disparition,
    Absence,
}

impl Pause {
    pub fn depuis_nom(nom: &str) -> Option<Pause> {
        match nom {
            "attente" => Some(Pause::Attente),
            "disparition" => Some(Pause::Disparition),
            "absence" => Some(Pause::Absence),
            _ => None,
        }
    }

    /// Les bornes, en secondes.
    pub fn bornes(self) -> (f32, f32) {
        match self {
            Pause::Attente | Pause::Absence => (0.0, PERIODE.1),
            Pause::Disparition => (0.0, 30.0),
        }
    }
}

/// Les trois durées d'une lueur, réglables sans saut (`changer_duree`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duree {
    Mouvement,
    Apparition,
    Couleur,
}

impl Element {
    pub const PAR_DEFAUT: Element = Element {
        x: 0.5,
        y: 0.0,
        rayon: 0.6,
        intensite: 0.25,
        flou: 1.0,
        couleur: [1.0, 1.0, 1.0],
        mouvement: Mouvement::Fixe,
        periode: 10.0,
        orbite: 0.1,
        points: [[0.0; 2]; MAX_POINTS],
        nb_points: 0,
        apparition: false,
        cycle: 8.0,
        presence: 0.6,
        fondu: 0.5,
        decalage: 0.0,
        cycle_couleur: false,
        duree_couleur: 12.0,
        etapes: [[1.0; 3]; MAX_ETAPES],
        nb_etapes: 0,
        nee_a: f64::NEG_INFINITY,
        ancres: [Ancre { instant: 0.0, phase: 0.0 }; 3],
        figure: Figure::Cercle,
        inclinaison: 0.0,
        longueur: 0.2,
        rotation_propre: 0.0,
        ancre_rotation_propre: Ancre { instant: 0.0, phase: 0.0 },
        attente: 0.0,
        disparition: 1.0,
        absence: 0.0,
        degrade: Degrade::Aucun,
        arrets: [[0.0; 3]; MAX_ARRETS],
        nb_arrets: 0,
        image: 0,
        defilement: 0.0,
        teinte: 0.0,
        ancre_defilement: Ancre { instant: 0.0, phase: 0.0 },
        ancre_teinte: Ancre { instant: 0.0, phase: 0.0 },
    };

    pub fn chemin(&self) -> &[[f32; 2]] {
        &self.points[..self.nb_points.min(MAX_POINTS)]
    }

    /// Ajoute un point après le dernier, décalé pour qu'on le voie apparaître.
    pub fn ajouter_point(&mut self) {
        if self.nb_points >= MAX_POINTS {
            return;
        }
        let [x, y] = self.chemin().last().copied().unwrap_or([self.x, self.y]);
        self.points[self.nb_points] = [(x + 0.2).min(1.0), y + if x + 0.2 > 1.0 { 0.2 } else { 0.0 }];
        self.nb_points += 1;
    }

    pub fn retirer_point(&mut self) {
        self.nb_points = self.nb_points.saturating_sub(1);
    }

    fn duree_mut(&mut self, quelle: Duree) -> &mut f32 {
        match quelle {
            Duree::Mouvement => &mut self.periode,
            Duree::Apparition => &mut self.cycle,
            Duree::Couleur => &mut self.duree_couleur,
        }
    }

    /// La durée d'un tour COMPLET du mouvement, temps d'arrêt compris — celle de
    /// l'horloge `Duree::Mouvement`.
    pub fn duree_du_tour(&self) -> f32 {
        match self.mouvement {
            Mouvement::Trajet => self.periode + self.attente + 2.0 * self.disparition + self.absence,
            Mouvement::AllerRetour | Mouvement::Boucle => self.periode + self.attente,
            Mouvement::Fixe | Mouvement::Orbite => self.periode,
        }
    }

    /// Les secondes écoulées dans le tour en cours, à l'instant `t`.
    fn seconde_du_tour(&self, t: f64) -> f32 {
        let tour = self.duree_du_tour().max(PERIODE.0);
        self.ancres[Duree::Mouvement as usize].phase(t, tour).rem_euclid(1.0) as f32 * tour
    }

    /// Change un temps d'arrêt À L'INSTANT `t` sans faire sauter la phase.
    pub fn changer_pause(&mut self, quelle: Pause, secondes: f32, t: f64) {
        if !secondes.is_finite() {
            return;
        }
        let k = Duree::Mouvement as usize;
        self.ancres[k] = self.ancres[k].reancree(t, self.duree_du_tour());
        let (min, max) = quelle.bornes();
        let v = secondes.clamp(min, max);
        match quelle {
            Pause::Attente => self.attente = v,
            Pause::Disparition => self.disparition = v,
            Pause::Absence => self.absence = v,
        }
    }

    /// Change une durée À L'INSTANT `t` sans faire sauter la phase (voir `Ancre`).
    pub fn changer_duree(&mut self, quelle: Duree, secondes: f32, t: f64) {
        if !secondes.is_finite() {
            return;
        }
        // L'horloge du mouvement compte le tour ENTIER, temps d'arrêt compris.
        let ancienne = if quelle == Duree::Mouvement { self.duree_du_tour() } else { *self.duree_mut(quelle) };
        let k = quelle as usize;
        self.ancres[k] = self.ancres[k].reancree(t, ancienne);
        *self.duree_mut(quelle) = secondes.clamp(PERIODE.0, PERIODE.1);
    }

    /// Change la rotation propre À L'INSTANT `t` sans faire sauter l'angle.
    pub fn changer_rotation_propre(&mut self, tours_par_minute: f32, t: f64) {
        if !tours_par_minute.is_finite() {
            return;
        }
        self.ancre_rotation_propre = Ancre { instant: t, phase: self.tours_propres(t) };
        self.rotation_propre = tours_par_minute.clamp(ROTATION.0, ROTATION.1);
    }

    /// **RÉINITIALISE LA ROTATION** (DEMANDE EXPLICITE, 2026-10-14) : la figure
    /// s'arrête ET revient à son inclinaison — les tours déjà faits sont oubliés.
    /// Mettre la vitesse à 0 ne suffisait pas : la figure restait figée de travers.
    pub fn reinitialiser_rotation(&mut self, t: f64) {
        self.rotation_propre = 0.0;
        self.ancre_rotation_propre = Ancre { instant: t, phase: 0.0 };
    }

    /// La rotation propre à l'instant `t`, en tours.
    fn tours_propres(&self, t: f64) -> f64 {
        let a = self.ancre_rotation_propre;
        a.phase + (t - a.instant) * f64::from(self.rotation_propre) / 60.0
    }

    /// Change de figure — en ramenant une épaisseur de DISQUE (60 % de l'écran par
    /// défaut) à celle d'un trait, sans quoi un segment tout neuf couvrirait tout.
    pub fn changer_figure(&mut self, figure: Figure) {
        let plafond = match figure {
            Figure::Cercle => f32::INFINITY,
            Figure::Rectangle | Figure::Image => 0.15,
            Figure::Segment | Figure::Droite => 0.05,
        };
        if self.figure != figure {
            self.rayon = self.rayon.min(plafond);
        }
        self.figure = figure;
    }

    pub fn couleurs_du_degrade(&self) -> &[[f32; 3]] {
        &self.arrets[..self.nb_arrets.min(MAX_ARRETS)]
    }

    /// Change de dégradé — un dégradé d'UNE couleur ne changerait rien : on lui en
    /// donne une seconde, la teinte opposée de la principale.
    pub fn changer_degrade(&mut self, degrade: Degrade) {
        self.degrade = degrade;
        if degrade != Degrade::Aucun && self.nb_arrets == 0 {
            let [r, g, b] = self.couleur;
            self.arrets[0] = if (r - g).abs() + (g - b).abs() < 0.05 { [0.2, 0.4, 1.0] } else { [1.0 - r, 1.0 - g, 1.0 - b] };
            self.nb_arrets = 1;
        }
    }

    /// Change la vitesse du défilement (`teinte: false`) ou de la teinte
    /// (`teinte: true`) À L'INSTANT `t`, sans saut (voir `Ancre`).
    pub fn changer_animation_du_degrade(&mut self, teinte: bool, tours_par_minute: f32, t: f64) {
        if !tours_par_minute.is_finite() {
            return;
        }
        let v = tours_par_minute.clamp(ROTATION.0, ROTATION.1);
        let (vitesse, ancre) = if teinte { (&mut self.teinte, &mut self.ancre_teinte) } else { (&mut self.defilement, &mut self.ancre_defilement) };
        *ancre = Ancre { instant: t, phase: phase_en_tours(*ancre, *vitesse, t) };
        *vitesse = v;
    }

    pub fn ajouter_arret(&mut self) {
        if self.nb_arrets >= MAX_ARRETS {
            return;
        }
        self.arrets[self.nb_arrets] = self.couleurs_du_degrade().last().copied().unwrap_or(self.couleur);
        self.nb_arrets += 1;
    }

    /// Jamais sous UNE couleur de plus que la principale.
    pub fn retirer_arret(&mut self) {
        self.nb_arrets = self.nb_arrets.saturating_sub(1).max(1);
    }

    pub fn couleurs_du_cycle(&self) -> &[[f32; 3]] {
        &self.etapes[..self.nb_etapes.min(MAX_ETAPES)]
    }

    /// Ajoute une couleur au cycle — la dernière, pour qu'on parte d'elle.
    pub fn ajouter_etape(&mut self) {
        if self.nb_etapes >= MAX_ETAPES {
            return;
        }
        self.etapes[self.nb_etapes] = self.couleurs_du_cycle().last().copied().unwrap_or(self.couleur);
        self.nb_etapes += 1;
    }

    pub fn retirer_etape(&mut self) {
        self.nb_etapes = self.nb_etapes.saturating_sub(1);
    }

    /// Le cycle devient un ARC-EN-CIEL : six teintes vives après la principale.
    pub fn arc_en_ciel(&mut self) {
        const TEINTES: [[f32; 3]; 6] =
            [[1.0, 0.25, 0.25], [1.0, 0.75, 0.2], [0.55, 1.0, 0.3], [0.25, 0.95, 0.95], [0.3, 0.45, 1.0], [0.85, 0.35, 1.0]];
        self.couleur = TEINTES[0];
        self.etapes[..5].copy_from_slice(&TEINTES[1..]);
        self.nb_etapes = 5;
        self.cycle_couleur = true;
    }
}

/// Les champs réglables d'un élément, par NOM (verbes de la page) : bornes et accès.
pub const CHAMPS: [(&str, f32, f32); 10] = [
    // ⭐ De −2 à 3 : le ZOOM de l'aperçu permet de placer hors écran (DEMANDE
    // EXPLICITE, 2026-10-14), jusqu'à deux écrans de chaque côté.
    ("x", -2.0, 3.0),
    ("y", -2.0, 3.0),
    // ⭐ Jusqu'à 0,1 % (DEMANDE EXPLICITE, 2026-10-14) : un trait d'un pixel ou
    // presque. Le curseur de la page est LOGARITHMIQUE, sans quoi ce bas de plage
    // tiendrait dans le premier pixel de sa course.
    ("rayon", 0.001, 2.0),
    ("intensite", 0.0, 1.0),
    ("flou", 0.0, 1.0),
    ("orbite", 0.0, 1.0),
    ("presence", 0.05, 1.0),
    ("fondu", 0.0, 1.0),
    ("decalage", 0.0, 1.0),
    ("longueur", 0.005, 3.0),
];
/// Les bornes de l'inclinaison, en degrés.
pub const INCLINAISON: (f32, f32) = (-180.0, 180.0);
/// Les bornes de la période ET du cycle d'apparition, en secondes.
pub const PERIODE: (f32, f32) = (0.5, 600.0);
/// Les bornes de la rotation du fond, en tours par minute (négatif : sens inverse).
pub const ROTATION: (f32, f32) = (-60.0, 60.0);

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub active: bool,
    /// 0..1 : assombrissement au-delà de tous les disques.
    pub assombrissement: f32,
    /// Tours par minute de toute la scène autour de `centre` (0 : immobile).
    pub rotation: f32,
    /// Centre de la rotation, en fractions de l'écran principal.
    pub centre: [f32; 2],
    pub elements: Vec<Element>,
    /// L'ancre de la rotation (voir `Ancre`) : la phase est en TOURS, et la
    /// « durée » d'un tour vaut `60 / rotation` secondes.
    pub ancre_rotation: Ancre,
    /// ⭐ **L'OPACITÉ GLOBALE** (DEMANDE EXPLICITE, 2026-10-14 : « un réglage
    /// d'opacité globale pour les lueurs dessinées ») : 0..1, multiplie l'intensité
    /// de TOUTES les lueurs. L'assombrissement a son propre curseur : il n'en
    /// dépend pas. Absente d'un fichier plus ancien : 1.
    pub opacite: f32,
}

impl Default for Scene {
    fn default() -> Self {
        Scene {
            active: false,
            assombrissement: 0.25,
            rotation: 0.0,
            centre: [0.5, 0.5],
            elements: vec![Element::PAR_DEFAUT],
            ancre_rotation: Ancre::default(),
            opacite: 1.0,
        }
    }
}

thread_local! {
    static SCENE: RefCell<Scene> = RefCell::new(Scene::default());
    /// Les lueurs RETIRÉES qui s'éteignent encore, avec l'instant du retrait.
    static SORTANTES: RefCell<Vec<(Element, f64)>> = const { RefCell::new(Vec::new()) };
    /// L'instant où « Lueurs dessinées » a changé — le fondu de toute la scène.
    /// `-∞` : aucun changement, la scène est d'emblée dans son état.
    static BASCULE: Cell<f64> = const { Cell::new(f64::NEG_INFINITY) };
}

/// L'origine du temps de l'animation.
static DEPART: OnceLock<Instant> = OnceLock::new();

pub fn scene() -> Scene {
    SCENE.with(|s| s.borrow().clone())
}

/// L'horloge de l'animation, en secondes.
fn maintenant() -> f64 {
    DEPART.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// Modifie la scène, puis l'enregistre. Si la case « Lueurs dessinées » change,
/// toute la scène s'allume ou s'éteint EN FONDU.
pub fn modifier(f: impl FnOnce(&mut Scene)) {
    let (texte, basculee) = SCENE.with(|s| {
        let mut s = s.borrow_mut();
        let avant = s.active;
        f(&mut s);
        borner(&mut s);
        (serialiser(&s), avant != s.active)
    });
    if basculee {
        BASCULE.with(|b| b.set(maintenant()));
    }
    crate::config::save_lueurs(texte);
}

/// Ajoute une lueur, décalée de la dernière pour qu'on la voie apparaître — EN
/// FONDU (`nee_a`).
pub fn ajouter() {
    let t = maintenant();
    modifier(|s| {
        if s.elements.len() < MAX_ELEMENTS {
            let mut e = s.elements.last().copied().unwrap_or(Element::PAR_DEFAUT);
            e.x = (e.x + 0.15).rem_euclid(1.0);
            e.y = (e.y + 0.15).rem_euclid(1.0);
            e.nee_a = t;
            s.elements.push(e);
        }
    });
}

/// La scène actuelle dans le format de `lueurs.txt` — un EFFET à enregistrer
/// (voir `win32::lueurs_effets`).
pub fn texte_de_la_scene() -> String {
    SCENE.with(|s| serialiser(&s.borrow()))
}

/// Vrai si `texte` ressemble à une scène : un en-tête dont le premier champ est
/// la case (0 ou 1). Sans ce contrôle, n'importe quel CSV s'importerait comme une
/// scène vide.
pub fn est_une_scene(texte: &str) -> bool {
    let Some(entete) = texte.trim_start_matches('\u{feff}').lines().find(|l| !l.trim().is_empty()) else { return false };
    let champs: Vec<&str> = entete.split(',').map(str::trim).collect();
    champs.len() >= 2 && matches!(champs[0], "0" | "1") && champs[1].parse::<f32>().is_ok_and(f32::is_finite)
}

/// **REMPLACE LA SCÈNE PAR UN EFFET, ET L'ALLUME** — en FONDU ENCHAÎNÉ : les
/// lueurs d'avant s'éteignent comme si on les retirait, les nouvelles montent
/// comme si on les ajoutait. Faux si `texte` n'est pas une scène.
pub fn remplacer_la_scene(texte: &str) -> bool {
    if !est_une_scene(texte) {
        return false;
    }
    let t = maintenant();
    let mut nouvelle = deserialiser(texte.trim_start_matches('\u{feff}'));
    nouvelle.active = true;
    for e in &mut nouvelle.elements {
        e.nee_a = t;
    }
    modifier(|s| {
        let ancienne = std::mem::replace(s, nouvelle);
        if ancienne.active {
            SORTANTES.with(|v| v.borrow_mut().extend(ancienne.elements.into_iter().map(|e| (e, t))));
        }
    });
    true
}

/// Change une durée de la lueur `i` sans saut (voir `Ancre`).
pub fn changer_duree(i: usize, quelle: Duree, secondes: f32) {
    let t = maintenant();
    modifier(|s| s.elements.get_mut(i).into_iter().for_each(|e| e.changer_duree(quelle, secondes, t)));
}

/// Change un temps d'arrêt de la lueur `i` sans saut (voir `Ancre`).
pub fn changer_pause(i: usize, quelle: Pause, secondes: f32) {
    let t = maintenant();
    modifier(|s| s.elements.get_mut(i).into_iter().for_each(|e| e.changer_pause(quelle, secondes, t)));
}

/// Change la vitesse de rotation du fond sans saut (voir `Ancre`).
pub fn changer_rotation(tours_par_minute: f32) {
    let t = maintenant();
    modifier(|s| {
        if tours_par_minute.is_finite() {
            s.ancre_rotation = Ancre { instant: t, phase: angle_en_tours(s, t) };
            s.rotation = tours_par_minute;
        }
    });
}

/// La rotation du fond à l'instant `t`, en tours.
fn angle_en_tours(scene: &Scene, t: f64) -> f64 {
    scene.ancre_rotation.phase + (t - scene.ancre_rotation.instant) * f64::from(scene.rotation) / 60.0
}

/// Change la rotation propre de la lueur `i` sans saut (voir `Ancre`).
pub fn changer_rotation_propre(i: usize, tours_par_minute: f32) {
    let t = maintenant();
    modifier(|s| s.elements.get_mut(i).into_iter().for_each(|e| e.changer_rotation_propre(tours_par_minute, t)));
}

/// Change une vitesse du dégradé animé de la lueur `i` sans saut (voir
/// `Element::changer_animation_du_degrade`).
pub fn changer_animation_du_degrade(i: usize, teinte: bool, tours_par_minute: f32) {
    let t = maintenant();
    modifier(|s| s.elements.get_mut(i).into_iter().for_each(|e| e.changer_animation_du_degrade(teinte, tours_par_minute, t)));
}

/// Arrête la lueur `i` et la remet à son inclinaison (voir
/// `Element::reinitialiser_rotation`).
pub fn reinitialiser_rotation(i: usize) {
    let t = maintenant();
    modifier(|s| s.elements.get_mut(i).into_iter().for_each(|e| e.reinitialiser_rotation(t)));
}

/// Retire la lueur `i` — elle continue de s'éteindre EN FONDU (`SORTANTES`).
pub fn retirer(i: usize) {
    let t = maintenant();
    let mut retiree = None;
    modifier(|s| {
        if i < s.elements.len() {
            retiree = Some(s.elements.remove(i));
        }
    });
    if let Some(e) = retiree {
        SORTANTES.with(|v| v.borrow_mut().push((e, t)));
    }
}

pub fn active() -> bool {
    SCENE.with(|s| s.borrow().active)
}

/// Les images que la scène DESSINE (lueurs présentes puis retirées qui
/// s'éteignent, dans l'ordre de `calculer`), sans doublon : la tranche d'une image
/// dans le tableau du GPU est son rang ici (voir `gfx::lueur_images::lier`).
pub fn images_utilisees() -> Vec<u32> {
    SORTANTES.with(|sortantes| SCENE.with(|s| images_de(&s.borrow(), &sortantes.borrow())))
}

fn images_de(scene: &Scene, sortantes: &[(Element, f64)]) -> Vec<u32> {
    let mut ids = Vec::new();
    let tous = scene.elements.iter().chain(sortantes.iter().map(|(e, _)| e)).take(MAX_ELEMENTS);
    for e in tous.filter(|e| e.figure == Figure::Image && e.image != 0) {
        if !ids.contains(&e.image) {
            ids.push(e.image);
        }
    }
    ids
}

/// Charge `lueurs.txt` (au démarrage). Un fichier absent garde la scène par défaut.
pub fn charger() {
    if let Some(texte) = crate::config::load_lueurs() {
        let scene = deserialiser(&texte);
        SCENE.with(|s| *s.borrow_mut() = scene);
    }
}

fn borner(scene: &mut Scene) {
    scene.assombrissement = borne(scene.assombrissement, 0.0, 1.0, 0.25);
    scene.opacite = borne(scene.opacite, 0.0, 1.0, 1.0);
    scene.rotation = borne(scene.rotation, ROTATION.0, ROTATION.1, 0.0);
    scene.centre = [borne(scene.centre[0], CHAMPS[0].1, CHAMPS[0].2, 0.5), borne(scene.centre[1], CHAMPS[1].1, CHAMPS[1].2, 0.5)];
    scene.elements.truncate(MAX_ELEMENTS);
    for e in &mut scene.elements {
        e.x = borne(e.x, CHAMPS[0].1, CHAMPS[0].2, 0.5);
        e.y = borne(e.y, CHAMPS[1].1, CHAMPS[1].2, 0.0);
        e.rayon = borne(e.rayon, CHAMPS[2].1, CHAMPS[2].2, 0.6);
        e.intensite = borne(e.intensite, CHAMPS[3].1, CHAMPS[3].2, 0.25);
        e.flou = borne(e.flou, CHAMPS[4].1, CHAMPS[4].2, 1.0);
        e.orbite = borne(e.orbite, CHAMPS[5].1, CHAMPS[5].2, 0.1);
        e.periode = borne(e.periode, PERIODE.0, PERIODE.1, 10.0);
        e.presence = borne(e.presence, CHAMPS[6].1, CHAMPS[6].2, 0.6);
        e.fondu = borne(e.fondu, CHAMPS[7].1, CHAMPS[7].2, 0.5);
        e.decalage = borne(e.decalage, CHAMPS[8].1, CHAMPS[8].2, 0.0);
        e.cycle = borne(e.cycle, PERIODE.0, PERIODE.1, 8.0);
        e.longueur = borne(e.longueur, CHAMPS[9].1, CHAMPS[9].2, 0.2);
        e.inclinaison = borne(e.inclinaison, INCLINAISON.0, INCLINAISON.1, 0.0);
        e.rotation_propre = borne(e.rotation_propre, ROTATION.0, ROTATION.1, 0.0);
        e.attente = borne(e.attente, Pause::Attente.bornes().0, Pause::Attente.bornes().1, 0.0);
        e.disparition = borne(e.disparition, Pause::Disparition.bornes().0, Pause::Disparition.bornes().1, 1.0);
        e.absence = borne(e.absence, Pause::Absence.bornes().0, Pause::Absence.bornes().1, 0.0);
        e.couleur = e.couleur.map(|c| borne(c, 0.0, 1.0, 1.0));
        e.duree_couleur = borne(e.duree_couleur, PERIODE.0, PERIODE.1, 12.0);
        e.nb_etapes = e.nb_etapes.min(MAX_ETAPES);
        for c in &mut e.etapes {
            *c = c.map(|v| borne(v, 0.0, 1.0, 1.0));
        }
        e.nb_points = e.nb_points.min(MAX_POINTS);
        e.nb_arrets = e.nb_arrets.min(MAX_ARRETS);
        e.defilement = borne(e.defilement, ROTATION.0, ROTATION.1, 0.0);
        e.teinte = borne(e.teinte, ROTATION.0, ROTATION.1, 0.0);
        for c in &mut e.arrets {
            *c = c.map(|v| borne(v, 0.0, 1.0, 1.0));
        }
        for p in &mut e.points {
            *p = p.map(|v| borne(v, CHAMPS[0].1, CHAMPS[0].2, 0.5));
        }
    }
}

/// La phase, en tours, d'une animation à `tours_par_minute` ancrée en `ancre`.
fn phase_en_tours(ancre: Ancre, tours_par_minute: f32, t: f64) -> f64 {
    ancre.phase + (t - ancre.instant) * f64::from(tours_par_minute) / 60.0
}

/// `couleur` dont la teinte a tourné de `tours` (0..1 : un tour de la roue).
fn tourner_la_teinte([r, g, b]: [f32; 3], tours: f32) -> [f32; 3] {
    if tours == 0.0 {
        return [r, g, b];
    }
    let (h, s, l) = crate::ui::color::rgb_to_hsl([r, g, b]);
    crate::ui::color::hsl_to_rgb((h + tours * 360.0).rem_euclid(360.0), s, l)
}

fn borne(v: f32, min: f32, max: f32, defaut: f32) -> f32 {
    if v.is_finite() { v.clamp(min, max) } else { defaut }
}

/// Le champ `nom` de `e`, s'il existe.
pub fn champ_mut<'a>(e: &'a mut Element, nom: &str) -> Option<&'a mut f32> {
    match nom {
        "x" => Some(&mut e.x),
        "y" => Some(&mut e.y),
        "rayon" => Some(&mut e.rayon),
        "intensite" => Some(&mut e.intensite),
        "flou" => Some(&mut e.flou),
        "orbite" => Some(&mut e.orbite),
        "presence" => Some(&mut e.presence),
        "fondu" => Some(&mut e.fondu),
        "decalage" => Some(&mut e.decalage),
        "longueur" => Some(&mut e.longueur),
        _ => None,
    }
}

fn serialiser(s: &Scene) -> String {
    let [cx, cy] = s.centre;
    let mut lignes = vec![format!("{},{},{},{cx},{cy},{}", u8::from(s.active), s.assombrissement, s.rotation, s.opacite)];
    for e in &s.elements {
        let [r, g, b] = e.couleur;
        let mut ligne = format!(
            "{},{},{},{},{},{r},{g},{b},{},{},{}",
            e.x,
            e.y,
            e.rayon,
            e.intensite,
            e.flou,
            e.mouvement.code(),
            e.periode,
            e.orbite
        );
        for [px, py] in e.chemin() {
            ligne.push_str(&format!(",{px},{py}"));
        }
        ligne.push_str(&format!(";{},{},{},{},{}", u8::from(e.apparition), e.cycle, e.presence, e.fondu, e.decalage));
        ligne.push_str(&format!(";{},{}", u8::from(e.cycle_couleur), e.duree_couleur));
        for [r, g, b] in e.couleurs_du_cycle() {
            ligne.push_str(&format!(",{r},{g},{b}"));
        }
        ligne.push_str(&format!(";{},{},{},{}", e.figure.code(), e.inclinaison, e.longueur, e.rotation_propre));
        ligne.push_str(&format!(";{},{},{}", e.attente, e.disparition, e.absence));
        ligne.push_str(&format!(";{}", e.degrade.code()));
        for [r, g, b] in e.couleurs_du_degrade() {
            ligne.push_str(&format!(",{r},{g},{b}"));
        }
        ligne.push_str(&format!(";{}", e.image));
        ligne.push_str(&format!(";{},{}", e.defilement, e.teinte));
        lignes.push(ligne);
    }
    lignes.join("\n")
}

fn deserialiser(texte: &str) -> Scene {
    let nombres = |ligne: &str| ligne.split(',').map(|v| v.trim().parse::<f32>().unwrap_or(f32::NAN)).collect::<Vec<_>>();
    let mut lignes = texte.lines().filter(|l| !l.trim().is_empty());
    let mut scene = Scene { elements: Vec::new(), ..Scene::default() };
    if let Some(entete) = lignes.next() {
        let v = nombres(entete);
        scene.active = v.first().is_some_and(|&a| a == 1.0);
        scene.assombrissement = v.get(1).copied().unwrap_or(0.25);
        scene.rotation = v.get(2).copied().unwrap_or(0.0);
        scene.centre = [v.get(3).copied().unwrap_or(0.5), v.get(4).copied().unwrap_or(0.5)];
        scene.opacite = v.get(5).copied().unwrap_or(1.0);
    }
    for ligne in lignes {
        let mut sections = ligne.split(';');
        let ligne = sections.next().unwrap_or("");
        let apparition = sections.next().unwrap_or("");
        let couleurs = sections.next().unwrap_or("");
        let figure = sections.next().unwrap_or("");
        let pauses = sections.next().unwrap_or("");
        let degrade = sections.next().unwrap_or("");
        // ⚠️ En ENTIER : une empreinte de 32 bits ne tient pas dans un `f32`.
        let image = sections.next().and_then(|t| t.trim().parse::<u32>().ok()).unwrap_or(0);
        let animation = sections.next().unwrap_or("");
        let v = nombres(ligne);
        if v.len() < 8 {
            continue;
        }
        let mut e = Element { x: v[0], y: v[1], rayon: v[2], intensite: v[3], flou: v[4], couleur: [v[5], v[6], v[7]], ..Element::PAR_DEFAUT };
        if let Some(&code) = v.get(8) {
            e.mouvement = Mouvement::depuis_code(code);
        }
        e.periode = v.get(9).copied().unwrap_or(e.periode);
        e.orbite = v.get(10).copied().unwrap_or(e.orbite);
        for paire in v.get(11..).unwrap_or(&[]).chunks_exact(2).take(MAX_POINTS) {
            e.points[e.nb_points] = [paire[0], paire[1]];
            e.nb_points += 1;
        }
        if !apparition.trim().is_empty() {
            let a = nombres(apparition);
            e.apparition = a.first().is_some_and(|&v| v == 1.0);
            e.cycle = a.get(1).copied().unwrap_or(e.cycle);
            e.presence = a.get(2).copied().unwrap_or(e.presence);
            e.fondu = a.get(3).copied().unwrap_or(e.fondu);
            e.decalage = a.get(4).copied().unwrap_or(e.decalage);
        }
        if !couleurs.trim().is_empty() {
            let c = nombres(couleurs);
            e.cycle_couleur = c.first().is_some_and(|&v| v == 1.0);
            e.duree_couleur = c.get(1).copied().unwrap_or(e.duree_couleur);
            for rgb in c.get(2..).unwrap_or(&[]).chunks_exact(3).take(MAX_ETAPES) {
                e.etapes[e.nb_etapes] = [rgb[0], rgb[1], rgb[2]];
                e.nb_etapes += 1;
            }
        }
        if !figure.trim().is_empty() {
            let f = nombres(figure);
            e.figure = f.first().map_or(Figure::Cercle, |&c| Figure::depuis_code(c));
            e.inclinaison = f.get(1).copied().unwrap_or(e.inclinaison);
            e.longueur = f.get(2).copied().unwrap_or(e.longueur);
            e.rotation_propre = f.get(3).copied().unwrap_or(e.rotation_propre);
        }
        if !pauses.trim().is_empty() {
            let p = nombres(pauses);
            e.attente = p.first().copied().unwrap_or(e.attente);
            e.disparition = p.get(1).copied().unwrap_or(e.disparition);
            e.absence = p.get(2).copied().unwrap_or(e.absence);
        }
        if !degrade.trim().is_empty() {
            let d = nombres(degrade);
            e.degrade = d.first().map_or(Degrade::Aucun, |&c| Degrade::depuis_code(c));
            for rgb in d.get(1..).unwrap_or(&[]).chunks_exact(3).take(MAX_ARRETS) {
                e.arrets[e.nb_arrets] = [rgb[0], rgb[1], rgb[2]];
                e.nb_arrets += 1;
            }
        }
        e.image = image;
        if !animation.trim().is_empty() {
            let a = nombres(animation);
            e.defilement = a.first().copied().unwrap_or(0.0);
            e.teinte = a.get(1).copied().unwrap_or(0.0);
        }
        scene.elements.push(e);
    }
    borner(&mut scene);
    scene
}

/// Les constantes GPU, à la suite de `DegradeGpu` (voir
/// `quad_renderer::ViewportConstants`) : un registre d'en-tête, puis deux tableaux
/// de `MAX_ELEMENTS` registres.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LueurGpu {
    /// Nombre d'éléments à dessiner — 0 : éteinte.
    nombre: f32,
    assombrissement: f32,
    _remplissage: [f32; 2],
    /// `(centre x, centre y, rayon, intensité)`, en pixels ÉCRAN.
    disques: [[f32; 4]; MAX_ELEMENTS],
    /// `(r, g, b, flou)`.
    teintes: [[f32; 4]; MAX_ELEMENTS],
    /// `(code de la figure, angle en radians, demi-longueur en pixels, 0)`.
    formes: [[f32; 4]; MAX_ELEMENTS],
    /// `(code du dégradé, nombre d'arrêts, tranche de l'image, défilement)` — le
    /// défilement est la phase (0..1) d'un dégradé qui défile, -1 s'il est immobile.
    degrades: [[f32; 4]; MAX_ELEMENTS],
    /// Les arrêts du dégradé de la lueur `i` : `arrets[i * MAX_ARRETS + k]`, RGB.
    arrets: [[f32; 4]; MAX_ELEMENTS * MAX_ARRETS],
}

// `derive(Default)` ne va pas au-delà de 32 éléments par tableau (`arrets`).
impl Default for LueurGpu {
    fn default() -> Self {
        LueurGpu {
            nombre: 0.0,
            assombrissement: 0.0,
            _remplissage: [0.0; 2],
            disques: [[0.0; 4]; MAX_ELEMENTS],
            teintes: [[0.0; 4]; MAX_ELEMENTS],
            formes: [[0.0; 4]; MAX_ELEMENTS],
            degrades: [[0.0; 4]; MAX_ELEMENTS],
            arrets: [[0.0; 4]; MAX_ELEMENTS * MAX_ARRETS],
        }
    }
}

pub(crate) fn constantes() -> LueurGpu {
    if !crate::gfx::capture_visibility::is_enabled() {
        return LueurGpu::default();
    }
    let ecran = unsafe { [GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)] }.map(|v| v.max(1) as f32);
    let t = maintenant();
    // Les lueurs retirées dont le fondu est fini ne se dessinent plus.
    SORTANTES.with(|v| v.borrow_mut().retain(|(_, parti)| t - parti < FONDU_AJOUT));
    let entree = progression(t - BASCULE.with(Cell::get));
    let facteur = if active() { entree } else { 1.0 - entree };
    if facteur <= 0.0 {
        return LueurGpu::default();
    }
    SORTANTES.with(|sortantes| SCENE.with(|s| calculer(&s.borrow(), &sortantes.borrow(), ecran, t, facteur)))
}

/// 0 → 1, adouci, sur `FONDU_AJOUT` secondes écoulées.
fn progression(ecoule: f64) -> f32 {
    let x = (ecoule / FONDU_AJOUT).clamp(0.0, 1.0) as f32;
    x * x * (3.0 - 2.0 * x)
}

/// Pure, donc testable sans écran ni horloge. `t` en secondes ; `facteur` (0..1)
/// est le fondu de toute la scène ; `sortantes` s'éteignent après les autres.
fn calculer(scene: &Scene, sortantes: &[(Element, f64)], [l, h]: [f32; 2], t: f64, facteur: f32) -> LueurGpu {
    let mut g = LueurGpu { assombrissement: scene.assombrissement * facteur, ..LueurGpu::default() };
    if facteur <= 0.0 {
        return g;
    }
    // La rotation du fond : un angle qui tourne avec le temps, autour du centre.
    let angle = angle_en_tours(scene, t).rem_euclid(1.0) as f32 * TAU;
    let (sin, cos) = angle.sin_cos();
    let centre = [scene.centre[0] * l, scene.centre[1] * h];
    // Chaque lueur, avec son fondu : d'entrée pour les présentes, de sortie pour
    // les retirées.
    let presentes = scene.elements.iter().map(|e| (e, progression(t - e.nee_a)));
    let retirees = sortantes.iter().map(|(e, parti)| (e, 1.0 - progression(t - parti)));
    let images = images_de(scene, sortantes);
    for (i, (e, fondu)) in presentes.chain(retirees).take(MAX_ELEMENTS).enumerate() {
        let [px, py] = position(e, [l, h], t);
        let (dx, dy) = (px - centre[0], py - centre[1]);
        let p = [centre[0] + dx * cos - dy * sin, centre[1] + dx * sin + dy * cos];
        g.disques[i] = [p[0], p[1], (e.rayon * l).max(1.0), e.intensite * visibilite(e, t) * fondu * facteur * scene.opacite];
        // La TEINTE qui tourne (voir `Element::teinte`) : sous un dégradé, ou sur
        // une image — dont le shader tourne les couleurs (`formes[i][3]`).
        let image = e.figure == Figure::Image;
        let avec_degrade = !image && e.degrade != Degrade::Aucun && e.nb_arrets > 0;
        let roue = if avec_degrade || image { phase_en_tours(e.ancre_teinte, e.teinte, t).rem_euclid(1.0) as f32 } else { 0.0 };
        let [r, v, b] = tourner_la_teinte(couleur_a(e, t), if image { 0.0 } else { roue });
        g.teintes[i] = [r, v, b, e.flou];
        // L'angle de la figure : son inclinaison, sa rotation propre, et celle du
        // fond — qui fait tourner les figures avec leurs centres.
        let tours = e.tours_propres(t) + angle_en_tours(scene, t);
        let radians = e.inclinaison.to_radians() + (tours.rem_euclid(1.0) as f32) * TAU;
        g.formes[i] = [f32::from(e.figure.code()), radians, (e.longueur * l).max(1.0), if image { roue } else { 0.0 }];
        let tranche = images.iter().position(|&id| id == e.image).map_or(-1.0, |k| k as f32);
        let arrets = e.couleurs_du_degrade();
        // Un dégradé sans arrêt ne serait qu'une couleur : le shader n'en voit aucun.
        let code = if arrets.is_empty() { 0 } else { e.degrade.code() };
        let defile = if e.defilement != 0.0 { phase_en_tours(e.ancre_defilement, e.defilement, t).rem_euclid(1.0) as f32 } else { -1.0 };
        g.degrades[i] = [f32::from(code), arrets.len() as f32, tranche, defile];
        for (k, c) in arrets.iter().enumerate() {
            let [r, v, b] = tourner_la_teinte(*c, roue);
            g.arrets[i * MAX_ARRETS + k] = [r, v, b, 0.0];
        }
        g.nombre = (i + 1) as f32;
    }
    g
}

/// La couleur d'un élément à l'instant `t` — fixe, ou le long de son cycle.
fn couleur_a(e: &Element, t: f64) -> [f32; 3] {
    if !e.cycle_couleur || e.nb_etapes == 0 {
        return e.couleur;
    }
    let couleurs: Vec<[f32; 3]> = std::iter::once(e.couleur).chain(e.couleurs_du_cycle().iter().copied()).collect();
    let n = couleurs.len();
    let u = e.ancres[Duree::Couleur as usize].phase(t, e.duree_couleur).rem_euclid(1.0) * n as f64;
    let k = (u as usize).min(n - 1);
    // Fondu ADOUCI d'une couleur à la suivante : la lueur s'attarde un peu sur
    // chacune au lieu de les traverser à vitesse constante.
    let f = (u - k as f64) as f32;
    let f = f * f * (3.0 - 2.0 * f);
    let (a, b) = (couleurs[k], couleurs[(k + 1) % n]);
    [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f]
}

/// La position d'un élément à l'instant `t`, en pixels ÉCRAN, AVANT la rotation.
fn position(e: &Element, [l, h]: [f32; 2], t: f64) -> [f32; 2] {
    let depart = [e.x * l, e.y * h];
    // `s` : les secondes écoulées dans le tour ; `u` : où en est le MOUVEMENT, de
    // 0 à 1 — immobile au départ pendant les temps d'arrêt qui le précèdent, et au
    // bout pendant ceux qui le suivent.
    let s = e.seconde_du_tour(t);
    let avant = match e.mouvement {
        Mouvement::Trajet => e.disparition + e.attente,
        Mouvement::AllerRetour | Mouvement::Boucle => e.attente,
        Mouvement::Fixe | Mouvement::Orbite => 0.0,
    };
    let u = ((s - avant) / e.periode.max(PERIODE.0)).clamp(0.0, 1.0);
    let chemin = || std::iter::once(depart).chain(e.chemin().iter().map(|[x, y]| [x * l, y * h])).collect::<Vec<_>>();
    match e.mouvement {
        Mouvement::Fixe => depart,
        Mouvement::Orbite => {
            let (sin, cos) = (u * TAU).sin_cos();
            let r = e.orbite * l;
            [depart[0] + r * cos, depart[1] + r * sin]
        }
        // Un triangle 0 → 1 → 0, adouci en cosinus : la lueur ralentit aux bouts
        // au lieu de rebondir.
        Mouvement::AllerRetour => {
            let triangle = 1.0 - (2.0 * u - 1.0).abs();
            le_long(&chemin(), (1.0 - (triangle * std::f32::consts::PI).cos()) / 2.0)
        }
        Mouvement::Boucle => {
            let mut points = chemin();
            points.push(depart);
            le_long(&points, u)
        }
        // Adouci aux deux bouts : la lueur DÉMARRE puis S'ARRÊTE au lieu de partir
        // et de s'immobiliser d'un coup.
        Mouvement::Trajet => le_long(&chemin(), (1.0 - (u * std::f32::consts::PI).cos()) / 2.0),
    }
}

/// 0..1 : combien la lueur est ALLUMÉE à l'instant `t` — son apparition en
/// fondu, et les fondus de son aller simple.
fn visibilite(e: &Element, t: f64) -> f32 {
    apparition(e, t) * visibilite_du_trajet(e, t)
}

/// 0..1 : l'aller simple ÉTEINT la lueur au bout et la RALLUME au départ (voir
/// `Mouvement::Trajet`). 1 pour les autres mouvements.
fn visibilite_du_trajet(e: &Element, t: f64) -> f32 {
    if e.mouvement != Mouvement::Trajet {
        return 1.0;
    }
    let s = e.seconde_du_tour(t);
    let f = e.disparition;
    let lisse = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    let arrivee = f + e.attente + e.periode;
    // `s < f` implique `f > 0` : aucune division par zéro.
    if s < f {
        lisse(s / f)
    } else if s < arrivee {
        1.0
    } else if s < arrivee + f {
        1.0 - lisse((s - arrivee) / f)
    } else {
        0.0
    }
}

/// 0..1 : la case « Apparaît et disparaît en fondu » (1 si décochée).
fn apparition(e: &Element, t: f64) -> f32 {
    if !e.apparition {
        return 1.0;
    }
    let u = (e.ancres[Duree::Apparition as usize].phase(t, e.cycle) + f64::from(e.decalage)).rem_euclid(1.0) as f32;
    if u >= e.presence {
        return 0.0;
    }
    // `v` : où en est la présence, de 0 à 1 ; chaque fondu en prend `fondu / 2`.
    let v = u / e.presence;
    let largeur = e.fondu / 2.0;
    if largeur <= f32::EPSILON {
        return 1.0;
    }
    let lisse = |x: f32| {
        let x = (x / largeur).clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    lisse(v) * lisse(1.0 - v)
}

/// Le point à la fraction `s` (0..1) de la LONGUEUR de la ligne brisée `points`.
fn le_long(points: &[[f32; 2]], s: f32) -> [f32; 2] {
    let longueur = |a: [f32; 2], b: [f32; 2]| ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
    let total: f32 = points.windows(2).map(|w| longueur(w[0], w[1])).sum();
    let Some(&premier) = points.first() else { return [0.0, 0.0] };
    if total <= f32::EPSILON {
        return premier;
    }
    let mut reste = s.clamp(0.0, 1.0) * total;
    for w in points.windows(2) {
        let d = longueur(w[0], w[1]);
        if reste <= d && d > 0.0 {
            let k = reste / d;
            return [w[0][0] + (w[1][0] - w[0][0]) * k, w[0][1] + (w[1][1] - w[0][1]) * k];
        }
        reste -= d;
    }
    *points.last().unwrap_or(&premier)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ECRAN: [f32; 2] = [3840.0, 2160.0];

    #[test]
    fn la_lueur_s_eteint_hors_du_mode_capturable() {
        let scene = Scene { active: true, ..Scene::default() };
        let g = calculer(&scene, &[], ECRAN, 0.0, 1.0);
        assert_eq!(g.nombre, 1.0);
        assert_eq!(g.disques[0], [1920.0, 0.0, 3840.0 * 0.6, 0.25]);
        assert_eq!(g.teintes[0], [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(calculer(&scene, &[], ECRAN, 0.0, 0.0).nombre, 0.0);
    }

    /// Le dégradé, ses arrêts et l'image passent par le fichier — l'empreinte en
    /// ENTIER, qu'un `f32` aurait arrondie.
    #[test]
    fn le_degrade_et_l_image_font_l_aller_retour() {
        let mut e = Element { figure: Figure::Image, image: 0xdead_beef, ..Element::PAR_DEFAUT };
        e.changer_degrade(Degrade::Angulaire);
        e.ajouter_arret();
        e.arrets[1] = [0.1, 0.2, 0.3];
        let scene = Scene { elements: vec![e], ..Scene::default() };
        let relue = deserialiser(&serialiser(&scene));
        assert_eq!(relue.elements[0].image, 0xdead_beef);
        assert_eq!(relue.elements[0].degrade, Degrade::Angulaire);
        assert_eq!(relue.elements[0].couleurs_du_degrade(), e.couleurs_du_degrade());
    }

    /// ⭐ Le dégradé ANIMÉ : la teinte tourne, le défilement avance — et changer
    /// une vitesse ne fait pas sauter la phase.
    #[test]
    fn le_degrade_anime_tourne_sa_teinte_et_defile() {
        let mut e = Element { couleur: [1.0, 0.0, 0.0], ..Element::PAR_DEFAUT };
        e.changer_degrade(Degrade::Lineaire);
        e.changer_animation_du_degrade(true, 20.0, 0.0);
        e.changer_animation_du_degrade(false, 6.0, 0.0);
        let scene = Scene { active: true, elements: vec![e], ..Scene::default() };
        // 20 tr/min : un tiers de tour en une seconde — le rouge devient VERT.
        let g = calculer(&scene, &[], ECRAN, 1.0, 1.0);
        let [r, v, b, _] = g.teintes[0];
        assert!(r < 0.01 && v > 0.99 && b < 0.01, "{:?}", g.teintes[0]);
        // 6 tr/min : un dixième de tour en une seconde.
        assert!((g.degrades[0][3] - 0.1).abs() < 1e-5, "{}", g.degrades[0][3]);
        // Immobile : -1, et la teinte d'origine.
        let fixe = calculer(&Scene { active: true, elements: vec![Element::PAR_DEFAUT], ..Scene::default() }, &[], ECRAN, 1.0, 1.0);
        assert_eq!(fixe.degrades[0][3], -1.0);
        // Ralentir à t = 1 garde la phase atteinte.
        let mut f = e;
        f.changer_animation_du_degrade(false, 0.5, 1.0);
        assert!((phase_en_tours(f.ancre_defilement, f.defilement, 1.0) - 0.1).abs() < 1e-9);
        // Et le fichier garde les deux vitesses.
        let relue = deserialiser(&serialiser(&scene));
        assert_eq!((relue.elements[0].defilement, relue.elements[0].teinte), (6.0, 20.0));
    }

    /// ⭐ Une IMAGE tourne sa teinte DANS LE SHADER (ses couleurs viennent de la
    /// texture) : sa phase passe par `formes[i][3]` ; une autre figure y laisse 0.
    #[test]
    fn une_image_tourne_sa_teinte_dans_le_shader() {
        let mut e = Element { figure: Figure::Image, image: 7, ..Element::PAR_DEFAUT };
        e.changer_animation_du_degrade(true, 20.0, 0.0);
        let cercle = Element { figure: Figure::Cercle, ..e };
        let g = calculer(&Scene { active: true, elements: vec![e, cercle], ..Scene::default() }, &[], ECRAN, 1.0, 1.0);
        assert!((g.formes[0][3] - 1.0 / 3.0).abs() < 1e-5, "{}", g.formes[0][3]);
        assert_eq!(g.formes[1][3], 0.0, "sans dégradé, un cercle ne tourne pas");
        assert_eq!(g.teintes[1][..3], [1.0, 1.0, 1.0]);
    }

    /// Une lueur blanche reçoit une seconde couleur VISIBLE (pas son opposé gris).
    #[test]
    fn un_degrade_tout_neuf_a_deux_couleurs() {
        let mut e = Element::PAR_DEFAUT;
        e.changer_degrade(Degrade::Lineaire);
        assert_eq!(e.nb_arrets, 1);
        assert_eq!(e.arrets[0], [0.2, 0.4, 1.0]);
        e.retirer_arret();
        assert_eq!(e.nb_arrets, 1, "jamais moins de deux couleurs");
    }

    /// Deux lueurs sur la MÊME image partagent sa tranche ; une sans image n'en a pas.
    #[test]
    fn chaque_image_a_une_tranche_et_une_seule() {
        let img = |id| Element { figure: Figure::Image, image: id, ..Element::PAR_DEFAUT };
        let scene = Scene { active: true, elements: vec![img(7), Element::PAR_DEFAUT, img(9), img(7)], ..Scene::default() };
        assert_eq!(images_de(&scene, &[]), vec![7, 9]);
        let g = calculer(&scene, &[], ECRAN, 0.0, 1.0);
        assert_eq!([g.degrades[0][2], g.degrades[1][2], g.degrades[2][2], g.degrades[3][2]], [0.0, -1.0, 1.0, 0.0]);
    }

    /// ⭐ VINGT lueurs à la fois (DEMANDE EXPLICITE, 2026-10-14).
    #[test]
    fn vingt_lueurs_se_dessinent() {
        let scene = Scene { active: true, elements: vec![Element::PAR_DEFAUT; 25], ..Scene::default() };
        let mut bornee = scene.clone();
        borner(&mut bornee);
        assert_eq!(bornee.elements.len(), 20);
        assert_eq!(calculer(&bornee, &[], ECRAN, 0.0, 1.0).nombre, 20.0);
    }

    #[test]
    fn la_scene_fait_un_aller_retour_par_le_fichier() {
        let mut anime = Element {
            x: 0.1,
            y: 0.9,
            rayon: 0.2,
            intensite: 0.7,
            flou: 0.3,
            couleur: [1.0, 0.5, 0.25],
            mouvement: Mouvement::Boucle,
            periode: 7.5,
            orbite: 0.3,
            ..Element::PAR_DEFAUT
        };
        anime.ajouter_point();
        anime.ajouter_point();
        anime.attente = 2.5;
        anime.disparition = 0.5;
        anime.absence = 4.0;
        let scene = Scene {
            active: true,
            assombrissement: 0.4,
            rotation: -3.0,
            centre: [0.25, 0.75],
            elements: vec![Element::PAR_DEFAUT, anime],
            ancre_rotation: Ancre::default(),
            opacite: 0.35,
        };
        assert_eq!(deserialiser(&serialiser(&scene)), scene);
    }

    /// L'opacité globale atténue TOUTES les lueurs, pas l'assombrissement ; un
    /// fichier d'avant elle se relit à 100 %.
    #[test]
    fn l_opacite_globale_attenue_les_lueurs_et_pas_l_assombrissement() {
        let scene = Scene { active: true, opacite: 0.5, ..Scene::default() };
        let g = calculer(&scene, &[], ECRAN, 0.0, 1.0);
        assert_eq!(g.disques[0][3], 0.25 * 0.5);
        assert_eq!(g.assombrissement, 0.25, "l'assombrissement a son propre curseur");
        assert_eq!(deserialiser("1,0.25,0,0.5,0.5\n0.5,0,0.6,0.3,1,1,1,1\n").opacite, 1.0);
        assert_eq!(deserialiser("1,0.25,0,0.5,0.5,7\n").opacite, 1.0, "bornée");
    }

    /// ⛔ Un fichier écrit AVANT l'animation (huit nombres par ligne) se relit fixe.
    #[test]
    fn un_fichier_sans_animation_se_relit_fixe() {
        let scene = deserialiser("1,0.25\n0.5,0,0.6,0.3,1,1,1,1\n");
        assert_eq!((scene.rotation, scene.centre), (0.0, [0.5, 0.5]));
        assert_eq!(scene.elements[0].mouvement, Mouvement::Fixe);
        assert_eq!(scene.elements[0].nb_points, 0);
    }

    #[test]
    fn un_fichier_abime_est_borne_et_tronque() {
        let mut texte = String::from("1,9,500,x,0\n");
        for _ in 0..MAX_ELEMENTS + 4 {
            texte.push_str("5,-5,0,2,x,1,1,1,9,0,7,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1\n");
        }
        texte.push_str("incomplet,1\n");
        let scene = deserialiser(&texte);
        assert_eq!(scene.assombrissement, 1.0);
        assert_eq!(scene.rotation, ROTATION.1);
        assert_eq!(scene.centre, [0.5, 0.0]);
        assert_eq!(scene.elements.len(), MAX_ELEMENTS);
        let e = scene.elements[0];
        assert_eq!((e.x, e.y, e.rayon, e.intensite, e.flou), (CHAMPS[0].2, CHAMPS[1].1, CHAMPS[2].1, 1.0, 1.0));
        assert_eq!((e.mouvement, e.periode, e.orbite, e.nb_points), (Mouvement::Fixe, PERIODE.0, 1.0, MAX_POINTS));
    }

    #[test]
    fn l_aller_retour_part_va_et_revient() {
        let mut e = Element { x: 0.0, y: 0.5, mouvement: Mouvement::AllerRetour, periode: 10.0, ..Element::PAR_DEFAUT };
        e.points[0] = [1.0, 0.5];
        e.nb_points = 1;
        let x = |t: f64| position(&e, ECRAN, t)[0];
        assert_eq!(x(0.0), 0.0);
        assert!((x(5.0) - 3840.0).abs() < 0.5, "au bout à mi-période : {}", x(5.0));
        assert!((x(2.5) - 1920.0).abs() < 0.5, "au milieu au quart : {}", x(2.5));
        assert!(x(10.0).abs() < 0.5, "revenu au départ : {}", x(10.0));
    }

    /// L'aller simple : réapparaît au départ, attend, part, s'éteint au bout, reste
    /// éteint, recommence. Tour : 1 + 2 + 10 + 1 + 3 = 17 s.
    #[test]
    fn l_aller_simple_attend_part_s_eteint_et_reprend_au_depart() {
        let mut e = Element {
            x: 0.0,
            y: 0.5,
            mouvement: Mouvement::Trajet,
            periode: 10.0,
            attente: 2.0,
            disparition: 1.0,
            absence: 3.0,
            ..Element::PAR_DEFAUT
        };
        e.points[0] = [1.0, 0.5];
        e.nb_points = 1;
        let x = |t: f64| position(&e, ECRAN, t)[0];
        let v = |t: f64| visibilite(&e, t);
        assert_eq!((x(0.0), v(0.0)), (0.0, 0.0), "rallumée au départ, depuis le noir");
        assert!(v(0.5) > 0.0 && v(0.5) < 1.0);
        assert_eq!((x(2.9), v(2.9)), (0.0, 1.0), "la MINUTERIE : immobile, allumée");
        assert!((x(8.0) - 1920.0).abs() < 0.5, "mi-chemin à mi-trajet : {}", x(8.0));
        assert_eq!((x(13.0), v(13.0)), (3840.0, 1.0), "arrivée au bout");
        assert!(x(13.5) == 3840.0 && v(13.5) > 0.0 && v(13.5) < 1.0, "s'éteint AU BOUT");
        assert_eq!(v(15.0), 0.0, "éteinte pendant l'absence");
        assert_eq!((x(17.0), v(17.0)), (0.0, 0.0), "un nouveau tour, au départ");
    }

    /// Sans fondu, l'aller simple disparaît et réapparaît d'un coup — sans diviser
    /// par zéro.
    #[test]
    fn l_aller_simple_sans_fondu_ne_divise_pas_par_zero() {
        let e = Element { mouvement: Mouvement::Trajet, periode: 4.0, disparition: 0.0, absence: 2.0, ..Element::PAR_DEFAUT };
        assert_eq!(visibilite(&e, 0.0), 1.0);
        assert_eq!(visibilite(&e, 3.9), 1.0);
        assert_eq!(visibilite(&e, 4.5), 0.0);
    }

    /// La minuterie retient aussi l'aller-retour au départ.
    #[test]
    fn l_attente_retient_l_aller_retour_au_depart() {
        let mut e = Element { x: 0.0, mouvement: Mouvement::AllerRetour, periode: 10.0, attente: 5.0, ..Element::PAR_DEFAUT };
        e.points[0] = [1.0, 0.0];
        e.nb_points = 1;
        assert_eq!(position(&e, ECRAN, 4.9)[0], 0.0);
        assert!((position(&e, ECRAN, 10.0)[0] - 3840.0).abs() < 0.5, "au bout à mi-mouvement");
    }

    /// ⛔ Changer un temps d'arrêt ne fait pas sauter la lueur (voir `Ancre`).
    #[test]
    fn changer_une_pause_garde_la_phase() {
        let mut e = Element { mouvement: Mouvement::Trajet, periode: 10.0, ..Element::PAR_DEFAUT };
        let t = 1234.5;
        let avant = e.ancres[Duree::Mouvement as usize].phase(t, e.duree_du_tour());
        e.changer_pause(Pause::Attente, 7.0, t);
        let apres = e.ancres[Duree::Mouvement as usize].phase(t, e.duree_du_tour());
        assert!((avant - apres).abs() < 1e-9);
        assert_eq!(e.attente, 7.0);
        e.changer_pause(Pause::Disparition, 99.0, t);
        assert_eq!(e.disparition, Pause::Disparition.bornes().1);
    }

    #[test]
    fn la_boucle_suit_le_chemin_a_vitesse_constante_et_se_referme() {
        // Un carré de 1000 px de côté : quatre côtés égaux, un quart de période chacun.
        let ecran = [1000.0, 1000.0];
        let mut e = Element { x: 0.0, y: 0.0, mouvement: Mouvement::Boucle, periode: 4.0, ..Element::PAR_DEFAUT };
        e.points[..3].copy_from_slice(&[[1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        e.nb_points = 3;
        let p = |t: f64| position(&e, ecran, t).map(f32::round);
        assert_eq!(p(1.0), [1000.0, 0.0]);
        assert_eq!(p(2.0), [1000.0, 1000.0]);
        assert_eq!(p(3.5), [0.0, 500.0], "le dernier côté RAMÈNE au départ");
    }

    #[test]
    fn l_orbite_tourne_autour_du_centre() {
        let e = Element { x: 0.5, y: 0.5, mouvement: Mouvement::Orbite, periode: 4.0, orbite: 0.1, ..Element::PAR_DEFAUT };
        let p = |t: f64| position(&e, ECRAN, t).map(f32::round);
        assert_eq!(p(0.0), [1920.0 + 384.0, 1080.0]);
        assert_eq!(p(1.0), [1920.0, 1080.0 + 384.0], "un rayon en pixels : un CERCLE, pas une ellipse");
    }

    #[test]
    fn la_rotation_du_fond_tourne_les_centres_autour_du_sien() {
        // Un tour par minute : un quart de tour en 15 s.
        let e = Element { x: 0.75, y: 0.5, ..Element::PAR_DEFAUT };
        let scene = Scene { active: true, rotation: 1.0, centre: [0.5, 0.5], elements: vec![e], ..Scene::default() };
        let g = calculer(&scene, &[], [1000.0, 1000.0], 15.0, 1.0);
        assert_eq!([g.disques[0][0].round(), g.disques[0][1].round()], [500.0, 750.0]);
    }

    /// ⭐ Le cycle d'apparition : éteinte au départ, allumée au milieu de sa
    /// présence, éteinte après — et sans saut aux bords du fondu.
    #[test]
    fn la_lueur_apparait_et_disparait_en_fondu() {
        let e = Element { apparition: true, cycle: 10.0, presence: 0.6, fondu: 0.5, ..Element::PAR_DEFAUT };
        assert_eq!(visibilite(&e, 0.0), 0.0, "le cycle commence éteint");
        assert_eq!(visibilite(&e, 3.0), 1.0, "allumée au milieu de sa présence (6 s)");
        assert!((visibilite(&e, 0.75) - 0.5).abs() < 0.01, "à mi-fondu d'entrée (1,5 s) : {}", visibilite(&e, 0.75));
        assert_eq!(visibilite(&e, 8.0), 0.0, "éteinte hors de sa présence");
        // Le décalage d'un demi-cycle échange allumé et éteint.
        let decalee = Element { decalage: 0.5, ..e };
        assert_eq!(visibilite(&decalee, 8.0), 1.0);
        assert_eq!(visibilite(&Element::PAR_DEFAUT, 8.0), 1.0, "sans apparition : toujours allumée");
    }

    #[test]
    fn le_cycle_d_apparition_passe_par_le_fichier_et_l_ancien_format_reste_lisible() {
        let mut e = Element { apparition: true, cycle: 12.0, presence: 0.4, fondu: 0.2, decalage: 0.25, ..Element::PAR_DEFAUT };
        e.ajouter_point();
        let scene = Scene { elements: vec![e], ..Scene::default() };
        assert_eq!(deserialiser(&serialiser(&scene)), scene);
        // Une ligne SANS `;` (écrite avant le fondu) garde ses points et reste allumée.
        let ancienne = deserialiser("1,0.25\n0.5,0,0.6,0.3,1,1,1,1,2,6,0.1,0.8,0.2\n");
        assert_eq!(ancienne.elements[0].nb_points, 1);
        assert!(!ancienne.elements[0].apparition);
    }

    /// ⭐ Une lueur AJOUTÉE monte en fondu, une RETIRÉE s'éteint en fondu, et
    /// la case « Lueurs dessinées » fond toute la scène.
    #[test]
    fn ajouter_et_retirer_se_font_en_fondu() {
        let nee = Element { nee_a: 10.0, ..Element::PAR_DEFAUT };
        let scene = Scene { active: true, elements: vec![nee], ..Scene::default() };
        let intensite = |g: LueurGpu, i: usize| g.disques[i][3];
        assert_eq!(intensite(calculer(&scene, &[], ECRAN, 10.0, 1.0), 0), 0.0, "invisible à l'instant de l'ajout");
        assert!((intensite(calculer(&scene, &[], ECRAN, 10.0 + FONDU_AJOUT / 2.0, 1.0), 0) - 0.125).abs() < 1e-4, "à moitié à mi-fondu");
        assert_eq!(intensite(calculer(&scene, &[], ECRAN, 10.0 + FONDU_AJOUT, 1.0), 0), 0.25, "pleine au bout du fondu");
        // La retirée se dessine APRÈS les présentes, et s'éteint.
        let sortantes = [(Element::PAR_DEFAUT, 20.0)];
        let g = calculer(&scene, &sortantes, ECRAN, 20.0, 1.0);
        assert_eq!((g.nombre, intensite(g, 1)), (2.0, 0.25), "encore pleine à l'instant du retrait");
        assert_eq!(intensite(calculer(&scene, &sortantes, ECRAN, 20.0 + FONDU_AJOUT, 1.0), 1), 0.0, "éteinte au bout du fondu");
        // Le fondu de toute la scène.
        assert_eq!(intensite(calculer(&scene, &[], ECRAN, 50.0, 0.5), 0), 0.125);
    }

    /// ⭐ Le cycle de couleurs passe par chaque étape, puis REVIENT à la principale.
    #[test]
    fn le_cycle_de_couleurs_passe_par_chaque_etape_et_revient() {
        let mut e = Element { couleur: [1.0, 0.0, 0.0], cycle_couleur: true, duree_couleur: 9.0, ..Element::PAR_DEFAUT };
        e.etapes[0] = [0.0, 1.0, 0.0];
        e.etapes[1] = [0.0, 0.0, 1.0];
        e.nb_etapes = 2;
        assert_eq!(couleur_a(&e, 0.0), [1.0, 0.0, 0.0]);
        assert_eq!(couleur_a(&e, 3.0), [0.0, 1.0, 0.0]);
        assert_eq!(couleur_a(&e, 6.0), [0.0, 0.0, 1.0]);
        assert_eq!(couleur_a(&e, 7.5), [0.5, 0.0, 0.5], "du bleu VERS la principale, à mi-chemin");
        assert_eq!(couleur_a(&Element { cycle_couleur: false, ..e }, 3.0), [1.0, 0.0, 0.0], "cycle coupé : la principale");
        // Et il passe par le fichier.
        let scene = Scene { elements: vec![e], ..Scene::default() };
        assert_eq!(deserialiser(&serialiser(&scene)), scene);
    }

    /// ⛔ **LE DÉFAUT SIGNALÉ** (2026-10-14) : changer la durée d'une boucle après
    /// une heure faisait SAUTER la lueur sur son chemin. La position juste avant et
    /// juste après le changement doit être LA MÊME, puis avancer au nouveau rythme.
    #[test]
    fn changer_une_duree_ne_fait_pas_sauter_la_lueur() {
        let mut e = Element { x: 0.0, y: 0.0, mouvement: Mouvement::Boucle, periode: 6.0, ..Element::PAR_DEFAUT };
        e.points[..3].copy_from_slice(&[[1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        e.nb_points = 3;
        let ecran = [1000.0, 1000.0];
        let t = 3601.0;
        let avant = position(&e, ecran, t);
        e.changer_duree(Duree::Mouvement, 7.0, t);
        let apres = position(&e, ecran, t);
        assert!((avant[0] - apres[0]).abs() < 0.01 && (avant[1] - apres[1]).abs() < 0.01, "saut : {avant:?} → {apres:?}");
        // Puis un tour complet prend bien 7 s.
        let un_tour = position(&e, ecran, t + 7.0);
        assert!((un_tour[0] - apres[0]).abs() < 0.01 && (un_tour[1] - apres[1]).abs() < 0.01);
        // Les deux autres horloges, même règle.
        let mut c = Element { cycle_couleur: true, nb_etapes: 1, etapes: [[0.0; 3]; MAX_ETAPES], ..Element::PAR_DEFAUT };
        let couleur = couleur_a(&c, t + 0.3);
        c.changer_duree(Duree::Couleur, 31.0, t + 0.3);
        assert_eq!(couleur_a(&c, t + 0.3), couleur);
        // La rotation du fond.
        let mut scene = Scene { rotation: 7.0, ..Scene::default() };
        let angle = angle_en_tours(&scene, t).rem_euclid(1.0);
        scene.ancre_rotation = Ancre { instant: t, phase: angle_en_tours(&scene, t) };
        scene.rotation = -2.0;
        assert!((angle_en_tours(&scene, t).rem_euclid(1.0) - angle).abs() < 1e-9);
    }

    /// ⭐ La figure, son inclinaison et sa rotation propre arrivent au shader ;
    /// la rotation du FOND tourne aussi la figure.
    #[test]
    fn la_figure_et_son_angle_arrivent_au_shader() {
        let e = Element { figure: Figure::Segment, inclinaison: 90.0, longueur: 0.25, rotation_propre: 6.0, ..Element::PAR_DEFAUT };
        let scene = Scene { elements: vec![e], ..Scene::default() };
        let g = calculer(&scene, &[], [1000.0, 500.0], 0.0, 1.0);
        assert_eq!(g.formes[0][0], 2.0, "le code du segment");
        assert!((g.formes[0][1] - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert_eq!(g.formes[0][2], 250.0, "la demi-longueur en pixels de LARGEUR");
        // 6 tr/min : un quart de tour en 2,5 s.
        let g = calculer(&scene, &[], [1000.0, 500.0], 2.5, 1.0);
        assert!((g.formes[0][1] - std::f32::consts::PI).abs() < 1e-4, "{}", g.formes[0][1]);
        // La rotation du fond s'ajoute.
        let tournee = Scene { rotation: 6.0, ..scene.clone() };
        let g = calculer(&tournee, &[], [1000.0, 500.0], 2.5, 1.0);
        assert!((g.formes[0][1] - 1.5 * std::f32::consts::PI).abs() < 1e-4, "{}", g.formes[0][1]);
    }

    #[test]
    fn la_figure_passe_par_le_fichier_et_changer_la_rotation_ne_saute_pas() {
        let mut e = Element { figure: Figure::Rectangle, inclinaison: -30.0, longueur: 0.4, rotation_propre: 3.0, x: -1.5, y: 2.5, ..Element::PAR_DEFAUT };
        let scene = Scene { elements: vec![e], ..Scene::default() };
        assert_eq!(deserialiser(&serialiser(&scene)), scene, "hors écran compris (−150 %, 250 %)");
        let avant = e.tours_propres(1234.5);
        e.changer_rotation_propre(-11.0, 1234.5);
        assert!((e.tours_propres(1234.5) - avant).abs() < 1e-9);
        // Réinitialiser : arrêtée, et revenue à son inclinaison.
        e.reinitialiser_rotation(1300.0);
        assert_eq!(e.rotation_propre, 0.0);
        assert_eq!(e.tours_propres(1300.0), 0.0);
        assert_eq!(e.tours_propres(5000.0), 0.0, "elle ne repart pas");
        // Un segment tout neuf prend une épaisseur de TRAIT.
        let mut d = Element::PAR_DEFAUT;
        d.changer_figure(Figure::Segment);
        assert_eq!(d.rayon, 0.05);
    }

    #[test]
    fn un_effet_se_reconnait_a_son_en_tete() {
        assert!(est_une_scene(&serialiser(&Scene::default())));
        assert!(est_une_scene("\u{feff}1,0.25\n"), "un BOM ne le rend pas illisible");
        assert!(!est_une_scene(""));
        assert!(!est_une_scene("nom,prix\npomme,2\n"));
        assert!(!est_une_scene("7,0.25\n"), "le premier champ est une CASE");
    }

    #[test]
    fn les_figures_se_nomment_dans_les_deux_sens() {
        for (f, nom) in Figure::TOUTES {
            assert_eq!(Figure::depuis_nom(nom), Some(f));
            assert_eq!(f.nom(), nom);
            assert_eq!(Figure::depuis_code(f32::from(f.code())), f);
        }
    }

    #[test]
    fn les_mouvements_se_nomment_dans_les_deux_sens() {
        for (m, nom) in Mouvement::TOUS {
            assert_eq!(Mouvement::depuis_nom(nom), Some(m));
            assert_eq!(m.nom(), nom);
            assert_eq!(Mouvement::depuis_code(f32::from(m.code())), m);
        }
    }

    #[test]
    fn le_bloc_gpu_tient_en_registres_entiers() {
        // L'en-tête, quatre tableaux d'un registre par lueur, puis les arrêts.
        assert_eq!(std::mem::size_of::<LueurGpu>(), 16 + 4 * 16 * MAX_ELEMENTS + 16 * MAX_ELEMENTS * MAX_ARRETS);
    }
}
