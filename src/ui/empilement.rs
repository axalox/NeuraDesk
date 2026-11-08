//! ⭐ LE REPLIEMENT DES ICÔNES DES BARRES LATÉRALES « COMME LE FAISAIT UNITY »
//! — DEMANDE EXPLICITE (2026-10-31). Trois essais inventés ont été refusés ;
//! celui-ci TRANSPOSE l'algorithme d'origine du lanceur de Unity 7 (Canonical,
//! GPL v3, `launcher/Launcher.cpp` : `RenderArgs` et `FillRenderArg`, et
//! `unity-shared/IconRenderer.cpp` pour la perspective), lu sur
//! git.launchpad.net/unity.
//!
//! Le principe : quand la somme de TOUTES les icônes de la barre (épinglées,
//! applications ouvertes, applets, espaces) dépasse sa hauteur, un SEUIL de
//! repliement est calculé ; chaque icône qui le franchit se replie
//! progressivement sur une hauteur d'icône : sa place fond jusqu'au quart
//! (`folding_constant = 0.25`), elle bascule en arrière autour de l'axe
//! horizontal jusqu'à 1 radian, vue en perspective. Au SURVOL de la barre, tout
//! se déplie en 200 ms (`hover_animation_`) ; la barre, alors plus haute que
//! l'écran, se fait défiler à la molette (`launcher_drag_delta_`).
//!
//! Ce module est le CALCUL PUR, testable ; `ui::dock` lui donne les
//! emplacements de la barre et en tire les rectangles, le shader
//! (`gfx::quad_renderer`, champ `bascule`) dessine la perspective.

use std::sync::Mutex;

pub const DUREE_MIN_MS: u16 = 50;
pub const DUREE_MAX_MS: u16 = 1000;

/// `folding_constant` : la place d'une icône repliée, en fraction de sa taille.
const PLI: f32 = 0.25;
/// `magic_constant` de Unity — « empêche le bas de “marcher” ».
const CONSTANTE_MAGIQUE: f32 = 1.3;
/// `NEG_FOLDED_ANGLE`, en radians (la bascule maximale).
pub const BASCULE_MAX: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Empilement {
    pub actif: bool,
    /// Durée du dépliement au survol (`ANIM_DURATION` = 200 ms chez Unity).
    pub duree_ms: u16,
}

impl Empilement {
    pub const DEFAUT: Self = Self { actif: false, duree_ms: 200 };

    /// « actif,durée_ms ».
    pub fn serialiser(&self) -> String {
        format!("{},{}", u8::from(self.actif), self.duree_ms)
    }

    /// Lit `serialiser` ; un champ illisible garde sa valeur par défaut, les
    /// valeurs sont ramenées dans leurs bornes. Un 3ᵉ champ (essai précédent)
    /// est ignoré.
    pub fn charger(texte: &str) -> Self {
        let mut r = Self::DEFAUT;
        let champs: Vec<&str> = texte.trim().split(',').map(str::trim).collect();
        if let Some(a) = champs.first() {
            r.actif = *a == "1";
        }
        if let Some(d) = champs.get(1).and_then(|d| d.parse::<u32>().ok()) {
            r.duree_ms = d.clamp(u32::from(DUREE_MIN_MS), u32::from(DUREE_MAX_MS)) as u16;
        }
        r
    }
}

/// Avance une progression LINÉAIRE `t` (0..1) vers `cible` en `duree_ms`
/// pendant `dt` secondes — les animations de Unity sont linéaires.
pub fn avancer(t: f32, cible: bool, dt: f32, duree_ms: u16) -> f32 {
    let pas = dt * 1000.0 / f32::from(duree_ms.max(1));
    if cible { (t + pas).min(1.0) } else { (t - pas).max(0.0) }
}

/// Un élément de la barre, tel que la barre NON repliée le pose : son étendue
/// (`etendue`, la taille d'une icône ou la hauteur d'un espace) et la place
/// qu'il occupe avec l'écart qui le suit (`place`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Element {
    pub etendue: f32,
    pub place: f32,
}

