//! ⭐ **LA BIBLIOTHÈQUE D'EFFETS DE FERMETURE.**
//!
//! DEMANDE EXPLICITE (2026-11-04) : « proposer ces effets » — ceux du mod
//! Windhawk `windows-animations` — en les réimplémentant DANS NeuraDesk
//! (voie B, voir `window-hook/README.md`). NeuraDesk capture déjà la fenêtre
//! et la dessine dans sa surcouche ; il ne manquait qu'un choix de trajectoire.
//!
//! Chaque effet est une fonction PURE de `t` (0 → 1). Aucun état, donc chacun
//! se teste sans fenêtre ni périphérique D3D (voir les tests en bas).
//!
//! **UN EFFET REND UNE OU PLUSIEURS `Partie`** : une pose, le sous-rectangle
//! de la texture qu'elle montre (`QuadInstance::uv`), et le rayon de ses coins.
//! Les effets d'un seul quadrilatère en rendent une ; « éclatement » en rend
//! seize (tuiles), « génie » seize bandes. À `t = 0` les parties SE RECOLLENT
//! exactement en la fenêtre : c'est ce qui évite un saut au moment où la vraie
//! fenêtre disparaît.

/// Une image à un instant donné. `rect` : x, y, largeur, hauteur en pixels
/// écran ; `angle` en radians ; `opacite` de 1 (opaque) à 0 (invisible).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Pose {
    pub rect: [f32; 4],
    pub angle: f32,
    pub opacite: f32,
}

/// **UNE IMAGE D'UN EFFET** : ce qu'il faut pour dessiner un quadrilatère.
/// `uv` est le sous-rectangle de la texture qu'il montre, `(u0, v0, u1, v1)`
/// (voir `QuadInstance::uv`) ; `rayon` est le rayon de ses coins, en pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Partie {
    pub pose: Pose,
    pub uv: [f32; 4],
    pub rayon: f32,
}

/// Le style de fermeture, réglable — voir `config::load_effet_fermeture_style`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Effet {
    /// Le retour vers l'icône de l'application (le comportement historique).
    #[default]
    Retour,
    /// S'éteindre comme un tube cathodique : en ligne, puis en point.
    Eteindre,
    /// S'effacer sur place.
    Fondu,
    /// Tomber en tournant, en s'effaçant.
    Tomber,
    /// Se contracter vers son propre centre.
    Zoom,
    /// Traverser la fenêtre de part en part (agrandir et restaurer, voir
    /// `Geste::Agrandir` / `Geste::Restaurer`). ⛔ Il n'est PAS dans le réglage :
    /// la fenêtre ne disparaît pas, elle change de place ; l'extinction ou la
    /// chute n'auraient aucun sens pour ce geste.
    Etirer,
    /// **VOLER EN ÉCLATS** : la fenêtre se découpe en poussière qui
    /// s'écarte du centre en s'effaçant (le « Square Shatter » du mod).
    Eclater,
    /// **ASPIRÉE VERS L'ICÔNE** : seize bandes horizontales se rétractent vers
    /// le point d'arrivée, avec un léger décalage du haut vers le bas.
    Genie,
    // ⭐ **LES DIX EFFETS DU MOD QUI MANQUAIENT** (2026-11-05, demande :
    // « il semble manquer des effets par rapport à ce que l'addon ajoute sur
    // windhawk ») — réimplémentés en quadrilatères (voir chaque fonction).
    // ⛔ Codes 8 à 17 : NE PAS RENUMÉROTER, ils vivent dans le dossier de
    // l'utilisateur.
    /// **THANOS** : les cases partent en poussière depuis le coin bas-droit,
    /// emportées par un vent qui tourne.
    Thanos,
    /// **DISSOLUTION** : chaque case s'efface quand le bruit de Perlin qui la
    /// couvre l'atteint — une brûlure organique.
    Perlin,
    /// **GLITCH** : des bandes se décalent horizontalement, puis la fenêtre se
    /// désagrège en cases qui disparaissent au hasard.
    Glitch,
    /// **COULURE** : des colonnes se mettent à couler vers le bas de l'écran,
    /// s'allongent puis s'effacent, chacune avec son retard.
    Coulure,
    /// **ENCRE** : une tache organique se répand depuis le centre (ou s'y
    /// résorbe) et découvre les cases une à une.
    Encre,
    /// **BRÛLURE** : le coin bas-droit se consume vers l'intérieur, les cases
    /// s'effaçant quand le front les atteint.
    Brulure,
    /// **FISSURE** : une fêlure part du centre et gagne toute la fenêtre en
    /// laissant les cases se détacher.
    Fissure,
    /// **MIRAGE** : les cases montrent une texture qui ondule et se resserre
    /// vers le centre en s'effaçant — un mirage de chaleur.
    Mirage,
    /// **POINTILLÉ** : des cases apparaissent/disparaissent en trame selon un
    /// seuil, comme une floraison de points.
    Pointille,
    /// **GONFLEMENT** : la fenêtre enfle de 18 % en s'évanouissant.
    Gonfler,
    /// **LA BASCULE** — l'animation d'Alt+Tab (voir le mod, « Window switch ») :
    /// la fenêtre grossit de 94 % à sa taille en apparaissant. ⛔ **INTERNE** :
    /// elle ne se choisit pas dans les Réglages (absente de `FORMES`), elle
    /// n'est jouée que par `Evenement::Bascule`.
    Bascule,
}

/// Le code rangé dans `effet_fermeture_style.txt` — ⛔ **NE PAS RENUMÉROTER** :
/// il vit dans le dossier de configuration de l'utilisateur.
impl Effet {
    /// Tous les styles, pour les tests de propriétés ci-dessous. `Etirer` en
    /// est absent : il ne disparaît pas, il traverse (voir sa doc).
    #[cfg(test)]
    pub(super) const TOUS: [Effet; 18] = [
        Effet::Retour,
        Effet::Eteindre,
        Effet::Fondu,
        Effet::Tomber,
        Effet::Zoom,
        Effet::Eclater,
        Effet::Genie,
        Effet::Thanos,
        Effet::Perlin,
        Effet::Glitch,
        Effet::Coulure,
        Effet::Encre,
        Effet::Brulure,
        Effet::Fissure,
        Effet::Mirage,
        Effet::Pointille,
        Effet::Gonfler,
        Effet::Bascule,
    ];

    pub(super) fn code(self) -> u32 {
        match self {
            Effet::Retour => 0,
            Effet::Eteindre => 1,
            Effet::Fondu => 2,
            Effet::Tomber => 3,
            Effet::Zoom => 4,
            // Jamais rangé : `Etirer` ne se choisit pas (voir sa doc).
            Effet::Etirer => 5,
            Effet::Eclater => 6,
            Effet::Genie => 7,
            Effet::Thanos => 8,
            Effet::Perlin => 9,
            Effet::Glitch => 10,
            Effet::Coulure => 11,
            Effet::Encre => 12,
            Effet::Brulure => 13,
            Effet::Fissure => 14,
            Effet::Mirage => 15,
            Effet::Pointille => 16,
            Effet::Gonfler => 17,
            // ⛔ INTERNE : jamais rangé dans les Réglages (voir `Moment::formes`).
            Effet::Bascule => 18,
        }
    }

