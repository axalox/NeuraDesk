//! **LE VOL** : l'allure de chaque espèce et la géométrie pure du comportement
//! (fuir le pointeur, choisir où se poser, se diriger). Tout en pixels
//! d'écran ; les vitesses et rayons sont en pixels logiques, multipliés par
//! l'échelle de l'écran.

use super::Espece;

/// Un rectangle d'écran en flottants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zone {
    pub gauche: f32,
    pub haut: f32,
    pub droite: f32,
    pub bas: f32,
}

impl Zone {
    pub fn retrecie(self, marge: f32) -> Zone {
        let (cx, cy) = ((self.gauche + self.droite) / 2.0, (self.haut + self.bas) / 2.0);
        Zone {
            gauche: (self.gauche + marge).min(cx),
            haut: (self.haut + marge).min(cy),
            droite: (self.droite - marge).max(cx),
            bas: (self.bas - marge).max(cy),
        }
    }

    pub fn contient(self, (x, y): (f32, f32)) -> bool {
        x >= self.gauche && x <= self.droite && y >= self.haut && y <= self.bas
    }

    fn borner(self, (x, y): (f32, f32)) -> (f32, f32) {
        (x.clamp(self.gauche, self.droite), y.clamp(self.haut, self.bas))
    }
}

/// La façon de voler d'une espèce.
#[derive(Clone, Copy, Debug)]
pub struct Allure {
    /// Vitesse de croisière et de fuite, pixels logiques par seconde.
    pub vitesse: f32,
    pub vitesse_fuite: f32,
    /// Rapidité à corriger sa vitesse (par seconde) : la mouche vire sec.
    pub agilite: f32,
    /// Écart maximal du cap à chaque virage (radians), et l'intervalle entre deux virages (s).
    pub virage: f32,
    pub entre_virages: (f32, f32),
    /// Le pointeur plus près que ça (pixels logiques) : il s'envole.
    pub rayon_fuite: f32,
    /// Durée d'une pose, en secondes.
    pub pose: (f32, f32),
    /// Elle trottine quand elle est posée.
    pub marche: bool,
    /// Battement d'ailes affiché en vol, et respiration posée (par seconde).
    pub battement_vol: f32,
    pub battement_pose: f32,
}