/// Ce que devient un élément : son CENTRE vertical, la hauteur qu'il occupe
/// réellement dans la barre (`demi` × 2, qui fond jusqu'au quart), sa
/// progression de repliement (0 déplié, 1 replié) et sa BASCULE en radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub centre: f32,
    pub demi: f32,
    pub pli: f32,
    pub bascule: f32,
    /// Dans le groupe replié du HAUT (passé au-dessus par le défilement) —
    /// décide de l'ordre de dessin (voir `Dock::ordre_de_dessin_plie`).
    pub en_haut: bool,
}

/// La barre repliée — `None` quand tout tient (`folded_ = false`).
///
/// `debut` : ordonnée du haut du premier élément (relative au haut de la
/// barre) ; `longueur` : hauteur de la barre (`launcher_size`) ; `fin_max` :
/// ce que le contenu peut atteindre sans déborder ; `survol` : progression du
/// dépliement (`hover_progress`) ; `defile` : pixels de contenu (déplié)
/// défilés au-dessus du haut de la barre, ≥ 0.
///
/// ⭐ LE HAUT AUSSI — DEMANDE EXPLICITE (2026-10-31) : « peux-tu le mettre
/// aussi en haut quand les icônes sont défilées ». Ce que le défilement a fait
/// passer au-dessus du haut n'en sort pas : il s'y replie, en MIROIR du bas
/// (bascule NÉGATIVE : c'est le bord du bas qui part en arrière). L'élément
/// à cheval sur la ligne se replie à mesure qu'il la franchit, sur une hauteur
/// d'icône — la même rampe que le seuil du bas. Le reste suit `RenderArgs` de
/// Unity, qui ne repliait que le bas. Au survol, tout se déplie, la barre
/// défilée d'autant : c'est la barre ordinaire, défilée.
pub fn plier(elements: &[Element], debut: f32, longueur: f32, fin_max: f32, survol: f32, defile: f32) -> Option<Vec<Place>> {
    if fin_depliee(elements, debut) <= fin_max {
        return None;
    }
    let reference = elements.iter().map(|e| e.etendue).fold(0.0, f32::max).max(1.0);
    let constante = PLI + (1.0 - PLI) * survol;
    let bascule_max = BASCULE_MAX * (1.0 - survol);
    let defile = defile.max(0.0);
    // Hauts des éléments dans la barre dépliée, depuis le premier.
    let mut hauts = Vec::with_capacity(elements.len());
    let mut cumul = 0.0;
    for e in elements {
        hauts.push(cumul);
        cumul += e.place;
    }
    let au_dessus = hauts.iter().position(|&h| h >= defile).unwrap_or(elements.len());
    let mut y = debut - defile * survol;
    let mut places = Vec::with_capacity(elements.len());
    for (e, &h) in elements[..au_dessus].iter().zip(&hauts) {
        let pli = ((defile - h) / reference).clamp(0.0, 1.0);
        let place = replier(e, &mut y, pli, constante, -bascule_max, true);
        places.push(place);
    }
    places.extend(plier_le_bas(&elements[au_dessus..], y, longueur, reference, constante, bascule_max));
    Some(places)
}

/// Bas du dernier élément de la barre dépliée.
fn fin_depliee(elements: &[Element], debut: f32) -> f32 {
    debut + elements.iter().map(|e| e.place).sum::<f32>() - elements.last().map_or(0.0, |e| e.place - e.etendue)
}

/// `FillRenderArg` pour un élément de progression de repliement `pli`, posé à
/// `y` (avancé d'autant que l'élément occupe).
fn replier(e: &Element, y: &mut f32, pli: f32, constante: f32, bascule_max: f32, en_haut: bool) -> Place {
    let c = e.etendue;
    let ecart = e.place - e.etendue;
    let replie = c * constante;
    let demi = replie / 2.0 + (c / 2.0 - replie / 2.0) * (1.0 - pli);
    let place = Place { centre: *y + demi, demi, pli, bascule: bascule_max * pli, en_haut };
    *y += 2.0 * demi + ecart * (1.0 - pli) + ecart * constante * pli;
    place
}