    /// Un code inconnu (fichier d'une version plus récente) rend le style par
    /// défaut : une fermeture doit toujours s'animer.
    pub(super) fn depuis_code(code: u32) -> Self {
        match code {
            1 => Effet::Eteindre,
            2 => Effet::Fondu,
            3 => Effet::Tomber,
            4 => Effet::Zoom,
            5 => Effet::Etirer,
            6 => Effet::Eclater,
            7 => Effet::Genie,
            8 => Effet::Thanos,
            9 => Effet::Perlin,
            10 => Effet::Glitch,
            11 => Effet::Coulure,
            12 => Effet::Encre,
            13 => Effet::Brulure,
            14 => Effet::Fissure,
            15 => Effet::Mirage,
            16 => Effet::Pointille,
            17 => Effet::Gonfler,
            18 => Effet::Bascule,
            _ => Effet::Retour,
        }
    }

}

/// **LES FORMES PROPOSÉES**, dans l'ordre des boutons des Réglages —
/// l'ouverture, la fermeture et la réduction les offrent toutes. L'étirement
/// n'y est pas : il ne va qu'à agrandir et restaurer (voir `AVEC_ETIREMENT`).
pub(super) const FORMES: [Effet; 17] = [
    Effet::Retour,
    Effet::Eteindre,
    Effet::Fondu,
    Effet::Tomber,
    Effet::Zoom,
    Effet::Eclater,
    Effet::Genie,
    Effet::Thanos,
    Effet::Perlin,
    Effet::Glitch,
    Effet::Coulure,
    Effet::Encre,
    Effet::Brulure,
    Effet::Fissure,
    Effet::Mirage,
    Effet::Pointille,
    Effet::Gonfler,
];

/// Les formes d'un AGRANDISSEMENT ou d'une RESTAURATION : les mêmes, plus
/// l'étirement, qui est la forme par défaut de ce geste — la seule qui montre
/// la fenêtre passer d'un rectangle à l'autre au lieu de disparaître.
pub(super) const AVEC_ETIREMENT: [Effet; 18] = [
    Effet::Etirer,
    Effet::Retour,
    Effet::Eteindre,
    Effet::Fondu,
    Effet::Tomber,
    Effet::Zoom,
    Effet::Eclater,
    Effet::Genie,
    Effet::Thanos,
    Effet::Perlin,
    Effet::Glitch,
    Effet::Coulure,
    Effet::Encre,
    Effet::Brulure,
    Effet::Fissure,
    Effet::Mirage,
    Effet::Pointille,
    Effet::Gonfler,
];