pub fn allure(espece: Espece) -> Allure {
    match espece {
        Espece::Mouche(_) => Allure {
            vitesse: 380.0,
            vitesse_fuite: 900.0,
            agilite: 7.0,
            virage: 1.1,
            entre_virages: (0.12, 0.45),
            rayon_fuite: 55.0,
            pose: (3.0, 12.0),
            marche: true,
            battement_vol: 23.0,
            battement_pose: 0.0,
        },
        Espece::Moustique(_) => Allure {
            vitesse: 150.0,
            vitesse_fuite: 400.0,
            agilite: 3.0,
            virage: 0.75,
            entre_virages: (0.3, 0.8),
            rayon_fuite: 50.0,
            pose: (4.0, 15.0),
            marche: false,
            battement_vol: 19.0,
            battement_pose: 0.0,
        },
        Espece::Papillon(_) => Allure {
            vitesse: 125.0,
            vitesse_fuite: 320.0,
            agilite: 2.4,
            virage: 0.9,
            entre_virages: (0.4, 1.1),
            rayon_fuite: 70.0,
            pose: (5.0, 18.0),
            marche: false,
            battement_vol: 7.0,
            battement_pose: 0.33,
        },
    }
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// Le pointeur frôle-t-il l'insecte ?
pub fn frole(insecte: (f32, f32), curseur: (f32, f32), rayon: f32) -> bool {
    distance(insecte, curseur) < rayon
}

/// La direction (unitaire) qui éloigne du pointeur. Pile dessus : vers le haut.
pub fn direction_de_fuite(insecte: (f32, f32), curseur: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (insecte.0 - curseur.0, insecte.1 - curseur.1);
    let d = dx.hypot(dy);
    if d < 1e-3 {
        (0.0, -1.0)
    } else {
        (dx / d, dy / d)
    }
}

fn tourner((x, y): (f32, f32), angle: f32) -> (f32, f32) {
    let (s, c) = angle.sin_cos();
    (x * c - y * s, x * s + y * c)
}

/// Où fuir : à `loin` pixels, à l'opposé du pointeur, sans quitter `zone`.
/// Si le mur est derrière l'insecte, il part en biais : on garde le point
/// le plus éloigné du pointeur.
pub fn point_de_fuite(insecte: (f32, f32), curseur: (f32, f32), zone: Zone, loin: f32) -> (f32, f32) {
    let dir = direction_de_fuite(insecte, curseur);
    let mut meilleur = zone.borner(insecte);
    for angle in [0.0f32, 0.6, -0.6, 1.2, -1.2, 1.8, -1.8] {
        let (dx, dy) = tourner(dir, angle);
        let p = zone.borner((insecte.0 + dx * loin, insecte.1 + dy * loin));
        if distance(p, curseur) > distance(meilleur, curseur) + 1.0 {
            meilleur = p;
        }
        if distance(p, curseur) >= loin * 0.8 {
            return p;
        }
    }
    meilleur
}

/// Où se poser : au hasard dans `zone`, à plus de `ecart` du pointeur —
/// sinon il se poserait sous le pointeur pour en repartir aussitôt.
/// `hasard` rend des nombres de 0 à 1.
pub fn point_de_pose(zone: Zone, curseur: (f32, f32), ecart: f32, hasard: &mut impl FnMut() -> f32) -> (f32, f32) {
    let mut meilleur = (zone.gauche, zone.haut);
    for _ in 0..24 {
        let p = (
            zone.gauche + hasard() * (zone.droite - zone.gauche),
            zone.haut + hasard() * (zone.bas - zone.haut),
        );
        if distance(p, curseur) >= ecart {
            return p;
        }
        if distance(p, curseur) > distance(meilleur, curseur) {
            meilleur = p;
        }
    }
    // L'écran est plus petit que l'écart : le coin le plus éloigné.
    [(zone.gauche, zone.haut), (zone.droite, zone.haut), (zone.gauche, zone.bas), (zone.droite, zone.bas), meilleur]
        .into_iter()
        .max_by(|a, b| distance(*a, curseur).total_cmp(&distance(*b, curseur)))
        .unwrap_or(meilleur)
}

/// Le point de sortie le plus proche : hors de `ecran`, de `au_dela` pixels.
pub fn point_de_sortie(pos: (f32, f32), ecran: Zone, au_dela: f32) -> (f32, f32) {
    let choix = [
        (pos.0 - ecran.gauche, (ecran.gauche - au_dela, pos.1)),
        (ecran.droite - pos.0, (ecran.droite + au_dela, pos.1)),
        (pos.1 - ecran.haut, (pos.0, ecran.haut - au_dela)),
        (ecran.bas - pos.1, (pos.0, ecran.bas + au_dela)),
    ];
    choix.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).map(|c| c.1).unwrap_or(pos)
}

/// Un point d'entrée juste hors d'un bord de `ecran` (`bord` 0..4, `t` 0..1).
pub fn point_d_entree(ecran: Zone, bord: u32, t: f32, au_dela: f32) -> (f32, f32) {
    let x = ecran.gauche + t * (ecran.droite - ecran.gauche);
    let y = ecran.haut + t * (ecran.bas - ecran.haut);
    match bord % 4 {
        0 => (ecran.gauche - au_dela, y),
        1 => (ecran.droite + au_dela, y),
        2 => (x, ecran.haut - au_dela),
        _ => (x, ecran.bas + au_dela),
    }
}

/// La nouvelle vitesse : vers `cible` (en ralentissant à l'approche), le cap
/// dévié de `deviation`, repoussée par le pointeur s'il est près.
#[allow(clippy::too_many_arguments)]
pub fn piloter(
    pos: (f32, f32),
    vit: (f32, f32),
    cible: (f32, f32),
    vitesse_max: f32,
    agilite: f32,
    deviation: f32,
    curseur: (f32, f32),
    rayon_fuite: f32,
    dt: f32,
) -> (f32, f32) {
    let d = distance(pos, cible);
    let mut voulu = if d < 1e-3 {
        (0.0, 0.0)
    } else {
        let ralentir = (d / (rayon_fuite * 1.5)).min(1.0).max(0.25);
        // Moins dévié à l'approche : il finit par atterrir où il visait.
        let dev = deviation * (d / (rayon_fuite * 3.0)).min(1.0);
        let dir = tourner(((cible.0 - pos.0) / d, (cible.1 - pos.1) / d), dev);
        (dir.0 * vitesse_max * ralentir, dir.1 * vitesse_max * ralentir)
    };
    let dc = distance(pos, curseur);
    if dc < rayon_fuite * 2.5 {
        let (fx, fy) = direction_de_fuite(pos, curseur);
        let force = (1.0 - dc / (rayon_fuite * 2.5)) * vitesse_max * 2.0;
        voulu = (voulu.0 + fx * force, voulu.1 + fy * force);
    }
    let k = (agilite * dt).min(1.0);
    (vit.0 + (voulu.0 - vit.0) * k, vit.1 + (voulu.1 - vit.1) * k)
}