/// `RenderArgs` : le repliement du BAS des `elements`, le premier posé à
/// `debut`. Rien n'est replié quand ils tiennent dans `longueur`.
fn plier_le_bas(elements: &[Element], debut: f32, longueur: f32, reference: f32, constante: f32, bascule_max: f32) -> Vec<Place> {
    let tient = fin_depliee(elements, debut) <= longueur;
    // Le seuil recule à mesure que la somme dépasse la barre.
    let mut somme = debut;
    let mut seuil = longueur - reference / 2.5;
    for e in elements {
        somme += e.place;
        seuil -= (somme - longueur).clamp(0.0, e.place * CONSTANTE_MAGIQUE) * PLI;
    }
    let mut y = debut;
    elements
        .iter()
        .map(|e| {
            let pli = if tient { 0.0 } else { ((y + reference - seuil) / reference).clamp(0.0, 1.0) };
            replier(e, &mut y, pli, constante, bascule_max, false)
        })
        .collect()
}

/// ⭐ Où va un élément ATTACHÉ à une icône basculée (sa pastille) : son
/// rectangle `rect`, posé par rapport à l'icône ENTIÈRE `icone`, projeté par
/// la même bascule `angle` que le shader applique à l'icône (voir
/// `gfx::quad_renderer`, `QuadInstance::bascule` : caméra à une demi-hauteur
/// d'écran, profondeur triplée, recul de 10 px par radian). Le rectangle rendu
/// est à redessiner avec la même `bascule`, qui le raccourcit lui-même.
pub fn projeter_attache(rect: [f32; 4], icone: [f32; 4], angle: f32, hauteur_ecran: f32) -> [f32; 4] {
    let (cx, cy) = (icone[0] + icone[2] * 0.5, icone[1] + icone[3] * 0.5);
    let (ox, oy) = (rect[0] + rect[2] * 0.5 - cx, rect[1] + rect[3] * 0.5 - cy);
    let camera = hauteur_ecran * 0.5;
    let z = 3.0 * (-oy * angle.sin() + 10.0 * angle.abs());
    let f = camera / (camera + z).max(1.0);
    let (x, y) = (cx + ox * f, cy + oy * angle.cos() * f);
    let (w, h) = (rect[2] * f, rect[3] * f);
    [x - w * 0.5, y - h * 0.5, w, h]
}

/// Borne du défilement au survol : `[min(0, longueur − contenu déplié), 0]`
/// (`launcher_drag_delta_min_`).
pub fn delta_min(elements: &[Element], debut: f32, longueur: f32) -> f32 {
    (longueur - (debut + elements.iter().map(|e| e.place).sum::<f32>())).min(0.0)
}

/// `SetDndDelta` : en entrant dans la barre par sa partie repliée, le
/// défilement qui amène l'élément `ancre`, déplié, sous la souris (`souris_y`)
/// — sans faire remonter la fin au-dessus du bas de la barre.
pub fn delta_d_entree(elements: &[Element], debut: f32, longueur: f32, ancre: usize, souris_y: f32) -> f32 {
    let avant: f32 = elements[..ancre].iter().map(|e| e.place).sum();
    let centre = debut + avant + elements[ancre].etendue / 2.0;
    (souris_y - centre).clamp(delta_min(elements, debut, longueur), 0.0)
}

static REGLAGES: Mutex<Empilement> = Mutex::new(Empilement::DEFAUT);