/// Adoucissement aux deux bouts (même courbe que les fondus, voir
/// `super::opacite_du_fondu`).
fn adouci(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Le centre d'un rectangle.
fn centre(r: [f32; 4]) -> (f32, f32) {
    (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)
}

/// Un rectangle centré sur `c`, de taille `(l, h)`.
fn centre_sur(c: (f32, f32), l: f32, h: f32) -> [f32; 4] {
    [c.0 - l / 2.0, c.1 - h / 2.0, l, h]
}

impl Effet {
    /// **CETTE FORME NE DOIT PAS S'ÉVANOUIR À L'OUVERTURE.** L'ouverture rejoue
    /// la fermeture À L'ENVERS (`parties(1 - t)`) : une opacité qui décroît
    /// pour disparaître devient, à l'envers, une fenêtre fantôme qui met toute
    /// l'animation à devenir opaque.
    ///
    /// MESURÉ (2026-11-05, Notepad++, forme Zoom, 2 000 ms) : à 0,44 s l'image
    /// n'était qu'à **39 %** d'opacité — on lisait les fenêtres d'en dessous à
    /// travers le texte, et à 1,04 s encore à 77 %. SIGNALÉ : « la vraie
    /// fenêtre se voit ».
    ///
    /// Pour ces formes, le DÉPLACEMENT raconte déjà l'apparition : l'image reste
    /// opaque du premier au dernier instant. Les formes dont l'opacité EST
    /// l'effet (Fondu, Éteindre, Bascule) gardent la leur.
    pub(super) fn opaque_a_l_ouverture(self) -> bool {
        matches!(self, Effet::Zoom | Effet::Gonfler)
    }

    /// La pose de l'image à `t` (0 = la fenêtre intacte, 1 = disparue).
    pub(super) fn pose(self, t: f32, fenetre: [f32; 4], arrivee: [f32; 4]) -> Pose {
        let t = t.clamp(0.0, 1.0);
        match self {
            Effet::Retour => {
                Pose { rect: retour(fenetre, arrivee, t), angle: 0.0, opacite: 1.0 }
            }
            Effet::Fondu => Pose { rect: fenetre, angle: 0.0, opacite: 1.0 - adouci(t) },
            Effet::Eteindre => eteindre(t, fenetre),
            Effet::Tomber => tomber(t, fenetre),
            Effet::Zoom => {
                let e = t * t;
                let k = 1.0 - 0.92 * e;
                Pose { rect: centre_sur(centre(fenetre), fenetre[2] * k, fenetre[3] * k), angle: 0.0, opacite: 1.0 - e }
            }
            // Les deux bouts comptent : la fenêtre part de SA place et arrive
            // sur le rectangle que Windows lui donnera, sans à-coup.
            Effet::Etirer => Pose { rect: glisser(fenetre, arrivee, adouci(t)), angle: 0.0, opacite: 1.0 },
            // ⭐ **LA BASCULE** (Alt+Tab) : 94 % → 100 % en apparaissant. Le
            // `1 - t` la joue donc, à l'envers, comme un grandissement depuis
            // 94 % — c'est l'effet « Window switch » du mod.
            Effet::Bascule => {
                let k = 1.0 - 0.06 * t;
                Pose {
                    rect: centre_sur(centre(fenetre), fenetre[2] * k, fenetre[3] * k),
                    angle: 0.0,
                    opacite: 1.0 - t,
                }
            }
            // ⭐ **LE GONFLEMENT** : la seule forme qui GRANDIT (voir le test
            // `aucun_effet_ne_regrandit_la_fenetre`, qui l'exempte) : +18 % en
            // s'évanouissant, la signature du « Swell » du mod.
            Effet::Gonfler => {
                let u = 1.0 - t;
                let gonfle = 1.0 - u * u * u;
                let echelle = 1.0 + 0.18 * gonfle;
                Pose {
                    rect: centre_sur(centre(fenetre), fenetre[2] * echelle, fenetre[3] * echelle),
                    angle: 0.0,
                    opacite: 1.0 - gonfle,
                }
            }
            // ⛔ **L'ENVELOPPE** : ces effets se dessinent en PLUSIEURS parties
            // (voir `parties`). Leur pose n'est qu'un repli — la fenêtre, qui
            // s'efface — et ne sert qu'aux tests d'ensemble.
            Effet::Eclater
            | Effet::Genie
            | Effet::Thanos
            | Effet::Perlin
            | Effet::Glitch
            | Effet::Coulure
            | Effet::Encre
            | Effet::Brulure
            | Effet::Fissure
            | Effet::Mirage
            | Effet::Pointille => Pose { rect: fenetre, angle: 0.0, opacite: 1.0 - adouci(t) },
        }
    }

    /// **LES IMAGES À DESSINER** — une par quadrilatère. Les effets d'un seul
    /// quadrilatère en rendent une ; éclatement et génie en rendent seize.
    pub(super) fn parties(self, t: f32, fenetre: [f32; 4], arrivee: [f32; 4], rayon: f32) -> Vec<Partie> {
        let t = t.clamp(0.0, 1.0);
        match self {
            Effet::Eclater => eclater(t, fenetre),
            Effet::Genie => genie(t, fenetre, arrivee),
            Effet::Thanos => thanos(t, fenetre),
            Effet::Perlin => perlin(t, fenetre),
            Effet::Glitch => glitch(t, fenetre),
            Effet::Coulure => coulure(t, fenetre),
            Effet::Encre => encre(t, fenetre),
            Effet::Brulure => brulure(t, fenetre),
            Effet::Fissure => fissure(t, fenetre),
            Effet::Mirage => mirage(t, fenetre),
            Effet::Pointille => pointille(t, fenetre),
            _ => vec![Partie { pose: self.pose(t, fenetre, arrivee), uv: super::UV_PLEINE, rayon }],
        }
    }
}

/// Les bandes horizontales du génie.
const BANDES: usize = 16;

/// **L'ÉCLATEMENT (« Square Shatter »)** : la fenêtre se découpe en poussière
/// qui s'écarte du centre — chaque case d'autant plus loin qu'elle en est
/// éloignée, et selon sa propre force — en s'effaçant linéairement. C'est la
/// version fine du mod : ⛔ la rotation a disparu (son `DrawBlock` ne tourne
/// pas), c'est la DENSITÉ qui fait l'éclatement.
fn eclater(t: f32, fenetre: [f32; 4]) -> Vec<Partie> {
    /// Écartement de base, en pixels écran.
    const ECART_BASE: f32 = 200.0;
    /// Écartement ajouté par la force de la case (0 → 1).
    const ECART_FORCE: f32 = 1000.0;

    let e = 1.0 - (1.0 - t).powi(5);
    let (cx, cy) = centre(fenetre);
    let opacite = (1.0 - t).max(0.0);
    let cases = decouper(fenetre, COTE_MOD);
    let mut parties = Vec::with_capacity(cases.len());
    for case in cases {
        let (tx, ty) = centre(case.src);
        let mut dx = tx - cx;
        let mut dy = ty - cy;
        let d = (dx * dx + dy * dy).sqrt();
        if d > 0.1 {
            dx /= d;
            dy /= d;
        }
        let force = hasard(case.i, case.j, 1);
        let voyage = e * (ECART_BASE + force * ECART_FORCE);
        let bruit_x = hasard(case.i, case.j, 2) * 2.0 - 1.0;
        let bruit_y = hasard(case.i, case.j, 3) * 2.0 - 1.0;
        let rect = [
            case.src[0] + (dx * voyage + bruit_x * voyage * 0.4),
            case.src[1] + (dy * voyage + bruit_y * voyage * 0.4),
            case.src[2],
            case.src[3],
        ];
        parties.push(partie(case, rect, opacite));
    }
    parties
}

/// **LE GÉNIE** : seize bandes horizontales aspirées vers le point d'arrivée.
/// Chacune se rétracte en largeur ET se rapproche du point, avec un léger
/// retard du haut vers le bas. ⛔ L'étirement courbe n'est pas dans le
/// nuanceur : les bandes restent droites, et c'est leur EMPILEMENT qui donne
/// le courant. Le retard doit rester PETIT (et les bandes garder leur hauteur)
/// : trop décalées, elles se séparent et l'on voit des lames qui tombent au
/// lieu d'une fenêtre aspirée.
fn genie(t: f32, fenetre: [f32; 4], arrivee: [f32; 4]) -> Vec<Partie> {
    /// Retard entre la première et la dernière bande, en fraction de la durée.
    const RETARD: f32 = 0.18;
    /// Largeur restante à l'arrivée : une lame, pas un point (sinon elle
    /// disparaîtrait avant d'avoir fini de voyager).
    const LAME: f32 = 0.07;
    /// Instant où l'effacement commence.
    const EFFACEMENT: f32 = 0.8;

    let (cx, _) = centre(fenetre);
    let (bx, by) = centre(arrivee);
    let opacite = if t <= EFFACEMENT { 1.0 } else { adouci((1.0 - t) / (1.0 - EFFACEMENT)) };
    let h = fenetre[3] / BANDES as f32;
    let mut parties = Vec::with_capacity(BANDES);
    for i in 0..BANDES {
        let rang = i as f32 / BANDES as f32;
        // Chaque bande a SA part du temps : celle du haut part la première.
        let ti = ((t - RETARD * rang) / (1.0 - RETARD)).clamp(0.0, 1.0);
        let e = adouci(ti);
        let y = fenetre[1] + h * (i as f32 + 0.5);
        parties.push(Partie {
            pose: Pose {
                rect: centre_sur(
                    (cx + (bx - cx) * e, y + (by - y) * e),
                    fenetre[2] * (1.0 - (1.0 - LAME) * e),
                    // ⛔ LA HAUTEUR SUIT L'ÉCARTEMENT, pas une autre courbe :
                    // l'espacement des centres vaut `h × (1 - e)`, donc la même
                    // hauteur les laisse CONTIGUËS. Plus hautes, elles se
                    // recouvrent et l'on voit des rayures (chaque bande montre
                    // une tranche différente par-dessus la précédente).
                    // Le recouvrement de 2 px, NUL au départ (pour que les
                    // bandes se recollent exactement en la fenêtre), ferme les
                    // lisières que le léger retard laisse entre deux bandes.
                    h * (1.0 - e) + 2.0 * e,
                ),
                angle: 0.0,
                opacite,
            },
            // Toute la largeur de la texture : la bande montre une TRANCHE.
            uv: [0.0, rang, 1.0, (i + 1) as f32 / BANDES as f32],
            rayon: 0.0,
        });
    }
    parties
}

/// Interpole centre ET taille entre deux rectangles (`e` déjà adouci).
fn glisser(de: [f32; 4], vers: [f32; 4], e: f32) -> [f32; 4] {
    let (ax, ay) = centre(de);
    let (bx, by) = centre(vers);
    let (cx, cy) = (ax + (bx - ax) * e, ay + (by - ay) * e);
    let (l, h) = (de[2] + (vers[2] - de[2]) * e, de[3] + (vers[3] - de[3]) * e);
    [cx - l / 2.0, cy - h / 2.0, l, h]
}

/// **LE TUBE QUI S'ÉTEINT** : la fenêtre s'aplatit en une ligne horizontale
/// (55 % du temps), puis la ligne se rétracte en un point ; le point s'efface
/// sur les 10 derniers pour cent, sinon il resterait une tache.
fn eteindre(t: f32, fenetre: [f32; 4]) -> Pose {
    /// Part du temps où l'aplatissement vertical est fini.
    const APLATISSEMENT: f32 = 0.55;
    /// L'épaisseur finale : un point, pas un trait (en pixels écran).
    const POINT: f32 = 2.0;

    let (l, h) = if t < APLATISSEMENT {
        let e = adouci(t / APLATISSEMENT);
        (fenetre[2], fenetre[3] * (1.0 - e) + POINT * e)
    } else {
        let e = adouci((t - APLATISSEMENT) / (1.0 - APLATISSEMENT));
        (fenetre[2] * (1.0 - e) + POINT * e, POINT)
    };
    let opacite = if t > 0.9 { (1.0 - t) / 0.1 } else { 1.0 };
    Pose { rect: centre_sur(centre(fenetre), l, h), angle: 0.0, opacite }
}

/// **LA CHUTE** : elle descend en accélérant, penche un peu, et s'efface.
fn tomber(t: f32, fenetre: [f32; 4]) -> Pose {
    /// Hauteur de chute, en fraction de la hauteur de la fenêtre.
    const CHUTE: f32 = 0.45;
    /// Inclinaison maximale, en degrés.
    const PENCHAISON: f32 = 9.0;

    let e = t * t;
    let (cx, cy) = centre(fenetre);
    let angle = PENCHAISON.to_radians() * e;
    Pose {
        rect: centre_sur((cx, cy + fenetre[3] * CHUTE * e), fenetre[2], fenetre[3]),
        angle,
        opacite: 1.0 - e,
    }
}

/// Le retour vers l'icône, en accélérant — l'inverse du vol d'ouverture, qui
/// ralentit en arrivant. (Historiquement dans `fermeture.rs` ; il est ici
/// parce qu'il est devenu UN effet parmi d'autres.)
pub(super) fn retour(fenetre: [f32; 4], origine: [f32; 4], t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    let e = t * t * t;
    let (ax, ay) = centre(fenetre);
    let (bx, by) = centre(origine);
    let (cx, cy) = (ax + (bx - ax) * e, ay + (by - ay) * e);
    let (l, h) = (fenetre[2] + (origine[2] - fenetre[2]) * e, fenetre[3] + (origine[3] - fenetre[3]) * e);
    [cx - l / 2.0, cy - h / 2.0, l, h]
}

// ═══════════════════════════════════════════════════════════════════════════
// ⭐ **LES DIX EFFETS DU MOD WINDHAWK** (« Windows Animations », ReDrag).
// Le mod les peint pixel par pixel ; ici chaque case de la grille est un
// quadrilatère TEXTURÉ (`uv`), et c'est le nuanceur qui l'affiche. Les
// formules sont reprises du mod (`RenderThanos`, `RenderPerlin`,
// `RenderGlitch`, `RenderPixelMelt`, `RenderInkSplash`, `RenderScorch`,
// `RenderSplinter`, `RenderMirage`, `RenderStipple`, `RenderSwell`), traduites
// en fonctions PURES de `t`.
// ⛔ **À `t = 0` TOUTES LES CASES SONT À LEUR PLACE** (position, angle,
// uv, opacité) : c'est l'invariant `chaque_effet_se_recompose_en_la_fenetre`,
// et ce qui évite un saut quand la vraie fenêtre disparaît.
// ═══════════════════════════════════════════════════════════════════════════

/// Une case de la grille de découpe.
#[derive(Clone, Copy, Debug)]
struct Case {
    /// Son rectangle dans la fenêtre.
    src: [f32; 4],
    /// Le sous-rectangle de la texture qu'elle montre.
    uv: [f32; 4],
    /// Ses indices dans la grille — les hasards en dépendent.
    i: u32,
    j: u32,
}

/// ⭐ DEMANDE EXPLICITE (2026-11-05) : « sur une grande fenêtre les effets sont
/// ratés, augmente et mets la résolution qui est dans le plugin ». MESURÉ contre
/// le mod (`windows-animations.wh.cpp`) : il dissout en blocs de **5 px**
/// (`close.shatter_block_size`), soit 191 552 particules sur une fenêtre
/// maximisée. L'ancien plafond de 1 200 cases élargissait donc les cases jusqu'à
/// **64,5 px** — une mosaïque qui s'envole, pas de la poussière.
///
/// Le plafond ne sert plus qu'aux fenêtres démesurées : à 5 px, un écran 4K
/// (331 776 cases) passe encore.
const COTE_MOD: f32 = 5.0;

/// **LE PLAFOND DU NOMBRE DE CASES** — chaque case est un quadrilatère texturé
/// redessiné à chaque image. Il ne mord que sur les très grandes fenêtres.
const MAXI_CASES: usize = 350_000;

/// **DÉCOUPE LA FENÊTRE EN CASES** d'environ `cote` pixels de côté. Les bords
/// droit et bas sont rognés pour que les cases SE RECOLLENT exactement en la
/// fenêtre (l'aire sommée vaut la sienne à `t = 0`).
///
/// Le côté grandit tant que la grille dépasse `MAXI_CASES` : une fenêtre 4K
/// découpée à 5 px en demanderait plus que le plafond.
fn decouper(fenetre: [f32; 4], cote: f32) -> Vec<Case> {
    let (l, h) = (fenetre[2].max(1.0), fenetre[3].max(1.0));
    let mut cote = cote.max(1.0);
    loop {
        let colonnes = (l / cote).ceil().max(1.0);
        let lignes = (h / cote).ceil().max(1.0);
        if (colonnes * lignes) as usize <= MAXI_CASES {
            break;
        }
        cote *= 1.2;
    }
    // La grille est connue d'avance : une seule allocation, pas douze
    // réallocations par image — c'est appelé soixante fois par seconde.
    let colonnes = (l / cote).ceil().max(1.0) as usize;
    let lignes = (h / cote).ceil().max(1.0) as usize;
    let mut cases = Vec::with_capacity(colonnes * lignes);
    let mut y = 0.0;
    let mut i = 0u32;
    while y < h - 0.5 {
        let ch = cote.min(h - y);
        let mut x = 0.0;
        let mut j = 0u32;
        while x < l - 0.5 {
            let cw = cote.min(l - x);
            cases.push(Case {
                src: [fenetre[0] + x, fenetre[1] + y, cw, ch],
                uv: [x / l, y / h, (x + cw) / l, (y + ch) / h],
                i,
                j,
            });
            x += cw;
            j += 1;
        }
        y += ch;
        i += 1;
    }
    cases
}

/// Un hasard STABLE et SANS ÉTAT pour une case : même `(i, j, graine)`, même
/// valeur à chaque image — sinon la poussière « grésillerait » d'une frame à
/// l'autre. Les multiplicateurs sont ceux du mod (`pseudo_hash`).
fn hasard(i: u32, j: u32, graine: u32) -> f32 {
    let mut x = i.wrapping_mul(73856093) ^ j.wrapping_mul(19349663) ^ graine.wrapping_mul(83492791);
    x ^= x >> 13;
    x = x.wrapping_mul(1274126177);
    x ^= x >> 16;
    (x & 0xFFFF) as f32 / 65535.0
}

/// Lissage entre deux bornes (le `smoothstep` du mod).
fn lissage(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0).max(1e-5)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Une image d'une case : mêmes `uv`, poignée à sa place.
fn partie(case: Case, rect: [f32; 4], opacite: f32) -> Partie {
    Partie {
        pose: Pose { rect, angle: 0.0, opacite },
        uv: case.uv,
        rayon: 0.0,
    }
}

/// **THANOS** : la poussière se désintègre depuis le coin bas-droit. Chaque
/// case a son heure de départ (le front) ; ensuite elle dérive selon un vent
/// (`wind`) puis une courbe (`curve`), et s'efface sur `VIE`.
/// Formules : `RenderThanos` du mod.
fn thanos(t: f32, fenetre: [f32; 4]) -> Vec<Partie> {
    /// Durée de vie d'une case, en fraction de l'animation (0,4 dans le mod).
    const VIE: f32 = 0.35;
    /// Le front n'atteint le coin haut-gauche qu'à 60 % du temps.
    const DEBUT_MAX: f32 = 0.6;
    /// Épaisseur du front (bruit ±).
    const ONDE: f32 = 0.15;

    let cases = decouper(fenetre, COTE_MOD);
    let (l, h) = (fenetre[2].max(1.0), fenetre[3].max(1.0));
    let diagonale = (l * l + h * h).sqrt().max(1.0);
    let mut parties = Vec::with_capacity(cases.len());
    for case in cases {
        // Distance au coin bas-droit, comme le mod (il part de là).
        let dist = (l - (case.src[0] - fenetre[0])) + (h - (case.src[1] - fenetre[1]));
        let base = dist / diagonale * DEBUT_MAX;
        let onde = (hasard(case.i, case.j, 5) - 0.5) * ONDE;
        let debut = (base + onde).clamp(0.0, 0.65);
        let local = ((t - debut) / VIE).clamp(0.0, 1.0);
        let opacite = 1.0 - local;
        let bruit_x = hasard(case.i, case.j, 6) * 2.0 - 1.0;
        let bruit_y = hasard(case.i, case.j, 7) * 2.0 - 1.0;
        let remous = ((case.src[1] - fenetre[1]) / h - 0.5) * 2.0;
        let vent_x = l * 0.5 + bruit_x * (l * 0.2);
        let vent_y = h * 0.2 + remous * (h * 0.3) + bruit_y * (h * 0.3);
        let courbe_x = bruit_y * (l * 0.4);
        let courbe_y = -bruit_x * (h * 0.4);
        let rect = [
            case.src[0] + vent_x * local + courbe_x * local * local,
            case.src[1] + vent_y * local + courbe_y * local * local,
            case.src[2],
            case.src[3],
        ];
        parties.push(partie(case, rect, opacite));
    }
    parties
}

/// **DISSOLUTION (Perlin)** : un champ de bruit de trois octaves décale l'heure
/// de disparition de chaque case — la brûlure gagne par taches organiques.
/// Formules : `RenderPerlin` du mod.
fn perlin(t: f32, fenetre: [f32; 4]) -> Vec<Partie> {
    /// Durée de vie d'une case, en fraction de l'animation.
    const VIE: f32 = 0.15;
    /// Échelle du bruit, en pixels (celle du mod).
    const ECHELLE: f32 = 150.0;

    let cases = decouper(fenetre, COTE_MOD);
    let mut parties = Vec::with_capacity(cases.len());
    for case in cases {
        let debut = bruit_du_perlin(case.src[0] - fenetre[0], case.src[1] - fenetre[1], ECHELLE) * 0.9;
        let local = ((t - debut) / VIE).clamp(0.0, 1.0);
        parties.push(partie(case, case.src, 1.0 - local));
    }
    parties
}

/// Le bruit de valeurs à trois octaves du mod (`PerlinNoiseScale = 150`) :
/// interpolation lissée de hasards sur une grille, trois échelles cumulées.
fn bruit_du_perlin(x: f32, y: f32, echelle: f32) -> f32 {
    let mut total = 0.0;
    let mut amplitude = 0.5;
    for octave in 0..3u32 {
        let f = (1u32 << octave) as f32;
        let tx = x / echelle * f;
        let ty = y / echelle * f;
        let ix = tx.floor();
        let iy = ty.floor();
        let fx = tx - ix;
        let fy = ty - iy;
        let ux = fx * fx * (3.0 - 2.0 * fx);
        let uy = fy * fy * (3.0 - 2.0 * fy);
        let (ix, iy) = (ix as i32 as u32, iy as i32 as u32);
        let h = |a: u32, b: u32| ((a.wrapping_mul(73856093) ^ b.wrapping_mul(19349663)) % 10000) as f32 / 10000.0;
        let a = h(ix, iy);
        let b = h(ix.wrapping_add(1), iy);
        let c = h(ix, iy.wrapping_add(1));
        let d = h(ix.wrapping_add(1), iy.wrapping_add(1));
        let (g, dr) = (1.0 - ux, 1.0 - uy);
        total += amplitude * (a * g * dr + b * ux * dr + c * g * uy + d * ux * uy);
        amplitude *= 0.5;
    }
    total
}

/// **GLITCH** : des bandes horizontales se décalent au hasard (les
/// « larmes »), puis la fenêtre se désagrège en cases qui disparaissent par
/// trame. Formules : `RenderGlitch` du mod.
fn glitch(t: f32, fenetre: [f32; 4]) -> Vec<Partie> {
    /// Le nombre de lignes de déchirure du mod.
    const LIGNES: f32 = 70.0;

    let p = adouci(t);
    let p_bande = (p * 18.0) as u32;
    let p_larme = (p * 32.0) as u32;
    let p_micro = (p * 64.0) as u32;
    let p_rupture = (p * 22.0) as u32;
    // La fenêtre s'efface deux fois : une première fois sur toute la durée,
    // une seconde à partir de 55 %.
    let opacite = (1.0 - lissage(0.0, 1.0, p)) * (1.0 - lissage(0.55, 1.0, p));
    let coupure = 1.0 - p * 0.92;
    let cases = decouper(fenetre, COTE_MOD);
    let (l, h) = (fenetre[2].max(1.0), fenetre[3].max(1.0));
    let mut parties = Vec::with_capacity(cases.len());
    for case in cases {
        let ligne = (((case.src[1] - fenetre[1]) / h) * LIGNES) as u32;
        let masque = if hasard(ligne, p_bande, 1) >= 0.55 { 1.0 } else { 0.0 };
        let larme = (hasard(ligne.wrapping_mul(2), p_larme, 2) - 0.5) * 0.10 * masque;
        let micro = (hasard(((case.src[1] - fenetre[1]) / h * 220.0) as u32, p_micro, 3) - 0.5) * 0.025;
        // ⛔ Le décalage est multiplié par `p` : à `t = 0` la fenêtre doit être
        // INTACTE (le mod, lui, démarre avec une larme déjà visible).
        let dx = (larme + micro) * l * p;
        // La trame de rupture : la case survit si son hasard est sous la coupure.
        let survit = hasard(case.j, case.i.wrapping_add(p_rupture.wrapping_mul(17)), 4) <= coupure;
        let alpha = if survit { opacite } else { 0.0 };
        parties.push(partie(case, [case.src[0] + dx, case.src[1], case.src[2], case.src[3]], alpha));
    }
    parties
}

/// **COULURE** : des colonnes coulent vers le bas, s'allongent puis s'effacent,
/// chacune avec son retard et sa vitesse. Formules : `RenderPixelMelt`.
fn coulure(t: f32, fenetre: [f32; 4]) -> Vec<Partie> {
    /// Largeur d'une colonne, en pixels.
    const COLONNE: f32 = 10.0;
    const MAXI: usize = 400;

    // ⛔ **DES COLONNES, PAS DES CASES** : `decouper` rend des carrés ; une
    // colonne doit montrer TOUTE sa hauteur (uv de 0 à 1), sinon l'étirement
    // ne ferait que tirer la tranche du haut. On borne le nombre en élargissant.
    let (l, h) = (fenetre[2].max(1.0), fenetre[3].max(1.0));
    let largeur = (l / MAXI as f32).max(COLONNE);
    let mut cases = Vec::new();
    let mut x = 0.0;
    let mut j = 0u32;
    while x < l - 0.5 {
        let cw = largeur.min(l - x);
        cases.push(Case {
            src: [fenetre[0] + x, fenetre[1], cw, h],
            uv: [x / l, 0.0, (x + cw) / l, 1.0],
            i: 0,
            j,
        });
        x += cw;
        j += 1;
    }
    let mut parties = Vec::with_capacity(cases.len());
    for case in cases {
        let retard = hasard(case.j, case.i, 1) * 0.35;
        let vitesse = 0.5 + hasard(case.j, case.i, 2);
        if t <= retard {
            // Pas encore partie : elle montre TOUTE sa hauteur.
            parties.push(partie(case, case.src, 1.0));
            continue;
        }
        let local = ((t - retard) / (1.0 - retard).max(0.2)).clamp(0.0, 1.0);
        let e = local * local * local;
        let chute = e * h * (0.75 + 0.95 * vitesse);
        let etirement = 1.0 + e * (0.3 + 0.7 * vitesse);
        let opacite = if e > 0.55 { (1.0 - (e - 0.55) / 0.45).powf(1.2) } else { 1.0 };
        parties.push(partie(
            case,
            [case.src[0], case.src[1] + chute, case.src[2], h * etirement],
            opacite,
        ));
    }
    parties
}

/// **ENCRE** : une tache organique se résorbe (ou, joué à l'envers, se répand)
/// depuis le centre — chaque case paraît quand son « bruit » passe sous le
/// front. Formules : `RenderInkSplash`.
fn encre(t: f32, fenetre: [f32; 4]) -> Vec<Partie> {
    // Le mod joue l'effet à l'envers pour une fermeture : p = 1 - progression.
    let p = 1.0 - t;
    let front = p * 1.7 - 0.15;
    let (cx, cy) = centre(fenetre);
    let rayon = (fenetre[2] * fenetre[2] + fenetre[3] * fenetre[3]).sqrt() / 2.0;
    let cases = decouper(fenetre, COTE_MOD);
    let mut parties = Vec::with_capacity(cases.len());
    for case in cases {
        let (x, y) = centre(case.src);
        // Distance au centre, bruitée : la tache n'est pas un cercle parfait.
        let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() / rayon.max(1.0);
        let bruit = d * 1.15 + (hasard(case.i, case.j, 3) - 0.5) * 0.12;
        let reveal = lissage(0.0, 1.0, (bruit - front - 0.04) / -0.08);
        parties.push(partie(case, case.src, reveal));
    }
    parties
}

/// **BRÛLURE** : le coin bas-droit se consume ; chaque case s'efface quand le
/// front, distordu par sa force, l'atteint. Formules : `RenderScorch`.
fn brulure(t: f32, fenetre: [f32; 4]) -> Vec<Partie> {
    let p = adouci(1.0 - t);
    let (l, h) = (fenetre[2].max(1.0), fenetre[3].max(1.0));
    let cases = decouper(fenetre, COTE_MOD);
    let mut parties = Vec::with_capacity(cases.len());
    for case in cases {
        let (x, y) = centre(case.src);
        let dir_x = (x - fenetre[0]) / l;
        let dir_y = (y - fenetre[1]) / h;
        let force = 0.42 + 0.58 * (hasard(case.i, case.j, 1) * 0.55).exp();
        let bruit = hasard(case.i, case.j, 2) * 0.42;
        let dist = ((1.0 - dir_x).powi(2) + (1.0 - dir_y).powi(2)).sqrt() * 1.55;
        let r = p - bruit;
        let reveal = if dist - p * force <= r { 1.0 } else { p.powi(4) };
        parties.push(partie(case, case.src, reveal));
    }
    parties
}

/// **FISSURE** : une fêlure part du centre ; les cases s'effacent quand le
/// front les dépasse, et la fenêtre se remplit à la toute fin.
/// Formules : `RenderSplinter`.
fn fissure(t: f32, fenetre: [f32; 4]) -> Vec<Partie> {
    /// Largeur du liseré de départ, en fraction du temps.
    const LISERÉ: f32 = 0.08;

    let p = adouci(1.0 - t);
    let dehors = lissage(0.0, LISERÉ, p);
    let fin = lissage(1.0 - LISERÉ, 1.0, p);
    let cases = decouper(fenetre, COTE_MOD);
    let mut parties = Vec::with_capacity(cases.len());
    for case in cases {
        let bruit_x = hasard(case.i, case.j, 1);
        let bruit_y = hasard(case.i, case.j, 2);
        let r = p - bruit_x;
        let dur = if bruit_y <= r { 1.0 } else { fin };
        parties.push(partie(case, case.src, dehors * dur));
    }
    parties
}

/// **MIRAGE** : chaque case montre une texture qui ondule et se resserre vers
/// le centre, en s'effaçant. C'est un décalage des `uv`, pas de la place.
/// Formules : `RenderMirage` (elle recadre la source dans l'image, comme ici).
fn mirage(t: f32, fenetre: [f32; 4]) -> Vec<Partie> {
    let p = adouci(1.0 - t);
    if p <= 0.01 {
        return Vec::new();
    }
    let inv = 1.0 - p;
    let (l, h) = (fenetre[2].max(1.0), fenetre[3].max(1.0));
    let cases = decouper(fenetre, 16.0);
    let mut parties = Vec::with_capacity(cases.len());
    for case in cases {
        let (x, y) = centre(case.src);
        let dir_x = (x - fenetre[0]) / l;
        let dir_y = (y - fenetre[1]) / h;
        let force = hasard(case.i, case.j, 1);
        let bruit = hasard(case.i, case.j, 2);
        // Position de la source, ramenée DANS la texture (comme le mod).
        let su = (dir_x + inv * force).clamp(0.0, 1.0);
        let sv = (dir_y + inv * bruit).clamp(0.0, 1.0);
        let bw = case.src[2] / l;
        let bh = case.src[3] / h;
        let u0 = (su - bw / 2.0).clamp(0.0, 1.0 - bw.min(1.0));
        let v0 = (sv - bh / 2.0).clamp(0.0, 1.0 - bh.min(1.0));
        parties.push(Partie {
            pose: Pose { rect: case.src, angle: 0.0, opacite: p },
            uv: [u0, v0, u0 + bw, v0 + bh],
            rayon: 0.0,
        });
    }
    parties
}

/// **POINTILLÉ** : une trame de cases apparaît/disparaît selon un seuil —
/// chaque case porte sa propre force, d'où une floraison de points.
/// Formules : `RenderStipple`.
fn pointille(t: f32, fenetre: [f32; 4]) -> Vec<Partie> {
    let p = adouci(1.0 - t);
    let cases = decouper(fenetre, 10.0);
    let mut parties = Vec::with_capacity(cases.len());
    for case in cases {
        // ⛔ `force` reste ≤ 1 : à `t = 0` (p = 1) le seuil vaut 1/force ≥ 1,
        // donc TOUTES les cases sont visibles — la fenêtre est intacte.
        let force = 0.25 + hasard(case.i, case.j, 1) * 0.75;
        let seuil = p / force;
        let visible = t < 1.0 && hasard(case.i, case.j, 2) <= seuil;
        let opacite = if visible { 1.0 } else { 0.0 };
        parties.push(partie(case, case.src, opacite));
    }
    parties
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICONE: [f32; 4] = [2500.0, 400.0, 48.0, 48.0];
    const FENETRE: [f32; 4] = [300.0, 200.0, 1200.0, 800.0];

    /// **CE QUE COÛTE UNE IMAGE**, sur une fenêtre maximisée de 2560×1440.
    /// Lancé à la main (`cargo test --release cout_d_une_image -- --ignored
    /// --nocapture`) — pas d'assertion de durée, elle dépendrait de la machine.
    ///
    /// Il ne compte que le calcul des cases : le dessin, lui, se mesure dans
    /// l'application. C'est la mesure qui a justifié `MAXI_CASES = 20_000`
    /// (avant : 1 200 cases de 64 px de côté).
    #[test]
    #[ignore]
    fn cout_d_une_image() {
        let fenetre = [0.0, 0.0, 2918.0, 1639.0];
        for effet in [Effet::Thanos, Effet::Perlin, Effet::Eclater, Effet::Pointille, Effet::Mirage] {
            let debut = std::time::Instant::now();
            const IMAGES: u32 = 60;
            let mut cases = 0;
            for image in 0..IMAGES {
                cases = effet.parties(image as f32 / IMAGES as f32, fenetre, ICONE, 0.0).len();
            }
            println!(
                "{effet:?} : {cases} cases, {:.2} ms par image",
                debut.elapsed().as_secs_f64() * 1000.0 / IMAGES as f64
            );
        }
    }

    /// ⛔ **CHAQUE EFFET PART DE LA FENÊTRE INTACTE** : la première image de
    /// l'effet doit se poser EXACTEMENT là où la vraie fenêtre vient d'être
    /// cachée, sinon on voit un saut au moment où elle disparaît.
    #[test]
    fn chaque_effet_part_de_la_fenetre_intacte() {
        for effet in Effet::TOUS {
            let pose = effet.pose(0.0, FENETRE, ICONE);
            assert_eq!(pose.rect, FENETRE, "{effet:?} : position de départ");
            assert_eq!(pose.angle, 0.0, "{effet:?} : angle de départ");
            assert_eq!(pose.opacite, 1.0, "{effet:?} : opacité de départ");
        }
    }

    /// ⛔ **ET CHACUN A DISPARU À LA FIN** : une opacité non nulle laisserait
    /// une vignette de la fenêtre collée à l'écran, ou une tache au point
    /// d'arrivée.
    #[test]
    fn chaque_effet_a_disparu_a_la_fin() {
        for effet in Effet::TOUS {
            let pose = effet.pose(1.0, FENETRE, ICONE);
            if effet == Effet::Retour {
                // Le retour finit SUR l'icône, opaque : c'est l'icône qui prend
                // le relais.
                assert_eq!(pose.rect, ICONE, "retour : point d'arrivée");
                assert_eq!(pose.opacite, 1.0, "retour : il arrive opaque");
                continue;
            }
            assert!(pose.opacite < 0.02, "{effet:?} : opacité finale {}", pose.opacite);
        }
    }

    /// L'extinction finit en POINT, pas en ligne : largeur ET hauteur au
    /// plancher.
    #[test]
    fn l_extinction_finit_en_un_point() {
        let pose = Effet::Eteindre.pose(1.0, FENETRE, ICONE);
        assert!(pose.rect[2] <= 3.0, "largeur finale {}", pose.rect[2]);
        assert!(pose.rect[3] <= 3.0, "hauteur finale {}", pose.rect[3]);
    }

    /// Entre les deux, l'extinction est une LIGNE : largeur intacte, hauteur
    /// au plancher. C'est la moitié de l'effet, et la seule mesure qui la
    /// distingue d'un zoom.
    #[test]
    fn l_extinction_passe_par_une_ligne() {
        // Le point de bascule (55 %) : l'aplatissement est fini, la rétraction
        // horizontale n'a pas commencé.
        let pose = Effet::Eteindre.pose(0.55, FENETRE, ICONE);
        assert!((pose.rect[2] - FENETRE[2]).abs() < 0.5, "largeur {}", pose.rect[2]);
        assert!(pose.rect[3] <= 4.0, "hauteur {}", pose.rect[3]);
    }

    /// Aucun effet ne regrandit la fenêtre : `t` croît, la surface décroît (ou
    /// reste égale). Un effet qui gonfle avant de disparaître se verrait tout
    /// de suite.
    #[test]
    fn aucun_effet_ne_regrandit_la_fenetre() {
        for effet in Effet::TOUS {
            // ⛔ UNE SEULE EXCEPTION, VOULUE : « Gonflement » enfle de 18 % en
            // s'évanouissant — c'est SA signature (le « Swell » du mod).
            if effet == Effet::Gonfler {
                continue;
            }
            let mut avant = f32::MAX;
            for i in 0..=20 {
                let pose = effet.pose(i as f32 / 20.0, FENETRE, ICONE);
                let aire = pose.rect[2] * pose.rect[3];
                assert!(aire <= avant + 1.0, "{effet:?} : l'aire remonte à t={}", i as f32 / 20.0);
                avant = aire;
            }
        }
    }

    /// Les codes sont STABLES (ils vivent dans le dossier de configuration) et
    /// tous distincts.
    #[test]
    fn les_codes_sont_stables_et_distincts() {
        let mut codes: Vec<u32> = Effet::TOUS.iter().map(|e| e.code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), Effet::TOUS.len(), "deux effets partagent un code");
        for effet in Effet::TOUS {
            assert_eq!(Effet::depuis_code(effet.code()), effet, "{effet:?} : aller-retour");
        }
        assert_eq!(Effet::depuis_code(99), Effet::Retour, "un code inconnu rend le défaut");
    }

    /// Le retour part EXACTEMENT de la fenêtre et finit EXACTEMENT sur l'origine.
    #[test]
    fn le_retour_part_de_la_fenetre_et_finit_sur_son_origine() {
        assert_eq!(retour(FENETRE, ICONE, 0.0), FENETRE);
        assert_eq!(retour(FENETRE, ICONE, 1.0), ICONE);
        assert_eq!(retour(FENETRE, ICONE, 3.0), ICONE, "borné");
    }

    /// ⭐ **AGRANDIR ET RESTAURER** : la fenêtre part de SA place, arrive
    /// EXACTEMENT sur le rectangle que Windows lui donnera, et y arrive sans
    /// à-coup (opacité pleine, pas d'angle — elle ne disparaît pas).
    #[test]
    fn l_etirement_part_de_la_fenetre_et_arrive_sur_la_cible() {
        const PLEIN: [f32; 4] = [0.0, 0.0, 3840.0, 2160.0];
        let depart = Effet::Etirer.pose(0.0, FENETRE, PLEIN);
        assert_eq!(depart.rect, FENETRE);
        assert_eq!((depart.opacite, depart.angle), (1.0, 0.0));
        let arrivee = Effet::Etirer.pose(1.0, FENETRE, PLEIN);
        assert_eq!(arrivee.rect, PLEIN);
        assert_eq!(arrivee.opacite, 1.0, "elle ne s'efface pas en s'agrandissant");
        // À mi-chemin elle est ENTRE les deux — ni saut, ni retour en arrière.
        let milieu = Effet::Etirer.pose(0.5, FENETRE, PLEIN).rect;
        assert!(milieu[2] > FENETRE[2] && milieu[2] < PLEIN[2], "largeur {}", milieu[2]);
        assert!(milieu[3] > FENETRE[3] && milieu[3] < PLEIN[3], "hauteur {}", milieu[3]);
    }

    /// Et dans l'autre sens (restaurer) : elle rétrécit, même chemin.
    #[test]
    fn l_etirement_restaure_dans_l_autre_sens() {
        const PLEIN: [f32; 4] = [0.0, 0.0, 3840.0, 2160.0];
        let milieu = Effet::Etirer.pose(0.5, PLEIN, FENETRE).rect;
        assert!(milieu[2] < PLEIN[2] && milieu[2] > FENETRE[2], "largeur {}", milieu[2]);
        assert_eq!(Effet::Etirer.pose(1.0, PLEIN, FENETRE).rect, FENETRE);
    }

    /// ⛔ **LES PARTIES SE RECOLLENT EN LA FENÊTRE À `t = 0`** — même raison
    /// que `chaque_effet_part_de_la_fenetre_intacte`, mais pour les effets qui
    /// découpent : leur aire SOMMÉE doit être celle de la fenêtre, sans trou ni
    /// recouvrement, et aucune ne doit tirer sa texture du dehors.
    #[test]
    fn chaque_effet_se_recompose_en_la_fenetre() {
        for effet in Effet::TOUS {
            let parties = effet.parties(0.0, FENETRE, ICONE, 8.0);
            assert!(!parties.is_empty(), "{effet:?} : aucune image");
            let aire: f32 = parties.iter().map(|p| p.pose.rect[2] * p.pose.rect[3]).sum();
            let attendue = FENETRE[2] * FENETRE[3];
            // ⛔ La tolérance suit la TAILLE : les effets fins découpent en plus
            // de mille cases, et le cumul des arrondis f32 vaut ~2 px² — alors
            // qu'un trou ou un recouvrement réel pèserait une case entière
            // (des centaines de px²).
            let tolerance = (attendue * 5e-5).max(1.0);
            assert!((aire - attendue).abs() < tolerance, "{effet:?} : aire {aire} au lieu de {attendue}");
            for partie in &parties {
                assert_eq!(partie.pose.opacite, 1.0, "{effet:?} : opacité de départ");
                assert_eq!(partie.pose.angle, 0.0, "{effet:?} : angle de départ");
                assert!(
                    partie.uv[0] >= 0.0 && partie.uv[2] <= 1.0 && partie.uv[1] >= 0.0 && partie.uv[3] <= 1.0,
                    "{effet:?} : uv hors de la texture {:?}",
                    partie.uv
                );
            }
        }
    }

    /// Et toutes ont disparu à la fin — sinon un éclat resterait collé à
    /// l'écran ou une bande au bout de son voyage.
    #[test]
    fn chacune_des_parties_a_disparu_a_la_fin() {
        for effet in Effet::TOUS {
            if effet == Effet::Retour {
                continue;
            }
            for partie in effet.parties(1.0, FENETRE, ICONE, 8.0) {
                assert!(partie.pose.opacite < 0.02, "{effet:?} : opacité finale {}", partie.pose.opacite);
            }
        }
    }

    /// L'éclatement découpe VRAIMENT : des dizaines de cases (la densité fait
    /// l'effet), toutes montrant une tranche de texture distincte, et chacune
    /// s'écarte du centre.
    #[test]
    fn l_eclatement_ecarte_une_poussiere_fine() {
        let debut = Effet::Eclater.parties(0.0, FENETRE, ICONE, 8.0);
        assert!(debut.len() > 100, "seulement {} cases : ce n'est pas de la poussière", debut.len());
        let mut uvs: Vec<[u32; 4]> =
            debut.iter().map(|p| p.uv.map(f32::to_bits)).collect();
        uvs.sort_unstable();
        uvs.dedup();
        assert_eq!(uvs.len(), debut.len(), "deux cases montrent la même tranche de texture");
        let fin = Effet::Eclater.parties(1.0, FENETRE, ICONE, 8.0);
        for (avant, apres) in debut.iter().zip(&fin) {
            let da = distance(centre(avant.pose.rect), centre(FENETRE));
            let db = distance(centre(apres.pose.rect), centre(FENETRE));
            assert!(db > da + 50.0, "une case n'a pas bougé : {da} → {db}");
        }
    }

    /// Le génie aspire VRAIMENT vers l'arrivée : à la fin, chaque bande est au
    /// centre de l'icône. Et chaque bande montre une TRANCHE horizontale.
    #[test]
    fn le_genie_aspire_les_bandes_vers_l_arrivee() {
        let debut = Effet::Genie.parties(0.0, FENETRE, ICONE, 8.0);
        assert_eq!(debut.len(), 16);
        // Les bandes se recouvrent exactement en hauteur, sans trou.
        let hauteur: f32 = debut.iter().map(|p| p.pose.rect[3]).sum();
        assert!((hauteur - FENETRE[3]).abs() < 1.0, "hauteur couverte {hauteur}");
        for partie in &debut {
            assert_eq!((partie.uv[0], partie.uv[2]), (0.0, 1.0), "une bande doit être pleine largeur");
        }
        let (bx, by) = centre(ICONE);
        for partie in Effet::Genie.parties(1.0, FENETRE, ICONE, 8.0) {
            let (x, y) = centre(partie.pose.rect);
            assert!((x - bx).abs() < 1.0 && (y - by).abs() < 1.0, "bande arrivée en ({x}, {y}) au lieu de ({bx}, {by})");
        }
    }

    fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
        ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
    }
}