/// L'angle `vers` atteint depuis `de` par le plus court chemin, d'au plus `pas`.
pub fn approcher_angle(de: f32, vers: f32, part: f32) -> f32 {
    let mut d = (vers - de) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    de + d * part.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ECRAN: Zone = Zone { gauche: 0.0, haut: 0.0, droite: 1920.0, bas: 1040.0 };

    #[test]
    fn le_pointeur_frole_sous_le_rayon_seulement() {
        assert!(frole((100.0, 100.0), (130.0, 140.0), 55.0), "50 px : frôlé");
        assert!(!frole((100.0, 100.0), (140.0, 140.0), 55.0), "56,6 px : non");
    }

    #[test]
    fn la_fuite_s_eloigne_du_pointeur() {
        let (dx, dy) = direction_de_fuite((500.0, 500.0), (480.0, 500.0));
        assert!((dx - 1.0).abs() < 1e-6 && dy.abs() < 1e-6, "pointeur à gauche : vers la droite");
        let p = point_de_fuite((500.0, 500.0), (480.0, 500.0), ECRAN, 300.0);
        assert!(p.0 > 700.0, "{p:?}");
        assert!(ECRAN.contient(p));
        // Acculée au bord gauche, pointeur à droite : elle part en biais, plus loin.
        let p = point_de_fuite((5.0, 500.0), (40.0, 500.0), ECRAN, 300.0);
        assert!(ECRAN.contient(p));
        assert!(distance(p, (40.0, 500.0)) > 200.0, "{p:?}");
    }

    #[test]
    fn elle_se_pose_dans_la_zone_et_loin_du_pointeur() {
        let mut graine = 7u32;
        let mut hasard = move || {
            graine = graine.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (graine >> 8) as f32 / (1u32 << 24) as f32
        };
        let zone = ECRAN.retrecie(40.0);
        for _ in 0..200 {
            let p = point_de_pose(zone, (960.0, 520.0), 165.0, &mut hasard);
            assert!(zone.contient(p), "{p:?}");
            assert!(distance(p, (960.0, 520.0)) >= 165.0, "{p:?}");
        }
        // Une zone minuscule sous le pointeur : le coin le plus éloigné.
        let petite = Zone { gauche: 0.0, haut: 0.0, droite: 50.0, bas: 50.0 };
        assert_eq!(point_de_pose(petite, (0.0, 0.0), 500.0, &mut hasard), (50.0, 50.0));
    }

    #[test]
    fn la_sortie_passe_par_le_bord_le_plus_proche() {
        assert_eq!(point_de_sortie((30.0, 500.0), ECRAN, 60.0), (-60.0, 500.0));
        assert_eq!(point_de_sortie((900.0, 1000.0), ECRAN, 60.0), (900.0, 1100.0));
        for bord in 0..4 {
            assert!(!ECRAN.contient(point_d_entree(ECRAN, bord, 0.5, 60.0)));
        }
    }

    #[test]
    fn piloter_mene_a_la_cible_et_evite_le_pointeur() {
        let a = allure(Espece::Mouche(0));
        let (mut pos, mut vit) = ((100.0f32, 100.0f32), (0.0f32, 0.0f32));
        let cible = (800.0, 600.0);
        let mut plus_pres = f32::MAX;
        for _ in 0..600 {
            vit = piloter(pos, vit, cible, a.vitesse, a.agilite, 0.0, (5000.0, 5000.0), a.rayon_fuite, 1.0 / 60.0);
            pos = (pos.0 + vit.0 / 60.0, pos.1 + vit.1 / 60.0);
            plus_pres = plus_pres.min(distance(pos, cible));
        }
        assert!(plus_pres < 4.0, "elle passe sur la cible : {plus_pres}");
        // Le pointeur juste devant : la vitesse le contourne au lieu d'y foncer.
        let v = piloter((100.0, 100.0), (0.0, 0.0), (400.0, 100.0), a.vitesse, 1000.0, 0.0, (130.0, 100.0), a.rayon_fuite, 1.0);
        assert!(v.0 < a.vitesse * 0.5, "freinée par le pointeur : {v:?}");
    }

    #[test]
    fn l_angle_prend_le_plus_court_chemin() {
        let a = approcher_angle(3.0, -3.0, 1.0);
        assert!((a - (-3.0 + std::f32::consts::TAU)).abs() < 1e-5, "{a}");
        assert!((approcher_angle(0.0, 1.0, 0.5) - 0.5).abs() < 1e-6);
    }
}