pub fn reglages() -> Empilement {
    *REGLAGES.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn modifier(f: impl FnOnce(&mut Empilement)) -> Empilement {
    let mut r = REGLAGES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(&mut r);
    *r
}

/// Applique un réglage venu de la page des Réglages — `false` s'il est inconnu
/// ou illisible (rien n'est alors modifié).
pub fn regler(r: &mut Empilement, reglage: &str, valeur: &str) -> bool {
    let nombre = valeur.parse::<f32>().ok().filter(|v| v.is_finite());
    match (reglage, nombre) {
        ("actif", _) => {
            r.actif = valeur == "1";
            true
        }
        ("duree", Some(ms)) => {
            r.duree_ms = (ms.round() as i32).clamp(i32::from(DUREE_MIN_MS), i32::from(DUREE_MAX_MS)) as u16;
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` icônes de 54 px (la tuile de Unity) espacées de 5.
    fn icones(n: usize) -> Vec<Element> {
        vec![Element { etendue: 54.0, place: 59.0 }; n]
    }

    #[test]
    fn une_barre_qui_tient_ne_se_replie_pas() {
        assert!(plier(&icones(10), 5.0, 1000.0, 995.0, 0.0, 0.0).is_none());
    }

    #[test]
    fn le_haut_reste_deplie_le_bas_se_replie_et_tient_dans_la_barre() {
        let places = plier(&icones(30), 5.0, 1000.0, 995.0, 0.0, 0.0).expect("déborde");
        let premiere = places[0];
        assert_eq!((premiere.pli, premiere.bascule), (0.0, 0.0));
        assert_eq!(premiere.centre, 5.0 + 27.0, "à sa place");
        let derniere = *places.last().unwrap();
        assert_eq!(derniere.pli, 1.0);
        assert!((derniere.demi - 54.0 * PLI / 2.0).abs() < 1e-4, "le quart : {}", derniere.demi);
        assert!((derniere.bascule - BASCULE_MAX).abs() < 1e-4);
        assert!(derniere.centre + derniere.demi <= 1000.0, "tient : {}", derniere.centre + derniere.demi);
        // Monotone : le repliement ne recule jamais en descendant.
        assert!(places.windows(2).all(|p| p[1].pli >= p[0].pli && p[1].centre > p[0].centre));
    }

    #[test]
    fn au_survol_tout_se_deplie_a_sa_place() {
        let places = plier(&icones(30), 5.0, 1000.0, 995.0, 1.0, 0.0).unwrap();
        for (k, p) in places.iter().enumerate() {
            assert_eq!(p.bascule, 0.0);
            assert!((p.demi - 27.0).abs() < 1e-4);
            assert!((p.centre - (5.0 + 59.0 * k as f32 + 27.0)).abs() < 1e-3, "{k} : {}", p.centre);
        }
        // Et le défilement les remonte d'autant.
        let defile = plier(&icones(30), 5.0, 1000.0, 995.0, 1.0, 100.0).unwrap();
        assert!((defile[3].centre - (places[3].centre - 100.0)).abs() < 1e-3);
        // À mi-survol, la bascule est à moitié.
        let mi = plier(&icones(30), 5.0, 1000.0, 995.0, 0.5, 0.0).unwrap();
        assert!(mi.last().unwrap().bascule <= BASCULE_MAX * 0.5 + 1e-4);
    }

    #[test]
    fn defile_le_haut_se_replie_en_miroir_et_le_bas_suit() {
        let e = icones(30);
        let places = plier(&e, 5.0, 1000.0, 995.0, 0.0, 59.0 * 10.0 + 20.0).unwrap();
        // Les dix premières sont passées au-dessus : repliées, bascule négative.
        for p in &places[..10] {
            assert!((p.pli - 1.0).abs() < 1e-4 && (p.bascule + BASCULE_MAX).abs() < 1e-4, "{p:?}");
        }
        // Celle à cheval sur la ligne se replie à proportion de ce qui a passé.
        assert!((places[10].pli - 20.0 / 54.0).abs() < 1e-4, "{:?}", places[10]);
        assert!(places[10].bascule < 0.0);
        // Rien ne sort par le haut, tout reste dans l'ordre.
        assert!(places[0].centre - places[0].demi >= 5.0 - 1e-4);
        assert!(places.windows(2).all(|p| p[1].centre > p[0].centre));
        // Une icône du milieu, loin des deux lignes, est droite.
        assert_eq!((places[14].pli, places[14].bascule), (0.0, 0.0));
        // Et le bas est toujours replié dans la barre.
        let derniere = *places.last().unwrap();
        assert!(derniere.bascule > 0.0 && derniere.centre + derniere.demi <= 1000.0);
        // Au défilement maximal, le bas ne se replie plus : il tient.
        let max = -delta_min(&e, 5.0, 1000.0);
        let au_bout = plier(&e, 5.0, 1000.0, 995.0, 0.0, max).unwrap();
        assert!(au_bout.last().unwrap().centre + au_bout.last().unwrap().demi <= 1000.0 + 1e-3);
    }

    #[test]
    fn le_delta_d_entree_amene_l_ancre_sous_la_souris_sans_trou_en_bas() {
        let e = icones(30);
        let min = delta_min(&e, 5.0, 1000.0);
        assert!((min - (1000.0 - 5.0 - 59.0 * 30.0)).abs() < 1e-3);
        let d = delta_d_entree(&e, 5.0, 1000.0, 20, 900.0);
        assert!((d - (900.0 - (5.0 + 59.0 * 20.0 + 27.0))).abs() < 1e-3);
        assert_eq!(delta_d_entree(&e, 5.0, 1000.0, 29, 10.0), min, "borné");
        assert_eq!(delta_d_entree(&e, 5.0, 1000.0, 0, 900.0), 0.0, "jamais positif");
    }

    #[test]
    fn une_pastille_suit_la_bascule_de_son_icone() {
        let icone = [0.0, 1000.0, 96.0, 96.0];
        let pastille = [40.0, 1100.0, 16.0, 6.0];
        assert_eq!(projeter_attache(pastille, icone, 0.0, 2000.0), pastille, "droite : inchangée");
        // Basculée vers l'arrière par le haut : le bas avance, la pastille
        // remonte vers le centre (raccourci) et grossit un peu (plus proche).
        let p = projeter_attache(pastille, icone, 1.0, 2000.0);
        let centre = p[1] + p[3] * 0.5;
        assert!(centre < 1103.0 && centre > 1048.0, "{p:?}");
        assert!(p[2] > 16.0, "plus proche : {p:?}");
        // Miroir : basculée par le bas, elle recule.
        let q = projeter_attache(pastille, icone, -1.0, 2000.0);
        assert!(q[2] < 16.0, "plus loin : {q:?}");
    }

    #[test]
    fn la_progression_met_la_duree_reglee() {
        assert!((avancer(0.0, true, 0.1, 200) - 0.5).abs() < 1e-5);
        assert_eq!(avancer(0.9, true, 0.2, 200), 1.0);
        assert_eq!(avancer(0.1, false, 0.2, 200), 0.0);
    }

    #[test]
    fn serialiser_puis_charger_rend_les_memes_reglages() {
        let e = Empilement { actif: true, duree_ms: 400 };
        assert_eq!(Empilement::charger(&e.serialiser()), e);
        assert_eq!(Empilement::charger(""), Empilement::DEFAUT);
        assert_eq!(Empilement::charger("1,99999").duree_ms, DUREE_MAX_MS);
        assert_eq!(Empilement::charger("1,250,0.9"), Empilement { actif: true, duree_ms: 250 }, "ancien format");
    }

    #[test]
    fn regler_route_chaque_reglage_et_refuse_l_inconnu() {
        let mut e = Empilement::DEFAUT;
        assert!(regler(&mut e, "actif", "1") && e.actif);
        assert!(regler(&mut e, "duree", "300") && e.duree_ms == 300);
        let avant = e;
        assert!(!regler(&mut e, "duree", "x"));
        assert!(!regler(&mut e, "taille", "80"));
        assert_eq!(e, avant);
    }
}
