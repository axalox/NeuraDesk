//! **LES VIGNETTES DESSINÉES** — des icônes vectorielles, pour ce que la police
//! d'icônes n'a pas.
//!
//! ⛔ **POURQUOI CE MODULE EXISTE, ET C'EST UNE MESURE QUI L'A DÉCIDÉ.** Les vignettes
//! prêtes teintaient un glyphe de Segoe Fluent Icons (voir `ui::vignettes_dossiers`).
//! Trois plages de cette police ont été **balayées EN ENTIER** :
//!
//! | plage | ce qu'elle contient |
//! | --- | --- |
//! | `E000..=E6FF` | les commandes multimédia et des tuiles de lettres |
//! | `E700..=E8FF` | **les pictogrammes** — c'est là qu'était tout |
//! | `E900..=EAFF` | du mobilier d'interface : grilles, dispositions, barres |
//!
//! **Il n'y a ni serveur, ni base de données, ni conteneur, ni machine virtuelle dans
//! cette police.** Elle n'en a jamais eu. On les **DESSINE** donc — et à la même règle
//! que le reste : un dessin n'est pas une image embarquée, il est **rendu par nous**,
//! teinté, mis à l'échelle, et rangé dans le même PNG que les glyphes.
//!
//! ⚠️ **LA MÊME DESCRIPTION SERT AU DOCK ET À LA GALERIE.** Le dock rasterise ces
//! formes ici ; la page, elle, les reconstruit en **SVG** (`pour_la_page`). Deux
//! rendus, **une seule description** — sans quoi l'aperçu de la galerie montrerait
//! autre chose que ce qu'il pose, ce que ce dépôt appelle un aperçu menteur.

/// **UNE FORME ÉLÉMENTAIRE.** Ses coordonnées vivent dans un **CARRÉ UNITÉ** : `0.0`
/// à `1.0`, **origine en HAUT À GAUCHE**, comme partout ailleurs dans le dépôt.
///
/// ⚠️ **L'UNITÉ EST CE QUI REND UN DESSIN INDÉPENDANT DE LA TAILLE DE RENDU.** La
/// vignette est rendue à `COTE_VIGNETTE` (96 px), mais la barre l'affiche à ~64 et son
/// épaisseur est RÉGLABLE : un dessin décrit en pixels serait figé à une taille.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Trace {
    /// Un disque PLEIN — la brique de tout le reste.
    Disque { cx: f32, cy: f32, r: f32 },
    /// Une BOÎTE à coins arrondis, pleine. `rayon` à `0.0` donne des coins droits.
    Boite { x: f32, y: f32, l: f32, h: f32, rayon: f32 },
    /// Un TRAIT à bouts RONDS — `epaisseur` est son diamètre.
    Trait { x1: f32, y1: f32, x2: f32, y2: f32, epaisseur: f32 },
    /// Un POLYGONE plein — pour un bouclier, un éclair, une flèche, un toit.
    Polygone { points: &'static [(f32, f32)] },
}

/// **CE QU'UNE FORME FAIT AU DESSIN.** Le RETRAIT compte autant que l'ajout : un
/// cylindre de base de données (un corps, moins deux ellipses), un écran évidé, un
/// anneau de clé, un conteneur à claire-voie se dessinent tous ainsi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Ajouter,
    Retirer,
}

/// Une forme, et ce qu'elle fait. C'est la brique d'un dessin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Forme {
    pub operation: Operation,
    pub trace: Trace,
}

/// Raccourcis de lecture : un tableau de dessin se lit alors comme une DESCRIPTION
/// (« un corps, PLUS une tête, MOINS un creux ») plutôt que comme du code.
pub const fn plus(trace: Trace) -> Forme {
    Forme { operation: Operation::Ajouter, trace }
}
pub const fn moins(trace: Trace) -> Forme {
    Forme { operation: Operation::Retirer, trace }
}

// ---------------------------------------------------------------------
// LES RACCOURCIS DE TRACÉ
// ---------------------------------------------------------------------
//
// ⚠️ **SANS EUX, CHAQUE ICÔNE FERAIT QUINZE LIGNES DE `Trace::…`** et le DESSIN se
// perdrait dans la syntaxe — or c'est lui qu'on vient lire. `boite(0.2, 0.3, 0.6, 0.4)`
// se lit ; `plus(Trace::Boite { x: 0.2, y: 0.3, l: 0.6, h: 0.4, rayon: 0.0 })` se déchiffre.

/// Un rectangle à coins DROITS, ajouté.
const fn boite(x: f32, y: f32, l: f32, h: f32) -> Forme {
    plus(Trace::Boite { x, y, l, h, rayon: 0.0 })
}
/// Une boîte à coins ARRONDIS, ajoutée — `rayon` en unités du carré.
const fn ronde(x: f32, y: f32, l: f32, h: f32, rayon: f32) -> Forme {
    plus(Trace::Boite { x, y, l, h, rayon })
}
/// Un rectangle à coins droits, RETIRÉ — le creux d'un cadre ou d'une rainure.
const fn trou(x: f32, y: f32, l: f32, h: f32) -> Forme {
    moins(Trace::Boite { x, y, l, h, rayon: 0.0 })
}
/// Un disque PLEIN, ajouté.
const fn rond(cx: f32, cy: f32, r: f32) -> Forme {
    plus(Trace::Disque { cx, cy, r })
}
/// Un disque, RETIRÉ — c'est lui qui fait un anneau, un creux, une pupille.
const fn creux(cx: f32, cy: f32, r: f32) -> Forme {
    moins(Trace::Disque { cx, cy, r })
}
/// Un trait à bouts ronds, ajouté.
const fn trait_(x1: f32, y1: f32, x2: f32, y2: f32, epaisseur: f32) -> Forme {
    plus(Trace::Trait { x1, y1, x2, y2, epaisseur })
}
/// Un polygone PLEIN, ajouté.
const fn poly(points: &'static [(f32, f32)]) -> Forme {
    plus(Trace::Polygone { points })
}
/// Un polygone, RETIRÉ.
const fn perce(points: &'static [(f32, f32)]) -> Forme {
    moins(Trace::Polygone { points })
}
/// **LE BÂTI D'UN ÉCRAN** — un cadre arrondi, évidé de `epaisseur`.
///
/// ⚠️ **UNE FONCTION, ET ELLE REND UN TABLEAU — PAS UNE SUITE DE FORMES.** J'ai essayé
/// le macro : il rendrait deux entrées séparées par une virgule, et **une expansion en
/// position d'expression doit produire UNE SEULE expression** — le compilateur refuse,
/// et il a raison. Un tableau, lui, ne s'étale pas dans un littéral de tranche non plus.
///
/// ⇒ Les deux dessins qui ont besoin d'un cadre (`Ordinateur portable`, `Téléphone`)
/// l'écrivent donc **en clair**. C'est deux lignes de plus, et elles disent exactement
/// ce qui se passe — ce qu'un macro refusé ne disait pas.
const fn ecran_creux(x: f32, y: f32, l: f32, h: f32, epaisseur: f32) -> Trace {
    Trace::Boite {
        x: x + epaisseur,
        y: y + epaisseur,
        l: l - epaisseur * 2.0,
        h: h - epaisseur * 2.0,
        rayon: 0.03,
    }
}

/// **LA DISTANCE SIGNÉE D'UN POINT À UNE FORME** — négative DEDANS, positive dehors,
/// en unités du carré (donc à multiplier par la taille pour des pixels).
///
/// ⚠️ **C'EST LE CŒUR DU MODULE, ET IL EST UNIQUE POUR TOUTES LES FORMES.** Une
/// couverture calculée forme par forme donnerait un bord doux ici et dur là, ce qui se
/// lirait comme un défaut de dessin, pas comme une différence de forme.
fn distance_signee(trace: Trace, x: f32, y: f32) -> f32 {
    match trace {
        Trace::Disque { cx, cy, r } => ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r,

        // Boîte arrondie : le décalage classique — on ramène le point dans le quadrant
        // utile, on prend la longueur du dépassement, et on retire le rayon.
        Trace::Boite { x: bx, y: by, l, h, rayon } => {
            let rayon = rayon.min(l * 0.5).min(h * 0.5).max(0.0);
            let demi = (l * 0.5 - rayon, h * 0.5 - rayon);
            let centre = (bx + l * 0.5, by + h * 0.5);
            let q = ((x - centre.0).abs() - demi.0, (y - centre.1).abs() - demi.1);
            let dehors = (q.0.max(0.0).powi(2) + q.1.max(0.0).powi(2)).sqrt();
            dehors + q.0.max(q.1).min(0.0) - rayon
        }

        // Capsule : la distance au SEGMENT, moins le demi-rayon. Un trait à bouts ronds
        // n'est rien d'autre qu'un segment épaissi.
        Trace::Trait { x1, y1, x2, y2, epaisseur } => {
            distance_au_segment(x, y, x1, y1, x2, y2) - epaisseur * 0.5
        }

        // Polygone : le minimum sur les ARÊTES, et le SIGNE donné par l'appartenance.
        // Le lancer de rayon vaut pour un polygone convexe comme pour un concave.
        Trace::Polygone { points } => {
            let mut distance = f32::INFINITY;
            let mut dedans = false;
            let n = points.len();
            for i in 0..n {
                let (x1, y1) = points[i];
                let (x2, y2) = points[(i + 1) % n];
                distance = distance.min(distance_au_segment(x, y, x1, y1, x2, y2));
                // Lancer de rayon VERS LE BAS : on compte les croisements à DROITE.
                if (y1 > y) != (y2 > y) {
                    let croise = x1 + (y - y1) / (y2 - y1) * (x2 - x1);
                    if x < croise {
                        dedans = !dedans;
                    }
                }
            }
            if dedans {
                -distance
            } else {
                distance
            }
        }
    }
}

/// Distance d'un point au SEGMENT `(x1,y1)-(x2,y2)` — le projeté borné, jamais la
/// droite entière : sans la borne, un trait dépasserait de ses propres extrémités.
fn distance_au_segment(x: f32, y: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let longueur2 = dx * dx + dy * dy;
    let t = if longueur2 <= f32::EPSILON {
        0.0
    } else {
        (((x - x1) * dx + (y - y1) * dy) / longueur2).clamp(0.0, 1.0)
    };
    let (px, py) = (x1 + dx * t, y1 + dy * t);
    ((x - px).powi(2) + (y - py).powi(2)).sqrt()
}

/// La couverture d'un point par une forme : `1.0` dedans, `0.0` dehors, et une
/// **transition douce sur UN PIXEL** au bord.
///
/// `pixel` est la taille d'un pixel DANS LE CARRÉ UNITÉ (`1.0 / taille`).
pub fn couverture(trace: Trace, x: f32, y: f32, pixel: f32) -> f32 {
    (0.5 - distance_signee(trace, x, y) / pixel).clamp(0.0, 1.0)
}

/// **LA COUVERTURE COMPOSÉE D'UN POINT PAR TOUT UN DESSIN** — les formes s'appliquent
/// DANS L'ORDRE : `Ajouter` prend le maximum (une union), `Retirer` soustrait.
///
/// ⚠️ **PURE, DONC VÉRIFIABLE SANS RIEN RENDRE** — c'est elle qui porte le dessin, et
/// un retrait oublié, un ordre inversé ou un signe faux ne se verraient qu'à l'œil,
/// sur 64 px, au fond d'un menu.
pub fn couverture_du_dessin(formes: &[Forme], x: f32, y: f32, pixel: f32) -> f32 {
    let mut couv = 0.0f32;
    for forme in formes {
        let c = couverture(forme.trace, x, y, pixel);
        couv = match forme.operation {
            Operation::Ajouter => couv.max(c),
            Operation::Retirer => (couv - c).max(0.0),
        };
    }
    couv
}

#[cfg(test)]
mod tests {
    /// SONDE — **UNE PLANCHE DES DESSINS, POUR LES REGARDER.** Les tests disent
    /// qu'un dessin est visible et unique, jamais qu'il RESSEMBLE à son libellé :
    /// ça, il faut le voir. Écrit une planche PNG (les `n` derniers dessins, dix
    /// par rangée, sur fond gris) dans `NEURADESK_PLANCHE`.
    #[test]
    #[ignore]
    fn planche_des_derniers_dessins() {
        use super::{rendre, CATALOGUE, COTE};
        let n: usize = std::env::var("NEURADESK_PLANCHE_N").ok().and_then(|v| v.parse().ok()).unwrap_or(50);
        let chemin = std::env::var("NEURADESK_PLANCHE").expect("NEURADESK_PLANCHE = chemin du PNG");
        let dessins = &CATALOGUE[CATALOGUE.len() - n.min(CATALOGUE.len())..];
        let (case, par_rangee) = (128u32, 10u32);
        let rangees = (dessins.len() as u32).div_ceil(par_rangee);
        let (l, h) = (case * par_rangee, case * rangees);
        let mut planche = vec![60u8; (l * h * 4) as usize];
        for (k, d) in dessins.iter().enumerate() {
            let (_, _, rgba) = rendre(d.formes, d.couleur, d.degrade);
            let (ox, oy) = ((k as u32 % par_rangee) * case, (k as u32 / par_rangee) * case);
            for y in 0..case {
                for x in 0..case {
                    let (sx, sy) = (x * COTE / case, y * COTE / case);
                    let s = ((sy * COTE + sx) * 4) as usize;
                    let a = rgba[s + 3] as u32;
                    let o = (((oy + y) * l + ox + x) * 4) as usize;
                    for c in 0..3 {
                        planche[o + c] = ((rgba[s + c] as u32 * a + 60 * (255 - a)) / 255) as u8;
                    }
                    planche[o + 3] = 255;
                }
            }
        }
        image::save_buffer(chemin, &planche, l, h, image::ColorType::Rgba8).expect("écriture de la planche");
    }

    use super::*;

    /// **LE DISQUE — LE CAS ÉTALON.** Au centre : plein. Loin : rien. Sur le bord : la
    /// moitié. C'est ce dernier point qui prouve l'anti-aliasing, et il est le seul
    /// qu'un dessin « tout ou rien » raterait.
    #[test]
    fn un_disque_est_plein_au_centre_vide_dehors_et_doux_sur_le_bord() {
        let trace = Trace::Disque { cx: 0.5, cy: 0.5, r: 0.3 };
        let pixel = 1.0 / 96.0;
        assert_eq!(couverture(trace, 0.5, 0.5, pixel), 1.0, "le centre est PLEIN");
        assert_eq!(couverture(trace, 0.05, 0.05, pixel), 0.0, "un coin est VIDE");
        let bord = couverture(trace, 0.8, 0.5, pixel);
        assert!((bord - 0.5).abs() < 0.02, "sur le bord, la couverture vaut un demi : {bord}");
    }

    /// **LE RETRAIT — ET C'EST LUI QUI DESSINE LA MOITIÉ DES ICÔNES.** Un anneau est un
    /// disque MOINS un disque plus petit ; sans le retrait, il n'y aurait qu'un aplat.
    #[test]
    fn un_retrait_creuse_vraiment_la_forme() {
        let pixel = 1.0 / 96.0;
        let plein = [plus(Trace::Disque { cx: 0.5, cy: 0.5, r: 0.4 })];
        let creux = [
            plus(Trace::Disque { cx: 0.5, cy: 0.5, r: 0.4 }),
            moins(Trace::Disque { cx: 0.5, cy: 0.5, r: 0.2 }),
        ];
        assert_eq!(couverture_du_dessin(&plein, 0.5, 0.5, pixel), 1.0, "prémisse : le plein est plein");
        assert_eq!(couverture_du_dessin(&creux, 0.5, 0.5, pixel), 0.0, "le centre est CREUSÉ");
        assert_eq!(
            couverture_du_dessin(&creux, 0.85, 0.5, pixel),
            1.0,
            "…et la couronne reste pleine"
        );
    }

    /// **LE POLYGONE — L'APPARTENANCE ET LA DISTANCE.** Un carré : dedans plein, dehors
    /// vide. Et le signe doit suivre l'appartenance, sinon un retrait de polygone
    /// enlèverait la mauvaise moitié.
    #[test]
    fn un_polygone_est_plein_dedans_et_vide_dehors() {
        const CARRE: &[(f32, f32)] = &[(0.25, 0.25), (0.75, 0.25), (0.75, 0.75), (0.25, 0.75)];
        let trace = Trace::Polygone { points: CARRE };
        let pixel = 1.0 / 96.0;
        assert_eq!(couverture(trace, 0.5, 0.5, pixel), 1.0, "le centre est dedans");
        assert_eq!(couverture(trace, 0.1, 0.5, pixel), 0.0, "à gauche, dehors");
        // Le signe : un point juste DEDANS a une distance négative, juste DEHORS positive.
        assert!(distance_signee(trace, 0.28, 0.5) < 0.0, "dedans : distance négative");
        assert!(distance_signee(trace, 0.20, 0.5) > 0.0, "dehors : distance positive");
    }

    /// **LA BOÎTE ARRONDIE — LE RAYON SE BORNE À LA DEMI-DIMENSION.** Un rayon plus grand
    /// que la moitié de la boîte produirait un décalage NÉGATIF, donc une boîte qui
    /// s'évide par le milieu. Le plafond est ce qui l'empêche.
    #[test]
    fn une_boite_arrondie_ne_se_retourne_pas_avec_un_rayon_trop_grand() {
        let trace = Trace::Boite { x: 0.25, y: 0.4, l: 0.5, h: 0.2, rayon: 99.0 };
        let pixel = 1.0 / 96.0;
        assert_eq!(couverture(trace, 0.5, 0.5, pixel), 1.0, "le centre reste PLEIN");
        assert_eq!(couverture(trace, 0.9, 0.5, pixel), 0.0, "et le dehors reste vide");
    }

    /// **LE RENDU COMPLET — LA TEINTE, LE FONDU, ET L'ALPHA.** Le pixel du centre porte
    /// la couleur, celui du coin est transparent, et le bord est PARTIEL : c'est ce qui
    /// distingue un dessin vectoriel d'un pochoir.
    #[test]
    fn le_rendu_teint_le_dessin_et_laisse_le_dehors_transparent() {
        let formes = [plus(Trace::Disque { cx: 0.5, cy: 0.5, r: 0.4 })];
        let (l, h, rgba) = rendre(&formes, [0x40, 0x80, 0xC0], None);
        assert_eq!((l, h), (COTE, COTE));
        assert_eq!(rgba.len(), (l * h * 4) as usize);
        let pixel = |x: u32, y: u32| {
            let i = ((y * l + x) * 4) as usize;
            [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
        };
        // ⚠️ **LE CENTRE SE CALCULE, IL NE SE SUPPOSE PAS.** J'avais écrit `48` en dur :
        // `COTE` n'est PAS 96 — c'est `DOSSIER_VIGNETTE_MAX_SIDE` — et ce test mesurait
        // donc un point à 0,44 du centre, c'est-à-dire **dehors**. Il a échoué comme ça,
        // et le code, lui, avait raison.
        let centre = l / 2;
        assert_eq!(pixel(centre, centre), [0x40, 0x80, 0xC0, 255], "le centre : la couleur, opaque");
        assert_eq!(pixel(1, 1)[3], 0, "le coin : transparent");
        // LE BORD EST DOUX — et on l'établit en PARCOURANT le rayon, jamais en visant un
        // pixel précis.
        //
        // ⚠️ **J'AI VISÉ UN INDEX, ET C'ÉTAIT FAUX** : la bande d'anti-aliasing fait UN
        // pixel, et la grille d'échantillonnage tombe rarement dessus — le pixel voisin
        // vaut alors 255, et un dessin juste échoue. Ce qui doit être vrai n'est pas
        // « ce pixel-ci est partiel », c'est **« il existe une transition »**.
        //
        // ⚠️ **ET LE BALAYAGE DOIT TRAVERSER le bord, PAS S'ARRÊTER DESSUS.** `rayon_px`
        // est TRONQUÉ (`0.4 × 256` = 102,4 → 102) : une boucle `0..102` s'arrête à
        // 1,4 px À L'INTÉRIEUR, où la couverture vaut 1 — elle ne voit donc jamais la
        // bande. C'est le troisième faux échec de ce test, et les trois étaient dans le
        // TEST, jamais dans le code : je mesurais un point de mon choix au lieu de
        // mesurer ce qui doit être vrai.
        let rayon_px = (0.4 * l as f32) as u32;
        let partiels = (0..rayon_px + 4)
            .filter(|k| {
                let a = pixel(centre, centre - k)[3];
                a > 0 && a < 255
            })
            .count();
        assert!(partiels > 0, "aucun pixel partiel le long du rayon : le bord est DUR");

        // Et le FONDU change la teinte du HAUT vers le BAS, comme pour les glyphes. Les
        // deux points de mesure sont DANS le disque (qui va de 0,1 à 0,9 du carré).
        let (_, _, degrade) = rendre(&formes, [0x00, 0x00, 0x00], Some([0xFF, 0xFF, 0xFF]));
        let teinte = |y: u32| {
            let i = ((y * l + centre) * 4) as usize;
            degrade[i]
        };
        assert!(teinte(l / 4) < teinte(3 * l / 4), "le haut est plus sombre que le bas : fondu vertical");
    }

    /// **CHAQUE DESSIN DU CATALOGUE EST VRAIMENT DESSINÉ** — au moins un pixel plein, et
    /// jamais un aplat : c'est ce qui attrape un dessin dont toutes les formes se
    /// retireraient l'une l'autre.
    #[test]
    fn chaque_dessin_du_catalogue_est_visible_et_n_est_pas_un_aplat() {
        for dessin in CATALOGUE {
            let (l, h, rgba) = rendre(dessin.formes, dessin.couleur, dessin.degrade);
            let opaques = (0..(l * h))
                .filter(|i| rgba[(*i * 4 + 3) as usize] == 255)
                .count();
            let total = (l * h) as usize;
            assert!(opaques > 40, "« {} » ne dessine presque rien ({opaques} px pleins)", dessin.libelle);
            assert!(
                opaques < total * 9 / 10,
                "« {} » remplit presque tout : c'est un aplat, pas un dessin ({opaques}/{total})",
                dessin.libelle
            );
        }
    }

    /// **ET AUCUN DESSIN N'EST EN DOUBLE.** Deux dessins identiques seraient deux tuiles
    /// indiscernables dans la galerie, et rien ne le signalerait. Ce qui les identifie
    /// est leur **description**, pas leur nom.
    #[test]
    fn deux_dessins_ne_peuvent_pas_avoir_la_meme_description() {
        let mut vus = std::collections::HashSet::new();
        for dessin in CATALOGUE {
            let signature = format!("{:?}", dessin.formes);
            assert!(vus.insert(signature), "dessin en double : {}", dessin.libelle);
        }
    }
}

/// Côté du carré rendu, en pixels — la même valeur que les vignettes prêtes, pour que
/// les deux familles se rangent au même format (voir `vignettes_dossiers::COTE_VIGNETTE`).
pub const COTE: u32 = crate::config::DOSSIER_VIGNETTE_MAX_SIDE;

/// **REND UN DESSIN EN PIXELS RGBA**, prêts pour
/// `config::store_dossier_vignette_from_rgba` — la MÊME destination que les glyphes.
///
/// ⚠️ **LE FONDU EST CELUI DES GLYPHES** (`vignettes_dossiers::teinte_melangee`, même
/// formule, même sens vertical) : deux familles de vignettes qui fondraient dans deux
/// sens différents se liraient comme un défaut, pas comme une variante.
pub fn rendre(formes: &[Forme], couleur: [u8; 3], degrade: Option<[u8; 3]>) -> (u32, u32, Vec<u8>) {
    let (l, h) = (COTE, COTE);
    let pixel = 1.0 / l as f32;
    let derniere = (h - 1).max(1) as f32;
    let mut rgba = vec![0u8; (l * h * 4) as usize];
    for py in 0..h {
        let teinte = match degrade {
            None => couleur,
            Some(vers) => teinte_melangee(couleur, vers, py as f32 / derniere),
        };
        for px in 0..l {
            // LE CENTRE DU PIXEL, pas son coin : sinon tout le dessin est décalé d'un
            // demi-pixel, ce qui se voit sur un trait de deux pixels d'épaisseur.
            let x = (px as f32 + 0.5) * pixel;
            let y = (py as f32 + 0.5) * pixel;
            let couv = couverture_du_dessin(formes, x, y, pixel);
            if couv <= 0.0 {
                continue;
            }
            let i = ((py * l + px) * 4) as usize;
            // ⚠️ **LA TEINTE EST POSÉE DIRECTEMENT**, et non multipliée comme pour un
            // glyphe de police : là-bas, l'encre est BLANCHE et sa forme vit dans
            // l'alpha, donc on multiplie les canaux. Ici le dessin n'a pas d'encre — sa
            // couleur EST la teinte, et la multiplier par un canal vide donnerait du
            // noir (voir `vignettes_dossiers::rendre`, qui, lui, multiplie).
            for (canal, teinte) in rgba[i..i + 3].iter_mut().zip(teinte) {
                *canal = teinte;
            }
            rgba[i + 3] = (couv * 255.0).round() as u8;
        }
    }
    (l, h, rgba)
}

/// **LE DOSSIER COLORÉ, EN DEUX COUCHES** — l'arrière, avec son onglet, puis
/// l'avant qui le recouvre en laissant voir l'onglet et une bande au-dessus. C'est
/// la forme du dossier de l'Explorateur, et la seule façon d'avoir DEUX teintes :
/// un dessin du catalogue n'en porte qu'une (voir `vignettes_dossiers::rendre_dossier_colore`).
const DOSSIER_ARRIERE: &[Forme] = &[poly(&[(0.10, 0.20), (0.40, 0.20), (0.48, 0.29), (0.90, 0.29), (0.90, 0.78), (0.10, 0.78)])];
const DOSSIER_AVANT: &[Forme] = &[ronde(0.10, 0.36, 0.80, 0.46, 0.04)];

/// **REND LE DOSSIER BICOLORE** — les deux couches rendues chacune dans sa teinte,
/// puis l'avant posé SUR l'arrière (« par-dessus », alpha non prémultiplié).
pub fn rendre_bicolore(arriere: [u8; 3], avant: [u8; 3]) -> (u32, u32, Vec<u8>) {
    let (l, h, mut fond) = rendre(DOSSIER_ARRIERE, arriere, None);
    let (_, _, dessus) = rendre(DOSSIER_AVANT, avant, None);
    for (f, d) in fond.chunks_exact_mut(4).zip(dessus.chunks_exact(4)) {
        let (ad, af) = (d[3] as f32 / 255.0, f[3] as f32 / 255.0);
        let a = ad + af * (1.0 - ad);
        if a <= 0.0 {
            continue;
        }
        for c in 0..3 {
            f[c] = ((d[c] as f32 * ad + f[c] as f32 * af * (1.0 - ad)) / a).round() as u8;
        }
        f[3] = (a * 255.0).round() as u8;
    }
    (l, h, fond)
}

/// Les deux couches du dossier bicolore, décrites pour la page — `(arrière, avant)`.
pub fn dossier_bicolore_pour_la_page() -> (serde_json::Value, serde_json::Value) {
    let decrire = |formes: &[Forme]| serde_json::Value::Array(formes.iter().map(forme_decrite).collect());
    (decrire(DOSSIER_ARRIERE), decrire(DOSSIER_AVANT))
}

/// Le fondu vertical — **la même formule que les glyphes de catégorie**, recopiée ici
/// plutôt qu'importée : `vignettes_dossiers` dépend de CE module, et l'inverse ferait un
/// cycle. Deux formules jumelles qu'un test compare (voir
/// `les_deux_familles_fondent_dans_le_meme_sens`).
fn teinte_melangee(depuis: [u8; 3], vers: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let canal = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8;
    [
        canal(depuis[0], vers[0]),
        canal(depuis[1], vers[1]),
        canal(depuis[2], vers[2]),
    ]
}

/// **UN DESSIN DU CATALOGUE** — son libellé, sa description, et ses deux teintes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dessin {
    pub libelle: &'static str,
    pub formes: &'static [Forme],
    pub couleur: [u8; 3],
    pub degrade: Option<[u8; 3]>,
}

// =========================================================================
// LES DESSINS — INFORMATIQUE
// =========================================================================
//
// ⚠️ **CHAQUE DESSIN EST UNE PHRASE.** « Un corps, plus une tête, moins un creux » —
// et l'ordre compte : un RETRAIT ne peut enlever que ce qu'un AJOUT a posé avant lui.

/// Le toit d'un serveur, réutilisé par les trois étages.
macro_rules! etage_serveur {
    ($y:expr) => {
        plus(Trace::Boite { x: 0.24, y: $y, l: 0.52, h: 0.17, rayon: 0.03 })
    };
}

/// **LE CATALOGUE DES DESSINS** — ce que la police n'avait pas.
///
/// ⚠️ **`clippy::approx_constant` EST ÉTEINT ICI, ET C'EST UN FAUX POSITIF.**
/// Les arguments de `trait_` sont des coordonnées normalisées entre 0 et 1, et
/// l'une d'elles vaut `0.318` (« Chiffre 5 ») — soit `1/π` à quatre décimales.
/// Le lint ne regarde pas le CONTEXTE : il voit la constante et refuse la
/// compilation (`correctness` est refusé par défaut, d'où un `error` et non un
/// `warning`). `cargo build` et `cargo test` l'ignorent entièrement.
///
/// ⚠️ **LE `allow` PORTE SUR LE CATALOGUE ENTIER**, faute de pouvoir viser un
/// élément de tableau — une vraie confusion avec `FRAC_1_PI` quelque part dans
/// ces 465 dessins passerait donc inaperçue. Si le lint se réveille ailleurs,
/// c'est probablement ICI qu'il faut regarder.
#[allow(clippy::approx_constant)]
pub const CATALOGUE: [Dessin; 465] = [
    // --- SERVEUR : trois étages empilés, chacun avec sa diode. ---
    Dessin {
        libelle: "Serveur",
        formes: &[
            etage_serveur!(0.11),
            etage_serveur!(0.41),
            etage_serveur!(0.71),
            plus(Trace::Disque { cx: 0.32, cy: 0.195, r: 0.022 }),
            plus(Trace::Disque { cx: 0.32, cy: 0.495, r: 0.022 }),
            plus(Trace::Disque { cx: 0.32, cy: 0.795, r: 0.022 }),
        ],
        couleur: [0x5B, 0x7B, 0x9B],
        degrade: None,
    },
    // --- BASE DE DONNÉES : un cylindre — un corps, plus un couvercle, moins deux
    //     tranches. Le RETRAIT est ce qui fait lire le volume. ---
    Dessin {
        libelle: "Base de données",
        formes: &[
            plus(Trace::Boite { x: 0.28, y: 0.16, l: 0.44, h: 0.68, rayon: 0.0 }),
            plus(Trace::Disque { cx: 0.5, cy: 0.16, r: 0.22 }),
            moins(Trace::Boite { x: 0.28, y: 0.84, l: 0.44, h: 0.16, rayon: 0.0 }),
            moins(Trace::Disque { cx: 0.5, cy: 0.16, r: 0.15 }),
            moins(Trace::Boite { x: 0.28, y: 0.48, l: 0.44, h: 0.02, rayon: 0.0 }),
            moins(Trace::Boite { x: 0.28, y: 0.66, l: 0.44, h: 0.02, rayon: 0.0 }),
        ],
        couleur: [0x4C, 0x8A, 0x7B],
        degrade: None,
    },
    // --- CONTENEUR : une caisse à claire-voie — le corps, moins de larges rainures. ---
    Dessin {
        libelle: "Conteneur",
        formes: &[
            plus(Trace::Boite { x: 0.16, y: 0.24, l: 0.68, h: 0.52, rayon: 0.04 }),
            moins(Trace::Boite { x: 0.24, y: 0.33, l: 0.52, h: 0.06, rayon: 0.0 }),
            moins(Trace::Boite { x: 0.24, y: 0.47, l: 0.52, h: 0.06, rayon: 0.0 }),
            moins(Trace::Boite { x: 0.24, y: 0.61, l: 0.52, h: 0.06, rayon: 0.0 }),
        ],
        couleur: [0x3C, 0x7B, 0x9B],
        degrade: Some([0x3C, 0xB8, 0xC9]),
    },
    // --- MACHINE VIRTUELLE : un cadre dans un cadre. ---
    Dessin {
        libelle: "Machine virtuelle",
        formes: &[
            plus(Trace::Boite { x: 0.10, y: 0.18, l: 0.80, h: 0.64, rayon: 0.05 }),
            moins(Trace::Boite { x: 0.17, y: 0.26, l: 0.66, h: 0.48, rayon: 0.03 }),
            plus(Trace::Boite { x: 0.28, y: 0.36, l: 0.44, h: 0.28, rayon: 0.03 }),
            moins(Trace::Boite { x: 0.34, y: 0.43, l: 0.32, h: 0.14, rayon: 0.01 }),
        ],
        couleur: [0x8A, 0x6D, 0xD4],
        degrade: None,
    },
    // --- TERMINAL : une fenêtre, et son invite `>_`. ---
    Dessin {
        libelle: "Terminal",
        formes: &[
            plus(Trace::Boite { x: 0.10, y: 0.20, l: 0.80, h: 0.60, rayon: 0.05 }),
            moins(Trace::Boite { x: 0.16, y: 0.30, l: 0.68, h: 0.44, rayon: 0.02 }),
            // Le chevron d'invite, en deux traits.
            plus(Trace::Trait { x1: 0.26, y1: 0.42, x2: 0.34, y2: 0.52, epaisseur: 0.05 }),
            plus(Trace::Trait { x1: 0.34, y1: 0.52, x2: 0.26, y2: 0.62, epaisseur: 0.05 }),
            plus(Trace::Trait { x1: 0.42, y1: 0.63, x2: 0.58, y2: 0.63, epaisseur: 0.05 }),
        ],
        couleur: [0x4A, 0x5B, 0x6B],
        degrade: None,
    },
    // --- DÉPÔT : la ramification de Git — deux têtes, une base, deux liens. ---
    Dessin {
        libelle: "Dépôt",
        formes: &[
            plus(Trace::Disque { cx: 0.68, cy: 0.24, r: 0.11 }),
            plus(Trace::Disque { cx: 0.68, cy: 0.76, r: 0.11 }),
            plus(Trace::Disque { cx: 0.32, cy: 0.50, r: 0.11 }),
            plus(Trace::Trait { x1: 0.68, y1: 0.24, x2: 0.68, y2: 0.76, epaisseur: 0.09 }),
            plus(Trace::Trait { x1: 0.32, y1: 0.50, x2: 0.68, y2: 0.50, epaisseur: 0.09 }),
            moins(Trace::Disque { cx: 0.68, cy: 0.24, r: 0.05 }),
            moins(Trace::Disque { cx: 0.68, cy: 0.76, r: 0.05 }),
            moins(Trace::Disque { cx: 0.32, cy: 0.50, r: 0.05 }),
        ],
        couleur: [0xC9, 0x6B, 0x3C],
        degrade: None,
    },
    // --- BOUCLIER : un écu — un polygone, et sa croix intérieure. ---
    Dessin {
        libelle: "Bouclier",
        formes: &[
            plus(Trace::Polygone {
                points: &[(0.50, 0.10), (0.84, 0.26), (0.84, 0.54), (0.50, 0.90), (0.16, 0.54), (0.16, 0.26)],
            }),
            moins(Trace::Polygone {
                points: &[(0.50, 0.20), (0.74, 0.31), (0.74, 0.52), (0.50, 0.78), (0.26, 0.52), (0.26, 0.31)],
            }),
            plus(Trace::Trait { x1: 0.50, y1: 0.32, x2: 0.50, y2: 0.68, epaisseur: 0.07 }),
            plus(Trace::Trait { x1: 0.36, y1: 0.46, x2: 0.64, y2: 0.46, epaisseur: 0.07 }),
        ],
        couleur: [0xC9, 0x5B, 0x5B],
        degrade: None,
    },
    // --- PARE-FEU : un mur de briques décalées — cinq rangées, moins les joints. ---
    Dessin {
        libelle: "Pare-feu",
        formes: &[
            plus(Trace::Boite { x: 0.14, y: 0.22, l: 0.72, h: 0.16, rayon: 0.02 }),
            plus(Trace::Boite { x: 0.14, y: 0.42, l: 0.72, h: 0.16, rayon: 0.02 }),
            plus(Trace::Boite { x: 0.14, y: 0.62, l: 0.72, h: 0.16, rayon: 0.02 }),
            moins(Trace::Boite { x: 0.47, y: 0.22, l: 0.06, h: 0.16, rayon: 0.0 }),
            moins(Trace::Boite { x: 0.30, y: 0.42, l: 0.06, h: 0.16, rayon: 0.0 }),
            moins(Trace::Boite { x: 0.64, y: 0.42, l: 0.06, h: 0.16, rayon: 0.0 }),
            moins(Trace::Boite { x: 0.47, y: 0.62, l: 0.06, h: 0.16, rayon: 0.0 }),
        ],
        couleur: [0xC9, 0x8A, 0x3C],
        degrade: Some([0xC9, 0x3C, 0x2E]),
    },
    // --- CLÉ : l'anneau, la tige, les dents — le creux est ce qui fait lire la clé. ---
    Dessin {
        libelle: "Clé",
        formes: &[
            plus(Trace::Disque { cx: 0.32, cy: 0.50, r: 0.22 }),
            moins(Trace::Disque { cx: 0.32, cy: 0.50, r: 0.10 }),
            plus(Trace::Trait { x1: 0.50, y1: 0.50, x2: 0.88, y2: 0.50, epaisseur: 0.11 }),
            plus(Trace::Boite { x: 0.72, y: 0.50, l: 0.06, h: 0.16, rayon: 0.0 }),
            plus(Trace::Boite { x: 0.60, y: 0.50, l: 0.05, h: 0.12, rayon: 0.0 }),
        ],
        couleur: [0xD9, 0xB1, 0x3F],
        degrade: None,
    },
    // --- PUCE : le boîtier et ses broches, de part et d'autre. ---
    Dessin {
        libelle: "Puce",
        formes: &[
            plus(Trace::Boite { x: 0.24, y: 0.24, l: 0.52, h: 0.52, rayon: 0.05 }),
            moins(Trace::Boite { x: 0.33, y: 0.33, l: 0.34, h: 0.34, rayon: 0.02 }),
            plus(Trace::Trait { x1: 0.10, y1: 0.34, x2: 0.24, y2: 0.34, epaisseur: 0.06 }),
            plus(Trace::Trait { x1: 0.10, y1: 0.50, x2: 0.24, y2: 0.50, epaisseur: 0.06 }),
            plus(Trace::Trait { x1: 0.10, y1: 0.66, x2: 0.24, y2: 0.66, epaisseur: 0.06 }),
            plus(Trace::Trait { x1: 0.76, y1: 0.34, x2: 0.90, y2: 0.34, epaisseur: 0.06 }),
            plus(Trace::Trait { x1: 0.76, y1: 0.50, x2: 0.90, y2: 0.50, epaisseur: 0.06 }),
            plus(Trace::Trait { x1: 0.76, y1: 0.66, x2: 0.90, y2: 0.66, epaisseur: 0.06 }),
        ],
        couleur: [0x5B, 0x8A, 0x6B],
        degrade: None,
    },
    // --- RÉSEAU : trois nœuds reliés à un concentrateur. ---
    Dessin {
        libelle: "Réseau",
        formes: &[
            plus(Trace::Disque { cx: 0.50, cy: 0.50, r: 0.13 }),
            plus(Trace::Disque { cx: 0.18, cy: 0.20, r: 0.10 }),
            plus(Trace::Disque { cx: 0.82, cy: 0.20, r: 0.10 }),
            plus(Trace::Disque { cx: 0.50, cy: 0.86, r: 0.10 }),
            plus(Trace::Trait { x1: 0.50, y1: 0.50, x2: 0.18, y2: 0.20, epaisseur: 0.07 }),
            plus(Trace::Trait { x1: 0.50, y1: 0.50, x2: 0.82, y2: 0.20, epaisseur: 0.07 }),
            plus(Trace::Trait { x1: 0.50, y1: 0.50, x2: 0.50, y2: 0.86, epaisseur: 0.07 }),
        ],
        couleur: [0x3C, 0x9B, 0xC9],
        degrade: Some([0x4A, 0x90, 0xD9]),
    },
    // --- DISQUE : le plateau et son axe — deux anneaux concentriques. ---
    Dessin {
        libelle: "Disque",
        formes: &[
            plus(Trace::Disque { cx: 0.50, cy: 0.50, r: 0.40 }),
            moins(Trace::Disque { cx: 0.50, cy: 0.50, r: 0.30 }),
            plus(Trace::Disque { cx: 0.50, cy: 0.50, r: 0.16 }),
            moins(Trace::Disque { cx: 0.50, cy: 0.50, r: 0.08 }),
        ],
        couleur: [0x8A, 0x98, 0xA8],
        degrade: None,
    },

    // =====================================================================
    // MATÉRIEL — ce qui se branche, se visse, se refroidit
    // =====================================================================
    Dessin { libelle: "Disque dur", formes: &[boite(0.12, 0.26, 0.76, 0.48), rond(0.36, 0.50, 0.17), creux(0.36, 0.50, 0.07), rond(0.74, 0.38, 0.035)], couleur: [0x8A, 0x98, 0xA8], degrade: None },
    Dessin { libelle: "SSD", formes: &[ronde(0.12, 0.30, 0.76, 0.40, 0.04), boite(0.20, 0.38, 0.14, 0.10), boite(0.40, 0.38, 0.14, 0.10), boite(0.60, 0.38, 0.14, 0.10), boite(0.20, 0.54, 0.54, 0.06)], couleur: [0x6B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Carte mère", formes: &[boite(0.12, 0.14, 0.76, 0.72), trou(0.30, 0.30, 0.24, 0.24), boite(0.60, 0.20, 0.22, 0.06), boite(0.60, 0.30, 0.22, 0.06), rond(0.20, 0.72, 0.04), rond(0.34, 0.72, 0.04)], couleur: [0x4C, 0x7B, 0x6B], degrade: None },
    Dessin { libelle: "Mémoire vive", formes: &[boite(0.10, 0.38, 0.80, 0.24), trou(0.16, 0.44, 0.10, 0.12), trou(0.30, 0.44, 0.10, 0.12), trou(0.44, 0.44, 0.10, 0.12), trou(0.58, 0.44, 0.10, 0.12), boite(0.30, 0.62, 0.10, 0.06)], couleur: [0x8A, 0x6D, 0xD4], degrade: None },
    Dessin { libelle: "Processeur", formes: &[boite(0.26, 0.26, 0.48, 0.48), trait_(0.06, 0.36, 0.26, 0.36, 0.05), trait_(0.06, 0.50, 0.26, 0.50, 0.05), trait_(0.06, 0.64, 0.26, 0.64, 0.05), trait_(0.74, 0.36, 0.94, 0.36, 0.05), trait_(0.74, 0.50, 0.94, 0.50, 0.05), trait_(0.74, 0.64, 0.94, 0.64, 0.05)], couleur: [0x5B, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Rack", formes: &[ronde(0.22, 0.08, 0.56, 0.84, 0.03), trou(0.28, 0.16, 0.44, 0.16), trou(0.28, 0.40, 0.44, 0.16), trou(0.28, 0.64, 0.44, 0.16), rond(0.64, 0.24, 0.025), rond(0.64, 0.48, 0.025)], couleur: [0x4A, 0x5B, 0x6B], degrade: None },
    Dessin { libelle: "Alimentation", formes: &[ronde(0.14, 0.24, 0.72, 0.52, 0.05), poly(&[(0.54, 0.30), (0.40, 0.54), (0.50, 0.54), (0.44, 0.72), (0.62, 0.46), (0.52, 0.46)])], couleur: [0xD9, 0xB1, 0x3F], degrade: None },
    Dessin { libelle: "Ventilateur", formes: &[rond(0.50, 0.50, 0.10), poly(&[(0.44, 0.42), (0.56, 0.42), (0.72, 0.16), (0.48, 0.28)]), poly(&[(0.56, 0.56), (0.44, 0.56), (0.28, 0.82), (0.52, 0.70)]), poly(&[(0.60, 0.46), (0.60, 0.54), (0.88, 0.58), (0.74, 0.40)])], couleur: [0x6B, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Ordinateur portable", formes: &[ronde(0.16, 0.20, 0.68, 0.44, 0.05), moins(ecran_creux(0.16, 0.20, 0.68, 0.44, 0.07)), poly(&[(0.06, 0.72), (0.94, 0.72), (0.88, 0.80), (0.12, 0.80)])], couleur: [0x5B, 0x6B, 0x8A], degrade: None },
    Dessin { libelle: "Téléphone", formes: &[ronde(0.30, 0.10, 0.40, 0.80, 0.05), moins(ecran_creux(0.30, 0.10, 0.40, 0.80, 0.06)), rond(0.50, 0.80, 0.035)], couleur: [0x4C, 0x8A, 0x7B], degrade: None },
    Dessin { libelle: "Montre", formes: &[boite(0.34, 0.30, 0.32, 0.40), trait_(0.42, 0.30, 0.42, 0.10, 0.09), trait_(0.58, 0.30, 0.58, 0.10, 0.09), trait_(0.42, 0.70, 0.42, 0.90, 0.09), trait_(0.58, 0.70, 0.58, 0.90, 0.09)], couleur: [0x8A, 0x5B, 0xC9], degrade: None },
    Dessin { libelle: "Imprimante", formes: &[boite(0.16, 0.20, 0.68, 0.16), ronde(0.10, 0.40, 0.80, 0.34, 0.04), trou(0.28, 0.62, 0.44, 0.24), boite(0.32, 0.68, 0.36, 0.24)], couleur: [0x6B, 0x7B, 0x8A], degrade: None },
    Dessin { libelle: "Scanner", formes: &[ronde(0.10, 0.34, 0.80, 0.40, 0.04), trou(0.16, 0.40, 0.68, 0.28), trait_(0.18, 0.54, 0.82, 0.54, 0.06), boite(0.36, 0.20, 0.28, 0.14)], couleur: [0x7B, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Clé USB", formes: &[boite(0.14, 0.40, 0.56, 0.20), creux(0.52, 0.50, 0.045), boite(0.70, 0.44, 0.16, 0.12), boite(0.74, 0.46, 0.08, 0.03), boite(0.74, 0.51, 0.08, 0.03)], couleur: [0x5B, 0x8A, 0xC9], degrade: None },
    Dessin { libelle: "Carte SD", formes: &[poly(&[(0.30, 0.16), (0.70, 0.16), (0.70, 0.84), (0.30, 0.84), (0.24, 0.70), (0.24, 0.30)]), trait_(0.40, 0.28, 0.40, 0.40, 0.05), trait_(0.54, 0.28, 0.54, 0.40, 0.05)], couleur: [0xC9, 0x8A, 0x3C], degrade: None },
    Dessin { libelle: "Prise réseau", formes: &[ronde(0.30, 0.28, 0.40, 0.44, 0.04), trou(0.40, 0.38, 0.20, 0.24), trait_(0.50, 0.72, 0.50, 0.88, 0.07)], couleur: [0x4A, 0x90, 0xD9], degrade: None },
    Dessin { libelle: "Routeur", formes: &[ronde(0.12, 0.48, 0.76, 0.30, 0.05), trait_(0.32, 0.48, 0.22, 0.22, 0.06), trait_(0.68, 0.48, 0.78, 0.22, 0.06), rond(0.28, 0.63, 0.035), rond(0.42, 0.63, 0.035), rond(0.56, 0.63, 0.035)], couleur: [0x3C, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Wi-Fi", formes: &[trait_(0.22, 0.62, 0.50, 0.34, 0.09), trait_(0.50, 0.34, 0.78, 0.62, 0.09), trait_(0.32, 0.72, 0.50, 0.54, 0.09), trait_(0.50, 0.54, 0.68, 0.72, 0.09), rond(0.50, 0.80, 0.075)], couleur: [0x3C, 0x9B, 0xC9], degrade: Some([0x4A, 0x90, 0xD9]) },
    Dessin { libelle: "Antenne", formes: &[trait_(0.50, 0.20, 0.50, 0.86, 0.08), trait_(0.34, 0.86, 0.66, 0.86, 0.08), trait_(0.30, 0.16, 0.70, 0.16, 0.08), trait_(0.38, 0.02, 0.62, 0.02, 0.07), trait_(0.42, 0.09, 0.58, 0.09, 0.06)], couleur: [0x6B, 0x7B, 0x8A], degrade: None },
    Dessin { libelle: "Satellite", formes: &[boite(0.42, 0.38, 0.16, 0.24), boite(0.10, 0.36, 0.28, 0.28), trait_(0.24, 0.42, 0.24, 0.58, 0.03), boite(0.62, 0.36, 0.28, 0.28), trait_(0.76, 0.42, 0.76, 0.58, 0.03), trait_(0.50, 0.62, 0.50, 0.78, 0.04), rond(0.50, 0.84, 0.08)], couleur: [0x7B, 0x8A, 0xC9], degrade: None },
    Dessin { libelle: "Batterie", formes: &[ronde(0.20, 0.30, 0.60, 0.40, 0.05), boite(0.80, 0.44, 0.06, 0.12), boite(0.26, 0.38, 0.14, 0.24), boite(0.44, 0.38, 0.14, 0.24)], couleur: [0x5B, 0x9B, 0x4C], degrade: None },
    Dessin { libelle: "Onduleur", formes: &[ronde(0.14, 0.30, 0.72, 0.44, 0.05), poly(&[(0.52, 0.34), (0.38, 0.54), (0.48, 0.54), (0.42, 0.70), (0.60, 0.50), (0.50, 0.50)]), boite(0.20, 0.58, 0.08, 0.10)], couleur: [0xC9, 0x8A, 0x3C], degrade: None },
    Dessin { libelle: "Webcam", formes: &[rond(0.50, 0.42, 0.26), creux(0.50, 0.42, 0.14), rond(0.50, 0.42, 0.06), boite(0.44, 0.66, 0.12, 0.10), trait_(0.32, 0.80, 0.68, 0.80, 0.08)], couleur: [0x8A, 0x6B, 0x8A], degrade: None },

    // =====================================================================
    // RÉSEAU — ce qui relie
    // =====================================================================
    Dessin { libelle: "Globe", formes: &[rond(0.50, 0.50, 0.38), creux(0.50, 0.50, 0.30), trait_(0.50, 0.12, 0.50, 0.88, 0.05), trait_(0.12, 0.50, 0.88, 0.50, 0.05), trait_(0.20, 0.30, 0.80, 0.30, 0.04), trait_(0.20, 0.70, 0.80, 0.70, 0.04)], couleur: [0x3C, 0x7B, 0xA8], degrade: None },
    Dessin { libelle: "Nuage", formes: &[rond(0.34, 0.54, 0.18), rond(0.54, 0.46, 0.22), rond(0.72, 0.56, 0.15), boite(0.20, 0.52, 0.62, 0.22)], couleur: [0x7B, 0x9B, 0xC9], degrade: None },
    Dessin { libelle: "Lien", formes: &[rond(0.34, 0.50, 0.20), creux(0.34, 0.50, 0.11), rond(0.66, 0.50, 0.20), creux(0.66, 0.50, 0.11), boite(0.30, 0.44, 0.40, 0.12)], couleur: [0x5B, 0x8A, 0xC9], degrade: None },
    Dessin { libelle: "Nœud", formes: &[rond(0.50, 0.50, 0.16), rond(0.18, 0.24, 0.09), rond(0.82, 0.30, 0.09), rond(0.40, 0.86, 0.09), trait_(0.50, 0.50, 0.18, 0.24, 0.05), trait_(0.50, 0.50, 0.82, 0.30, 0.05), trait_(0.50, 0.50, 0.40, 0.86, 0.05)], couleur: [0x4A, 0x90, 0xD9], degrade: None },
    Dessin { libelle: "Flux", formes: &[rond(0.16, 0.50, 0.10), rond(0.50, 0.50, 0.10), rond(0.84, 0.50, 0.10), trait_(0.26, 0.50, 0.40, 0.50, 0.06), trait_(0.60, 0.50, 0.74, 0.50, 0.06), poly(&[(0.70, 0.42), (0.84, 0.50), (0.70, 0.58)])], couleur: [0x3C, 0x9B, 0x9B], degrade: None },
    Dessin { libelle: "Balise", formes: &[trait_(0.30, 0.26, 0.14, 0.50, 0.07), trait_(0.14, 0.50, 0.30, 0.74, 0.07), trait_(0.70, 0.26, 0.86, 0.50, 0.07), trait_(0.86, 0.50, 0.70, 0.74, 0.07), trait_(0.58, 0.24, 0.42, 0.76, 0.06)], couleur: [0xC9, 0x8A, 0x5B], degrade: None },
    Dessin { libelle: "Serveur web", formes: &[rond(0.50, 0.34, 0.22), trait_(0.50, 0.12, 0.50, 0.56, 0.04), trait_(0.28, 0.34, 0.72, 0.34, 0.04), ronde(0.22, 0.64, 0.56, 0.26, 0.04), rond(0.32, 0.77, 0.03), rond(0.44, 0.77, 0.03)], couleur: [0x4A, 0x7B, 0xC9], degrade: Some([0x3C, 0xB8, 0xC9]) },
    Dessin { libelle: "Proxy", formes: &[boite(0.10, 0.36, 0.22, 0.28), boite(0.68, 0.36, 0.22, 0.28), trait_(0.32, 0.50, 0.48, 0.50, 0.06), trait_(0.52, 0.50, 0.68, 0.50, 0.06), rond(0.50, 0.50, 0.09)], couleur: [0x6B, 0x8A, 0x7B], degrade: None },

    // =====================================================================
    // SÉCURITÉ — ce qui protège
    // =====================================================================
    Dessin { libelle: "Cadenas", formes: &[trait_(0.34, 0.44, 0.34, 0.30, 0.08), trait_(0.34, 0.30, 0.66, 0.30, 0.08), trait_(0.66, 0.30, 0.66, 0.44, 0.08), ronde(0.22, 0.44, 0.56, 0.42, 0.05), creux(0.50, 0.62, 0.075), trou(0.46, 0.62, 0.08, 0.14)], couleur: [0xC9, 0xA8, 0x3C], degrade: None },
    Dessin { libelle: "Cadenas ouvert", formes: &[trait_(0.60, 0.44, 0.60, 0.28, 0.08), trait_(0.60, 0.28, 0.34, 0.28, 0.08), ronde(0.22, 0.44, 0.56, 0.42, 0.05), creux(0.50, 0.62, 0.075), trou(0.46, 0.62, 0.08, 0.14)], couleur: [0xC9, 0x8A, 0x3C], degrade: None },
    Dessin { libelle: "Empreinte", formes: &[rond(0.50, 0.56, 0.34), creux(0.50, 0.56, 0.26), rond(0.50, 0.56, 0.20), creux(0.50, 0.56, 0.13), rond(0.50, 0.56, 0.07), trou(0.44, 0.10, 0.12, 0.20)], couleur: [0x8A, 0x6B, 0x9B], degrade: None },
    Dessin { libelle: "Mot de passe", formes: &[rond(0.24, 0.42, 0.07), rond(0.42, 0.42, 0.07), rond(0.60, 0.42, 0.07), rond(0.78, 0.42, 0.07), ronde(0.32, 0.60, 0.36, 0.28, 0.04), creux(0.50, 0.74, 0.05)], couleur: [0xC9, 0x5B, 0x6B], degrade: None },
    Dessin { libelle: "Certificat", formes: &[ronde(0.18, 0.10, 0.64, 0.56, 0.04), trait_(0.28, 0.24, 0.72, 0.24, 0.04), trait_(0.28, 0.34, 0.64, 0.34, 0.04), rond(0.50, 0.68, 0.14), creux(0.50, 0.68, 0.07), poly(&[(0.42, 0.78), (0.50, 0.94), (0.58, 0.78)])], couleur: [0x5B, 0x8A, 0x5B], degrade: None },
    Dessin { libelle: "VPN", formes: &[trait_(0.16, 0.34, 0.84, 0.34, 0.06), trait_(0.16, 0.66, 0.84, 0.66, 0.06), ronde(0.36, 0.42, 0.28, 0.34, 0.04), trait_(0.44, 0.42, 0.44, 0.34, 0.05), trait_(0.44, 0.34, 0.58, 0.34, 0.05), trait_(0.58, 0.34, 0.58, 0.42, 0.05)], couleur: [0x4A, 0x6B, 0x9B], degrade: None },
    Dessin { libelle: "Antivirus", formes: &[poly(&[(0.50, 0.10), (0.84, 0.26), (0.84, 0.54), (0.50, 0.90), (0.16, 0.54), (0.16, 0.26)]), trait_(0.34, 0.48, 0.46, 0.62, 0.08), trait_(0.46, 0.62, 0.68, 0.36, 0.08)], couleur: [0x4C, 0x9B, 0x5B], degrade: Some([0x3C, 0x8A, 0x5B]) },
    Dessin { libelle: "Alerte", formes: &[poly(&[(0.50, 0.12), (0.90, 0.84), (0.10, 0.84)]), perce(&[(0.50, 0.30), (0.78, 0.78), (0.22, 0.78)]), trait_(0.50, 0.42, 0.50, 0.62, 0.07), rond(0.50, 0.72, 0.04)], couleur: [0xD9, 0x8A, 0x3C], degrade: None },
    Dessin { libelle: "Audit", formes: &[ronde(0.14, 0.10, 0.60, 0.70, 0.04), trait_(0.24, 0.26, 0.64, 0.26, 0.04), trait_(0.24, 0.38, 0.54, 0.38, 0.04), rond(0.66, 0.66, 0.22), creux(0.66, 0.66, 0.15), trait_(0.80, 0.82, 0.94, 0.94, 0.07)], couleur: [0x6B, 0x7B, 0x8A], degrade: None },

    // =====================================================================
    // DONNÉES — ce qui se range et se compte
    // =====================================================================
    Dessin { libelle: "Table", formes: &[boite(0.12, 0.18, 0.76, 0.64), trou(0.20, 0.26, 0.60, 0.10), trait_(0.20, 0.46, 0.80, 0.46, 0.03), trait_(0.20, 0.62, 0.80, 0.62, 0.03), trait_(0.42, 0.26, 0.42, 0.74, 0.03), trait_(0.62, 0.26, 0.62, 0.74, 0.03)], couleur: [0x5B, 0x8A, 0x7B], degrade: None },
    Dessin { libelle: "Histogramme", formes: &[trait_(0.14, 0.84, 0.86, 0.84, 0.05), trait_(0.14, 0.84, 0.14, 0.16, 0.05), boite(0.26, 0.58, 0.12, 0.26), boite(0.44, 0.40, 0.12, 0.44), boite(0.62, 0.22, 0.12, 0.62)], couleur: [0x4C, 0x9B, 0x6B], degrade: None },
    Dessin { libelle: "Camembert", formes: &[rond(0.50, 0.52, 0.36), perce(&[(0.50, 0.52), (0.50, 0.10), (0.90, 0.52)]), trait_(0.50, 0.52, 0.50, 0.16, 0.03), trait_(0.50, 0.52, 0.84, 0.52, 0.03)], couleur: [0xC9, 0x7B, 0x5B], degrade: None },
    Dessin { libelle: "Entrepôt de données", formes: &[boite(0.30, 0.16, 0.40, 0.52), rond(0.50, 0.16, 0.20), perce(&[(0.30, 0.68), (0.70, 0.68), (0.70, 0.84), (0.30, 0.84)]), creux(0.50, 0.16, 0.11), trait_(0.10, 0.30, 0.30, 0.30, 0.06), trait_(0.10, 0.52, 0.30, 0.52, 0.06), trait_(0.70, 0.30, 0.90, 0.30, 0.06), trait_(0.70, 0.52, 0.90, 0.52, 0.06)], couleur: [0x3C, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Sauvegarde", formes: &[rond(0.50, 0.50, 0.34), creux(0.50, 0.50, 0.25), trait_(0.50, 0.28, 0.50, 0.58, 0.07), poly(&[(0.38, 0.50), (0.50, 0.66), (0.62, 0.50)])], couleur: [0x4C, 0x8A, 0x5B], degrade: None },
    Dessin { libelle: "Restauration", formes: &[rond(0.50, 0.50, 0.34), creux(0.50, 0.50, 0.25), trait_(0.50, 0.72, 0.50, 0.42, 0.07), poly(&[(0.38, 0.50), (0.50, 0.34), (0.62, 0.50)])], couleur: [0x5B, 0x7B, 0xC9], degrade: None },
    Dessin { libelle: "Index", formes: &[ronde(0.16, 0.12, 0.68, 0.76, 0.04), trait_(0.26, 0.28, 0.50, 0.28, 0.05), trait_(0.34, 0.44, 0.74, 0.44, 0.05), trait_(0.34, 0.58, 0.66, 0.58, 0.05), trait_(0.34, 0.72, 0.58, 0.72, 0.05)], couleur: [0x8A, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Clé étrangère", formes: &[ronde(0.08, 0.16, 0.36, 0.30, 0.03), ronde(0.56, 0.54, 0.36, 0.30, 0.03), rond(0.16, 0.31, 0.05), rond(0.64, 0.69, 0.05), trait_(0.30, 0.44, 0.66, 0.56, 0.05)], couleur: [0x6B, 0x7B, 0x9B], degrade: None },

    // =====================================================================
    // DÉVELOPPEMENT — ce qui se construit
    // =====================================================================
    Dessin { libelle: "Branche", formes: &[trait_(0.34, 0.16, 0.34, 0.86, 0.08), trait_(0.34, 0.50, 0.70, 0.50, 0.08), trait_(0.70, 0.50, 0.70, 0.22, 0.08), rond(0.34, 0.14, 0.09), rond(0.34, 0.88, 0.09), rond(0.70, 0.18, 0.09)], couleur: [0xC9, 0x6B, 0x3C], degrade: None },
    Dessin { libelle: "Fusion", formes: &[trait_(0.24, 0.16, 0.24, 0.62, 0.08), trait_(0.24, 0.62, 0.68, 0.62, 0.08), trait_(0.76, 0.16, 0.76, 0.44, 0.08), trait_(0.76, 0.44, 0.68, 0.62, 0.08), rond(0.24, 0.84, 0.10), rond(0.24, 0.14, 0.09), rond(0.76, 0.14, 0.09)], couleur: [0x8A, 0x6D, 0xD4], degrade: None },
    Dessin { libelle: "Validation", formes: &[trait_(0.30, 0.14, 0.30, 0.86, 0.07), rond(0.30, 0.16, 0.09), rond(0.30, 0.50, 0.09), rond(0.30, 0.84, 0.09), rond(0.70, 0.50, 0.13), trait_(0.64, 0.50, 0.68, 0.55, 0.06), trait_(0.68, 0.55, 0.77, 0.43, 0.06)], couleur: [0x4C, 0x9B, 0x5B], degrade: None },
    Dessin { libelle: "Bogue", formes: &[rond(0.50, 0.56, 0.20), rond(0.50, 0.30, 0.14), trait_(0.34, 0.26, 0.22, 0.14, 0.05), trait_(0.66, 0.26, 0.78, 0.14, 0.05), trait_(0.30, 0.46, 0.12, 0.40, 0.05), trait_(0.30, 0.60, 0.12, 0.60, 0.05), trait_(0.70, 0.46, 0.88, 0.40, 0.05), trait_(0.70, 0.60, 0.88, 0.60, 0.05)], couleur: [0xC9, 0x5B, 0x4C], degrade: None },
    Dessin { libelle: "Correctif", formes: &[rond(0.50, 0.50, 0.32), creux(0.50, 0.50, 0.24), rond(0.50, 0.50, 0.06), trait_(0.50, 0.18, 0.50, 0.26, 0.05), trait_(0.50, 0.74, 0.50, 0.82, 0.05), trait_(0.18, 0.50, 0.26, 0.50, 0.05), trait_(0.74, 0.50, 0.82, 0.50, 0.05)], couleur: [0x8A, 0x6B, 0x4C], degrade: None },
    Dessin { libelle: "Test", formes: &[rond(0.50, 0.50, 0.36), creux(0.50, 0.50, 0.28), trait_(0.34, 0.50, 0.46, 0.64, 0.08), trait_(0.46, 0.64, 0.68, 0.36, 0.08)], couleur: [0x4C, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Compilation", formes: &[boite(0.14, 0.30, 0.44, 0.40), trou(0.22, 0.38, 0.28, 0.24), trait_(0.62, 0.50, 0.82, 0.50, 0.07), poly(&[(0.78, 0.40), (0.94, 0.50), (0.78, 0.60)]), trait_(0.22, 0.20, 0.50, 0.20, 0.05)], couleur: [0x7B, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Paquet", formes: &[poly(&[(0.50, 0.12), (0.86, 0.32), (0.50, 0.52), (0.14, 0.32)]), poly(&[(0.14, 0.32), (0.50, 0.52), (0.50, 0.88), (0.14, 0.68)]), poly(&[(0.86, 0.32), (0.50, 0.52), (0.50, 0.88), (0.86, 0.68)]), trait_(0.32, 0.22, 0.68, 0.42, 0.05)], couleur: [0xC9, 0x9B, 0x4C], degrade: None },
    Dessin { libelle: "Bibliothèque", formes: &[boite(0.16, 0.14, 0.10, 0.50), boite(0.30, 0.20, 0.09, 0.44), boite(0.43, 0.14, 0.11, 0.50), boite(0.58, 0.24, 0.09, 0.40), boite(0.71, 0.16, 0.10, 0.48), boite(0.10, 0.66, 0.80, 0.06), boite(0.10, 0.80, 0.80, 0.06)], couleur: [0x9B, 0x6B, 0x4C], degrade: None },
    Dessin { libelle: "Documentation", formes: &[poly(&[(0.50, 0.28), (0.10, 0.18), (0.10, 0.76), (0.50, 0.86)]), poly(&[(0.50, 0.28), (0.90, 0.18), (0.90, 0.76), (0.50, 0.86)]), trait_(0.50, 0.28, 0.50, 0.86, 0.04)], couleur: [0x5B, 0x6B, 0x9B], degrade: None },
    Dessin { libelle: "Modèle", formes: &[ronde(0.10, 0.12, 0.80, 0.76, 0.04), trou(0.18, 0.22, 0.28, 0.22), trou(0.54, 0.22, 0.28, 0.22), trou(0.18, 0.56, 0.28, 0.22)], couleur: [0x8A, 0x7B, 0x9B], degrade: None },

    // =====================================================================
    // FICHIERS — ce qui se stocke
    // =====================================================================
    Dessin { libelle: "Dossier", formes: &[poly(&[(0.12, 0.28), (0.42, 0.28), (0.50, 0.38), (0.88, 0.38), (0.88, 0.82), (0.12, 0.82)]), perce(&[(0.20, 0.46), (0.80, 0.46), (0.80, 0.74), (0.20, 0.74)])], couleur: [0xC9, 0x9B, 0x4C], degrade: None },
    Dessin { libelle: "Fichier", formes: &[poly(&[(0.26, 0.10), (0.62, 0.10), (0.78, 0.28), (0.78, 0.90), (0.26, 0.90)]), perce(&[(0.62, 0.10), (0.78, 0.28), (0.62, 0.28)]), trait_(0.34, 0.48, 0.70, 0.48, 0.04), trait_(0.34, 0.62, 0.70, 0.62, 0.04)], couleur: [0x7B, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Image", formes: &[ronde(0.10, 0.18, 0.80, 0.64, 0.05), poly(&[(0.16, 0.76), (0.40, 0.44), (0.58, 0.66), (0.70, 0.54), (0.84, 0.76)]), rond(0.68, 0.34, 0.09)], couleur: [0xC9, 0x7B, 0x5B], degrade: None },
    Dessin { libelle: "Vidéo", formes: &[boite(0.10, 0.24, 0.80, 0.52), trou(0.16, 0.32, 0.10, 0.08), trou(0.16, 0.60, 0.10, 0.08), trou(0.32, 0.32, 0.10, 0.08), trou(0.32, 0.60, 0.10, 0.08), trou(0.48, 0.32, 0.10, 0.08), trou(0.48, 0.60, 0.10, 0.08), trou(0.64, 0.32, 0.10, 0.08), trou(0.64, 0.60, 0.10, 0.08), poly(&[(0.40, 0.42), (0.62, 0.50), (0.40, 0.58)])], couleur: [0x9B, 0x5B, 0x6B], degrade: None },
    Dessin { libelle: "Audio", formes: &[rond(0.32, 0.70, 0.14), trait_(0.46, 0.70, 0.46, 0.20, 0.07), trait_(0.46, 0.20, 0.76, 0.28, 0.07), trait_(0.76, 0.28, 0.76, 0.42, 0.07)], couleur: [0xC0, 0x5C, 0xC0], degrade: None },
    Dessin { libelle: "Archive ZIP", formes: &[ronde(0.20, 0.14, 0.60, 0.72, 0.04), trou(0.46, 0.14, 0.08, 0.42), boite(0.46, 0.20, 0.08, 0.06), boite(0.46, 0.32, 0.08, 0.06), boite(0.46, 0.44, 0.08, 0.06)], couleur: [0x9B, 0x8A, 0x5B], degrade: None },
    Dessin { libelle: "PDF", formes: &[poly(&[(0.26, 0.10), (0.62, 0.10), (0.78, 0.28), (0.78, 0.90), (0.26, 0.90)]), perce(&[(0.62, 0.10), (0.78, 0.28), (0.62, 0.28)]), boite(0.34, 0.60, 0.36, 0.10)], couleur: [0xC9, 0x5B, 0x4C], degrade: None },
    Dessin { libelle: "Script", formes: &[ronde(0.14, 0.12, 0.72, 0.76, 0.04), trait_(0.34, 0.40, 0.24, 0.50, 0.06), trait_(0.24, 0.50, 0.34, 0.60, 0.06), trait_(0.44, 0.62, 0.68, 0.62, 0.06)], couleur: [0x6B, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Exécutable", formes: &[rond(0.50, 0.50, 0.30), creux(0.50, 0.50, 0.16), trait_(0.50, 0.20, 0.50, 0.10, 0.08), trait_(0.50, 0.80, 0.50, 0.90, 0.08), trait_(0.20, 0.50, 0.10, 0.50, 0.08), trait_(0.80, 0.50, 0.90, 0.50, 0.08), trait_(0.29, 0.29, 0.22, 0.22, 0.07), trait_(0.71, 0.71, 0.78, 0.78, 0.07)], couleur: [0x5B, 0x7B, 0x6B], degrade: None },
    Dessin { libelle: "Lien symbolique", formes: &[poly(&[(0.16, 0.62), (0.16, 0.42), (0.44, 0.42), (0.44, 0.30), (0.62, 0.52), (0.44, 0.74), (0.44, 0.62)]), ronde(0.54, 0.62, 0.34, 0.28, 0.04)], couleur: [0x5B, 0x9B, 0x8B], degrade: None },

    // =====================================================================
    // DIVERS — ce qui se repère d'un coup d'œil
    // =====================================================================
    Dessin { libelle: "Maison", formes: &[poly(&[(0.50, 0.12), (0.92, 0.48), (0.08, 0.48)]), boite(0.18, 0.46, 0.64, 0.42), trou(0.42, 0.60, 0.16, 0.28)], couleur: [0x8A, 0x6B, 0x5B], degrade: None },
    Dessin { libelle: "Immeuble", formes: &[boite(0.20, 0.14, 0.60, 0.76), trou(0.28, 0.24, 0.12, 0.12), trou(0.60, 0.24, 0.12, 0.12), trou(0.28, 0.44, 0.12, 0.12), trou(0.60, 0.44, 0.12, 0.12), trou(0.28, 0.64, 0.12, 0.12), trou(0.60, 0.64, 0.12, 0.12)], couleur: [0x7B, 0x7B, 0x8A], degrade: None },
    Dessin { libelle: "Usine", formes: &[poly(&[(0.10, 0.44), (0.30, 0.28), (0.30, 0.44), (0.50, 0.28), (0.50, 0.44), (0.70, 0.28), (0.70, 0.44), (0.90, 0.44), (0.90, 0.84), (0.10, 0.84)]), trou(0.30, 0.60, 0.14, 0.24)], couleur: [0x8A, 0x7B, 0x6B], degrade: None },
    Dessin { libelle: "Entrepôt", formes: &[poly(&[(0.50, 0.16), (0.92, 0.46), (0.08, 0.46)]), boite(0.16, 0.44, 0.68, 0.44), trou(0.36, 0.60, 0.28, 0.28), trait_(0.36, 0.74, 0.64, 0.74, 0.03)], couleur: [0x7B, 0x8A, 0x7B], degrade: None },
    Dessin { libelle: "Camion", formes: &[boite(0.08, 0.36, 0.46, 0.34), poly(&[(0.54, 0.44), (0.72, 0.44), (0.86, 0.60), (0.86, 0.70), (0.54, 0.70)]), rond(0.24, 0.74, 0.09), rond(0.72, 0.74, 0.09), trou(0.14, 0.44, 0.32, 0.18)], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Avion", formes: &[poly(&[(0.50, 0.14), (0.58, 0.44), (0.90, 0.60), (0.90, 0.68), (0.58, 0.60), (0.54, 0.82), (0.64, 0.90), (0.64, 0.94), (0.50, 0.90), (0.36, 0.94), (0.36, 0.90), (0.46, 0.82), (0.42, 0.60), (0.10, 0.68), (0.10, 0.60), (0.42, 0.44)]), perce(&[(0.46, 0.20), (0.54, 0.20), (0.54, 0.30), (0.46, 0.30)])], couleur: [0x5B, 0x8A, 0xC9], degrade: None },
    Dessin { libelle: "Bateau", formes: &[poly(&[(0.08, 0.62), (0.92, 0.62), (0.76, 0.86), (0.24, 0.86)]), trait_(0.50, 0.20, 0.50, 0.60, 0.06), poly(&[(0.54, 0.24), (0.82, 0.56), (0.54, 0.56)])], couleur: [0x4A, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Fusée", formes: &[poly(&[(0.50, 0.08), (0.66, 0.34), (0.66, 0.70), (0.34, 0.70), (0.34, 0.34)]), poly(&[(0.34, 0.52), (0.16, 0.76), (0.34, 0.72)]), poly(&[(0.66, 0.52), (0.84, 0.76), (0.66, 0.72)]), rond(0.50, 0.38, 0.09), poly(&[(0.42, 0.72), (0.50, 0.94), (0.58, 0.72)])], couleur: [0xC9, 0x6B, 0x4C], degrade: None },
    Dessin { libelle: "Horloge", formes: &[rond(0.50, 0.52, 0.38), creux(0.50, 0.52, 0.29), trait_(0.50, 0.52, 0.50, 0.30, 0.05), trait_(0.50, 0.52, 0.68, 0.60, 0.05)], couleur: [0x6B, 0x7B, 0x8A], degrade: None },
    Dessin { libelle: "Sablier", formes: &[poly(&[(0.24, 0.12), (0.76, 0.12), (0.50, 0.50)]), poly(&[(0.24, 0.88), (0.76, 0.88), (0.50, 0.50)]), boite(0.20, 0.08, 0.60, 0.06), boite(0.20, 0.86, 0.60, 0.06)], couleur: [0xC9, 0xA8, 0x5B], degrade: None },
    Dessin { libelle: "Calendrier", formes: &[ronde(0.12, 0.16, 0.76, 0.70, 0.04), trou(0.20, 0.26, 0.60, 0.12), trait_(0.20, 0.50, 0.80, 0.50, 0.03), trait_(0.44, 0.38, 0.44, 0.78, 0.03), trait_(0.62, 0.38, 0.62, 0.78, 0.03)], couleur: [0xC9, 0x5B, 0x5B], degrade: None },
    Dessin { libelle: "Carte", formes: &[poly(&[(0.10, 0.26), (0.38, 0.16), (0.62, 0.26), (0.90, 0.16), (0.90, 0.74), (0.62, 0.84), (0.38, 0.74), (0.10, 0.84)]), trait_(0.38, 0.16, 0.38, 0.74, 0.03), trait_(0.62, 0.26, 0.62, 0.84, 0.03)], couleur: [0x5B, 0x9B, 0x6B], degrade: None },
    Dessin { libelle: "Boussole", formes: &[rond(0.50, 0.52, 0.38), creux(0.50, 0.52, 0.29), poly(&[(0.50, 0.26), (0.60, 0.52), (0.50, 0.78), (0.40, 0.52)]), rond(0.50, 0.52, 0.04)], couleur: [0x3C, 0x8A, 0x7B], degrade: None },
    Dessin { libelle: "Ampoule", formes: &[rond(0.50, 0.42, 0.26), boite(0.40, 0.62, 0.20, 0.14), trait_(0.40, 0.80, 0.60, 0.80, 0.05), trait_(0.42, 0.88, 0.58, 0.88, 0.05)], couleur: [0xD9, 0xB1, 0x3F], degrade: None },
    Dessin { libelle: "Éclair", formes: &[poly(&[(0.56, 0.08), (0.28, 0.54), (0.46, 0.54), (0.38, 0.92), (0.70, 0.42), (0.50, 0.42)])], couleur: [0xD9, 0xC9, 0x3C], degrade: None },
    Dessin { libelle: "Étoile", formes: &[poly(&[(0.50, 0.08), (0.61, 0.38), (0.92, 0.38), (0.67, 0.57), (0.77, 0.88), (0.50, 0.69), (0.23, 0.88), (0.33, 0.57), (0.08, 0.38), (0.39, 0.38)])], couleur: [0xD9, 0xB1, 0x3F], degrade: None },
    Dessin { libelle: "Cœur", formes: &[rond(0.34, 0.38, 0.20), rond(0.66, 0.38, 0.20), poly(&[(0.14, 0.42), (0.86, 0.42), (0.50, 0.88)])], couleur: [0xD9, 0x5B, 0x7B], degrade: None },
    Dessin { libelle: "Drapeau", formes: &[trait_(0.26, 0.10, 0.26, 0.92, 0.07), poly(&[(0.30, 0.14), (0.84, 0.24), (0.30, 0.46)])], couleur: [0xC9, 0x5B, 0x5B], degrade: None },
    Dessin { libelle: "Marque-page", formes: &[poly(&[(0.30, 0.10), (0.70, 0.10), (0.70, 0.90), (0.50, 0.72), (0.30, 0.90)])], couleur: [0x9B, 0x6B, 0xC9], degrade: None },
    Dessin { libelle: "Étiquette", formes: &[poly(&[(0.50, 0.10), (0.90, 0.50), (0.50, 0.90), (0.10, 0.50)]), creux(0.36, 0.36, 0.07)], couleur: [0x5B, 0x9B, 0x9B], degrade: None },
    Dessin { libelle: "Boîte", formes: &[poly(&[(0.50, 0.10), (0.88, 0.30), (0.50, 0.50), (0.12, 0.30)]), poly(&[(0.12, 0.30), (0.50, 0.50), (0.50, 0.90), (0.12, 0.70)]), poly(&[(0.88, 0.30), (0.50, 0.50), (0.50, 0.90), (0.88, 0.70)])], couleur: [0xC9, 0x9B, 0x6B], degrade: None },
    Dessin { libelle: "Cadeau", formes: &[boite(0.14, 0.34, 0.72, 0.50), trait_(0.50, 0.34, 0.50, 0.84, 0.06), trait_(0.14, 0.54, 0.86, 0.54, 0.06), rond(0.36, 0.26, 0.10), rond(0.64, 0.26, 0.10)], couleur: [0xC9, 0x5B, 0x8B], degrade: None },
    Dessin { libelle: "Panier", formes: &[poly(&[(0.14, 0.42), (0.86, 0.42), (0.76, 0.84), (0.24, 0.84)]), creux(0.50, 0.36, 0.18), boite(0.14, 0.46, 0.72, 0.06)], couleur: [0xC9, 0x8A, 0x3C], degrade: None },
    Dessin { libelle: "Sac", formes: &[boite(0.20, 0.36, 0.60, 0.50), creux(0.50, 0.36, 0.16), perce(&[(0.20, 0.36), (0.80, 0.36), (0.80, 0.48), (0.20, 0.48)])], couleur: [0x9B, 0x7B, 0x5B], degrade: None },
    Dessin { libelle: "Classeur", formes: &[poly(&[(0.10, 0.24), (0.34, 0.24), (0.40, 0.32), (0.90, 0.32), (0.90, 0.84), (0.10, 0.84)]), trait_(0.20, 0.50, 0.80, 0.50, 0.04), trait_(0.20, 0.66, 0.80, 0.66, 0.04)], couleur: [0x7B, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Poubelle", formes: &[trait_(0.22, 0.28, 0.78, 0.28, 0.08), boite(0.28, 0.28, 0.44, 0.58), trait_(0.40, 0.42, 0.40, 0.74, 0.04), trait_(0.60, 0.42, 0.60, 0.74, 0.04), trait_(0.42, 0.18, 0.58, 0.18, 0.06)], couleur: [0x7B, 0x7B, 0x8A], degrade: None },
    Dessin { libelle: "Recyclage", formes: &[trait_(0.50, 0.14, 0.30, 0.44, 0.08), poly(&[(0.24, 0.40), (0.34, 0.54), (0.24, 0.56)]), trait_(0.32, 0.52, 0.62, 0.68, 0.08), poly(&[(0.66, 0.62), (0.60, 0.78), (0.74, 0.72)]), trait_(0.68, 0.62, 0.52, 0.86, 0.08), poly(&[(0.58, 0.84), (0.44, 0.84), (0.52, 0.92)])], couleur: [0x4C, 0x9B, 0x6B], degrade: None },
    Dessin { libelle: "Balance", formes: &[trait_(0.50, 0.16, 0.50, 0.82, 0.06), trait_(0.16, 0.28, 0.84, 0.28, 0.06), trait_(0.16, 0.28, 0.16, 0.46, 0.05), trait_(0.84, 0.28, 0.84, 0.46, 0.05), poly(&[(0.04, 0.46), (0.28, 0.46), (0.16, 0.62)]), poly(&[(0.72, 0.46), (0.96, 0.46), (0.84, 0.62)]), boite(0.34, 0.82, 0.32, 0.06)], couleur: [0x8A, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Pluie", formes: &[rond(0.34, 0.38, 0.17), rond(0.54, 0.32, 0.20), rond(0.70, 0.40, 0.14), boite(0.20, 0.36, 0.60, 0.20), trait_(0.32, 0.64, 0.26, 0.84, 0.05), trait_(0.50, 0.64, 0.44, 0.84, 0.05), trait_(0.68, 0.64, 0.62, 0.84, 0.05)], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Soleil", formes: &[rond(0.50, 0.50, 0.22), trait_(0.50, 0.06, 0.50, 0.18, 0.06), trait_(0.50, 0.82, 0.50, 0.94, 0.06), trait_(0.06, 0.50, 0.18, 0.50, 0.06), trait_(0.82, 0.50, 0.94, 0.50, 0.06), trait_(0.19, 0.19, 0.28, 0.28, 0.05), trait_(0.72, 0.72, 0.81, 0.81, 0.05), trait_(0.81, 0.19, 0.72, 0.28, 0.05), trait_(0.28, 0.72, 0.19, 0.81, 0.05)], couleur: [0xD9, 0xB1, 0x3F], degrade: None },
    Dessin { libelle: "Lune", formes: &[rond(0.52, 0.50, 0.36), creux(0.38, 0.40, 0.32)], couleur: [0x8A, 0x9B, 0xC9], degrade: None },
    Dessin { libelle: "Feuille", formes: &[poly(&[(0.16, 0.86), (0.20, 0.34), (0.50, 0.12), (0.84, 0.30), (0.80, 0.62), (0.44, 0.80)]), trait_(0.18, 0.84, 0.74, 0.24, 0.04)], couleur: [0x4C, 0x9B, 0x5B], degrade: None },
    Dessin { libelle: "Arbre", formes: &[boite(0.42, 0.62, 0.16, 0.30), rond(0.50, 0.40, 0.26), rond(0.32, 0.52, 0.16), rond(0.68, 0.52, 0.16)], couleur: [0x3C, 0x8A, 0x5B], degrade: None },
    Dessin { libelle: "Fleur", formes: &[rond(0.50, 0.28, 0.14), rond(0.28, 0.46, 0.14), rond(0.72, 0.46, 0.14), rond(0.36, 0.70, 0.14), rond(0.64, 0.70, 0.14), rond(0.50, 0.50, 0.12)], couleur: [0xC9, 0x6B, 0x9B], degrade: Some([0xC9, 0x9B, 0x4C]) },

    // =====================================================================
    // 50 DESSINS DE PLUS — DEMANDE EXPLICITE (2026-10-03) : « crée 50 nouvelles
    // vignettes de dossiers ». AJOUTÉS EN FIN : une vignette posée est retrouvée
    // par son RANG dans ce tableau (voir `cef_dossier_menu`), donc rien de ce qui
    // précède ne bouge.
    // =====================================================================
    Dessin { libelle: "Notes de musique", formes: &[rond(0.28, 0.74, 0.11), rond(0.66, 0.66, 0.11), trait_(0.38, 0.74, 0.38, 0.22, 0.05), trait_(0.76, 0.66, 0.76, 0.16, 0.05), poly(&[(0.36, 0.20), (0.78, 0.12), (0.78, 0.24), (0.36, 0.32)])], couleur: [0x9B, 0x5B, 0xC9], degrade: None },
    Dessin { libelle: "Casque audio", formes: &[rond(0.50, 0.50, 0.36), creux(0.50, 0.50, 0.28), trou(0.10, 0.50, 0.80, 0.45), ronde(0.12, 0.46, 0.18, 0.32, 0.06), ronde(0.70, 0.46, 0.18, 0.32, 0.06)], couleur: [0x5B, 0x6B, 0x9B], degrade: None },
    Dessin { libelle: "Microphone", formes: &[rond(0.50, 0.44, 0.26), creux(0.50, 0.44, 0.20), trou(0.20, 0.10, 0.60, 0.34), ronde(0.38, 0.10, 0.24, 0.46, 0.12), trait_(0.50, 0.70, 0.50, 0.86, 0.05), trait_(0.36, 0.88, 0.64, 0.88, 0.05)], couleur: [0x7B, 0x6B, 0x9B], degrade: None },
    Dessin { libelle: "Appareil photo", formes: &[ronde(0.10, 0.30, 0.80, 0.54, 0.06), boite(0.34, 0.20, 0.32, 0.12), creux(0.50, 0.57, 0.18), rond(0.50, 0.57, 0.11)], couleur: [0x5B, 0x5B, 0x6B], degrade: None },
    Dessin { libelle: "Télévision", formes: &[ronde(0.10, 0.24, 0.80, 0.54, 0.05), moins(ecran_creux(0.10, 0.24, 0.80, 0.54, 0.05)), trait_(0.40, 0.10, 0.50, 0.22, 0.04), trait_(0.60, 0.10, 0.50, 0.22, 0.04), trait_(0.34, 0.86, 0.66, 0.86, 0.05)], couleur: [0x4A, 0x5B, 0x7B], degrade: None },
    Dessin { libelle: "Écran", formes: &[ronde(0.08, 0.14, 0.84, 0.52, 0.04), moins(ecran_creux(0.08, 0.14, 0.84, 0.52, 0.05)), boite(0.44, 0.66, 0.12, 0.14), boite(0.30, 0.80, 0.40, 0.06)], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Clavier", formes: &[ronde(0.06, 0.30, 0.88, 0.42, 0.05), trou(0.13, 0.38, 0.10, 0.08), trou(0.27, 0.38, 0.10, 0.08), trou(0.41, 0.38, 0.10, 0.08), trou(0.55, 0.38, 0.10, 0.08), trou(0.69, 0.38, 0.10, 0.08), trou(0.13, 0.50, 0.10, 0.08), trou(0.27, 0.50, 0.10, 0.08), trou(0.41, 0.50, 0.10, 0.08), trou(0.55, 0.50, 0.10, 0.08), trou(0.69, 0.50, 0.10, 0.08), trou(0.27, 0.62, 0.46, 0.06)], couleur: [0x6B, 0x6B, 0x7B], degrade: None },
    Dessin { libelle: "Souris", formes: &[ronde(0.30, 0.12, 0.40, 0.76, 0.20), trou(0.49, 0.12, 0.02, 0.30), trou(0.30, 0.40, 0.40, 0.02)], couleur: [0x7B, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Manette", formes: &[ronde(0.08, 0.32, 0.84, 0.38, 0.18), rond(0.24, 0.66, 0.14), rond(0.76, 0.66, 0.14), trou(0.20, 0.46, 0.14, 0.04), trou(0.25, 0.41, 0.04, 0.14), creux(0.70, 0.44, 0.035), creux(0.78, 0.52, 0.035)], couleur: [0x6B, 0x5B, 0x9B], degrade: None },
    Dessin { libelle: "Code", formes: &[trait_(0.32, 0.30, 0.14, 0.50, 0.07), trait_(0.14, 0.50, 0.32, 0.70, 0.07), trait_(0.68, 0.30, 0.86, 0.50, 0.07), trait_(0.86, 0.50, 0.68, 0.70, 0.07), trait_(0.58, 0.22, 0.42, 0.78, 0.06)], couleur: [0x3C, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Robot", formes: &[ronde(0.20, 0.28, 0.60, 0.50, 0.08), creux(0.38, 0.48, 0.07), creux(0.62, 0.48, 0.07), trou(0.38, 0.64, 0.24, 0.04), trait_(0.50, 0.28, 0.50, 0.14, 0.04), rond(0.50, 0.12, 0.05), boite(0.12, 0.44, 0.08, 0.16), boite(0.80, 0.44, 0.08, 0.16)], couleur: [0x5B, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Loupe", formes: &[rond(0.42, 0.42, 0.28), creux(0.42, 0.42, 0.20), trait_(0.62, 0.62, 0.86, 0.86, 0.10)], couleur: [0x5B, 0x7B, 0x8A], degrade: None },
    Dessin { libelle: "Engrenage", formes: &[rond(0.50, 0.50, 0.28), trait_(0.50, 0.22, 0.50, 0.10, 0.10), trait_(0.50, 0.78, 0.50, 0.90, 0.10), trait_(0.22, 0.50, 0.10, 0.50, 0.10), trait_(0.78, 0.50, 0.90, 0.50, 0.10), trait_(0.70, 0.30, 0.78, 0.22, 0.10), trait_(0.30, 0.70, 0.22, 0.78, 0.10), trait_(0.30, 0.30, 0.22, 0.22, 0.10), trait_(0.70, 0.70, 0.78, 0.78, 0.10), creux(0.50, 0.50, 0.12)], couleur: [0x7B, 0x7B, 0x7B], degrade: None },
    Dessin { libelle: "Clé à molette", formes: &[trait_(0.24, 0.82, 0.60, 0.46, 0.12), rond(0.68, 0.34, 0.18), creux(0.76, 0.26, 0.10)], couleur: [0x7B, 0x8A, 0x8A], degrade: None },
    Dessin { libelle: "Marteau", formes: &[trait_(0.28, 0.88, 0.62, 0.38, 0.09), poly(&[(0.50, 0.15), (0.87, 0.40), (0.78, 0.53), (0.41, 0.28)])], couleur: [0x8A, 0x6B, 0x5B], degrade: None },
    Dessin { libelle: "Pinceau", formes: &[trait_(0.50, 0.90, 0.50, 0.54, 0.08), boite(0.40, 0.40, 0.20, 0.15), poly(&[(0.40, 0.38), (0.60, 0.38), (0.58, 0.20), (0.50, 0.08), (0.42, 0.20)])], couleur: [0xC9, 0x7B, 0x5B], degrade: None },
    Dessin { libelle: "Palette de peinture", formes: &[rond(0.48, 0.52, 0.38), creux(0.70, 0.70, 0.12), creux(0.32, 0.36, 0.06), creux(0.50, 0.28, 0.06), creux(0.68, 0.38, 0.06), creux(0.30, 0.58, 0.06)], couleur: [0xB1, 0x8A, 0x5B], degrade: None },
    Dessin { libelle: "Crayon", formes: &[poly(&[(0.70, 0.12), (0.88, 0.30), (0.36, 0.82), (0.18, 0.64)]), poly(&[(0.18, 0.64), (0.36, 0.82), (0.12, 0.88)]), moins(Trace::Trait { x1: 0.58, y1: 0.26, x2: 0.74, y2: 0.42, epaisseur: 0.03 })], couleur: [0xD9, 0xA8, 0x3C], degrade: None },
    Dessin { libelle: "Ciseaux", formes: &[rond(0.30, 0.72, 0.12), creux(0.30, 0.72, 0.07), rond(0.70, 0.72, 0.12), creux(0.70, 0.72, 0.07), trait_(0.36, 0.62, 0.66, 0.14, 0.06), trait_(0.64, 0.62, 0.34, 0.14, 0.06)], couleur: [0x9B, 0x5B, 0x5B], degrade: None },
    Dessin { libelle: "Trombone", formes: &[ronde(0.30, 0.10, 0.40, 0.80, 0.20), moins(Trace::Boite { x: 0.36, y: 0.16, l: 0.28, h: 0.68, rayon: 0.14 }), ronde(0.40, 0.30, 0.20, 0.48, 0.10), moins(Trace::Boite { x: 0.45, y: 0.35, l: 0.10, h: 0.38, rayon: 0.05 })], couleur: [0x8A, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Enveloppe", formes: &[ronde(0.10, 0.24, 0.80, 0.52, 0.04), moins(Trace::Trait { x1: 0.12, y1: 0.27, x2: 0.50, y2: 0.56, epaisseur: 0.04 }), moins(Trace::Trait { x1: 0.88, y1: 0.27, x2: 0.50, y2: 0.56, epaisseur: 0.04 })], couleur: [0xC9, 0x8A, 0x5B], degrade: None },
    Dessin { libelle: "Bulle", formes: &[ronde(0.10, 0.16, 0.80, 0.52, 0.14), poly(&[(0.26, 0.62), (0.44, 0.62), (0.22, 0.86)]), creux(0.32, 0.42, 0.05), creux(0.50, 0.42, 0.05), creux(0.68, 0.42, 0.05)], couleur: [0x4C, 0x8A, 0xC9], degrade: None },
    Dessin { libelle: "Cloche", formes: &[rond(0.50, 0.40, 0.26), boite(0.24, 0.40, 0.52, 0.30), poly(&[(0.16, 0.74), (0.84, 0.74), (0.76, 0.66), (0.24, 0.66)]), rond(0.50, 0.82, 0.07), rond(0.50, 0.13, 0.05)], couleur: [0xD9, 0xA8, 0x3C], degrade: None },
    Dessin { libelle: "Pièces", formes: &[rond(0.40, 0.56, 0.26), creux(0.40, 0.56, 0.19), rond(0.64, 0.40, 0.24), creux(0.64, 0.40, 0.17)], couleur: [0xC9, 0xA8, 0x3C], degrade: None },
    Dessin { libelle: "Carte bancaire", formes: &[ronde(0.08, 0.22, 0.84, 0.56, 0.06), trou(0.08, 0.34, 0.84, 0.10), trou(0.18, 0.54, 0.16, 0.12), trou(0.44, 0.62, 0.30, 0.04)], couleur: [0x3C, 0x6B, 0x9B], degrade: None },
    Dessin { libelle: "Croissance", formes: &[trait_(0.10, 0.90, 0.90, 0.90, 0.04), trait_(0.12, 0.80, 0.38, 0.52, 0.06), trait_(0.38, 0.52, 0.56, 0.66, 0.06), trait_(0.56, 0.66, 0.84, 0.28, 0.06), poly(&[(0.70, 0.22), (0.90, 0.18), (0.86, 0.38)])], couleur: [0x4C, 0x9B, 0x6B], degrade: None },
    Dessin { libelle: "Diplôme", formes: &[poly(&[(0.50, 0.16), (0.94, 0.36), (0.50, 0.56), (0.06, 0.36)]), poly(&[(0.24, 0.46), (0.76, 0.46), (0.76, 0.70), (0.50, 0.80), (0.24, 0.70)]), trait_(0.86, 0.40, 0.86, 0.66, 0.04), rond(0.86, 0.70, 0.05)], couleur: [0x4A, 0x4A, 0x6B], degrade: None },
    Dessin { libelle: "Trophée", formes: &[poly(&[(0.26, 0.12), (0.74, 0.12), (0.70, 0.46), (0.50, 0.58), (0.30, 0.46)]), rond(0.20, 0.28, 0.10), creux(0.20, 0.28, 0.05), rond(0.80, 0.28, 0.10), creux(0.80, 0.28, 0.05), boite(0.45, 0.56, 0.10, 0.16), boite(0.30, 0.72, 0.40, 0.10)], couleur: [0xD9, 0xB1, 0x3F], degrade: None },
    Dessin { libelle: "Médaille", formes: &[poly(&[(0.28, 0.08), (0.44, 0.08), (0.56, 0.42), (0.40, 0.42)]), poly(&[(0.72, 0.08), (0.56, 0.08), (0.44, 0.42), (0.60, 0.42)]), rond(0.50, 0.64, 0.24), creux(0.50, 0.64, 0.16), rond(0.50, 0.64, 0.09)], couleur: [0xC9, 0x8A, 0x3C], degrade: None },
    Dessin { libelle: "Ballon", formes: &[rond(0.50, 0.50, 0.38), moins(Trace::Trait { x1: 0.12, y1: 0.50, x2: 0.88, y2: 0.50, epaisseur: 0.04 }), moins(Trace::Trait { x1: 0.50, y1: 0.12, x2: 0.50, y2: 0.88, epaisseur: 0.04 })], couleur: [0xD9, 0x7B, 0x3C], degrade: None },
    Dessin { libelle: "Vélo", formes: &[rond(0.24, 0.64, 0.18), creux(0.24, 0.64, 0.13), rond(0.76, 0.64, 0.18), creux(0.76, 0.64, 0.13), trait_(0.24, 0.64, 0.44, 0.40, 0.04), trait_(0.44, 0.40, 0.70, 0.40, 0.04), trait_(0.70, 0.40, 0.76, 0.64, 0.04), trait_(0.44, 0.40, 0.52, 0.64, 0.04), trait_(0.52, 0.64, 0.24, 0.64, 0.04), trait_(0.64, 0.28, 0.74, 0.28, 0.05)], couleur: [0x3C, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Voiture", formes: &[poly(&[(0.08, 0.62), (0.14, 0.48), (0.28, 0.46), (0.38, 0.30), (0.66, 0.30), (0.78, 0.46), (0.90, 0.50), (0.92, 0.66), (0.08, 0.66)]), perce(&[(0.42, 0.36), (0.54, 0.36), (0.54, 0.46), (0.34, 0.46)]), perce(&[(0.58, 0.36), (0.64, 0.36), (0.72, 0.46), (0.58, 0.46)]), rond(0.28, 0.68, 0.10), rond(0.72, 0.68, 0.10)], couleur: [0xC9, 0x5B, 0x4C], degrade: None },
    Dessin { libelle: "Train", formes: &[ronde(0.22, 0.10, 0.56, 0.66, 0.10), trou(0.30, 0.20, 0.40, 0.20), creux(0.34, 0.58, 0.05), creux(0.66, 0.58, 0.05), trait_(0.30, 0.80, 0.22, 0.92, 0.05), trait_(0.70, 0.80, 0.78, 0.92, 0.05)], couleur: [0x5B, 0x6B, 0x8A], degrade: None },
    Dessin { libelle: "Montagne", formes: &[poly(&[(0.04, 0.84), (0.36, 0.28), (0.52, 0.54), (0.66, 0.38), (0.96, 0.84)]), perce(&[(0.28, 0.42), (0.36, 0.28), (0.44, 0.42), (0.40, 0.38), (0.36, 0.44), (0.32, 0.38)])], couleur: [0x5B, 0x7B, 0x6B], degrade: None },
    Dessin { libelle: "Vague", formes: &[poly(&[(0.06, 0.60), (0.20, 0.40), (0.38, 0.32), (0.56, 0.38), (0.62, 0.52), (0.52, 0.58), (0.46, 0.50), (0.40, 0.56), (0.50, 0.66), (0.72, 0.62), (0.86, 0.50), (0.94, 0.60), (0.94, 0.86), (0.06, 0.86)])], couleur: [0x3C, 0x7B, 0xB1], degrade: None },
    Dessin { libelle: "Flocon", formes: &[trait_(0.50, 0.10, 0.50, 0.90, 0.06), trait_(0.15, 0.30, 0.85, 0.70, 0.06), trait_(0.15, 0.70, 0.85, 0.30, 0.06), trait_(0.50, 0.22, 0.40, 0.14, 0.04), trait_(0.50, 0.22, 0.60, 0.14, 0.04), trait_(0.50, 0.78, 0.40, 0.86, 0.04), trait_(0.50, 0.78, 0.60, 0.86, 0.04)], couleur: [0x7B, 0xA8, 0xC9], degrade: None },
    Dessin { libelle: "Flamme", formes: &[poly(&[(0.50, 0.08), (0.70, 0.36), (0.78, 0.60), (0.70, 0.82), (0.50, 0.92), (0.30, 0.82), (0.22, 0.60), (0.34, 0.40), (0.40, 0.54), (0.46, 0.30)]), perce(&[(0.50, 0.56), (0.60, 0.70), (0.56, 0.84), (0.44, 0.84), (0.40, 0.72)])], couleur: [0xD9, 0x6B, 0x2E], degrade: None },
    Dessin { libelle: "Goutte", formes: &[rond(0.50, 0.62, 0.26), poly(&[(0.50, 0.10), (0.74, 0.54), (0.26, 0.54)]), creux(0.42, 0.66, 0.06)], couleur: [0x3C, 0x8A, 0xC9], degrade: None },
    Dessin { libelle: "Tasse", formes: &[ronde(0.18, 0.34, 0.50, 0.50, 0.08), rond(0.74, 0.54, 0.14), creux(0.74, 0.54, 0.07), trait_(0.32, 0.12, 0.32, 0.26, 0.04), trait_(0.48, 0.10, 0.48, 0.26, 0.04)], couleur: [0x8A, 0x5B, 0x3C], degrade: None },
    Dessin { libelle: "Pomme", formes: &[rond(0.38, 0.58, 0.24), rond(0.62, 0.58, 0.24), rond(0.50, 0.70, 0.22), creux(0.50, 0.34, 0.06), poly(&[(0.52, 0.30), (0.66, 0.12), (0.74, 0.18), (0.58, 0.32)]), trait_(0.50, 0.34, 0.48, 0.20, 0.04)], couleur: [0xC9, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Puzzle", formes: &[boite(0.18, 0.30, 0.52, 0.52), rond(0.44, 0.26, 0.10), rond(0.74, 0.56, 0.10), creux(0.18, 0.56, 0.08)], couleur: [0x6B, 0x9B, 0x5B], degrade: None },
    Dessin { libelle: "Dé", formes: &[ronde(0.14, 0.14, 0.72, 0.72, 0.12), creux(0.32, 0.32, 0.07), creux(0.50, 0.50, 0.07), creux(0.68, 0.68, 0.07), creux(0.68, 0.32, 0.07), creux(0.32, 0.68, 0.07)], couleur: [0xB1, 0x5B, 0x5B], degrade: None },
    Dessin { libelle: "Ancre", formes: &[rond(0.50, 0.58, 0.32), creux(0.50, 0.58, 0.26), trou(0.14, 0.20, 0.72, 0.46), trait_(0.50, 0.27, 0.50, 0.84, 0.07), trait_(0.32, 0.38, 0.68, 0.38, 0.06), rond(0.50, 0.18, 0.09), creux(0.50, 0.18, 0.045)], couleur: [0x3C, 0x5B, 0x8A], degrade: None },
    Dessin { libelle: "Parapluie", formes: &[rond(0.50, 0.48, 0.38), trou(0.10, 0.48, 0.80, 0.44), creux(0.31, 0.50, 0.095), creux(0.50, 0.50, 0.095), creux(0.69, 0.50, 0.095), trait_(0.50, 0.30, 0.50, 0.84, 0.05), trait_(0.50, 0.86, 0.38, 0.86, 0.05), trait_(0.38, 0.86, 0.36, 0.78, 0.05)], couleur: [0x9B, 0x4C, 0x8A], degrade: None },
    Dessin { libelle: "Lunettes", formes: &[rond(0.28, 0.54, 0.17), creux(0.28, 0.54, 0.11), rond(0.72, 0.54, 0.17), creux(0.72, 0.54, 0.11), trait_(0.40, 0.50, 0.60, 0.50, 0.04), trait_(0.11, 0.50, 0.06, 0.36, 0.04), trait_(0.89, 0.50, 0.94, 0.36, 0.04)], couleur: [0x4A, 0x5B, 0x6B], degrade: None },
    Dessin { libelle: "Livre", formes: &[poly(&[(0.10, 0.22), (0.48, 0.28), (0.48, 0.86), (0.10, 0.80)]), poly(&[(0.52, 0.28), (0.90, 0.22), (0.90, 0.80), (0.52, 0.86)])], couleur: [0x8A, 0x5B, 0x4C], degrade: None },
    Dessin { libelle: "Cible", formes: &[rond(0.50, 0.50, 0.40), creux(0.50, 0.50, 0.31), rond(0.50, 0.50, 0.22), creux(0.50, 0.50, 0.14), rond(0.50, 0.50, 0.06)], couleur: [0xC9, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Ticket", formes: &[ronde(0.08, 0.30, 0.84, 0.40, 0.04), creux(0.08, 0.50, 0.08), creux(0.92, 0.50, 0.08), moins(Trace::Trait { x1: 0.64, y1: 0.34, x2: 0.64, y2: 0.66, epaisseur: 0.02 })], couleur: [0xC9, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Sapin", formes: &[poly(&[(0.50, 0.08), (0.70, 0.36), (0.60, 0.36), (0.78, 0.58), (0.64, 0.58), (0.86, 0.80), (0.14, 0.80), (0.36, 0.58), (0.22, 0.58), (0.40, 0.36), (0.30, 0.36)]), boite(0.44, 0.80, 0.12, 0.12)], couleur: [0x2E, 0x7B, 0x4C], degrade: None },
    Dessin { libelle: "Couronne", formes: &[poly(&[(0.12, 0.30), (0.32, 0.52), (0.50, 0.24), (0.68, 0.52), (0.88, 0.30), (0.82, 0.74), (0.18, 0.74)]), boite(0.18, 0.78, 0.64, 0.08), rond(0.12, 0.28, 0.05), rond(0.50, 0.22, 0.05), rond(0.88, 0.28, 0.05)], couleur: [0xD9, 0xB1, 0x3F], degrade: None },

    // =====================================================================
    // 100 DESSINS DE PLUS — DEMANDE EXPLICITE (2026-10-03) : « ajoute-en encore
    // 100 ». Toujours EN FIN, pour la même raison que le lot précédent.
    // =====================================================================
    Dessin { libelle: "Guitare", formes: &[rond(0.40, 0.66, 0.20), rond(0.54, 0.50, 0.15), creux(0.43, 0.62, 0.06), trait_(0.58, 0.46, 0.84, 0.16, 0.07), trait_(0.82, 0.18, 0.90, 0.10, 0.11)], couleur: [0xB1, 0x6B, 0x3C], degrade: None },
    Dessin { libelle: "Piano", formes: &[ronde(0.08, 0.22, 0.84, 0.56, 0.04), trou(0.13, 0.28, 0.74, 0.44), trait_(0.31, 0.28, 0.31, 0.72, 0.03), trait_(0.50, 0.28, 0.50, 0.72, 0.03), trait_(0.69, 0.28, 0.69, 0.72, 0.03), boite(0.25, 0.28, 0.12, 0.26), boite(0.44, 0.28, 0.12, 0.26), boite(0.63, 0.28, 0.12, 0.26)], couleur: [0x6B, 0x6B, 0x8A], degrade: None },
    Dessin { libelle: "Tambour", formes: &[ronde(0.20, 0.38, 0.60, 0.44, 0.06), trou(0.20, 0.47, 0.60, 0.03), moins(Trace::Trait { x1: 0.28, y1: 0.54, x2: 0.4, y2: 0.76, epaisseur: 0.03 }), moins(Trace::Trait { x1: 0.4, y1: 0.76, x2: 0.52, y2: 0.54, epaisseur: 0.03 }), moins(Trace::Trait { x1: 0.52, y1: 0.54, x2: 0.64, y2: 0.76, epaisseur: 0.03 }), moins(Trace::Trait { x1: 0.64, y1: 0.76, x2: 0.72, y2: 0.6, epaisseur: 0.03 }), trait_(0.20, 0.10, 0.46, 0.34, 0.04), trait_(0.80, 0.10, 0.54, 0.34, 0.04)], couleur: [0xC9, 0x5B, 0x4C], degrade: None },
    Dessin { libelle: "Haut-parleur", formes: &[rond(0.56, 0.50, 0.34), creux(0.56, 0.50, 0.28), rond(0.56, 0.50, 0.21), creux(0.56, 0.50, 0.15), trou(0.10, 0.08, 0.46, 0.84), boite(0.14, 0.38, 0.16, 0.24), poly(&[(0.30, 0.38), (0.52, 0.18), (0.52, 0.82), (0.30, 0.62)])], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Radio", formes: &[ronde(0.10, 0.34, 0.80, 0.48, 0.06), creux(0.32, 0.58, 0.12), trou(0.54, 0.44, 0.26, 0.06), trou(0.54, 0.56, 0.26, 0.06), trou(0.54, 0.68, 0.26, 0.06), trait_(0.24, 0.34, 0.70, 0.10, 0.04)], couleur: [0x8A, 0x6B, 0x5B], degrade: None },
    Dessin { libelle: "Disque vinyle", formes: &[rond(0.50, 0.50, 0.40), creux(0.50, 0.50, 0.33), rond(0.50, 0.50, 0.31), creux(0.50, 0.50, 0.22), rond(0.50, 0.50, 0.20), creux(0.50, 0.50, 0.035)], couleur: [0x6B, 0x6B, 0x7B], degrade: None },
    Dessin { libelle: "Claquette", formes: &[poly(&[(0.12, 0.26), (0.84, 0.12), (0.86, 0.24), (0.14, 0.38)]), boite(0.12, 0.42, 0.76, 0.44), trou(0.12, 0.54, 0.76, 0.03), perce(&[(0.22, 0.42), (0.32, 0.42), (0.26, 0.54), (0.16, 0.54)]), perce(&[(0.42, 0.42), (0.52, 0.42), (0.46, 0.54), (0.36, 0.54)]), perce(&[(0.62, 0.42), (0.72, 0.42), (0.66, 0.54), (0.56, 0.54)])], couleur: [0x4A, 0x4A, 0x5B], degrade: None },
    Dessin { libelle: "Pellicule", formes: &[rond(0.50, 0.50, 0.38), creux(0.50, 0.50, 0.06), creux(0.50, 0.27, 0.08), creux(0.50, 0.73, 0.08), creux(0.27, 0.50, 0.08), creux(0.73, 0.50, 0.08)], couleur: [0x5B, 0x5B, 0x6B], degrade: None },
    Dessin { libelle: "Haltère", formes: &[trait_(0.24, 0.50, 0.76, 0.50, 0.06), ronde(0.14, 0.30, 0.12, 0.40, 0.03), ronde(0.74, 0.30, 0.12, 0.40, 0.03), ronde(0.06, 0.38, 0.08, 0.24, 0.02), ronde(0.86, 0.38, 0.08, 0.24, 0.02)], couleur: [0x5B, 0x6B, 0x7B], degrade: None },
    Dessin { libelle: "Raquette", formes: &[rond(0.42, 0.38, 0.27), creux(0.42, 0.38, 0.21), trait_(0.32, 0.20, 0.32, 0.56, 0.02), trait_(0.42, 0.17, 0.42, 0.59, 0.02), trait_(0.52, 0.20, 0.52, 0.56, 0.02), trait_(0.24, 0.30, 0.60, 0.30, 0.02), trait_(0.21, 0.40, 0.63, 0.40, 0.02), trait_(0.24, 0.50, 0.60, 0.50, 0.02), trait_(0.60, 0.58, 0.84, 0.86, 0.08)], couleur: [0x4C, 0x9B, 0x6B], degrade: None },
    Dessin { libelle: "Pion d'échecs", formes: &[rond(0.50, 0.24, 0.11), poly(&[(0.42, 0.34), (0.58, 0.34), (0.66, 0.72), (0.34, 0.72)]), ronde(0.24, 0.72, 0.52, 0.14, 0.04)], couleur: [0x4A, 0x4A, 0x5B], degrade: None },
    Dessin { libelle: "Cartes à jouer", formes: &[ronde(0.40, 0.12, 0.44, 0.60, 0.04), moins(Trace::Boite { x: 0.13, y: 0.25, l: 0.5, h: 0.66, rayon: 0.06 }), ronde(0.16, 0.28, 0.44, 0.60, 0.04), creux(0.33, 0.52, 0.06), creux(0.43, 0.52, 0.06), perce(&[(0.27, 0.54), (0.49, 0.54), (0.38, 0.68)])], couleur: [0xC9, 0x4C, 0x5B], degrade: None },
    Dessin { libelle: "Couverts", formes: &[trait_(0.34, 0.40, 0.34, 0.88, 0.06), trait_(0.26, 0.12, 0.26, 0.36, 0.035), trait_(0.34, 0.12, 0.34, 0.36, 0.035), trait_(0.42, 0.12, 0.42, 0.36, 0.035), ronde(0.24, 0.32, 0.20, 0.10, 0.04), ronde(0.60, 0.10, 0.14, 0.44, 0.07), trait_(0.67, 0.50, 0.67, 0.88, 0.06)], couleur: [0x7B, 0x7B, 0x8A], degrade: None },
    Dessin { libelle: "Verre de vin", formes: &[poly(&[(0.30, 0.12), (0.70, 0.12), (0.68, 0.32), (0.58, 0.46), (0.42, 0.46), (0.32, 0.32)]), trait_(0.50, 0.46, 0.50, 0.80, 0.05), trait_(0.34, 0.84, 0.66, 0.84, 0.06), trou(0.30, 0.12, 0.40, 0.10)], couleur: [0x9B, 0x2E, 0x4C], degrade: None },
    Dessin { libelle: "Bouteille", formes: &[ronde(0.34, 0.36, 0.32, 0.56, 0.08), poly(&[(0.38, 0.38), (0.62, 0.38), (0.56, 0.24), (0.44, 0.24)]), boite(0.44, 0.08, 0.12, 0.18), trou(0.38, 0.54, 0.24, 0.16)], couleur: [0x3C, 0x7B, 0x4C], degrade: None },
    Dessin { libelle: "Gâteau", formes: &[boite(0.16, 0.50, 0.68, 0.36), trou(0.16, 0.64, 0.68, 0.03), trait_(0.34, 0.36, 0.34, 0.48, 0.04), trait_(0.50, 0.34, 0.50, 0.48, 0.04), trait_(0.66, 0.36, 0.66, 0.48, 0.04), rond(0.34, 0.29, 0.035), rond(0.50, 0.27, 0.035), rond(0.66, 0.29, 0.035)], couleur: [0xC9, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Pizza", formes: &[poly(&[(0.50, 0.90), (0.14, 0.20), (0.86, 0.20)]), ronde(0.12, 0.12, 0.76, 0.12, 0.06), creux(0.44, 0.36, 0.06), creux(0.58, 0.46, 0.05), creux(0.48, 0.60, 0.045)], couleur: [0xD9, 0x8A, 0x3C], degrade: None },
    Dessin { libelle: "Œuf", formes: &[rond(0.50, 0.60, 0.28), rond(0.50, 0.42, 0.22)], couleur: [0xD9, 0xC9, 0xA8], degrade: None },
    Dessin { libelle: "Poisson", formes: &[poly(&[(0.10, 0.50), (0.36, 0.28), (0.62, 0.32), (0.74, 0.50), (0.62, 0.68), (0.36, 0.72)]), poly(&[(0.70, 0.50), (0.92, 0.28), (0.92, 0.72)]), creux(0.28, 0.46, 0.04)], couleur: [0x3C, 0x8A, 0xB1], degrade: None },
    Dessin { libelle: "Os", formes: &[trait_(0.30, 0.70, 0.70, 0.30, 0.12), rond(0.22, 0.70, 0.09), rond(0.30, 0.78, 0.09), rond(0.70, 0.22, 0.09), rond(0.78, 0.30, 0.09)], couleur: [0xB1, 0xA8, 0x8A], degrade: None },
    Dessin { libelle: "Cerise", formes: &[rond(0.32, 0.70, 0.16), rond(0.66, 0.66, 0.16), trait_(0.34, 0.56, 0.54, 0.16, 0.035), trait_(0.64, 0.52, 0.54, 0.16, 0.035), poly(&[(0.54, 0.16), (0.80, 0.10), (0.70, 0.24)])], couleur: [0xB1, 0x2E, 0x3C], degrade: None },
    Dessin { libelle: "Carotte", formes: &[poly(&[(0.30, 0.34), (0.60, 0.34), (0.48, 0.92), (0.42, 0.92)]), trait_(0.45, 0.32, 0.36, 0.10, 0.05), trait_(0.45, 0.32, 0.50, 0.08, 0.05), trait_(0.45, 0.32, 0.62, 0.12, 0.05), moins(Trace::Trait { x1: 0.34, y1: 0.48, x2: 0.46, y2: 0.48, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.42, y1: 0.64, x2: 0.52, y2: 0.64, epaisseur: 0.025 })], couleur: [0xD9, 0x7B, 0x2E], degrade: None },
    Dessin { libelle: "Citron", formes: &[rond(0.50, 0.50, 0.38), creux(0.50, 0.50, 0.32), rond(0.50, 0.50, 0.29), moins(Trace::Trait { x1: 0.5, y1: 0.21, x2: 0.5, y2: 0.79, epaisseur: 0.03 }), moins(Trace::Trait { x1: 0.25, y1: 0.355, x2: 0.75, y2: 0.645, epaisseur: 0.03 }), moins(Trace::Trait { x1: 0.25, y1: 0.645, x2: 0.75, y2: 0.355, epaisseur: 0.03 }), creux(0.50, 0.50, 0.04)], couleur: [0xD9, 0xC9, 0x3C], degrade: None },
    Dessin { libelle: "Champignon", formes: &[rond(0.50, 0.48, 0.36), trou(0.0, 0.48, 1.0, 0.6), creux(0.36, 0.30, 0.06), creux(0.60, 0.24, 0.05), ronde(0.38, 0.44, 0.24, 0.42, 0.06)], couleur: [0xB1, 0x4C, 0x3C], degrade: None },
    Dessin { libelle: "Cornet de glace", formes: &[rond(0.50, 0.32, 0.22), poly(&[(0.30, 0.42), (0.70, 0.42), (0.50, 0.92)]), moins(Trace::Trait { x1: 0.36, y1: 0.52, x2: 0.56, y2: 0.74, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.64, y1: 0.52, x2: 0.44, y2: 0.74, epaisseur: 0.025 })], couleur: [0xD9, 0x9B, 0x8A], degrade: None },
    Dessin { libelle: "Lit", formes: &[boite(0.08, 0.50, 0.84, 0.18), boite(0.08, 0.30, 0.10, 0.56), boite(0.84, 0.60, 0.08, 0.26), ronde(0.22, 0.38, 0.22, 0.12, 0.05), boite(0.48, 0.42, 0.44, 0.08)], couleur: [0x5B, 0x6B, 0x9B], degrade: None },
    Dessin { libelle: "Canapé", formes: &[ronde(0.16, 0.30, 0.68, 0.30, 0.06), trou(0.24, 0.50, 0.52, 0.02), ronde(0.08, 0.44, 0.16, 0.34, 0.05), ronde(0.76, 0.44, 0.16, 0.34, 0.05), boite(0.20, 0.54, 0.60, 0.20), trait_(0.16, 0.80, 0.16, 0.88, 0.05), trait_(0.84, 0.80, 0.84, 0.88, 0.05)], couleur: [0x8A, 0x5B, 0x6B], degrade: None },
    Dessin { libelle: "Lampe", formes: &[poly(&[(0.30, 0.14), (0.70, 0.14), (0.82, 0.44), (0.18, 0.44)]), trait_(0.50, 0.44, 0.50, 0.82, 0.05), ronde(0.30, 0.80, 0.40, 0.08, 0.03)], couleur: [0xC9, 0xA8, 0x5B], degrade: None },
    Dessin { libelle: "Porte", formes: &[ronde(0.26, 0.08, 0.48, 0.84, 0.03), trou(0.33, 0.16, 0.34, 0.28), trou(0.33, 0.50, 0.34, 0.32), rond(0.62, 0.47, 0.035)], couleur: [0x8A, 0x5B, 0x3C], degrade: None },
    Dessin { libelle: "Fenêtre", formes: &[ronde(0.14, 0.14, 0.72, 0.72, 0.03), trou(0.20, 0.20, 0.27, 0.27), trou(0.53, 0.20, 0.27, 0.27), trou(0.20, 0.53, 0.27, 0.27), trou(0.53, 0.53, 0.27, 0.27)], couleur: [0x5B, 0x8A, 0xB1], degrade: None },
    Dessin { libelle: "Bougie", formes: &[boite(0.38, 0.40, 0.24, 0.50), poly(&[(0.50, 0.08), (0.60, 0.26), (0.50, 0.34), (0.40, 0.26)]), trait_(0.50, 0.34, 0.50, 0.40, 0.02)], couleur: [0xD9, 0x9B, 0x5B], degrade: None },
    Dessin { libelle: "Prise électrique", formes: &[rond(0.50, 0.50, 0.38), creux(0.50, 0.50, 0.32), rond(0.50, 0.50, 0.28), creux(0.38, 0.50, 0.05), creux(0.62, 0.50, 0.05)], couleur: [0x7B, 0x7B, 0x8A], degrade: None },
    Dessin { libelle: "Robinet", formes: &[trait_(0.20, 0.36, 0.70, 0.36, 0.10), trait_(0.70, 0.36, 0.70, 0.56, 0.10), trait_(0.40, 0.20, 0.40, 0.36, 0.08), trait_(0.28, 0.16, 0.52, 0.16, 0.07), rond(0.70, 0.78, 0.06), poly(&[(0.70, 0.64), (0.75, 0.76), (0.65, 0.76)])], couleur: [0x5B, 0x8A, 0xB1], degrade: None },
    Dessin { libelle: "Baignoire", formes: &[trait_(0.06, 0.48, 0.94, 0.48, 0.05), poly(&[(0.10, 0.50), (0.90, 0.50), (0.84, 0.74), (0.16, 0.74)]), trait_(0.22, 0.74, 0.20, 0.84, 0.05), trait_(0.78, 0.74, 0.80, 0.84, 0.05), trait_(0.18, 0.48, 0.18, 0.24, 0.04), trait_(0.18, 0.24, 0.30, 0.24, 0.04), rond(0.50, 0.38, 0.06), rond(0.62, 0.32, 0.05), rond(0.40, 0.30, 0.04)], couleur: [0x5B, 0x9B, 0xB1], degrade: None },
    Dessin { libelle: "Mallette", formes: &[ronde(0.10, 0.30, 0.80, 0.54, 0.05), ronde(0.36, 0.16, 0.28, 0.18, 0.04), trou(0.42, 0.21, 0.16, 0.09), trou(0.10, 0.50, 0.80, 0.03)], couleur: [0x6B, 0x4A, 0x3C], degrade: None },
    Dessin { libelle: "Punaise", formes: &[rond(0.50, 0.30, 0.20), poly(&[(0.38, 0.42), (0.62, 0.42), (0.56, 0.56), (0.44, 0.56)]), ronde(0.28, 0.54, 0.44, 0.08, 0.03), trait_(0.50, 0.62, 0.50, 0.92, 0.03)], couleur: [0xC9, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Bloc-notes", formes: &[ronde(0.20, 0.16, 0.60, 0.74, 0.04), trou(0.27, 0.28, 0.46, 0.54), boite(0.36, 0.10, 0.28, 0.12), trait_(0.34, 0.40, 0.66, 0.40, 0.04), trait_(0.34, 0.52, 0.66, 0.52, 0.04), trait_(0.34, 0.64, 0.58, 0.64, 0.04)], couleur: [0x8A, 0x6B, 0x4C], degrade: None },
    Dessin { libelle: "Règle", formes: &[poly(&[(0.10, 0.66), (0.66, 0.10), (0.90, 0.34), (0.34, 0.90)]), moins(Trace::Trait { x1: 0.212, y1: 0.548, x2: 0.292, y2: 0.628, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.296, y1: 0.464, x2: 0.346, y2: 0.514, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.38, y1: 0.38, x2: 0.46, y2: 0.46, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.464, y1: 0.296, x2: 0.514, y2: 0.346, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.548, y1: 0.212, x2: 0.628, y2: 0.292, epaisseur: 0.025 })], couleur: [0xC9, 0xA8, 0x5B], degrade: None },
    Dessin { libelle: "Calculatrice", formes: &[ronde(0.22, 0.10, 0.56, 0.80, 0.05), trou(0.30, 0.18, 0.40, 0.16), trou(0.30, 0.42, 0.11, 0.10), trou(0.45, 0.42, 0.11, 0.10), trou(0.59, 0.42, 0.11, 0.10), trou(0.30, 0.56, 0.11, 0.10), trou(0.45, 0.56, 0.11, 0.10), trou(0.59, 0.56, 0.11, 0.10), trou(0.30, 0.70, 0.11, 0.10), trou(0.45, 0.70, 0.11, 0.10), trou(0.59, 0.70, 0.11, 0.10)], couleur: [0x5B, 0x5B, 0x6B], degrade: None },
    Dessin { libelle: "Tampon", formes: &[rond(0.50, 0.20, 0.12), trait_(0.50, 0.28, 0.50, 0.50, 0.10), ronde(0.20, 0.50, 0.60, 0.16, 0.04), boite(0.16, 0.66, 0.68, 0.10), trait_(0.20, 0.88, 0.80, 0.88, 0.04)], couleur: [0xB1, 0x4C, 0x5B], degrade: None },
    Dessin { libelle: "Badge", formes: &[ronde(0.20, 0.18, 0.60, 0.72, 0.06), trou(0.42, 0.24, 0.16, 0.05), creux(0.50, 0.46, 0.11), trou(0.32, 0.64, 0.36, 0.06), trou(0.32, 0.74, 0.24, 0.05)], couleur: [0x3C, 0x6B, 0x9B], degrade: None },
    Dessin { libelle: "Organigramme", formes: &[ronde(0.38, 0.10, 0.24, 0.18, 0.03), ronde(0.10, 0.66, 0.22, 0.18, 0.03), ronde(0.39, 0.66, 0.22, 0.18, 0.03), ronde(0.68, 0.66, 0.22, 0.18, 0.03), trait_(0.50, 0.28, 0.50, 0.66, 0.035), trait_(0.21, 0.48, 0.79, 0.48, 0.035), trait_(0.21, 0.48, 0.21, 0.66, 0.035), trait_(0.79, 0.48, 0.79, 0.66, 0.035)], couleur: [0x5B, 0x7B, 0x8A], degrade: None },
    Dessin { libelle: "Liste de tâches", formes: &[ronde(0.12, 0.16, 0.16, 0.16, 0.03), trou(0.15, 0.19, 0.10, 0.10), trait_(0.36, 0.24, 0.88, 0.24, 0.05), trait_(0.15, 0.23, 0.20, 0.28, 0.035), trait_(0.20, 0.28, 0.31, 0.14, 0.035), ronde(0.12, 0.42, 0.16, 0.16, 0.03), trou(0.15, 0.45, 0.10, 0.10), trait_(0.36, 0.50, 0.88, 0.50, 0.05), trait_(0.15, 0.49, 0.20, 0.54, 0.035), trait_(0.20, 0.54, 0.31, 0.40, 0.035), ronde(0.12, 0.68, 0.16, 0.16, 0.03), trou(0.15, 0.71, 0.10, 0.10), trait_(0.36, 0.76, 0.88, 0.76, 0.05)], couleur: [0x4C, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Coche", formes: &[rond(0.50, 0.50, 0.38), moins(Trace::Trait { x1: 0.32, y1: 0.52, x2: 0.45, y2: 0.65, epaisseur: 0.07 }), moins(Trace::Trait { x1: 0.45, y1: 0.65, x2: 0.7, y2: 0.36, epaisseur: 0.07 })], couleur: [0x3C, 0x9B, 0x5B], degrade: None },
    Dessin { libelle: "Croix médicale", formes: &[ronde(0.38, 0.12, 0.24, 0.76, 0.03), ronde(0.12, 0.38, 0.76, 0.24, 0.03)], couleur: [0xC9, 0x3C, 0x3C], degrade: None },
    Dessin { libelle: "Pilule", formes: &[trait_(0.28, 0.72, 0.72, 0.28, 0.24), moins(Trace::Trait { x1: 0.39, y1: 0.39, x2: 0.61, y2: 0.61, epaisseur: 0.03 })], couleur: [0x9B, 0x6B, 0xC9], degrade: None },
    Dessin { libelle: "Seringue", formes: &[trait_(0.30, 0.70, 0.64, 0.36, 0.14), trait_(0.30, 0.70, 0.12, 0.88, 0.025), trait_(0.64, 0.36, 0.80, 0.20, 0.04), trait_(0.72, 0.12, 0.88, 0.28, 0.05), moins(Trace::Trait { x1: 0.4, y1: 0.52, x2: 0.46, y2: 0.58, epaisseur: 0.02 }), moins(Trace::Trait { x1: 0.48, y1: 0.44, x2: 0.54, y2: 0.5, epaisseur: 0.02 })], couleur: [0x5B, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Thermomètre", formes: &[trait_(0.50, 0.16, 0.50, 0.66, 0.14), rond(0.50, 0.76, 0.14), moins(Trace::Trait { x1: 0.5, y1: 0.16, x2: 0.5, y2: 0.62, epaisseur: 0.06 }), trait_(0.50, 0.42, 0.50, 0.70, 0.06)], couleur: [0xC9, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Pansement", formes: &[trait_(0.22, 0.78, 0.78, 0.22, 0.26), creux(0.46, 0.46, 0.025), creux(0.54, 0.54, 0.025), creux(0.54, 0.46, 0.025), creux(0.46, 0.54, 0.025)], couleur: [0xC9, 0x9B, 0x7B], degrade: None },
    Dessin { libelle: "ADN", formes: &[trait_(0.30, 0.10, 0.70, 0.30, 0.05), trait_(0.70, 0.30, 0.30, 0.50, 0.05), trait_(0.30, 0.50, 0.70, 0.70, 0.05), trait_(0.70, 0.70, 0.30, 0.90, 0.05), trait_(0.70, 0.10, 0.30, 0.30, 0.05), trait_(0.30, 0.30, 0.70, 0.50, 0.05), trait_(0.70, 0.50, 0.30, 0.70, 0.05), trait_(0.30, 0.70, 0.70, 0.90, 0.05), trait_(0.40, 0.20, 0.60, 0.20, 0.03), trait_(0.40, 0.40, 0.60, 0.40, 0.03), trait_(0.40, 0.60, 0.60, 0.60, 0.03), trait_(0.40, 0.80, 0.60, 0.80, 0.03)], couleur: [0x3C, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Atome", formes: &[rond(0.50, 0.50, 0.38), creux(0.50, 0.50, 0.34), rond(0.50, 0.50, 0.22), creux(0.50, 0.50, 0.18), rond(0.50, 0.50, 0.08), rond(0.88, 0.50, 0.05), rond(0.31, 0.17, 0.05), rond(0.31, 0.83, 0.05), rond(0.50, 0.28, 0.04)], couleur: [0x5B, 0x8A, 0xC9], degrade: None },
    Dessin { libelle: "Fiole", formes: &[poly(&[(0.40, 0.12), (0.60, 0.12), (0.60, 0.36), (0.84, 0.86), (0.16, 0.86), (0.40, 0.36)]), perce(&[(0.44, 0.17), (0.56, 0.17), (0.56, 0.37), (0.68, 0.60), (0.32, 0.60), (0.44, 0.37)])], couleur: [0x4C, 0x9B, 0x8A], degrade: None },
    Dessin { libelle: "Aimant", formes: &[rond(0.50, 0.60, 0.34), creux(0.50, 0.60, 0.18), trou(0.0, 0.0, 1.0, 0.60), boite(0.16, 0.20, 0.16, 0.40), boite(0.68, 0.20, 0.16, 0.40), trou(0.16, 0.32, 0.16, 0.03), trou(0.68, 0.32, 0.16, 0.03)], couleur: [0xC9, 0x3C, 0x3C], degrade: None },
    Dessin { libelle: "Planète", formes: &[rond(0.50, 0.50, 0.24), moins(Trace::Trait { x1: 0.1, y1: 0.62, x2: 0.9, y2: 0.38, epaisseur: 0.12 }), trait_(0.10, 0.62, 0.90, 0.38, 0.05)], couleur: [0xC9, 0x8A, 0x5B], degrade: None },
    Dessin { libelle: "Comète", formes: &[rond(0.68, 0.32, 0.14), trait_(0.62, 0.38, 0.12, 0.88, 0.06), trait_(0.58, 0.30, 0.20, 0.66, 0.04), trait_(0.70, 0.42, 0.36, 0.82, 0.04)], couleur: [0x7B, 0x9B, 0xC9], degrade: None },
    Dessin { libelle: "Arc-en-ciel", formes: &[rond(0.50, 0.80, 0.42), creux(0.50, 0.80, 0.36), rond(0.50, 0.80, 0.32), creux(0.50, 0.80, 0.26), rond(0.50, 0.80, 0.22), creux(0.50, 0.80, 0.16), trou(0.0, 0.80, 1.0, 0.3)], couleur: [0xC9, 0x5B, 0x9B], degrade: None },
    Dessin { libelle: "Cactus", formes: &[ronde(0.42, 0.16, 0.16, 0.72, 0.08), trait_(0.24, 0.36, 0.24, 0.56, 0.12), trait_(0.24, 0.56, 0.42, 0.56, 0.12), trait_(0.76, 0.26, 0.76, 0.46, 0.12), trait_(0.76, 0.46, 0.58, 0.46, 0.12), trait_(0.24, 0.90, 0.76, 0.90, 0.05)], couleur: [0x3C, 0x8A, 0x4C], degrade: None },
    Dessin { libelle: "Palmier", formes: &[trait_(0.50, 0.90, 0.54, 0.40, 0.07), poly(&[(0.54, 0.36), (0.16, 0.30), (0.34, 0.20)]), poly(&[(0.54, 0.36), (0.92, 0.32), (0.74, 0.20)]), poly(&[(0.54, 0.36), (0.30, 0.56), (0.24, 0.42)]), poly(&[(0.54, 0.36), (0.80, 0.58), (0.84, 0.44)]), poly(&[(0.54, 0.36), (0.46, 0.10), (0.62, 0.12)])], couleur: [0x3C, 0x8A, 0x5B], degrade: None },
    Dessin { libelle: "Coquillage", formes: &[poly(&[(0.50, 0.86), (0.12, 0.40), (0.24, 0.20), (0.50, 0.12), (0.76, 0.20), (0.88, 0.40)]), moins(Trace::Trait { x1: 0.5, y1: 0.82, x2: 0.24, y2: 0.26, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.5, y1: 0.82, x2: 0.4, y2: 0.17, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.5, y1: 0.82, x2: 0.6, y2: 0.17, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.5, y1: 0.82, x2: 0.76, y2: 0.26, epaisseur: 0.025 }), boite(0.40, 0.80, 0.20, 0.10)], couleur: [0xD9, 0x9B, 0x7B], degrade: None },
    Dessin { libelle: "Papillon", formes: &[rond(0.32, 0.36, 0.18), rond(0.68, 0.36, 0.18), rond(0.36, 0.66, 0.13), rond(0.64, 0.66, 0.13), moins(Trace::Trait { x1: 0.5, y1: 0.18, x2: 0.5, y2: 0.84, epaisseur: 0.08 }), trait_(0.50, 0.24, 0.50, 0.78, 0.05), trait_(0.50, 0.24, 0.40, 0.08, 0.025), trait_(0.50, 0.24, 0.60, 0.08, 0.025)], couleur: [0x9B, 0x5B, 0xC9], degrade: None },
    Dessin { libelle: "Oiseau", formes: &[rond(0.44, 0.56, 0.24), rond(0.66, 0.36, 0.13), poly(&[(0.77, 0.32), (0.92, 0.38), (0.77, 0.42)]), poly(&[(0.24, 0.58), (0.06, 0.44), (0.10, 0.64)]), creux(0.68, 0.33, 0.03), moins(Trace::Trait { x1: 0.3, y1: 0.54, x2: 0.52, y2: 0.62, epaisseur: 0.04 })], couleur: [0x3C, 0x7B, 0xB1], degrade: None },
    Dessin { libelle: "Chat", formes: &[rond(0.50, 0.56, 0.32), poly(&[(0.22, 0.44), (0.24, 0.12), (0.46, 0.30)]), poly(&[(0.78, 0.44), (0.76, 0.12), (0.54, 0.30)]), creux(0.38, 0.52, 0.05), creux(0.62, 0.52, 0.05), perce(&[(0.46, 0.64), (0.54, 0.64), (0.50, 0.70)])], couleur: [0x8A, 0x7B, 0x6B], degrade: None },
    Dessin { libelle: "Patte", formes: &[rond(0.50, 0.66, 0.20), rond(0.24, 0.42, 0.09), rond(0.40, 0.26, 0.09), rond(0.60, 0.26, 0.09), rond(0.76, 0.42, 0.09)], couleur: [0x8A, 0x6B, 0x5B], degrade: None },
    Dessin { libelle: "Tortue", formes: &[rond(0.50, 0.58, 0.30), trou(0.0, 0.58, 1.0, 0.5), rond(0.30, 0.66, 0.07), rond(0.70, 0.66, 0.07), rond(0.86, 0.56, 0.08), trait_(0.18, 0.60, 0.82, 0.60, 0.06), moins(Trace::Trait { x1: 0.4, y1: 0.32, x2: 0.36, y2: 0.56, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.6, y1: 0.32, x2: 0.64, y2: 0.56, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.26, y1: 0.46, x2: 0.74, y2: 0.46, epaisseur: 0.025 })], couleur: [0x4C, 0x8A, 0x5B], degrade: None },
    Dessin { libelle: "Bus", formes: &[ronde(0.14, 0.14, 0.72, 0.66, 0.08), trou(0.20, 0.24, 0.60, 0.24), creux(0.30, 0.64, 0.05), creux(0.70, 0.64, 0.05), boite(0.20, 0.80, 0.12, 0.08), boite(0.68, 0.80, 0.12, 0.08)], couleur: [0xD9, 0xA8, 0x3C], degrade: None },
    Dessin { libelle: "Montgolfière", formes: &[rond(0.50, 0.36, 0.28), poly(&[(0.24, 0.46), (0.76, 0.46), (0.56, 0.72), (0.44, 0.72)]), moins(Trace::Trait { x1: 0.4, y1: 0.1, x2: 0.42, y2: 0.68, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.6, y1: 0.1, x2: 0.58, y2: 0.68, epaisseur: 0.025 }), trait_(0.44, 0.72, 0.42, 0.80, 0.02), trait_(0.56, 0.72, 0.58, 0.80, 0.02), boite(0.40, 0.80, 0.20, 0.10)], couleur: [0xC9, 0x5B, 0x4C], degrade: None },
    Dessin { libelle: "Feu tricolore", formes: &[ronde(0.34, 0.08, 0.32, 0.76, 0.08), creux(0.50, 0.22, 0.08), creux(0.50, 0.46, 0.08), creux(0.50, 0.70, 0.08), rond(0.50, 0.70, 0.055), trait_(0.50, 0.84, 0.50, 0.94, 0.06)], couleur: [0x4A, 0x5B, 0x5B], degrade: None },
    Dessin { libelle: "Panneau stop", formes: &[poly(&[(0.653, 0.130), (0.870, 0.347), (0.870, 0.653), (0.653, 0.870), (0.347, 0.870), (0.130, 0.653), (0.130, 0.347), (0.347, 0.130)]), perce(&[(0.626, 0.195), (0.805, 0.374), (0.805, 0.626), (0.626, 0.805), (0.374, 0.805), (0.195, 0.626), (0.195, 0.374), (0.374, 0.195)]), poly(&[(0.611, 0.232), (0.768, 0.389), (0.768, 0.611), (0.611, 0.768), (0.389, 0.768), (0.232, 0.611), (0.232, 0.389), (0.389, 0.232)]), trou(0.30, 0.46, 0.40, 0.08)], couleur: [0xC9, 0x3C, 0x3C], degrade: None },
    Dessin { libelle: "Roue", formes: &[rond(0.50, 0.50, 0.40), creux(0.50, 0.50, 0.30), rond(0.50, 0.50, 0.08), trait_(0.50, 0.50, 0.5, 0.2, 0.04), trait_(0.50, 0.50, 0.5, 0.8, 0.04), trait_(0.50, 0.50, 0.24, 0.35, 0.04), trait_(0.50, 0.50, 0.76, 0.65, 0.04), trait_(0.50, 0.50, 0.24, 0.65, 0.04), trait_(0.50, 0.50, 0.76, 0.35, 0.04)], couleur: [0x5B, 0x5B, 0x6B], degrade: None },
    Dessin { libelle: "Phare", formes: &[poly(&[(0.38, 0.30), (0.62, 0.30), (0.70, 0.88), (0.30, 0.88)]), trou(0.30, 0.48, 0.40, 0.07), trou(0.30, 0.66, 0.40, 0.07), boite(0.36, 0.18, 0.28, 0.12), trou(0.44, 0.21, 0.12, 0.06), poly(&[(0.34, 0.18), (0.66, 0.18), (0.50, 0.06)]), poly(&[(0.66, 0.22), (0.92, 0.14), (0.92, 0.30)]), poly(&[(0.34, 0.22), (0.08, 0.14), (0.08, 0.30)])], couleur: [0xC9, 0x5B, 0x4C], degrade: None },
    Dessin { libelle: "Château", formes: &[boite(0.16, 0.40, 0.68, 0.48), boite(0.16, 0.30, 0.10, 0.12), boite(0.34, 0.30, 0.10, 0.12), boite(0.56, 0.30, 0.10, 0.12), boite(0.74, 0.30, 0.10, 0.12), trou(0.40, 0.70, 0.20, 0.18), creux(0.50, 0.70, 0.10)], couleur: [0x7B, 0x7B, 0x8A], degrade: None },
    Dessin { libelle: "Tente", formes: &[poly(&[(0.50, 0.14), (0.92, 0.84), (0.08, 0.84)]), perce(&[(0.50, 0.44), (0.64, 0.84), (0.36, 0.84)]), trait_(0.50, 0.14, 0.50, 0.06, 0.03)], couleur: [0x4C, 0x8A, 0x5B], degrade: None },
    Dessin { libelle: "Pont", formes: &[boite(0.06, 0.40, 0.88, 0.44), creux(0.28, 0.84, 0.16), creux(0.72, 0.84, 0.16), trait_(0.06, 0.30, 0.94, 0.30, 0.03), trait_(0.12, 0.30, 0.12, 0.40, 0.03), trait_(0.50, 0.30, 0.50, 0.40, 0.03), trait_(0.88, 0.30, 0.88, 0.40, 0.03)], couleur: [0x7B, 0x6B, 0x5B], degrade: None },
    Dessin { libelle: "Hôpital", formes: &[boite(0.16, 0.20, 0.68, 0.70), trou(0.44, 0.30, 0.12, 0.30), trou(0.35, 0.39, 0.30, 0.12), trou(0.42, 0.72, 0.16, 0.18)], couleur: [0x5B, 0x8A, 0xB1], degrade: None },
    Dessin { libelle: "Banque", formes: &[poly(&[(0.50, 0.10), (0.92, 0.32), (0.08, 0.32)]), creux(0.50, 0.24, 0.04), boite(0.16, 0.36, 0.10, 0.40), boite(0.36, 0.36, 0.10, 0.40), boite(0.54, 0.36, 0.10, 0.40), boite(0.74, 0.36, 0.10, 0.40), boite(0.10, 0.78, 0.80, 0.10)], couleur: [0x7B, 0x7B, 0x6B], degrade: None },
    Dessin { libelle: "Magasin", formes: &[boite(0.10, 0.14, 0.80, 0.18), rond(0.18, 0.32, 0.08), rond(0.34, 0.32, 0.08), rond(0.50, 0.32, 0.08), rond(0.66, 0.32, 0.08), rond(0.82, 0.32, 0.08), moins(Trace::Trait { x1: 0.34, y1: 0.14, x2: 0.34, y2: 0.34, epaisseur: 0.03 }), moins(Trace::Trait { x1: 0.66, y1: 0.14, x2: 0.66, y2: 0.34, epaisseur: 0.03 }), boite(0.16, 0.44, 0.68, 0.46), trou(0.24, 0.56, 0.18, 0.34), trou(0.52, 0.54, 0.26, 0.20)], couleur: [0xC9, 0x5B, 0x4C], degrade: None },
    Dessin { libelle: "Infini", formes: &[rond(0.32, 0.50, 0.20), creux(0.32, 0.50, 0.11), rond(0.68, 0.50, 0.20), creux(0.68, 0.50, 0.11)], couleur: [0x6B, 0x5B, 0x9B], degrade: None },
    Dessin { libelle: "Point d'exclamation", formes: &[ronde(0.42, 0.10, 0.16, 0.56, 0.08), rond(0.50, 0.82, 0.09)], couleur: [0xD9, 0x7B, 0x2E], degrade: None },
    Dessin { libelle: "Plus", formes: &[rond(0.50, 0.50, 0.38), trou(0.44, 0.26, 0.12, 0.48), trou(0.26, 0.44, 0.48, 0.12)], couleur: [0x4C, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Synchronisation", formes: &[rond(0.50, 0.50, 0.34), creux(0.50, 0.50, 0.24), trou(0.0, 0.46, 1.0, 0.08), poly(&[(0.74, 0.40), (0.94, 0.40), (0.84, 0.56)]), poly(&[(0.06, 0.60), (0.26, 0.60), (0.16, 0.44)])], couleur: [0x3C, 0x8A, 0xB1], degrade: None },
    Dessin { libelle: "Téléchargement", formes: &[trait_(0.50, 0.12, 0.50, 0.54, 0.10), poly(&[(0.28, 0.46), (0.72, 0.46), (0.50, 0.70)]), trait_(0.16, 0.68, 0.16, 0.86, 0.06), trait_(0.16, 0.86, 0.84, 0.86, 0.06), trait_(0.84, 0.86, 0.84, 0.68, 0.06)], couleur: [0x3C, 0x7B, 0xB1], degrade: None },
    Dessin { libelle: "Envoi", formes: &[poly(&[(0.08, 0.46), (0.92, 0.12), (0.62, 0.88), (0.48, 0.56)]), moins(Trace::Trait { x1: 0.48, y1: 0.56, x2: 0.9, y2: 0.14, epaisseur: 0.03 })], couleur: [0x3C, 0x8A, 0xC9], degrade: None },
    Dessin { libelle: "Partage", formes: &[rond(0.72, 0.24, 0.11), rond(0.28, 0.50, 0.11), rond(0.72, 0.76, 0.11), trait_(0.28, 0.50, 0.72, 0.24, 0.05), trait_(0.28, 0.50, 0.72, 0.76, 0.05)], couleur: [0x5B, 0x9B, 0x6B], degrade: None },
    Dessin { libelle: "Œil", formes: &[poly(&[(0.06, 0.50), (0.30, 0.30), (0.50, 0.24), (0.70, 0.30), (0.94, 0.50), (0.70, 0.70), (0.50, 0.76), (0.30, 0.70)]), creux(0.50, 0.50, 0.17), rond(0.50, 0.50, 0.09)], couleur: [0x5B, 0x6B, 0x8A], degrade: None },
    Dessin { libelle: "Main", formes: &[ronde(0.28, 0.44, 0.44, 0.46, 0.12), ronde(0.28, 0.16, 0.09, 0.36, 0.045), ronde(0.39, 0.10, 0.09, 0.40, 0.045), ronde(0.50, 0.12, 0.09, 0.38, 0.045), ronde(0.61, 0.20, 0.09, 0.32, 0.045), trait_(0.30, 0.64, 0.14, 0.46, 0.10)], couleur: [0xC9, 0x9B, 0x7B], degrade: None },
    Dessin { libelle: "Sourire", formes: &[rond(0.50, 0.50, 0.38), creux(0.37, 0.40, 0.05), creux(0.63, 0.40, 0.05), perce(&[(0.28, 0.56), (0.72, 0.56), (0.62, 0.70), (0.50, 0.74), (0.38, 0.70)])], couleur: [0xD9, 0xB1, 0x3F], degrade: None },
    Dessin { libelle: "Personne", formes: &[rond(0.50, 0.28, 0.15), rond(0.50, 0.86, 0.32), trou(0.0, 0.90, 1.0, 0.2)], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Groupe", formes: &[rond(0.22, 0.36, 0.09), rond(0.22, 0.78, 0.18), rond(0.78, 0.36, 0.09), rond(0.78, 0.78, 0.18), creux(0.50, 0.80, 0.30), creux(0.50, 0.30, 0.15), rond(0.50, 0.30, 0.12), rond(0.50, 0.80, 0.26), trou(0.0, 0.84, 1.0, 0.2)], couleur: [0x5B, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "T-shirt", formes: &[poly(&[(0.32, 0.14), (0.42, 0.18), (0.58, 0.18), (0.68, 0.14), (0.92, 0.30), (0.82, 0.46), (0.72, 0.40), (0.72, 0.88), (0.28, 0.88), (0.28, 0.40), (0.18, 0.46), (0.08, 0.30)]), creux(0.50, 0.16, 0.08)], couleur: [0x3C, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Chapeau", formes: &[ronde(0.30, 0.24, 0.40, 0.42, 0.06), ronde(0.08, 0.62, 0.84, 0.12, 0.06), trou(0.30, 0.52, 0.40, 0.06)], couleur: [0x4A, 0x4A, 0x5B], degrade: None },
    Dessin { libelle: "Cravate", formes: &[poly(&[(0.42, 0.10), (0.58, 0.10), (0.55, 0.24), (0.45, 0.24)]), poly(&[(0.45, 0.26), (0.55, 0.26), (0.66, 0.74), (0.50, 0.90), (0.34, 0.74)])], couleur: [0x9B, 0x3C, 0x4C], degrade: None },
    Dessin { libelle: "Diamant", formes: &[poly(&[(0.26, 0.18), (0.74, 0.18), (0.92, 0.38), (0.50, 0.88), (0.08, 0.38)]), moins(Trace::Trait { x1: 0.08, y1: 0.38, x2: 0.92, y2: 0.38, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.38, y1: 0.18, x2: 0.3, y2: 0.38, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.3, y1: 0.38, x2: 0.5, y2: 0.86, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.62, y1: 0.18, x2: 0.7, y2: 0.38, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.7, y1: 0.38, x2: 0.5, y2: 0.86, epaisseur: 0.025 })], couleur: [0x5B, 0xA8, 0xC9], degrade: None },
    Dessin { libelle: "Bague", formes: &[rond(0.50, 0.62, 0.28), creux(0.50, 0.62, 0.21), poly(&[(0.40, 0.24), (0.60, 0.24), (0.66, 0.30), (0.50, 0.44), (0.34, 0.30)])], couleur: [0xC9, 0xA8, 0x3C], degrade: None },
    Dessin { libelle: "Parfum", formes: &[ronde(0.24, 0.36, 0.52, 0.52, 0.08), trou(0.34, 0.50, 0.32, 0.22), boite(0.44, 0.26, 0.12, 0.10), ronde(0.38, 0.12, 0.24, 0.14, 0.03)], couleur: [0xC9, 0x7B, 0xB1], degrade: None },
    Dessin { libelle: "Ruche", formes: &[poly(&[(0.470, 0.360), (0.395, 0.490), (0.245, 0.490), (0.170, 0.360), (0.245, 0.230), (0.395, 0.230)]), poly(&[(0.830, 0.360), (0.755, 0.490), (0.605, 0.490), (0.530, 0.360), (0.605, 0.230), (0.755, 0.230)]), poly(&[(0.650, 0.680), (0.575, 0.810), (0.425, 0.810), (0.350, 0.680), (0.425, 0.550), (0.575, 0.550)])], couleur: [0xD9, 0xA8, 0x3C], degrade: None },
    Dessin { libelle: "Chronomètre", formes: &[rond(0.50, 0.56, 0.34), creux(0.50, 0.56, 0.27), boite(0.44, 0.10, 0.12, 0.10), trait_(0.76, 0.26, 0.82, 0.20, 0.06), trait_(0.50, 0.56, 0.50, 0.38, 0.05), rond(0.50, 0.56, 0.05)], couleur: [0x5B, 0x6B, 0x7B], degrade: None },
    Dessin { libelle: "Jumelles", formes: &[ronde(0.12, 0.36, 0.32, 0.50, 0.12), ronde(0.56, 0.36, 0.32, 0.50, 0.12), boite(0.40, 0.44, 0.20, 0.14), creux(0.28, 0.70, 0.10), creux(0.72, 0.70, 0.10), ronde(0.20, 0.20, 0.18, 0.18, 0.04), ronde(0.62, 0.20, 0.18, 0.18, 0.04)], couleur: [0x4A, 0x5B, 0x5B], degrade: None },
    Dessin { libelle: "Télescope", formes: &[trait_(0.16, 0.64, 0.70, 0.30, 0.13), trait_(0.70, 0.30, 0.84, 0.21, 0.19), trait_(0.46, 0.48, 0.30, 0.90, 0.04), trait_(0.46, 0.48, 0.62, 0.90, 0.04)], couleur: [0x4A, 0x5B, 0x7B], degrade: None },
    Dessin { libelle: "Ballon de baudruche", formes: &[rond(0.50, 0.38, 0.28), poly(&[(0.46, 0.64), (0.54, 0.64), (0.50, 0.70)]), trait_(0.50, 0.70, 0.44, 0.80, 0.025), trait_(0.44, 0.80, 0.54, 0.92, 0.025), creux(0.40, 0.28, 0.05)], couleur: [0xC9, 0x4C, 0x7B], degrade: None },
    Dessin { libelle: "Bonhomme de neige", formes: &[rond(0.50, 0.70, 0.22), rond(0.50, 0.38, 0.16), creux(0.45, 0.34, 0.03), creux(0.55, 0.34, 0.03), creux(0.50, 0.64, 0.03), creux(0.50, 0.76, 0.03), boite(0.36, 0.20, 0.28, 0.04), boite(0.41, 0.08, 0.18, 0.13)], couleur: [0x7B, 0x9B, 0xB1], degrade: None },

    // =====================================================================
    // 200 DESSINS DE PLUS — DEMANDE EXPLICITE (2026-10-03) : « génère encore 200
    // vignettes variées ». Lettres, chiffres, flèches et polygones sont CALCULÉS
    // (arcs approchés par des segments, flèches tournées autour du centre) ;
    // le reste est dessiné à la main. Toujours EN FIN, pour la même raison.
    // =====================================================================
    Dessin { libelle: "Lettre A", formes: &[trait_(0.28, 0.84, 0.5, 0.16, 0.09), trait_(0.5, 0.16, 0.72, 0.84, 0.09), trait_(0.36, 0.6, 0.64, 0.6, 0.09)], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Lettre B", formes: &[trait_(0.3, 0.16, 0.3, 0.84, 0.09), trait_(0.3, 0.16, 0.52, 0.16, 0.09), trait_(0.52, 0.16, 0.52, 0.16, 0.09), trait_(0.52, 0.16, 0.578, 0.17, 0.09), trait_(0.578, 0.17, 0.629, 0.2, 0.09), trait_(0.629, 0.2, 0.667, 0.245, 0.09), trait_(0.667, 0.245, 0.687, 0.3, 0.09), trait_(0.687, 0.3, 0.687, 0.36, 0.09), trait_(0.687, 0.36, 0.667, 0.415, 0.09), trait_(0.667, 0.415, 0.629, 0.46, 0.09), trait_(0.629, 0.46, 0.578, 0.49, 0.09), trait_(0.578, 0.49, 0.52, 0.5, 0.09), trait_(0.52, 0.5, 0.3, 0.5, 0.09), trait_(0.3, 0.5, 0.54, 0.5, 0.09), trait_(0.54, 0.5, 0.54, 0.5, 0.09), trait_(0.54, 0.5, 0.602, 0.51, 0.09), trait_(0.602, 0.51, 0.656, 0.54, 0.09), trait_(0.656, 0.54, 0.696, 0.585, 0.09), trait_(0.696, 0.585, 0.717, 0.64, 0.09), trait_(0.717, 0.64, 0.717, 0.7, 0.09), trait_(0.717, 0.7, 0.696, 0.755, 0.09), trait_(0.696, 0.755, 0.656, 0.8, 0.09), trait_(0.656, 0.8, 0.602, 0.83, 0.09), trait_(0.602, 0.83, 0.54, 0.84, 0.09), trait_(0.54, 0.84, 0.3, 0.84, 0.09)], couleur: [0x9B, 0x5B, 0x7B], degrade: None },
    Dessin { libelle: "Lettre C", formes: &[trait_(0.704, 0.719, 0.64, 0.794, 0.09), trait_(0.64, 0.794, 0.562, 0.835, 0.09), trait_(0.562, 0.835, 0.478, 0.835, 0.09), trait_(0.478, 0.835, 0.4, 0.794, 0.09), trait_(0.4, 0.794, 0.336, 0.719, 0.09), trait_(0.336, 0.719, 0.294, 0.616, 0.09), trait_(0.294, 0.616, 0.28, 0.5, 0.09), trait_(0.28, 0.5, 0.294, 0.384, 0.09), trait_(0.294, 0.384, 0.336, 0.281, 0.09), trait_(0.336, 0.281, 0.4, 0.206, 0.09), trait_(0.4, 0.206, 0.478, 0.165, 0.09), trait_(0.478, 0.165, 0.562, 0.165, 0.09), trait_(0.562, 0.165, 0.64, 0.206, 0.09), trait_(0.64, 0.206, 0.704, 0.281, 0.09)], couleur: [0x4C, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Lettre D", formes: &[trait_(0.3, 0.16, 0.3, 0.84, 0.09), trait_(0.3, 0.16, 0.44, 0.16, 0.09), trait_(0.44, 0.16, 0.44, 0.16, 0.09), trait_(0.44, 0.16, 0.536, 0.181, 0.09), trait_(0.536, 0.181, 0.62, 0.24, 0.09), trait_(0.62, 0.24, 0.682, 0.33, 0.09), trait_(0.682, 0.33, 0.716, 0.441, 0.09), trait_(0.716, 0.441, 0.716, 0.559, 0.09), trait_(0.716, 0.559, 0.682, 0.67, 0.09), trait_(0.682, 0.67, 0.62, 0.76, 0.09), trait_(0.62, 0.76, 0.536, 0.819, 0.09), trait_(0.536, 0.819, 0.44, 0.84, 0.09), trait_(0.44, 0.84, 0.3, 0.84, 0.09)], couleur: [0xC9, 0x7B, 0x3C], degrade: None },
    Dessin { libelle: "Lettre E", formes: &[trait_(0.7, 0.16, 0.3, 0.16, 0.09), trait_(0.3, 0.16, 0.3, 0.84, 0.09), trait_(0.3, 0.84, 0.7, 0.84, 0.09), trait_(0.3, 0.5, 0.62, 0.5, 0.09)], couleur: [0x7B, 0x5B, 0x9B], degrade: None },
    Dessin { libelle: "Lettre F", formes: &[trait_(0.7, 0.16, 0.3, 0.16, 0.09), trait_(0.3, 0.16, 0.3, 0.84, 0.09), trait_(0.3, 0.5, 0.62, 0.5, 0.09)], couleur: [0x3C, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Lettre G", formes: &[trait_(0.756, 0.559, 0.726, 0.673, 0.09), trait_(0.726, 0.673, 0.67, 0.765, 0.09), trait_(0.67, 0.765, 0.594, 0.823, 0.09), trait_(0.594, 0.823, 0.509, 0.84, 0.09), trait_(0.509, 0.84, 0.425, 0.812, 0.09), trait_(0.425, 0.812, 0.353, 0.745, 0.09), trait_(0.353, 0.745, 0.303, 0.645, 0.09), trait_(0.303, 0.645, 0.281, 0.528, 0.09), trait_(0.281, 0.528, 0.289, 0.406, 0.09), trait_(0.289, 0.406, 0.327, 0.297, 0.09), trait_(0.327, 0.297, 0.39, 0.214, 0.09), trait_(0.39, 0.214, 0.47, 0.167, 0.09), trait_(0.47, 0.167, 0.556, 0.164, 0.09), trait_(0.556, 0.164, 0.638, 0.204, 0.09), trait_(0.638, 0.204, 0.704, 0.281, 0.09), trait_(0.56, 0.54, 0.76, 0.54, 0.09)], couleur: [0xB1, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Lettre H", formes: &[trait_(0.3, 0.16, 0.3, 0.84, 0.09), trait_(0.7, 0.16, 0.7, 0.84, 0.09), trait_(0.3, 0.5, 0.7, 0.5, 0.09)], couleur: [0x8A, 0x7B, 0x4C], degrade: None },
    Dessin { libelle: "Lettre I", formes: &[trait_(0.5, 0.16, 0.5, 0.84, 0.09), trait_(0.36, 0.16, 0.64, 0.16, 0.09), trait_(0.36, 0.84, 0.64, 0.84, 0.09)], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Lettre J", formes: &[trait_(0.44, 0.16, 0.72, 0.16, 0.09), trait_(0.64, 0.16, 0.64, 0.62, 0.09), trait_(0.64, 0.62, 0.64, 0.62, 0.09), trait_(0.64, 0.62, 0.63, 0.695, 0.09), trait_(0.63, 0.695, 0.603, 0.761, 0.09), trait_(0.603, 0.761, 0.56, 0.811, 0.09), trait_(0.56, 0.811, 0.508, 0.837, 0.09), trait_(0.508, 0.837, 0.452, 0.837, 0.09), trait_(0.452, 0.837, 0.4, 0.811, 0.09), trait_(0.4, 0.811, 0.357, 0.761, 0.09), trait_(0.357, 0.761, 0.33, 0.695, 0.09), trait_(0.33, 0.695, 0.32, 0.62, 0.09)], couleur: [0x9B, 0x5B, 0x7B], degrade: None },
    Dessin { libelle: "Lettre K", formes: &[trait_(0.3, 0.16, 0.3, 0.84, 0.09), trait_(0.7, 0.16, 0.32, 0.54, 0.09), trait_(0.44, 0.44, 0.72, 0.84, 0.09)], couleur: [0x4C, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Lettre L", formes: &[trait_(0.3, 0.16, 0.3, 0.84, 0.09), trait_(0.3, 0.84, 0.7, 0.84, 0.09)], couleur: [0xC9, 0x7B, 0x3C], degrade: None },
    Dessin { libelle: "Lettre M", formes: &[trait_(0.24, 0.84, 0.24, 0.16, 0.09), trait_(0.24, 0.16, 0.5, 0.58, 0.09), trait_(0.5, 0.58, 0.76, 0.16, 0.09), trait_(0.76, 0.16, 0.76, 0.84, 0.09)], couleur: [0x7B, 0x5B, 0x9B], degrade: None },
    Dessin { libelle: "Lettre N", formes: &[trait_(0.3, 0.84, 0.3, 0.16, 0.09), trait_(0.3, 0.16, 0.7, 0.84, 0.09), trait_(0.7, 0.84, 0.7, 0.16, 0.09)], couleur: [0x3C, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Lettre O", formes: &[trait_(0.73, 0.5, 0.716, 0.616, 0.09), trait_(0.716, 0.616, 0.676, 0.719, 0.09), trait_(0.676, 0.719, 0.615, 0.794, 0.09), trait_(0.615, 0.794, 0.54, 0.835, 0.09), trait_(0.54, 0.835, 0.46, 0.835, 0.09), trait_(0.46, 0.835, 0.385, 0.794, 0.09), trait_(0.385, 0.794, 0.324, 0.719, 0.09), trait_(0.324, 0.719, 0.284, 0.616, 0.09), trait_(0.284, 0.616, 0.27, 0.5, 0.09), trait_(0.27, 0.5, 0.284, 0.384, 0.09), trait_(0.284, 0.384, 0.324, 0.281, 0.09), trait_(0.324, 0.281, 0.385, 0.206, 0.09), trait_(0.385, 0.206, 0.46, 0.165, 0.09), trait_(0.46, 0.165, 0.54, 0.165, 0.09), trait_(0.54, 0.165, 0.615, 0.206, 0.09), trait_(0.615, 0.206, 0.676, 0.281, 0.09), trait_(0.676, 0.281, 0.716, 0.384, 0.09), trait_(0.716, 0.384, 0.73, 0.5, 0.09)], couleur: [0xB1, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Lettre P", formes: &[trait_(0.3, 0.84, 0.3, 0.16, 0.09), trait_(0.3, 0.16, 0.52, 0.16, 0.09), trait_(0.52, 0.16, 0.52, 0.16, 0.09), trait_(0.52, 0.16, 0.582, 0.171, 0.09), trait_(0.582, 0.171, 0.636, 0.202, 0.09), trait_(0.636, 0.202, 0.676, 0.25, 0.09), trait_(0.676, 0.25, 0.697, 0.309, 0.09), trait_(0.697, 0.309, 0.697, 0.371, 0.09), trait_(0.697, 0.371, 0.676, 0.43, 0.09), trait_(0.676, 0.43, 0.636, 0.478, 0.09), trait_(0.636, 0.478, 0.582, 0.509, 0.09), trait_(0.582, 0.509, 0.52, 0.52, 0.09), trait_(0.52, 0.52, 0.3, 0.52, 0.09)], couleur: [0x8A, 0x7B, 0x4C], degrade: None },
    Dessin { libelle: "Lettre Q", formes: &[trait_(0.73, 0.48, 0.716, 0.589, 0.09), trait_(0.716, 0.589, 0.676, 0.686, 0.09), trait_(0.676, 0.686, 0.615, 0.757, 0.09), trait_(0.615, 0.757, 0.54, 0.795, 0.09), trait_(0.54, 0.795, 0.46, 0.795, 0.09), trait_(0.46, 0.795, 0.385, 0.757, 0.09), trait_(0.385, 0.757, 0.324, 0.686, 0.09), trait_(0.324, 0.686, 0.284, 0.589, 0.09), trait_(0.284, 0.589, 0.27, 0.48, 0.09), trait_(0.27, 0.48, 0.284, 0.371, 0.09), trait_(0.284, 0.371, 0.324, 0.274, 0.09), trait_(0.324, 0.274, 0.385, 0.203, 0.09), trait_(0.385, 0.203, 0.46, 0.165, 0.09), trait_(0.46, 0.165, 0.54, 0.165, 0.09), trait_(0.54, 0.165, 0.615, 0.203, 0.09), trait_(0.615, 0.203, 0.676, 0.274, 0.09), trait_(0.676, 0.274, 0.716, 0.371, 0.09), trait_(0.716, 0.371, 0.73, 0.48, 0.09), trait_(0.56, 0.66, 0.76, 0.88, 0.09)], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Lettre R", formes: &[trait_(0.3, 0.84, 0.3, 0.16, 0.09), trait_(0.3, 0.16, 0.52, 0.16, 0.09), trait_(0.52, 0.16, 0.52, 0.16, 0.09), trait_(0.52, 0.16, 0.582, 0.171, 0.09), trait_(0.582, 0.171, 0.636, 0.202, 0.09), trait_(0.636, 0.202, 0.676, 0.25, 0.09), trait_(0.676, 0.25, 0.697, 0.309, 0.09), trait_(0.697, 0.309, 0.697, 0.371, 0.09), trait_(0.697, 0.371, 0.676, 0.43, 0.09), trait_(0.676, 0.43, 0.636, 0.478, 0.09), trait_(0.636, 0.478, 0.582, 0.509, 0.09), trait_(0.582, 0.509, 0.52, 0.52, 0.09), trait_(0.52, 0.52, 0.3, 0.52, 0.09), trait_(0.46, 0.52, 0.72, 0.84, 0.09)], couleur: [0x9B, 0x5B, 0x7B], degrade: None },
    Dessin { libelle: "Lettre S", formes: &[trait_(0.673, 0.245, 0.629, 0.2, 0.09), trait_(0.629, 0.2, 0.568, 0.17, 0.09), trait_(0.568, 0.17, 0.5, 0.16, 0.09), trait_(0.5, 0.16, 0.432, 0.17, 0.09), trait_(0.432, 0.17, 0.371, 0.2, 0.09), trait_(0.371, 0.2, 0.327, 0.245, 0.09), trait_(0.327, 0.245, 0.303, 0.3, 0.09), trait_(0.303, 0.3, 0.303, 0.36, 0.09), trait_(0.303, 0.36, 0.327, 0.415, 0.09), trait_(0.327, 0.415, 0.371, 0.46, 0.09), trait_(0.371, 0.46, 0.432, 0.49, 0.09), trait_(0.432, 0.49, 0.5, 0.5, 0.09), trait_(0.5, 0.5, 0.5, 0.5, 0.09), trait_(0.5, 0.5, 0.568, 0.51, 0.09), trait_(0.568, 0.51, 0.629, 0.54, 0.09), trait_(0.629, 0.54, 0.673, 0.585, 0.09), trait_(0.673, 0.585, 0.697, 0.64, 0.09), trait_(0.697, 0.64, 0.697, 0.7, 0.09), trait_(0.697, 0.7, 0.673, 0.755, 0.09), trait_(0.673, 0.755, 0.629, 0.8, 0.09), trait_(0.629, 0.8, 0.568, 0.83, 0.09), trait_(0.568, 0.83, 0.5, 0.84, 0.09), trait_(0.5, 0.84, 0.432, 0.83, 0.09), trait_(0.432, 0.83, 0.371, 0.8, 0.09), trait_(0.371, 0.8, 0.327, 0.755, 0.09)], couleur: [0x4C, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Lettre T", formes: &[trait_(0.26, 0.16, 0.74, 0.16, 0.09), trait_(0.5, 0.16, 0.5, 0.84, 0.09)], couleur: [0xC9, 0x7B, 0x3C], degrade: None },
    Dessin { libelle: "Lettre U", formes: &[trait_(0.3, 0.16, 0.3, 0.6, 0.09), trait_(0.3, 0.6, 0.3, 0.6, 0.09), trait_(0.3, 0.6, 0.312, 0.682, 0.09), trait_(0.312, 0.682, 0.347, 0.754, 0.09), trait_(0.347, 0.754, 0.4, 0.808, 0.09), trait_(0.4, 0.808, 0.465, 0.836, 0.09), trait_(0.465, 0.836, 0.535, 0.836, 0.09), trait_(0.535, 0.836, 0.6, 0.808, 0.09), trait_(0.6, 0.808, 0.653, 0.754, 0.09), trait_(0.653, 0.754, 0.688, 0.682, 0.09), trait_(0.688, 0.682, 0.7, 0.6, 0.09), trait_(0.7, 0.6, 0.7, 0.16, 0.09)], couleur: [0x7B, 0x5B, 0x9B], degrade: None },
    Dessin { libelle: "Lettre V", formes: &[trait_(0.28, 0.16, 0.5, 0.84, 0.09), trait_(0.5, 0.84, 0.72, 0.16, 0.09)], couleur: [0x3C, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Lettre W", formes: &[trait_(0.2, 0.16, 0.34, 0.84, 0.09), trait_(0.34, 0.84, 0.5, 0.4, 0.09), trait_(0.5, 0.4, 0.66, 0.84, 0.09), trait_(0.66, 0.84, 0.8, 0.16, 0.09)], couleur: [0xB1, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Lettre X", formes: &[trait_(0.3, 0.16, 0.7, 0.84, 0.09), trait_(0.7, 0.16, 0.3, 0.84, 0.09)], couleur: [0x8A, 0x7B, 0x4C], degrade: None },
    Dessin { libelle: "Lettre Y", formes: &[trait_(0.28, 0.16, 0.5, 0.5, 0.09), trait_(0.5, 0.5, 0.72, 0.16, 0.09), trait_(0.5, 0.5, 0.5, 0.84, 0.09)], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Lettre Z", formes: &[trait_(0.3, 0.16, 0.7, 0.16, 0.09), trait_(0.7, 0.16, 0.3, 0.84, 0.09), trait_(0.3, 0.84, 0.7, 0.84, 0.09)], couleur: [0x9B, 0x5B, 0x7B], degrade: None },
    Dessin { libelle: "Chiffre 0", formes: &[trait_(0.7, 0.5, 0.688, 0.609, 0.09), trait_(0.688, 0.609, 0.653, 0.706, 0.09), trait_(0.653, 0.706, 0.6, 0.777, 0.09), trait_(0.6, 0.777, 0.535, 0.815, 0.09), trait_(0.535, 0.815, 0.465, 0.815, 0.09), trait_(0.465, 0.815, 0.4, 0.777, 0.09), trait_(0.4, 0.777, 0.347, 0.706, 0.09), trait_(0.347, 0.706, 0.312, 0.609, 0.09), trait_(0.312, 0.609, 0.3, 0.5, 0.09), trait_(0.3, 0.5, 0.312, 0.391, 0.09), trait_(0.312, 0.391, 0.347, 0.294, 0.09), trait_(0.347, 0.294, 0.4, 0.223, 0.09), trait_(0.4, 0.223, 0.465, 0.185, 0.09), trait_(0.465, 0.185, 0.535, 0.185, 0.09), trait_(0.535, 0.185, 0.6, 0.223, 0.09), trait_(0.6, 0.223, 0.653, 0.294, 0.09), trait_(0.653, 0.294, 0.688, 0.391, 0.09), trait_(0.688, 0.391, 0.7, 0.5, 0.09), trait_(0.4, 0.64, 0.6, 0.36, 0.09)], couleur: [0x4C, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Chiffre 1", formes: &[trait_(0.38, 0.28, 0.54, 0.16, 0.09), trait_(0.54, 0.16, 0.54, 0.84, 0.09), trait_(0.38, 0.84, 0.68, 0.84, 0.09)], couleur: [0xC9, 0x7B, 0x3C], degrade: None },
    Dessin { libelle: "Chiffre 2", formes: &[trait_(0.303, 0.309, 0.329, 0.247, 0.09), trait_(0.329, 0.247, 0.377, 0.198, 0.09), trait_(0.377, 0.198, 0.443, 0.168, 0.09), trait_(0.443, 0.168, 0.515, 0.161, 0.09), trait_(0.515, 0.161, 0.586, 0.178, 0.09), trait_(0.586, 0.178, 0.645, 0.216, 0.09), trait_(0.645, 0.216, 0.685, 0.272, 0.09), trait_(0.685, 0.272, 0.7, 0.337, 0.09), trait_(0.7, 0.337, 0.688, 0.402, 0.09), trait_(0.688, 0.402, 0.3, 0.84, 0.09), trait_(0.3, 0.84, 0.72, 0.84, 0.09)], couleur: [0x7B, 0x5B, 0x9B], degrade: None },
    Dessin { libelle: "Chiffre 3", formes: &[trait_(0.321, 0.272, 0.356, 0.219, 0.09), trait_(0.356, 0.219, 0.41, 0.18, 0.09), trait_(0.41, 0.18, 0.475, 0.161, 0.09), trait_(0.475, 0.161, 0.544, 0.165, 0.09), trait_(0.544, 0.165, 0.607, 0.189, 0.09), trait_(0.607, 0.189, 0.656, 0.232, 0.09), trait_(0.656, 0.232, 0.684, 0.288, 0.09), trait_(0.684, 0.288, 0.689, 0.35, 0.09), trait_(0.689, 0.35, 0.669, 0.408, 0.09), trait_(0.669, 0.408, 0.626, 0.457, 0.09), trait_(0.626, 0.457, 0.568, 0.489, 0.09), trait_(0.568, 0.489, 0.5, 0.5, 0.09), trait_(0.5, 0.5, 0.5, 0.5, 0.09), trait_(0.5, 0.5, 0.575, 0.511, 0.09), trait_(0.575, 0.511, 0.64, 0.543, 0.09), trait_(0.64, 0.543, 0.686, 0.592, 0.09), trait_(0.686, 0.592, 0.709, 0.65, 0.09), trait_(0.709, 0.65, 0.704, 0.712, 0.09), trait_(0.704, 0.712, 0.672, 0.768, 0.09), trait_(0.672, 0.768, 0.618, 0.811, 0.09), trait_(0.618, 0.811, 0.548, 0.835, 0.09), trait_(0.548, 0.835, 0.473, 0.839, 0.09), trait_(0.473, 0.839, 0.4, 0.82, 0.09), trait_(0.4, 0.82, 0.341, 0.781, 0.09), trait_(0.341, 0.781, 0.303, 0.728, 0.09)], couleur: [0x3C, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Chiffre 4", formes: &[trait_(0.62, 0.84, 0.62, 0.16, 0.09), trait_(0.62, 0.16, 0.28, 0.62, 0.09), trait_(0.28, 0.62, 0.74, 0.62, 0.09)], couleur: [0xB1, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Chiffre 5", formes: &[trait_(0.7, 0.16, 0.36, 0.16, 0.09), trait_(0.36, 0.16, 0.33, 0.48, 0.09), trait_(0.33, 0.48, 0.339, 0.511, 0.09), trait_(0.339, 0.511, 0.397, 0.466, 0.09), trait_(0.397, 0.466, 0.469, 0.442, 0.09), trait_(0.469, 0.442, 0.544, 0.444, 0.09), trait_(0.544, 0.444, 0.614, 0.472, 0.09), trait_(0.614, 0.472, 0.669, 0.521, 0.09), trait_(0.669, 0.521, 0.702, 0.586, 0.09), trait_(0.702, 0.586, 0.709, 0.657, 0.09), trait_(0.709, 0.657, 0.689, 0.727, 0.09), trait_(0.689, 0.727, 0.645, 0.785, 0.09), trait_(0.645, 0.785, 0.582, 0.824, 0.09), trait_(0.582, 0.824, 0.508, 0.84, 0.09), trait_(0.508, 0.84, 0.433, 0.83, 0.09), trait_(0.433, 0.83, 0.367, 0.795, 0.09), trait_(0.367, 0.795, 0.318, 0.74, 0.09)], couleur: [0x8A, 0x7B, 0x4C], degrade: None },
    Dessin { libelle: "Chiffre 6", formes: &[trait_(0.7, 0.64, 0.688, 0.708, 0.09), trait_(0.688, 0.708, 0.653, 0.769, 0.09), trait_(0.653, 0.769, 0.6, 0.813, 0.09), trait_(0.6, 0.813, 0.535, 0.837, 0.09), trait_(0.535, 0.837, 0.465, 0.837, 0.09), trait_(0.465, 0.837, 0.4, 0.813, 0.09), trait_(0.4, 0.813, 0.347, 0.769, 0.09), trait_(0.347, 0.769, 0.312, 0.708, 0.09), trait_(0.312, 0.708, 0.3, 0.64, 0.09), trait_(0.3, 0.64, 0.312, 0.572, 0.09), trait_(0.312, 0.572, 0.347, 0.511, 0.09), trait_(0.347, 0.511, 0.4, 0.467, 0.09), trait_(0.4, 0.467, 0.465, 0.443, 0.09), trait_(0.465, 0.443, 0.535, 0.443, 0.09), trait_(0.535, 0.443, 0.6, 0.467, 0.09), trait_(0.6, 0.467, 0.653, 0.511, 0.09), trait_(0.653, 0.511, 0.688, 0.572, 0.09), trait_(0.688, 0.572, 0.7, 0.64, 0.09), trait_(0.3, 0.64, 0.36, 0.34, 0.09), trait_(0.36, 0.34, 0.62, 0.16, 0.09)], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Chiffre 7", formes: &[trait_(0.28, 0.16, 0.72, 0.16, 0.09), trait_(0.72, 0.16, 0.42, 0.84, 0.09)], couleur: [0x9B, 0x5B, 0x7B], degrade: None },
    Dessin { libelle: "Chiffre 8", formes: &[trait_(0.66, 0.32, 0.65, 0.375, 0.09), trait_(0.65, 0.375, 0.623, 0.423, 0.09), trait_(0.623, 0.423, 0.58, 0.459, 0.09), trait_(0.58, 0.459, 0.528, 0.478, 0.09), trait_(0.528, 0.478, 0.472, 0.478, 0.09), trait_(0.472, 0.478, 0.42, 0.459, 0.09), trait_(0.42, 0.459, 0.377, 0.423, 0.09), trait_(0.377, 0.423, 0.35, 0.375, 0.09), trait_(0.35, 0.375, 0.34, 0.32, 0.09), trait_(0.34, 0.32, 0.35, 0.265, 0.09), trait_(0.35, 0.265, 0.377, 0.217, 0.09), trait_(0.377, 0.217, 0.42, 0.181, 0.09), trait_(0.42, 0.181, 0.472, 0.162, 0.09), trait_(0.472, 0.162, 0.528, 0.162, 0.09), trait_(0.528, 0.162, 0.58, 0.181, 0.09), trait_(0.58, 0.181, 0.623, 0.217, 0.09), trait_(0.623, 0.217, 0.65, 0.265, 0.09), trait_(0.65, 0.265, 0.66, 0.32, 0.09), trait_(0.69, 0.66, 0.679, 0.722, 0.09), trait_(0.679, 0.722, 0.646, 0.776, 0.09), trait_(0.646, 0.776, 0.595, 0.816, 0.09), trait_(0.595, 0.816, 0.533, 0.837, 0.09), trait_(0.533, 0.837, 0.467, 0.837, 0.09), trait_(0.467, 0.837, 0.405, 0.816, 0.09), trait_(0.405, 0.816, 0.354, 0.776, 0.09), trait_(0.354, 0.776, 0.321, 0.722, 0.09), trait_(0.321, 0.722, 0.31, 0.66, 0.09), trait_(0.31, 0.66, 0.321, 0.598, 0.09), trait_(0.321, 0.598, 0.354, 0.544, 0.09), trait_(0.354, 0.544, 0.405, 0.504, 0.09), trait_(0.405, 0.504, 0.467, 0.483, 0.09), trait_(0.467, 0.483, 0.533, 0.483, 0.09), trait_(0.533, 0.483, 0.595, 0.504, 0.09), trait_(0.595, 0.504, 0.646, 0.544, 0.09), trait_(0.646, 0.544, 0.679, 0.598, 0.09), trait_(0.679, 0.598, 0.69, 0.66, 0.09)], couleur: [0x4C, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Chiffre 9", formes: &[trait_(0.7, 0.36, 0.688, 0.428, 0.09), trait_(0.688, 0.428, 0.653, 0.489, 0.09), trait_(0.653, 0.489, 0.6, 0.533, 0.09), trait_(0.6, 0.533, 0.535, 0.557, 0.09), trait_(0.535, 0.557, 0.465, 0.557, 0.09), trait_(0.465, 0.557, 0.4, 0.533, 0.09), trait_(0.4, 0.533, 0.347, 0.489, 0.09), trait_(0.347, 0.489, 0.312, 0.428, 0.09), trait_(0.312, 0.428, 0.3, 0.36, 0.09), trait_(0.3, 0.36, 0.312, 0.292, 0.09), trait_(0.312, 0.292, 0.347, 0.231, 0.09), trait_(0.347, 0.231, 0.4, 0.187, 0.09), trait_(0.4, 0.187, 0.465, 0.163, 0.09), trait_(0.465, 0.163, 0.535, 0.163, 0.09), trait_(0.535, 0.163, 0.6, 0.187, 0.09), trait_(0.6, 0.187, 0.653, 0.231, 0.09), trait_(0.653, 0.231, 0.688, 0.292, 0.09), trait_(0.688, 0.292, 0.7, 0.36, 0.09), trait_(0.7, 0.36, 0.64, 0.66, 0.09), trait_(0.64, 0.66, 0.38, 0.84, 0.09)], couleur: [0xC9, 0x7B, 0x3C], degrade: None },
    Dessin { libelle: "Flèche haut", formes: &[trait_(0.5, 0.86, 0.5, 0.38, 0.13), poly(&[(0.5, 0.12), (0.8, 0.44), (0.2, 0.44)])], couleur: [0x7B, 0x5B, 0x9B], degrade: None },
    Dessin { libelle: "Flèche haut-droite", formes: &[trait_(0.245, 0.755, 0.585, 0.415, 0.13), poly(&[(0.769, 0.231), (0.755, 0.67), (0.33, 0.245)])], couleur: [0x3C, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Flèche droite", formes: &[trait_(0.14, 0.5, 0.62, 0.5, 0.13), poly(&[(0.88, 0.5), (0.56, 0.8), (0.56, 0.2)])], couleur: [0xB1, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Flèche bas-droite", formes: &[trait_(0.245, 0.245, 0.585, 0.585, 0.13), poly(&[(0.769, 0.769), (0.33, 0.755), (0.755, 0.33)])], couleur: [0x8A, 0x7B, 0x4C], degrade: None },
    Dessin { libelle: "Flèche bas", formes: &[trait_(0.5, 0.14, 0.5, 0.62, 0.13), poly(&[(0.5, 0.88), (0.2, 0.56), (0.8, 0.56)])], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Flèche bas-gauche", formes: &[trait_(0.755, 0.245, 0.415, 0.585, 0.13), poly(&[(0.231, 0.769), (0.245, 0.33), (0.67, 0.755)])], couleur: [0x9B, 0x5B, 0x7B], degrade: None },
    Dessin { libelle: "Flèche gauche", formes: &[trait_(0.86, 0.5, 0.38, 0.5, 0.13), poly(&[(0.12, 0.5), (0.44, 0.2), (0.44, 0.8)])], couleur: [0x4C, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Flèche haut-gauche", formes: &[trait_(0.755, 0.755, 0.415, 0.415, 0.13), poly(&[(0.231, 0.231), (0.67, 0.245), (0.245, 0.67)])], couleur: [0xC9, 0x7B, 0x3C], degrade: None },
    Dessin { libelle: "Triangle", formes: &[poly(&[(0.5, 0.12), (0.9, 0.84), (0.1, 0.84)])], couleur: [0x7B, 0x5B, 0x9B], degrade: None },
    Dessin { libelle: "Losange", formes: &[poly(&[(0.5, 0.12), (0.88, 0.5), (0.5, 0.88), (0.12, 0.5)])], couleur: [0x3C, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Pentagone", formes: &[poly(&[(0.5, 0.14), (0.88, 0.416), (0.735, 0.864), (0.265, 0.864), (0.12, 0.416)])], couleur: [0xB1, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Hexagone", formes: &[poly(&[(0.9, 0.5), (0.7, 0.846), (0.3, 0.846), (0.1, 0.5), (0.3, 0.154), (0.7, 0.154)])], couleur: [0x8A, 0x7B, 0x4C], degrade: None },
    Dessin { libelle: "Octogone", formes: &[poly(&[(0.653, 0.13), (0.87, 0.347), (0.87, 0.653), (0.653, 0.87), (0.347, 0.87), (0.13, 0.653), (0.13, 0.347), (0.347, 0.13)])], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Anneau", formes: &[rond(0.5, 0.5, 0.38), creux(0.5, 0.5, 0.24)], couleur: [0x9B, 0x5B, 0x7B], degrade: None },
    Dessin { libelle: "Étoile à six branches", formes: &[poly(&[(0.5, 0.1), (0.846, 0.7), (0.154, 0.7)]), poly(&[(0.5, 0.9), (0.154, 0.3), (0.846, 0.3)])], couleur: [0x4C, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Demi-cercle", formes: &[rond(0.5, 0.66, 0.4), trou(0.0, 0.66, 1.0, 0.4)], couleur: [0xC9, 0x7B, 0x3C], degrade: None },
    Dessin { libelle: "Trapèze", formes: &[poly(&[(0.28, 0.24), (0.72, 0.24), (0.9, 0.76), (0.1, 0.76)])], couleur: [0x7B, 0x5B, 0x9B], degrade: None },
    Dessin { libelle: "Parallélogramme", formes: &[poly(&[(0.3, 0.24), (0.92, 0.24), (0.7, 0.76), (0.08, 0.76)])], couleur: [0x3C, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Croix", formes: &[trait_(0.2, 0.2, 0.8, 0.8, 0.16), trait_(0.8, 0.2, 0.2, 0.8, 0.16)], couleur: [0xB1, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Pique", formes: &[rond(0.34, 0.54, 0.18), rond(0.66, 0.54, 0.18), poly(&[(0.16, 0.52), (0.84, 0.52), (0.5, 0.1)]), poly(&[(0.5, 0.56), (0.62, 0.88), (0.38, 0.88)])], couleur: [0x7B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Trèfle", formes: &[rond(0.5, 0.3, 0.17), rond(0.3, 0.58, 0.17), rond(0.7, 0.58, 0.17), poly(&[(0.5, 0.48), (0.62, 0.9), (0.38, 0.9)])], couleur: [0x7B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Carreau", formes: &[poly(&[(0.5, 0.06), (0.8, 0.5), (0.5, 0.94), (0.2, 0.5)])], couleur: [0xC9, 0x3C, 0x3C], degrade: None },
    Dessin { libelle: "Éclaircie", formes: &[rond(0.64, 0.34, 0.17), trait_(0.64, 0.06, 0.64, 0.11, 0.04), trait_(0.89, 0.34, 0.94, 0.34, 0.04), trait_(0.82, 0.16, 0.86, 0.12, 0.04), creux(0.44, 0.62, 0.24), rond(0.28, 0.66, 0.14), rond(0.46, 0.58, 0.18), rond(0.62, 0.68, 0.12), boite(0.14, 0.66, 0.56, 0.14)], couleur: [0xC9, 0x9B, 0x3C], degrade: None },
    Dessin { libelle: "Orage", formes: &[rond(0.34, 0.4, 0.16), rond(0.54, 0.32, 0.2), rond(0.72, 0.42, 0.14), boite(0.18, 0.4, 0.62, 0.16), poly(&[(0.52, 0.54), (0.38, 0.76), (0.5, 0.76), (0.42, 0.94), (0.64, 0.68), (0.52, 0.68), (0.6, 0.54)])], couleur: [0x5B, 0x5B, 0x7B], degrade: None },
    Dessin { libelle: "Neige", formes: &[rond(0.34, 0.4, 0.16), rond(0.54, 0.32, 0.2), rond(0.72, 0.42, 0.14), boite(0.18, 0.4, 0.62, 0.16), rond(0.32, 0.72, 0.04), rond(0.5, 0.78, 0.04), rond(0.68, 0.72, 0.04), rond(0.41, 0.9, 0.035), rond(0.59, 0.9, 0.035)], couleur: [0x7B, 0x9B, 0xB1], degrade: None },
    Dessin { libelle: "Brouillard", formes: &[trait_(0.14, 0.3, 0.7, 0.3, 0.07), trait_(0.3, 0.46, 0.86, 0.46, 0.07), trait_(0.14, 0.62, 0.76, 0.62, 0.07), trait_(0.28, 0.78, 0.84, 0.78, 0.07)], couleur: [0x7B, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Vent", formes: &[trait_(0.1, 0.4, 0.62, 0.4, 0.06), trait_(0.62, 0.4, 0.62, 0.4, 0.06), trait_(0.62, 0.4, 0.654, 0.394, 0.06), trait_(0.654, 0.394, 0.684, 0.377, 0.06), trait_(0.684, 0.377, 0.707, 0.35, 0.06), trait_(0.707, 0.35, 0.718, 0.317, 0.06), trait_(0.718, 0.317, 0.718, 0.283, 0.06), trait_(0.718, 0.283, 0.707, 0.25, 0.06), trait_(0.707, 0.25, 0.684, 0.223, 0.06), trait_(0.684, 0.223, 0.654, 0.206, 0.06), trait_(0.654, 0.206, 0.62, 0.2, 0.06), trait_(0.62, 0.2, 0.586, 0.206, 0.06), trait_(0.586, 0.206, 0.556, 0.223, 0.06), trait_(0.556, 0.223, 0.533, 0.25, 0.06), trait_(0.1, 0.6, 0.74, 0.6, 0.06), trait_(0.74, 0.6, 0.74, 0.6, 0.06), trait_(0.74, 0.6, 0.774, 0.606, 0.06), trait_(0.774, 0.606, 0.804, 0.623, 0.06), trait_(0.804, 0.623, 0.827, 0.65, 0.06), trait_(0.827, 0.65, 0.838, 0.683, 0.06), trait_(0.838, 0.683, 0.838, 0.717, 0.06), trait_(0.838, 0.717, 0.827, 0.75, 0.06), trait_(0.827, 0.75, 0.804, 0.777, 0.06), trait_(0.804, 0.777, 0.774, 0.794, 0.06), trait_(0.774, 0.794, 0.74, 0.8, 0.06), trait_(0.74, 0.8, 0.706, 0.794, 0.06), trait_(0.706, 0.794, 0.676, 0.777, 0.06), trait_(0.676, 0.777, 0.653, 0.75, 0.06), trait_(0.1, 0.5, 0.4, 0.5, 0.06)], couleur: [0x5B, 0x9B, 0xB1], degrade: None },
    Dessin { libelle: "Tornade", formes: &[trait_(0.14, 0.18, 0.86, 0.18, 0.07), trait_(0.22, 0.32, 0.8, 0.32, 0.07), trait_(0.3, 0.46, 0.72, 0.46, 0.07), trait_(0.38, 0.6, 0.64, 0.6, 0.07), trait_(0.44, 0.74, 0.56, 0.74, 0.07), trait_(0.48, 0.86, 0.52, 0.86, 0.07)], couleur: [0x6B, 0x7B, 0x8A], degrade: None },
    Dessin { libelle: "Visage triste", formes: &[rond(0.5, 0.5, 0.38), creux(0.37, 0.4, 0.05), creux(0.63, 0.4, 0.05), moins(Trace::Trait { x1: 0.35, y1: 0.752, x2: 0.377, y2: 0.71, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.377, y1: 0.71, x2: 0.42, y2: 0.679, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.42, y1: 0.679, x2: 0.472, y2: 0.662, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.472, y1: 0.662, x2: 0.528, y2: 0.662, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.528, y1: 0.662, x2: 0.58, y2: 0.679, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.58, y1: 0.679, x2: 0.623, y2: 0.71, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.623, y1: 0.71, x2: 0.65, y2: 0.752, epaisseur: 0.05 })], couleur: [0xD9, 0xB1, 0x3F], degrade: None },
    Dessin { libelle: "Visage surpris", formes: &[rond(0.5, 0.5, 0.38), creux(0.37, 0.4, 0.05), creux(0.63, 0.4, 0.05), creux(0.5, 0.66, 0.08)], couleur: [0xD9, 0xB1, 0x3F], degrade: None },
    Dessin { libelle: "Clin d'œil", formes: &[rond(0.5, 0.5, 0.38), creux(0.37, 0.4, 0.05), moins(Trace::Trait { x1: 0.56, y1: 0.41, x2: 0.7, y2: 0.41, epaisseur: 0.04 }), moins(Trace::Trait { x1: 0.669, y1: 0.575, x2: 0.638, y2: 0.623, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.638, y1: 0.623, x2: 0.59, y2: 0.659, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.59, y1: 0.659, x2: 0.531, y2: 0.678, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.531, y1: 0.678, x2: 0.469, y2: 0.678, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.469, y1: 0.678, x2: 0.41, y2: 0.659, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.41, y1: 0.659, x2: 0.362, y2: 0.623, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.362, y1: 0.623, x2: 0.331, y2: 0.575, epaisseur: 0.05 })], couleur: [0xD9, 0xB1, 0x3F], degrade: None },
    Dessin { libelle: "Colère", formes: &[rond(0.5, 0.5, 0.38), creux(0.37, 0.4, 0.05), creux(0.63, 0.4, 0.05), moins(Trace::Trait { x1: 0.28, y1: 0.28, x2: 0.44, y2: 0.34, epaisseur: 0.04 }), moins(Trace::Trait { x1: 0.72, y1: 0.28, x2: 0.56, y2: 0.34, epaisseur: 0.04 }), moins(Trace::Trait { x1: 0.38, y1: 0.68, x2: 0.62, y2: 0.68, epaisseur: 0.05 })], couleur: [0xC9, 0x5B, 0x3C], degrade: None },
    Dessin { libelle: "Visage neutre", formes: &[rond(0.5, 0.5, 0.38), creux(0.37, 0.4, 0.05), creux(0.63, 0.4, 0.05), moins(Trace::Trait { x1: 0.36, y1: 0.66, x2: 0.64, y2: 0.66, epaisseur: 0.05 })], couleur: [0xD9, 0xB1, 0x3F], degrade: None },
    Dessin { libelle: "Amoureux", formes: &[rond(0.5, 0.5, 0.38), creux(0.33, 0.38, 0.045), creux(0.41, 0.38, 0.045), perce(&[(0.29, 0.4), (0.45, 0.4), (0.37, 0.5)]), creux(0.59, 0.38, 0.045), creux(0.67, 0.38, 0.045), perce(&[(0.55, 0.4), (0.71, 0.4), (0.63, 0.5)]), moins(Trace::Trait { x1: 0.65, y1: 0.588, x2: 0.623, y2: 0.63, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.623, y1: 0.63, x2: 0.58, y2: 0.661, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.58, y1: 0.661, x2: 0.528, y2: 0.678, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.528, y1: 0.678, x2: 0.472, y2: 0.678, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.472, y1: 0.678, x2: 0.42, y2: 0.661, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.42, y1: 0.661, x2: 0.377, y2: 0.63, epaisseur: 0.05 }), moins(Trace::Trait { x1: 0.377, y1: 0.63, x2: 0.35, y2: 0.588, epaisseur: 0.05 })], couleur: [0xD9, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Rire", formes: &[rond(0.5, 0.5, 0.38), moins(Trace::Trait { x1: 0.3, y1: 0.42, x2: 0.37, y2: 0.34, epaisseur: 0.04 }), moins(Trace::Trait { x1: 0.37, y1: 0.34, x2: 0.44, y2: 0.42, epaisseur: 0.04 }), moins(Trace::Trait { x1: 0.56, y1: 0.42, x2: 0.63, y2: 0.34, epaisseur: 0.04 }), moins(Trace::Trait { x1: 0.63, y1: 0.34, x2: 0.7, y2: 0.42, epaisseur: 0.04 }), perce(&[(0.28, 0.54), (0.72, 0.54), (0.66, 0.68), (0.56, 0.76), (0.44, 0.76), (0.34, 0.68)])], couleur: [0xD9, 0xB1, 0x3F], degrade: None },
    Dessin { libelle: "Endormi", formes: &[rond(0.46, 0.54, 0.34), moins(Trace::Trait { x1: 0.416, y1: 0.477, x2: 0.404, y2: 0.492, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.404, y1: 0.492, x2: 0.385, y2: 0.503, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.385, y1: 0.503, x2: 0.362, y2: 0.509, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.362, y1: 0.509, x2: 0.338, y2: 0.509, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.338, y1: 0.509, x2: 0.315, y2: 0.503, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.315, y1: 0.503, x2: 0.296, y2: 0.492, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.296, y1: 0.492, x2: 0.284, y2: 0.477, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.636, y1: 0.477, x2: 0.624, y2: 0.492, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.624, y1: 0.492, x2: 0.605, y2: 0.503, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.605, y1: 0.503, x2: 0.582, y2: 0.509, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.582, y1: 0.509, x2: 0.558, y2: 0.509, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.558, y1: 0.509, x2: 0.535, y2: 0.503, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.535, y1: 0.503, x2: 0.516, y2: 0.492, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.516, y1: 0.492, x2: 0.504, y2: 0.477, epaisseur: 0.035 }), creux(0.46, 0.68, 0.04), trait_(0.76, 0.08, 0.9, 0.08, 0.035), trait_(0.9, 0.08, 0.76, 0.22, 0.035), trait_(0.76, 0.22, 0.9, 0.22, 0.035)], couleur: [0x8A, 0x9B, 0xC9], degrade: None },
    Dessin { libelle: "Tour (échecs)", formes: &[boite(0.3, 0.3, 0.4, 0.46), boite(0.26, 0.16, 0.12, 0.14), boite(0.44, 0.16, 0.12, 0.14), boite(0.62, 0.16, 0.12, 0.14), boite(0.26, 0.26, 0.48, 0.06), ronde(0.22, 0.74, 0.56, 0.14, 0.04)], couleur: [0x7B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Roi (échecs)", formes: &[poly(&[(0.38, 0.4), (0.62, 0.4), (0.68, 0.74), (0.32, 0.74)]), ronde(0.34, 0.32, 0.32, 0.08, 0.03), rond(0.5, 0.28, 0.08), trait_(0.5, 0.06, 0.5, 0.22, 0.05), trait_(0.42, 0.13, 0.58, 0.13, 0.05), ronde(0.22, 0.74, 0.56, 0.14, 0.04)], couleur: [0x7B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Reine (échecs)", formes: &[poly(&[(0.38, 0.4), (0.62, 0.4), (0.68, 0.74), (0.32, 0.74)]), poly(&[(0.32, 0.2), (0.42, 0.32), (0.5, 0.14), (0.58, 0.32), (0.68, 0.2), (0.64, 0.4), (0.36, 0.4)]), rond(0.32, 0.18, 0.045), rond(0.5, 0.12, 0.045), rond(0.68, 0.18, 0.045), ronde(0.22, 0.74, 0.56, 0.14, 0.04)], couleur: [0x7B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Fou (échecs)", formes: &[rond(0.5, 0.34, 0.15), poly(&[(0.4, 0.26), (0.5, 0.1), (0.6, 0.26)]), rond(0.5, 0.08, 0.04), moins(Trace::Trait { x1: 0.46, y1: 0.32, x2: 0.6, y2: 0.22, epaisseur: 0.03 }), poly(&[(0.4, 0.48), (0.6, 0.48), (0.66, 0.74), (0.34, 0.74)]), ronde(0.22, 0.74, 0.56, 0.14, 0.04)], couleur: [0x7B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Cavalier (échecs)", formes: &[poly(&[(0.32, 0.74), (0.36, 0.5), (0.28, 0.4), (0.4, 0.2), (0.5, 0.12), (0.58, 0.18), (0.74, 0.42), (0.7, 0.52), (0.58, 0.46), (0.56, 0.56), (0.66, 0.74)]), creux(0.5, 0.26, 0.03), ronde(0.22, 0.74, 0.56, 0.14, 0.04)], couleur: [0x7B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Lecture", formes: &[poly(&[(0.3, 0.18), (0.8, 0.5), (0.3, 0.82)])], couleur: [0x3C, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Pause", formes: &[ronde(0.28, 0.18, 0.16, 0.64, 0.04), ronde(0.56, 0.18, 0.16, 0.64, 0.04)], couleur: [0xB1, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Arrêt", formes: &[ronde(0.22, 0.22, 0.56, 0.56, 0.06)], couleur: [0xB1, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Suivant", formes: &[poly(&[(0.12, 0.22), (0.46, 0.5), (0.12, 0.78)]), poly(&[(0.44, 0.22), (0.78, 0.5), (0.44, 0.78)]), boite(0.8, 0.22, 0.08, 0.56)], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Précédent", formes: &[poly(&[(0.88, 0.22), (0.54, 0.5), (0.88, 0.78)]), poly(&[(0.56, 0.22), (0.22, 0.5), (0.56, 0.78)]), boite(0.12, 0.22, 0.08, 0.56)], couleur: [0x9B, 0x5B, 0x7B], degrade: None },
    Dessin { libelle: "Répéter", formes: &[trait_(0.24, 0.54, 0.24, 0.34, 0.07), trait_(0.24, 0.34, 0.66, 0.34, 0.07), poly(&[(0.64, 0.22), (0.82, 0.34), (0.64, 0.46)]), trait_(0.76, 0.46, 0.76, 0.66, 0.07), trait_(0.76, 0.66, 0.34, 0.66, 0.07), poly(&[(0.36, 0.54), (0.18, 0.66), (0.36, 0.78)])], couleur: [0x4C, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Aléatoire", formes: &[trait_(0.12, 0.3, 0.34, 0.3, 0.06), trait_(0.34, 0.3, 0.62, 0.7, 0.06), trait_(0.62, 0.7, 0.74, 0.7, 0.06), poly(&[(0.72, 0.6), (0.9, 0.7), (0.72, 0.8)]), trait_(0.12, 0.7, 0.34, 0.7, 0.06), trait_(0.34, 0.7, 0.62, 0.3, 0.06), trait_(0.62, 0.3, 0.74, 0.3, 0.06), poly(&[(0.72, 0.2), (0.9, 0.3), (0.72, 0.4)])], couleur: [0xC9, 0x7B, 0x3C], degrade: None },
    Dessin { libelle: "Enregistrement", formes: &[rond(0.5, 0.5, 0.38), creux(0.5, 0.5, 0.3), rond(0.5, 0.5, 0.2)], couleur: [0xC9, 0x3C, 0x3C], degrade: None },
    Dessin { libelle: "Volume", formes: &[boite(0.12, 0.38, 0.14, 0.24), poly(&[(0.26, 0.38), (0.46, 0.2), (0.46, 0.8), (0.26, 0.62)]), trait_(0.56, 0.361, 0.592, 0.397, 0.05), trait_(0.592, 0.397, 0.613, 0.445, 0.05), trait_(0.613, 0.445, 0.62, 0.5, 0.05), trait_(0.62, 0.5, 0.613, 0.555, 0.05), trait_(0.613, 0.555, 0.592, 0.603, 0.05), trait_(0.592, 0.603, 0.56, 0.639, 0.05), trait_(0.649, 0.254, 0.718, 0.337, 0.05), trait_(0.718, 0.337, 0.755, 0.443, 0.05), trait_(0.755, 0.443, 0.755, 0.557, 0.05), trait_(0.755, 0.557, 0.718, 0.663, 0.05), trait_(0.718, 0.663, 0.649, 0.746, 0.05)], couleur: [0x3C, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Muet", formes: &[boite(0.12, 0.38, 0.14, 0.24), poly(&[(0.26, 0.38), (0.46, 0.2), (0.46, 0.8), (0.26, 0.62)]), trait_(0.62, 0.38, 0.86, 0.62, 0.06), trait_(0.86, 0.38, 0.62, 0.62, 0.06)], couleur: [0xB1, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Signal", formes: &[boite(0.14, 0.66, 0.13, 0.18), boite(0.33, 0.52, 0.13, 0.32), boite(0.52, 0.38, 0.13, 0.46), boite(0.71, 0.2, 0.13, 0.64)], couleur: [0x3C, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Menu", formes: &[trait_(0.2, 0.3, 0.8, 0.3, 0.08), trait_(0.2, 0.5, 0.8, 0.5, 0.08), trait_(0.2, 0.7, 0.8, 0.7, 0.08)], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Grille", formes: &[ronde(0.16, 0.16, 0.18, 0.18, 0.03), ronde(0.41, 0.16, 0.18, 0.18, 0.03), ronde(0.66, 0.16, 0.18, 0.18, 0.03), ronde(0.16, 0.41, 0.18, 0.18, 0.03), ronde(0.41, 0.41, 0.18, 0.18, 0.03), ronde(0.66, 0.41, 0.18, 0.18, 0.03), ronde(0.16, 0.66, 0.18, 0.18, 0.03), ronde(0.41, 0.66, 0.18, 0.18, 0.03), ronde(0.66, 0.66, 0.18, 0.18, 0.03)], couleur: [0x9B, 0x5B, 0x7B], degrade: None },
    Dessin { libelle: "Liste", formes: &[rond(0.22, 0.28, 0.05), trait_(0.36, 0.28, 0.82, 0.28, 0.06), rond(0.22, 0.5, 0.05), trait_(0.36, 0.5, 0.82, 0.5, 0.06), rond(0.22, 0.72, 0.05), trait_(0.36, 0.72, 0.82, 0.72, 0.06)], couleur: [0x4C, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Curseurs", formes: &[trait_(0.16, 0.28, 0.84, 0.28, 0.04), rond(0.66, 0.28, 0.08), trait_(0.16, 0.5, 0.84, 0.5, 0.04), rond(0.34, 0.5, 0.08), trait_(0.16, 0.72, 0.84, 0.72, 0.04), rond(0.56, 0.72, 0.08)], couleur: [0xC9, 0x7B, 0x3C], degrade: None },
    Dessin { libelle: "Filtre", formes: &[poly(&[(0.12, 0.18), (0.88, 0.18), (0.58, 0.52), (0.58, 0.84), (0.42, 0.74), (0.42, 0.52)])], couleur: [0x7B, 0x5B, 0x9B], degrade: None },
    Dessin { libelle: "Copier", formes: &[ronde(0.34, 0.14, 0.5, 0.56, 0.05), trou(0.4, 0.2, 0.38, 0.44), moins(Trace::Boite { x: 0.12, y: 0.3, l: 0.58, h: 0.62, rayon: 0.06 }), ronde(0.16, 0.34, 0.5, 0.54, 0.05), trou(0.22, 0.4, 0.38, 0.42)], couleur: [0x3C, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Annuler", formes: &[trait_(0.54, 0.32, 0.32, 0.32, 0.07), poly(&[(0.34, 0.18), (0.14, 0.32), (0.34, 0.46)]), trait_(0.54, 0.32, 0.629, 0.336, 0.07), trait_(0.629, 0.336, 0.707, 0.381, 0.07), trait_(0.707, 0.381, 0.765, 0.45, 0.07), trait_(0.765, 0.45, 0.796, 0.535, 0.07), trait_(0.796, 0.535, 0.796, 0.625, 0.07), trait_(0.796, 0.625, 0.765, 0.71, 0.07), trait_(0.765, 0.71, 0.707, 0.779, 0.07), trait_(0.707, 0.779, 0.629, 0.824, 0.07), trait_(0.629, 0.824, 0.54, 0.84, 0.07), trait_(0.54, 0.84, 0.4, 0.84, 0.07)], couleur: [0xB1, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Rétablir", formes: &[trait_(0.46, 0.32, 0.68, 0.32, 0.07), poly(&[(0.66, 0.18), (0.86, 0.32), (0.66, 0.46)]), trait_(0.46, 0.32, 0.371, 0.336, 0.07), trait_(0.371, 0.336, 0.293, 0.381, 0.07), trait_(0.293, 0.381, 0.235, 0.45, 0.07), trait_(0.235, 0.45, 0.204, 0.535, 0.07), trait_(0.204, 0.535, 0.204, 0.625, 0.07), trait_(0.204, 0.625, 0.235, 0.71, 0.07), trait_(0.235, 0.71, 0.293, 0.779, 0.07), trait_(0.293, 0.779, 0.371, 0.824, 0.07), trait_(0.371, 0.824, 0.46, 0.84, 0.07), trait_(0.46, 0.84, 0.6, 0.84, 0.07)], couleur: [0x8A, 0x7B, 0x4C], degrade: None },
    Dessin { libelle: "Zoom avant", formes: &[rond(0.42, 0.42, 0.28), creux(0.42, 0.42, 0.2), trait_(0.62, 0.62, 0.86, 0.86, 0.1), trait_(0.32, 0.42, 0.52, 0.42, 0.05), trait_(0.42, 0.32, 0.42, 0.52, 0.05)], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Zoom arrière", formes: &[rond(0.42, 0.42, 0.28), creux(0.42, 0.42, 0.2), trait_(0.62, 0.62, 0.86, 0.86, 0.1), trait_(0.32, 0.42, 0.52, 0.42, 0.05)], couleur: [0x9B, 0x5B, 0x7B], degrade: None },
    Dessin { libelle: "Plein écran", formes: &[trait_(0.16, 0.36, 0.16, 0.16, 0.07), trait_(0.16, 0.16, 0.36, 0.16, 0.07), trait_(0.64, 0.16, 0.84, 0.16, 0.07), trait_(0.84, 0.16, 0.84, 0.36, 0.07), trait_(0.84, 0.64, 0.84, 0.84, 0.07), trait_(0.84, 0.84, 0.64, 0.84, 0.07), trait_(0.36, 0.84, 0.16, 0.84, 0.07), trait_(0.16, 0.84, 0.16, 0.64, 0.07)], couleur: [0x4C, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Fermer", formes: &[rond(0.5, 0.5, 0.38), moins(Trace::Trait { x1: 0.36, y1: 0.36, x2: 0.64, y2: 0.64, epaisseur: 0.07 }), moins(Trace::Trait { x1: 0.64, y1: 0.36, x2: 0.36, y2: 0.64, epaisseur: 0.07 })], couleur: [0xB1, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Information", formes: &[rond(0.5, 0.5, 0.38), trou(0.45, 0.44, 0.1, 0.3), creux(0.5, 0.32, 0.06)], couleur: [0x3C, 0x7B, 0xB1], degrade: None },
    Dessin { libelle: "Aide", formes: &[rond(0.5, 0.5, 0.38), moins(Trace::Trait { x1: 0.38, y1: 0.4, x2: 0.387, y2: 0.359, epaisseur: 0.06 }), moins(Trace::Trait { x1: 0.387, y1: 0.359, x2: 0.408, y2: 0.323, epaisseur: 0.06 }), moins(Trace::Trait { x1: 0.408, y1: 0.323, x2: 0.44, y2: 0.296, epaisseur: 0.06 }), moins(Trace::Trait { x1: 0.44, y1: 0.296, x2: 0.479, y2: 0.282, epaisseur: 0.06 }), moins(Trace::Trait { x1: 0.479, y1: 0.282, x2: 0.521, y2: 0.282, epaisseur: 0.06 }), moins(Trace::Trait { x1: 0.521, y1: 0.282, x2: 0.56, y2: 0.296, epaisseur: 0.06 }), moins(Trace::Trait { x1: 0.56, y1: 0.296, x2: 0.592, y2: 0.323, epaisseur: 0.06 }), moins(Trace::Trait { x1: 0.592, y1: 0.323, x2: 0.613, y2: 0.359, epaisseur: 0.06 }), moins(Trace::Trait { x1: 0.613, y1: 0.359, x2: 0.62, y2: 0.4, epaisseur: 0.06 }), moins(Trace::Trait { x1: 0.62, y1: 0.4, x2: 0.613, y2: 0.441, epaisseur: 0.06 }), moins(Trace::Trait { x1: 0.613, y1: 0.441, x2: 0.592, y2: 0.477, epaisseur: 0.06 }), moins(Trace::Trait { x1: 0.592, y1: 0.477, x2: 0.5, y2: 0.58, epaisseur: 0.06 }), creux(0.5, 0.72, 0.045)], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Interdit", formes: &[rond(0.5, 0.5, 0.38), creux(0.5, 0.5, 0.3), trait_(0.29, 0.29, 0.71, 0.71, 0.08)], couleur: [0xC9, 0x3C, 0x3C], degrade: None },
    Dessin { libelle: "Lien externe", formes: &[ronde(0.14, 0.3, 0.56, 0.56, 0.04), trou(0.21, 0.37, 0.42, 0.42), moins(Trace::Trait { x1: 0.46, y1: 0.54, x2: 0.84, y2: 0.16, epaisseur: 0.16 }), trait_(0.46, 0.54, 0.78, 0.22, 0.06), poly(&[(0.58, 0.14), (0.86, 0.14), (0.86, 0.42)])], couleur: [0x8A, 0x7B, 0x4C], degrade: None },
    Dessin { libelle: "Ajouter un utilisateur", formes: &[rond(0.4, 0.3, 0.14), rond(0.4, 0.86, 0.28), trou(0.0, 0.9, 1.0, 0.2), trait_(0.66, 0.3, 0.9, 0.3, 0.06), trait_(0.78, 0.18, 0.78, 0.42, 0.06)], couleur: [0x4C, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Retirer un utilisateur", formes: &[rond(0.4, 0.3, 0.14), rond(0.4, 0.86, 0.28), trou(0.0, 0.9, 1.0, 0.2), trait_(0.66, 0.3, 0.9, 0.3, 0.06)], couleur: [0xB1, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Réveil", formes: &[rond(0.24, 0.22, 0.1), rond(0.76, 0.22, 0.1), rond(0.5, 0.56, 0.32), creux(0.5, 0.56, 0.25), trait_(0.5, 0.56, 0.5, 0.4, 0.05), trait_(0.5, 0.56, 0.62, 0.62, 0.05), trait_(0.3, 0.82, 0.24, 0.9, 0.05), trait_(0.7, 0.82, 0.76, 0.9, 0.05)], couleur: [0xC9, 0x5B, 0x4C], degrade: None },
    Dessin { libelle: "Localisation", formes: &[rond(0.5, 0.38, 0.26), poly(&[(0.28, 0.5), (0.72, 0.5), (0.5, 0.9)]), creux(0.5, 0.38, 0.1)], couleur: [0xC9, 0x3C, 0x3C], degrade: None },
    Dessin { libelle: "Navigation", formes: &[poly(&[(0.86, 0.14), (0.5, 0.86), (0.44, 0.56), (0.14, 0.5)])], couleur: [0x3C, 0x7B, 0xB1], degrade: None },
    Dessin { libelle: "Kanban", formes: &[ronde(0.1, 0.14, 0.8, 0.72, 0.04), trou(0.16, 0.2, 0.2, 0.6), trou(0.4, 0.2, 0.2, 0.6), trou(0.64, 0.2, 0.2, 0.6), boite(0.19, 0.24, 0.14, 0.12), boite(0.19, 0.4, 0.14, 0.12), boite(0.43, 0.24, 0.14, 0.12), boite(0.67, 0.24, 0.14, 0.12), boite(0.67, 0.4, 0.14, 0.12), boite(0.67, 0.56, 0.14, 0.12)], couleur: [0x3C, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Dossier ouvert", formes: &[poly(&[(0.12, 0.22), (0.4, 0.22), (0.48, 0.3), (0.82, 0.3), (0.82, 0.44), (0.12, 0.44)]), poly(&[(0.2, 0.5), (0.94, 0.5), (0.82, 0.84), (0.08, 0.84)])], couleur: [0xC9, 0x9B, 0x4C], degrade: None },
    Dessin { libelle: "Nouveau dossier", formes: &[poly(&[(0.12, 0.28), (0.42, 0.28), (0.5, 0.38), (0.88, 0.38), (0.88, 0.82), (0.12, 0.82)]), trou(0.46, 0.48, 0.08, 0.24), trou(0.38, 0.56, 0.24, 0.08)], couleur: [0xC9, 0x9B, 0x4C], degrade: None },
    Dessin { libelle: "Dossier favori", formes: &[poly(&[(0.12, 0.28), (0.42, 0.28), (0.50, 0.38), (0.88, 0.38), (0.88, 0.82), (0.12, 0.82)]), perce(&[(0.500, 0.450), (0.541, 0.553), (0.652, 0.561), (0.567, 0.632), (0.594, 0.739), (0.500, 0.680), (0.406, 0.739), (0.433, 0.632), (0.348, 0.561), (0.459, 0.553)])], couleur: [0xC9, 0x9B, 0x4C], degrade: None },
    Dessin { libelle: "Dossier verrouillé", formes: &[poly(&[(0.12, 0.28), (0.42, 0.28), (0.5, 0.38), (0.88, 0.38), (0.88, 0.82), (0.12, 0.82)]), trou(0.4, 0.58, 0.2, 0.16), creux(0.5, 0.55, 0.07), rond(0.5, 0.55, 0.035), trou(0.4, 0.58, 0.2, 0.03)], couleur: [0xC9, 0x9B, 0x4C], degrade: None },
    Dessin { libelle: "Dossier aimé", formes: &[poly(&[(0.12, 0.28), (0.42, 0.28), (0.5, 0.38), (0.88, 0.38), (0.88, 0.82), (0.12, 0.82)]), creux(0.44, 0.56, 0.06), creux(0.56, 0.56, 0.06), perce(&[(0.38, 0.58), (0.62, 0.58), (0.5, 0.72)])], couleur: [0xC9, 0x9B, 0x4C], degrade: None },
    Dessin { libelle: "Dossier en ligne", formes: &[poly(&[(0.12, 0.28), (0.42, 0.28), (0.5, 0.38), (0.88, 0.38), (0.88, 0.82), (0.12, 0.82)]), creux(0.42, 0.64, 0.07), creux(0.54, 0.58, 0.09), creux(0.64, 0.64, 0.06), trou(0.38, 0.64, 0.3, 0.07)], couleur: [0xC9, 0x9B, 0x4C], degrade: None },
    Dessin { libelle: "Dossier musique", formes: &[poly(&[(0.12, 0.28), (0.42, 0.28), (0.5, 0.38), (0.88, 0.38), (0.88, 0.82), (0.12, 0.82)]), creux(0.44, 0.7, 0.055), moins(Trace::Trait { x1: 0.49, y1: 0.7, x2: 0.49, y2: 0.5, epaisseur: 0.03 }), moins(Trace::Trait { x1: 0.49, y1: 0.5, x2: 0.6, y2: 0.54, epaisseur: 0.03 })], couleur: [0xC9, 0x9B, 0x4C], degrade: None },
    Dessin { libelle: "Dossier images", formes: &[poly(&[(0.12, 0.28), (0.42, 0.28), (0.5, 0.38), (0.88, 0.38), (0.88, 0.82), (0.12, 0.82)]), perce(&[(0.3, 0.74), (0.44, 0.54), (0.54, 0.66), (0.6, 0.6), (0.72, 0.74)]), creux(0.64, 0.52, 0.04)], couleur: [0xC9, 0x9B, 0x4C], degrade: None },
    Dessin { libelle: "Dossier vidéos", formes: &[poly(&[(0.12, 0.28), (0.42, 0.28), (0.5, 0.38), (0.88, 0.38), (0.88, 0.82), (0.12, 0.82)]), perce(&[(0.42, 0.5), (0.62, 0.61), (0.42, 0.72)])], couleur: [0xC9, 0x9B, 0x4C], degrade: None },
    Dessin { libelle: "Dossier de code", formes: &[poly(&[(0.12, 0.28), (0.42, 0.28), (0.5, 0.38), (0.88, 0.38), (0.88, 0.82), (0.12, 0.82)]), moins(Trace::Trait { x1: 0.4, y1: 0.52, x2: 0.32, y2: 0.61, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.32, y1: 0.61, x2: 0.4, y2: 0.7, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.6, y1: 0.52, x2: 0.68, y2: 0.61, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.68, y1: 0.61, x2: 0.6, y2: 0.7, epaisseur: 0.035 })], couleur: [0xC9, 0x9B, 0x4C], degrade: None },
    Dessin { libelle: "Dossier téléchargements", formes: &[poly(&[(0.12, 0.28), (0.42, 0.28), (0.5, 0.38), (0.88, 0.38), (0.88, 0.82), (0.12, 0.82)]), moins(Trace::Trait { x1: 0.5, y1: 0.48, x2: 0.5, y2: 0.64, epaisseur: 0.05 }), perce(&[(0.4, 0.62), (0.6, 0.62), (0.5, 0.74)])], couleur: [0xC9, 0x9B, 0x4C], degrade: None },
    Dessin { libelle: "Dossier documents", formes: &[poly(&[(0.12, 0.28), (0.42, 0.28), (0.5, 0.38), (0.88, 0.38), (0.88, 0.82), (0.12, 0.82)]), moins(Trace::Trait { x1: 0.32, y1: 0.54, x2: 0.68, y2: 0.54, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.32, y1: 0.64, x2: 0.68, y2: 0.64, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.32, y1: 0.74, x2: 0.56, y2: 0.74, epaisseur: 0.035 })], couleur: [0xC9, 0x9B, 0x4C], degrade: None },
    Dessin { libelle: "Tableur", formes: &[poly(&[(0.26, 0.1), (0.62, 0.1), (0.78, 0.28), (0.78, 0.9), (0.26, 0.9)]), perce(&[(0.62, 0.1), (0.78, 0.28), (0.62, 0.28)]), trou(0.32, 0.4, 0.16, 0.1), trou(0.52, 0.4, 0.16, 0.1), trou(0.32, 0.54, 0.16, 0.1), trou(0.52, 0.54, 0.16, 0.1), trou(0.32, 0.68, 0.16, 0.1), trou(0.52, 0.68, 0.16, 0.1)], couleur: [0x3C, 0x8A, 0x5B], degrade: None },
    Dessin { libelle: "Présentation", formes: &[poly(&[(0.26, 0.1), (0.62, 0.1), (0.78, 0.28), (0.78, 0.9), (0.26, 0.9)]), perce(&[(0.62, 0.1), (0.78, 0.28), (0.62, 0.28)]), trou(0.32, 0.4, 0.4, 0.24), moins(Trace::Trait { x1: 0.36, y1: 0.76, x2: 0.66, y2: 0.76, epaisseur: 0.04 })], couleur: [0xC9, 0x6B, 0x3C], degrade: None },
    Dessin { libelle: "Fichier de code", formes: &[poly(&[(0.26, 0.1), (0.62, 0.1), (0.78, 0.28), (0.78, 0.9), (0.26, 0.9)]), perce(&[(0.62, 0.1), (0.78, 0.28), (0.62, 0.28)]), moins(Trace::Trait { x1: 0.42, y1: 0.46, x2: 0.34, y2: 0.58, epaisseur: 0.04 }), moins(Trace::Trait { x1: 0.34, y1: 0.58, x2: 0.42, y2: 0.7, epaisseur: 0.04 }), moins(Trace::Trait { x1: 0.62, y1: 0.46, x2: 0.7, y2: 0.58, epaisseur: 0.04 }), moins(Trace::Trait { x1: 0.7, y1: 0.58, x2: 0.62, y2: 0.7, epaisseur: 0.04 })], couleur: [0x3C, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Fichier son", formes: &[poly(&[(0.26, 0.1), (0.62, 0.1), (0.78, 0.28), (0.78, 0.9), (0.26, 0.9)]), perce(&[(0.62, 0.1), (0.78, 0.28), (0.62, 0.28)]), creux(0.46, 0.7, 0.06), moins(Trace::Trait { x1: 0.51, y1: 0.7, x2: 0.51, y2: 0.46, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.51, y1: 0.46, x2: 0.64, y2: 0.52, epaisseur: 0.035 })], couleur: [0x9B, 0x5B, 0xC9], degrade: None },
    Dessin { libelle: "Fichier de police", formes: &[poly(&[(0.26, 0.1), (0.62, 0.1), (0.78, 0.28), (0.78, 0.9), (0.26, 0.9)]), perce(&[(0.62, 0.1), (0.78, 0.28), (0.62, 0.28)]), moins(Trace::Trait { x1: 0.36, y1: 0.78, x2: 0.52, y2: 0.4, epaisseur: 0.045 }), moins(Trace::Trait { x1: 0.52, y1: 0.4, x2: 0.68, y2: 0.78, epaisseur: 0.045 }), moins(Trace::Trait { x1: 0.42, y1: 0.66, x2: 0.62, y2: 0.66, epaisseur: 0.04 })], couleur: [0x5B, 0x5B, 0x7B], degrade: None },
    Dessin { libelle: "Fichier 3D", formes: &[poly(&[(0.26, 0.1), (0.62, 0.1), (0.78, 0.28), (0.78, 0.9), (0.26, 0.9)]), perce(&[(0.62, 0.1), (0.78, 0.28), (0.62, 0.28)]), moins(Trace::Trait { x1: 0.52, y1: 0.46, x2: 0.68, y2: 0.54, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.52, y1: 0.46, x2: 0.36, y2: 0.54, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.36, y1: 0.54, x2: 0.36, y2: 0.72, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.68, y1: 0.54, x2: 0.68, y2: 0.72, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.36, y1: 0.72, x2: 0.52, y2: 0.8, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.68, y1: 0.72, x2: 0.52, y2: 0.8, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.52, y1: 0.62, x2: 0.52, y2: 0.8, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.36, y1: 0.54, x2: 0.52, y2: 0.62, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.68, y1: 0.54, x2: 0.52, y2: 0.62, epaisseur: 0.025 })], couleur: [0x5B, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Chien", formes: &[ronde(0.12, 0.26, 0.18, 0.42, 0.09), ronde(0.7, 0.26, 0.18, 0.42, 0.09), rond(0.5, 0.52, 0.28), creux(0.4, 0.46, 0.045), creux(0.6, 0.46, 0.045), perce(&[(0.44, 0.62), (0.56, 0.62), (0.5, 0.7)])], couleur: [0x9B, 0x7B, 0x5B], degrade: None },
    Dessin { libelle: "Lapin", formes: &[ronde(0.32, 0.06, 0.14, 0.42, 0.07), ronde(0.54, 0.06, 0.14, 0.42, 0.07), rond(0.5, 0.64, 0.26), creux(0.41, 0.6, 0.04), creux(0.59, 0.6, 0.04), creux(0.5, 0.72, 0.03)], couleur: [0xB1, 0xA8, 0x9B], degrade: None },
    Dessin { libelle: "Ours", formes: &[rond(0.24, 0.26, 0.12), rond(0.76, 0.26, 0.12), rond(0.5, 0.56, 0.32), creux(0.38, 0.48, 0.045), creux(0.62, 0.48, 0.045), creux(0.5, 0.68, 0.11), rond(0.5, 0.65, 0.045)], couleur: [0x8A, 0x5B, 0x3C], degrade: None },
    Dessin { libelle: "Hibou", formes: &[ronde(0.22, 0.2, 0.56, 0.7, 0.26), poly(&[(0.22, 0.3), (0.24, 0.1), (0.4, 0.24)]), poly(&[(0.78, 0.3), (0.76, 0.1), (0.6, 0.24)]), creux(0.38, 0.42, 0.1), rond(0.38, 0.42, 0.04), creux(0.62, 0.42, 0.1), rond(0.62, 0.42, 0.04), perce(&[(0.46, 0.54), (0.54, 0.54), (0.5, 0.62)])], couleur: [0x8A, 0x6B, 0x4C], degrade: None },
    Dessin { libelle: "Abeille", formes: &[rond(0.4, 0.26, 0.12), rond(0.58, 0.24, 0.12), ronde(0.2, 0.38, 0.56, 0.38, 0.19), trou(0.4, 0.38, 0.06, 0.38), trou(0.54, 0.38, 0.06, 0.38), poly(&[(0.76, 0.52), (0.9, 0.57), (0.76, 0.62)]), creux(0.3, 0.52, 0.03)], couleur: [0xD9, 0xA8, 0x3C], degrade: None },
    Dessin { libelle: "Escargot", formes: &[rond(0.58, 0.48, 0.26), creux(0.58, 0.48, 0.19), rond(0.58, 0.48, 0.13), creux(0.58, 0.48, 0.06), poly(&[(0.08, 0.84), (0.12, 0.72), (0.88, 0.72), (0.92, 0.84)]), trait_(0.18, 0.72, 0.12, 0.5, 0.03), trait_(0.24, 0.72, 0.26, 0.5, 0.03)], couleur: [0x9B, 0x7B, 0x5B], degrade: None },
    Dessin { libelle: "Baleine", formes: &[trait_(0.3, 0.4, 0.3, 0.24, 0.03), trait_(0.3, 0.26, 0.22, 0.16, 0.03), trait_(0.3, 0.26, 0.38, 0.16, 0.03), ronde(0.08, 0.42, 0.68, 0.36, 0.18), poly(&[(0.72, 0.58), (0.92, 0.4), (0.86, 0.6), (0.94, 0.78)]), creux(0.22, 0.56, 0.03), moins(Trace::Trait { x1: 0.1, y1: 0.66, x2: 0.34, y2: 0.66, epaisseur: 0.02 })], couleur: [0x3C, 0x6B, 0x9B], degrade: None },
    Dessin { libelle: "Tulipe", formes: &[poly(&[(0.3, 0.18), (0.4, 0.3), (0.5, 0.16), (0.6, 0.3), (0.7, 0.18), (0.7, 0.4), (0.6, 0.54), (0.4, 0.54), (0.3, 0.4)]), trait_(0.5, 0.54, 0.5, 0.9, 0.04), poly(&[(0.5, 0.82), (0.28, 0.62), (0.32, 0.8)])], couleur: [0xC9, 0x4C, 0x6B], degrade: None },
    Dessin { libelle: "Trèfle porte-bonheur", formes: &[rond(0.38, 0.36, 0.14), rond(0.62, 0.36, 0.14), rond(0.38, 0.6, 0.14), rond(0.62, 0.6, 0.14), moins(Trace::Trait { x1: 0.5, y1: 0.18, x2: 0.5, y2: 0.78, epaisseur: 0.03 }), moins(Trace::Trait { x1: 0.22, y1: 0.48, x2: 0.78, y2: 0.48, epaisseur: 0.03 }), trait_(0.56, 0.68, 0.7, 0.92, 0.04)], couleur: [0x3C, 0x8A, 0x4C], degrade: None },
    Dessin { libelle: "Épi de blé", formes: &[trait_(0.5, 0.3, 0.5, 0.92, 0.03), rond(0.5, 0.12, 0.065), rond(0.42, 0.22, 0.065), rond(0.58, 0.22, 0.065), rond(0.42, 0.34, 0.065), rond(0.58, 0.34, 0.065), rond(0.42, 0.46, 0.065), rond(0.58, 0.46, 0.065), rond(0.42, 0.58, 0.065), rond(0.58, 0.58, 0.065)], couleur: [0xC9, 0xA8, 0x5B], degrade: None },
    Dessin { libelle: "Bambou", formes: &[trait_(0.38, 0.08, 0.38, 0.92, 0.1), trait_(0.62, 0.18, 0.62, 0.92, 0.1), trou(0.3, 0.36, 0.16, 0.02), trou(0.3, 0.64, 0.16, 0.02), trou(0.54, 0.44, 0.16, 0.02), trou(0.54, 0.72, 0.16, 0.02), poly(&[(0.44, 0.3), (0.72, 0.2), (0.52, 0.36)])], couleur: [0x4C, 0x8A, 0x4C], degrade: None },
    Dessin { libelle: "Tournesol", formes: &[rond(0.77, 0.5, 0.11), rond(0.691, 0.691, 0.11), rond(0.5, 0.77, 0.11), rond(0.309, 0.691, 0.11), rond(0.23, 0.5, 0.11), rond(0.309, 0.309, 0.11), rond(0.5, 0.23, 0.11), rond(0.691, 0.309, 0.11), creux(0.5, 0.5, 0.18), rond(0.5, 0.5, 0.14)], couleur: [0xD9, 0xB1, 0x3F], degrade: None },
    Dessin { libelle: "Hamburger", formes: &[rond(0.5, 0.44, 0.34), trou(0.0, 0.44, 1.0, 0.6), creux(0.4, 0.28, 0.02), creux(0.56, 0.24, 0.02), creux(0.62, 0.34, 0.02), ronde(0.14, 0.48, 0.72, 0.08, 0.04), ronde(0.12, 0.58, 0.76, 0.1, 0.05), ronde(0.16, 0.7, 0.68, 0.12, 0.06)], couleur: [0xC9, 0x8A, 0x4C], degrade: None },
    Dessin { libelle: "Frites", formes: &[trait_(0.34, 0.46, 0.3, 0.16, 0.06), trait_(0.44, 0.46, 0.42, 0.1, 0.06), trait_(0.54, 0.46, 0.56, 0.12, 0.06), trait_(0.64, 0.46, 0.7, 0.18, 0.06), moins(Trace::Boite { x: 0.2, y: 0.4, l: 0.6, h: 0.06, rayon: 0.0 }), poly(&[(0.24, 0.44), (0.76, 0.44), (0.7, 0.88), (0.3, 0.88)])], couleur: [0xD9, 0x9B, 0x3C], degrade: None },
    Dessin { libelle: "Théière", formes: &[rond(0.2, 0.56, 0.12), creux(0.2, 0.56, 0.07), rond(0.48, 0.58, 0.26), trou(0.0, 0.82, 1.0, 0.2), rond(0.48, 0.28, 0.06), ronde(0.34, 0.3, 0.28, 0.06, 0.03), trait_(0.7, 0.56, 0.88, 0.38, 0.06)], couleur: [0x5B, 0x8A, 0x7B], degrade: None },
    Dessin { libelle: "Croissant", formes: &[rond(0.5, 0.62, 0.36), creux(0.5, 0.8, 0.3), moins(Trace::Trait { x1: 0.3, y1: 0.38, x2: 0.36, y2: 0.5, epaisseur: 0.03 }), moins(Trace::Trait { x1: 0.5, y1: 0.3, x2: 0.5, y2: 0.46, epaisseur: 0.03 }), moins(Trace::Trait { x1: 0.7, y1: 0.38, x2: 0.64, y2: 0.5, epaisseur: 0.03 })], couleur: [0xC9, 0x8A, 0x3C], degrade: None },
    Dessin { libelle: "Baguette", formes: &[trait_(0.12, 0.8, 0.88, 0.2, 0.16), moins(Trace::Trait { x1: 0.26, y1: 0.58, x2: 0.36, y2: 0.7, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.42, y1: 0.46, x2: 0.52, y2: 0.58, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.58, y1: 0.34, x2: 0.68, y2: 0.46, epaisseur: 0.025 })], couleur: [0xC9, 0x9B, 0x5B], degrade: None },
    Dessin { libelle: "Fromage", formes: &[poly(&[(0.1, 0.7), (0.8, 0.3), (0.9, 0.4), (0.9, 0.78), (0.1, 0.78)]), creux(0.4, 0.66, 0.05), creux(0.62, 0.58, 0.04), creux(0.76, 0.7, 0.04)], couleur: [0xD9, 0xB1, 0x3F], degrade: None },
    Dessin { libelle: "Raisin", formes: &[rond(0.36, 0.34, 0.08), rond(0.5, 0.32, 0.08), rond(0.64, 0.34, 0.08), rond(0.43, 0.47, 0.08), rond(0.57, 0.47, 0.08), rond(0.5, 0.61, 0.08), rond(0.36, 0.6, 0.08), trait_(0.5, 0.24, 0.54, 0.1, 0.03), poly(&[(0.54, 0.14), (0.76, 0.08), (0.68, 0.22)])], couleur: [0x7B, 0x4C, 0x8A], degrade: None },
    Dessin { libelle: "Fraise", formes: &[rond(0.36, 0.42, 0.14), rond(0.64, 0.42, 0.14), poly(&[(0.22, 0.4), (0.78, 0.4), (0.7, 0.64), (0.5, 0.9), (0.3, 0.64)]), creux(0.4, 0.52, 0.02), creux(0.58, 0.5, 0.02), creux(0.5, 0.64, 0.02), creux(0.42, 0.72, 0.02), creux(0.6, 0.68, 0.02), poly(&[(0.3, 0.3), (0.5, 0.16), (0.7, 0.3), (0.5, 0.26)])], couleur: [0xC9, 0x3C, 0x4C], degrade: None },
    Dessin { libelle: "Pastèque", formes: &[rond(0.5, 0.36, 0.4), creux(0.5, 0.36, 0.33), rond(0.5, 0.36, 0.3), trou(0.0, 0.0, 1.0, 0.36), creux(0.36, 0.48, 0.025), creux(0.5, 0.54, 0.025), creux(0.64, 0.48, 0.025), creux(0.44, 0.44, 0.02), creux(0.58, 0.44, 0.02)], couleur: [0x4C, 0x9B, 0x5B], degrade: None },
    Dessin { libelle: "Sucette", formes: &[rond(0.5, 0.34, 0.24), creux(0.5, 0.34, 0.17), rond(0.5, 0.34, 0.12), creux(0.5, 0.34, 0.05), trait_(0.5, 0.58, 0.5, 0.92, 0.04)], couleur: [0xC9, 0x5B, 0x9B], degrade: None },
    Dessin { libelle: "Beignet", formes: &[rond(0.5, 0.5, 0.36), creux(0.5, 0.5, 0.13), moins(Trace::Trait { x1: 0.32, y1: 0.3, x2: 0.38, y2: 0.34, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.62, y1: 0.26, x2: 0.68, y2: 0.3, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.7, y1: 0.56, x2: 0.74, y2: 0.62, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.3, y1: 0.62, x2: 0.36, y2: 0.66, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.48, y1: 0.74, x2: 0.54, y2: 0.72, epaisseur: 0.025 })], couleur: [0xC9, 0x7B, 0x8A], degrade: None },
    Dessin { libelle: "Bouée", formes: &[rond(0.5, 0.5, 0.38), creux(0.5, 0.5, 0.2), moins(Trace::Trait { x1: 0.23, y1: 0.23, x2: 0.36, y2: 0.36, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.77, y1: 0.23, x2: 0.64, y2: 0.36, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.23, y1: 0.77, x2: 0.36, y2: 0.64, epaisseur: 0.035 }), moins(Trace::Trait { x1: 0.77, y1: 0.77, x2: 0.64, y2: 0.64, epaisseur: 0.035 })], couleur: [0xC9, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Lanterne", formes: &[rond(0.5, 0.16, 0.08), creux(0.5, 0.16, 0.05), boite(0.36, 0.2, 0.28, 0.08), ronde(0.3, 0.28, 0.4, 0.52, 0.06), trou(0.36, 0.36, 0.28, 0.36), poly(&[(0.5, 0.42), (0.56, 0.56), (0.5, 0.64), (0.44, 0.56)]), boite(0.26, 0.8, 0.48, 0.08)], couleur: [0xC9, 0x9B, 0x3C], degrade: None },
    Dessin { libelle: "Hache", formes: &[trait_(0.26, 0.90, 0.60, 0.24, 0.06), poly(&[(0.44, 0.22), (0.62, 0.12), (0.88, 0.22), (0.92, 0.50), (0.70, 0.46), (0.58, 0.38)])], couleur: [0x8A, 0x6B, 0x5B], degrade: None },
    Dessin { libelle: "Pelle", formes: &[trait_(0.4, 0.1, 0.6, 0.1, 0.06), trait_(0.5, 0.1, 0.5, 0.56, 0.06), poly(&[(0.34, 0.56), (0.66, 0.56), (0.66, 0.76), (0.5, 0.92), (0.34, 0.76)])], couleur: [0x7B, 0x7B, 0x8A], degrade: None },
    Dessin { libelle: "Arrosoir", formes: &[rond(0.46, 0.36, 0.14), creux(0.46, 0.36, 0.09), ronde(0.24, 0.4, 0.44, 0.42, 0.06), trait_(0.68, 0.62, 0.88, 0.32, 0.06), ronde(0.84, 0.24, 0.1, 0.08, 0.02)], couleur: [0x4C, 0x8A, 0x6B], degrade: None },
    Dessin { libelle: "Seau", formes: &[rond(0.5, 0.38, 0.3), creux(0.5, 0.38, 0.26), trou(0.0, 0.38, 1.0, 0.6), poly(&[(0.2, 0.36), (0.8, 0.36), (0.72, 0.86), (0.28, 0.86)])], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Échelle", formes: &[trait_(0.32, 0.08, 0.32, 0.92, 0.05), trait_(0.68, 0.08, 0.68, 0.92, 0.05), trait_(0.32, 0.22, 0.68, 0.22, 0.04), trait_(0.32, 0.4, 0.68, 0.4, 0.04), trait_(0.32, 0.58, 0.68, 0.58, 0.04), trait_(0.32, 0.76, 0.68, 0.76, 0.04)], couleur: [0x9B, 0x7B, 0x5B], degrade: None },
    Dessin { libelle: "Tournevis", formes: &[ronde(0.4, 0.06, 0.2, 0.36, 0.08), trou(0.44, 0.1, 0.03, 0.28), trou(0.53, 0.1, 0.03, 0.28), trait_(0.5, 0.42, 0.5, 0.82, 0.05), boite(0.46, 0.8, 0.08, 0.1)], couleur: [0xC9, 0x5B, 0x3C], degrade: None },
    Dessin { libelle: "Écrou", formes: &[poly(&[(0.88, 0.5), (0.69, 0.829), (0.31, 0.829), (0.12, 0.5), (0.31, 0.171), (0.69, 0.171)]), creux(0.5, 0.5, 0.16)], couleur: [0x7B, 0x7B, 0x8A], degrade: None },
    Dessin { libelle: "Hélicoptère", formes: &[trait_(0.14, 0.28, 0.74, 0.28, 0.04), trait_(0.44, 0.28, 0.44, 0.38, 0.04), rond(0.44, 0.56, 0.2), perce(&[(0.3, 0.5), (0.44, 0.42), (0.44, 0.56), (0.3, 0.56)]), trait_(0.6, 0.52, 0.9, 0.44, 0.05), trait_(0.9, 0.36, 0.9, 0.52, 0.04), trait_(0.26, 0.86, 0.62, 0.86, 0.04), trait_(0.34, 0.76, 0.32, 0.86, 0.03), trait_(0.54, 0.76, 0.56, 0.86, 0.03)], couleur: [0xC9, 0x6B, 0x3C], degrade: None },
    Dessin { libelle: "Sous-marin", formes: &[trait_(0.5, 0.3, 0.5, 0.16, 0.03), trait_(0.5, 0.16, 0.58, 0.16, 0.03), ronde(0.4, 0.3, 0.2, 0.2, 0.04), ronde(0.1, 0.46, 0.78, 0.3, 0.15), creux(0.3, 0.61, 0.04), creux(0.48, 0.61, 0.04), creux(0.66, 0.61, 0.04), poly(&[(0.88, 0.5), (0.94, 0.44), (0.94, 0.78), (0.88, 0.72)])], couleur: [0xC9, 0xA8, 0x3C], degrade: None },
    Dessin { libelle: "Tracteur", formes: &[trait_(0.34, 0.4, 0.34, 0.24, 0.04), boite(0.28, 0.4, 0.36, 0.24), ronde(0.44, 0.18, 0.2, 0.24, 0.03), trou(0.48, 0.22, 0.12, 0.12), rond(0.34, 0.72, 0.18), creux(0.34, 0.72, 0.08), rond(0.74, 0.76, 0.12), creux(0.74, 0.76, 0.05)], couleur: [0x4C, 0x8A, 0x4C], degrade: None },
    Dessin { libelle: "Trottinette", formes: &[trait_(0.2, 0.82, 0.7, 0.82, 0.05), trait_(0.7, 0.82, 0.62, 0.18, 0.05), trait_(0.52, 0.18, 0.72, 0.18, 0.05), rond(0.2, 0.84, 0.07), rond(0.72, 0.84, 0.07)], couleur: [0x3C, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Planche à roulettes", formes: &[ronde(0.1, 0.48, 0.8, 0.1, 0.05), boite(0.2, 0.58, 0.12, 0.04), boite(0.68, 0.58, 0.12, 0.04), rond(0.26, 0.68, 0.07), rond(0.74, 0.68, 0.07)], couleur: [0x9B, 0x5B, 0x7B], degrade: None },
    Dessin { libelle: "Ballon de football", formes: &[rond(0.5, 0.5, 0.38), perce(&[(0.5, 0.37), (0.624, 0.46), (0.576, 0.605), (0.424, 0.605), (0.376, 0.46)]), moins(Trace::Trait { x1: 0.5, y1: 0.37, x2: 0.5, y2: 0.149, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.624, y1: 0.46, x2: 0.834, y2: 0.392, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.576, y1: 0.605, x2: 0.706, y2: 0.784, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.424, y1: 0.605, x2: 0.294, y2: 0.784, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.376, y1: 0.46, x2: 0.166, y2: 0.392, epaisseur: 0.025 })], couleur: [0x7B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Golf", formes: &[trait_(0.42, 0.12, 0.42, 0.8, 0.04), poly(&[(0.44, 0.12), (0.8, 0.22), (0.44, 0.32)]), ronde(0.22, 0.8, 0.4, 0.08, 0.04), rond(0.72, 0.78, 0.07)], couleur: [0x3C, 0x8A, 0x4C], degrade: None },
    Dessin { libelle: "Quille", formes: &[rond(0.5, 0.2, 0.1), poly(&[(0.44, 0.26), (0.56, 0.26), (0.62, 0.5), (0.38, 0.5)]), rond(0.5, 0.66, 0.2), trou(0.36, 0.34, 0.28, 0.03), trou(0.36, 0.4, 0.28, 0.03)], couleur: [0xB1, 0xA8, 0x9B], degrade: None },
    Dessin { libelle: "Volant", formes: &[poly(&[(0.4, 0.66), (0.6, 0.66), (0.8, 0.14), (0.2, 0.14)]), moins(Trace::Trait { x1: 0.5, y1: 0.62, x2: 0.5, y2: 0.16, epaisseur: 0.02 }), moins(Trace::Trait { x1: 0.44, y1: 0.64, x2: 0.34, y2: 0.16, epaisseur: 0.02 }), moins(Trace::Trait { x1: 0.56, y1: 0.64, x2: 0.66, y2: 0.16, epaisseur: 0.02 }), trou(0.3, 0.4, 0.4, 0.03), rond(0.5, 0.76, 0.12)], couleur: [0x7B, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Sifflet", formes: &[trait_(0.24, 0.42, 0.18, 0.2, 0.03), ronde(0.14, 0.4, 0.56, 0.32, 0.16), boite(0.62, 0.4, 0.26, 0.12), creux(0.36, 0.56, 0.08)], couleur: [0x5B, 0x7B, 0x8A], degrade: None },
    Dessin { libelle: "Podium", formes: &[boite(0.08, 0.48, 0.27, 0.4), boite(0.365, 0.3, 0.27, 0.58), boite(0.65, 0.58, 0.27, 0.3), moins(Trace::Trait { x1: 0.5, y1: 0.38, x2: 0.5, y2: 0.54, epaisseur: 0.04 })], couleur: [0xC9, 0xA8, 0x3C], degrade: None },
    Dessin { libelle: "Caméra", formes: &[rond(0.24, 0.2, 0.1), rond(0.48, 0.2, 0.1), ronde(0.08, 0.32, 0.56, 0.4, 0.06), poly(&[(0.64, 0.44), (0.92, 0.3), (0.92, 0.74), (0.64, 0.6)])], couleur: [0x4A, 0x5B, 0x6B], degrade: None },
    Dessin { libelle: "Projecteur", formes: &[ronde(0.1, 0.36, 0.8, 0.36, 0.06), creux(0.3, 0.54, 0.12), rond(0.3, 0.54, 0.07), trou(0.56, 0.46, 0.24, 0.04), trou(0.56, 0.56, 0.24, 0.04), boite(0.18, 0.72, 0.08, 0.08), boite(0.74, 0.72, 0.08, 0.08)], couleur: [0x5B, 0x5B, 0x6B], degrade: None },
    Dessin { libelle: "Pop-corn", formes: &[rond(0.3, 0.38, 0.1), rond(0.44, 0.3, 0.11), rond(0.58, 0.32, 0.11), rond(0.7, 0.4, 0.09), rond(0.5, 0.2, 0.09), poly(&[(0.24, 0.44), (0.76, 0.44), (0.68, 0.9), (0.32, 0.9)]), trou(0.4, 0.44, 0.06, 0.46), trou(0.54, 0.44, 0.06, 0.46)], couleur: [0xC9, 0x4C, 0x4C], degrade: None },
    Dessin { libelle: "Église", formes: &[trait_(0.5, 0.04, 0.5, 0.2, 0.04), trait_(0.44, 0.1, 0.56, 0.1, 0.04), boite(0.42, 0.2, 0.16, 0.14), poly(&[(0.16, 0.48), (0.5, 0.26), (0.84, 0.48)]), boite(0.22, 0.46, 0.56, 0.42), trou(0.42, 0.68, 0.16, 0.2), creux(0.5, 0.68, 0.08), creux(0.5, 0.54, 0.05)], couleur: [0x8A, 0x7B, 0x6B], degrade: None },
    Dessin { libelle: "Moulin", formes: &[trait_(0.5, 0.36, 0.2, 0.1, 0.08), trait_(0.5, 0.36, 0.8, 0.1, 0.08), trait_(0.5, 0.36, 0.2, 0.62, 0.08), trait_(0.5, 0.36, 0.8, 0.62, 0.08), poly(&[(0.38, 0.4), (0.62, 0.4), (0.7, 0.9), (0.3, 0.9)]), poly(&[(0.36, 0.42), (0.5, 0.3), (0.64, 0.42)]), trou(0.45, 0.74, 0.1, 0.16)], couleur: [0x9B, 0x6B, 0x5B], degrade: None },
    Dessin { libelle: "Igloo", formes: &[rond(0.5, 0.76, 0.4), trou(0.0, 0.76, 1.0, 0.3), creux(0.5, 0.76, 0.12), trou(0.38, 0.76, 0.24, 0.2), moins(Trace::Trait { x1: 0.12, y1: 0.62, x2: 0.88, y2: 0.62, epaisseur: 0.02 }), moins(Trace::Trait { x1: 0.22, y1: 0.48, x2: 0.78, y2: 0.48, epaisseur: 0.02 }), moins(Trace::Trait { x1: 0.5, y1: 0.36, x2: 0.5, y2: 0.48, epaisseur: 0.02 }), moins(Trace::Trait { x1: 0.3, y1: 0.48, x2: 0.3, y2: 0.62, epaisseur: 0.02 }), moins(Trace::Trait { x1: 0.7, y1: 0.48, x2: 0.7, y2: 0.62, epaisseur: 0.02 })], couleur: [0x7B, 0xA8, 0xC9], degrade: None },
    Dessin { libelle: "Pyramide", formes: &[poly(&[(0.5, 0.14), (0.92, 0.84), (0.08, 0.84)]), moins(Trace::Trait { x1: 0.5, y1: 0.14, x2: 0.62, y2: 0.84, epaisseur: 0.025 }), moins(Trace::Trait { x1: 0.3, y1: 0.48, x2: 0.7, y2: 0.48, epaisseur: 0.02 }), moins(Trace::Trait { x1: 0.2, y1: 0.66, x2: 0.8, y2: 0.66, epaisseur: 0.02 })], couleur: [0xC9, 0xA8, 0x5B], degrade: None },
    Dessin { libelle: "Gratte-ciel", formes: &[trait_(0.5, 0.06, 0.5, 0.2, 0.03), boite(0.36, 0.2, 0.28, 0.7), trou(0.42, 0.26, 0.04, 0.58), trou(0.54, 0.26, 0.04, 0.58), boite(0.14, 0.46, 0.2, 0.44), boite(0.66, 0.36, 0.2, 0.54), trou(0.2, 0.52, 0.08, 0.32), trou(0.72, 0.42, 0.08, 0.42)], couleur: [0x5B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Drone", formes: &[trait_(0.38, 0.42, 0.2, 0.26, 0.04), trait_(0.62, 0.42, 0.8, 0.26, 0.04), trait_(0.38, 0.58, 0.2, 0.74, 0.04), trait_(0.62, 0.58, 0.8, 0.74, 0.04), rond(0.2, 0.26, 0.11), creux(0.2, 0.26, 0.07), rond(0.8, 0.26, 0.11), creux(0.8, 0.26, 0.07), rond(0.2, 0.74, 0.11), creux(0.2, 0.74, 0.07), rond(0.8, 0.74, 0.11), creux(0.8, 0.74, 0.07), ronde(0.36, 0.4, 0.28, 0.2, 0.05)], couleur: [0x4A, 0x5B, 0x6B], degrade: None },
    Dessin { libelle: "Casque de réalité virtuelle", formes: &[trait_(0.04, 0.48, 0.96, 0.48, 0.04), ronde(0.1, 0.32, 0.8, 0.36, 0.1), creux(0.32, 0.5, 0.09), creux(0.68, 0.5, 0.09), perce(&[(0.42, 0.7), (0.58, 0.7), (0.5, 0.58)])], couleur: [0x6B, 0x5B, 0x9B], degrade: None },
    Dessin { libelle: "Code QR", formes: &[ronde(0.12, 0.12, 0.26, 0.26, 0.02), trou(0.17, 0.17, 0.16, 0.16), boite(0.21, 0.21, 0.08, 0.08), ronde(0.62, 0.12, 0.26, 0.26, 0.02), trou(0.67, 0.17, 0.16, 0.16), boite(0.71, 0.21, 0.08, 0.08), ronde(0.12, 0.62, 0.26, 0.26, 0.02), trou(0.17, 0.67, 0.16, 0.16), boite(0.21, 0.71, 0.08, 0.08), boite(0.62, 0.62, 0.1, 0.1), boite(0.78, 0.74, 0.1, 0.12), boite(0.46, 0.46, 0.08, 0.08), boite(0.7, 0.48, 0.08, 0.08), boite(0.46, 0.72, 0.08, 0.14), boite(0.46, 0.14, 0.08, 0.1), boite(0.14, 0.46, 0.1, 0.08), boite(0.3, 0.48, 0.06, 0.06)], couleur: [0x7B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Code-barres", formes: &[boite(0.12, 0.2, 0.04, 0.6), boite(0.19, 0.2, 0.02, 0.6), boite(0.24, 0.2, 0.06, 0.6), boite(0.33, 0.2, 0.02, 0.6), boite(0.38, 0.2, 0.04, 0.6), boite(0.45, 0.2, 0.06, 0.6), boite(0.54, 0.2, 0.02, 0.6), boite(0.59, 0.2, 0.04, 0.6), boite(0.66, 0.2, 0.02, 0.6), boite(0.71, 0.2, 0.06, 0.6), boite(0.8, 0.2, 0.02, 0.6), boite(0.85, 0.2, 0.04, 0.6)], couleur: [0x7B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Panneau solaire", formes: &[poly(&[(0.2, 0.2), (0.9, 0.2), (0.8, 0.66), (0.1, 0.66)]), moins(Trace::Trait { x1: 0.375, y1: 0.2, x2: 0.275, y2: 0.66, epaisseur: 0.02 }), moins(Trace::Trait { x1: 0.55, y1: 0.2, x2: 0.45, y2: 0.66, epaisseur: 0.02 }), moins(Trace::Trait { x1: 0.725, y1: 0.2, x2: 0.625, y2: 0.66, epaisseur: 0.02 }), moins(Trace::Trait { x1: 0.17, y1: 0.36, x2: 0.87, y2: 0.36, epaisseur: 0.02 }), moins(Trace::Trait { x1: 0.13, y1: 0.51, x2: 0.83, y2: 0.51, epaisseur: 0.02 }), trait_(0.5, 0.66, 0.5, 0.88, 0.05), trait_(0.34, 0.88, 0.66, 0.88, 0.05)], couleur: [0x3C, 0x5B, 0x8A], degrade: None },
    Dessin { libelle: "Éolienne", formes: &[poly(&[(0.48, 0.4), (0.52, 0.4), (0.56, 0.92), (0.44, 0.92)]), trait_(0.5, 0.36, 0.5, 0.06, 0.05), trait_(0.5, 0.36, 0.76, 0.51, 0.05), trait_(0.5, 0.36, 0.24, 0.51, 0.05), rond(0.5, 0.36, 0.05)], couleur: [0x7B, 0x9B, 0xB1], degrade: None },
    Dessin { libelle: "Disquette", formes: &[ronde(0.14, 0.14, 0.72, 0.72, 0.04), trou(0.26, 0.52, 0.48, 0.28), trou(0.32, 0.14, 0.36, 0.22), boite(0.52, 0.18, 0.1, 0.14)], couleur: [0x3C, 0x5B, 0x8A], degrade: None },
    Dessin { libelle: "Bluetooth", formes: &[ronde(0.2, 0.06, 0.6, 0.88, 0.3), moins(Trace::Trait { x1: 0.36, y1: 0.32, x2: 0.64, y2: 0.62, epaisseur: 0.06 }), moins(Trace::Trait { x1: 0.64, y1: 0.62, x2: 0.5, y2: 0.78, epaisseur: 0.06 }), moins(Trace::Trait { x1: 0.5, y1: 0.78, x2: 0.5, y2: 0.22, epaisseur: 0.06 }), moins(Trace::Trait { x1: 0.5, y1: 0.22, x2: 0.64, y2: 0.38, epaisseur: 0.06 }), moins(Trace::Trait { x1: 0.64, y1: 0.38, x2: 0.36, y2: 0.68, epaisseur: 0.06 })], couleur: [0x3C, 0x6B, 0xB1], degrade: None },
    Dessin { libelle: "USB", formes: &[trait_(0.5, 0.14, 0.5, 0.84, 0.05), poly(&[(0.4, 0.22), (0.5, 0.06), (0.6, 0.22)]), trait_(0.5, 0.64, 0.3, 0.5, 0.04), trait_(0.3, 0.5, 0.3, 0.38, 0.04), rond(0.3, 0.33, 0.055), trait_(0.5, 0.54, 0.7, 0.42, 0.04), trait_(0.7, 0.42, 0.7, 0.32, 0.04), boite(0.65, 0.22, 0.1, 0.1), rond(0.5, 0.86, 0.08)], couleur: [0x5B, 0x6B, 0x7B], degrade: None },
    Dessin { libelle: "Gomme", formes: &[poly(&[(0.12, 0.58), (0.46, 0.24), (0.86, 0.58), (0.52, 0.92)]), moins(Trace::Trait { x1: 0.29, y1: 0.41, x2: 0.69, y2: 0.75, epaisseur: 0.03 })], couleur: [0xC9, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Compas", formes: &[trait_(0.5, 0.04, 0.5, 0.12, 0.04), rond(0.5, 0.16, 0.06), trait_(0.5, 0.16, 0.28, 0.88, 0.05), trait_(0.5, 0.16, 0.72, 0.88, 0.05), trait_(0.39, 0.52, 0.61, 0.52, 0.03)], couleur: [0x7B, 0x7B, 0x8A], degrade: None },
    Dessin { libelle: "Équerre", formes: &[poly(&[(0.14, 0.14), (0.14, 0.86), (0.86, 0.86)]), perce(&[(0.26, 0.42), (0.26, 0.74), (0.58, 0.74)])], couleur: [0x5B, 0x9B, 0xB1], degrade: None },
    Dessin { libelle: "Sac à dos", formes: &[rond(0.5, 0.2, 0.1), creux(0.5, 0.2, 0.06), ronde(0.2, 0.2, 0.6, 0.7, 0.16), moins(Trace::Trait { x1: 0.24, y1: 0.4, x2: 0.76, y2: 0.4, epaisseur: 0.025 }), trou(0.3, 0.54, 0.4, 0.24), boite(0.33, 0.58, 0.34, 0.17)], couleur: [0x4C, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Feu d'artifice", formes: &[trait_(0.62, 0.5, 0.88, 0.5, 0.04), trait_(0.604, 0.56, 0.829, 0.69, 0.04), trait_(0.56, 0.604, 0.69, 0.829, 0.04), trait_(0.5, 0.62, 0.5, 0.88, 0.04), trait_(0.44, 0.604, 0.31, 0.829, 0.04), trait_(0.396, 0.56, 0.171, 0.69, 0.04), trait_(0.38, 0.5, 0.12, 0.5, 0.04), trait_(0.396, 0.44, 0.171, 0.31, 0.04), trait_(0.44, 0.396, 0.31, 0.171, 0.04), trait_(0.5, 0.38, 0.5, 0.12, 0.04), trait_(0.56, 0.396, 0.69, 0.171, 0.04), trait_(0.604, 0.44, 0.829, 0.31, 0.04), rond(0.5, 0.5, 0.05)], couleur: [0xC9, 0x5B, 0x9B], degrade: None },
    Dessin { libelle: "Valise", formes: &[ronde(0.38, 0.14, 0.24, 0.18, 0.05), trou(0.43, 0.19, 0.14, 0.09), ronde(0.16, 0.28, 0.68, 0.56, 0.06), trou(0.34, 0.28, 0.04, 0.56), trou(0.62, 0.28, 0.04, 0.56), rond(0.3, 0.88, 0.04), rond(0.7, 0.88, 0.04)], couleur: [0x8A, 0x5B, 0x6B], degrade: None },
    Dessin { libelle: "Passeport", formes: &[ronde(0.22, 0.1, 0.56, 0.8, 0.04), creux(0.5, 0.4, 0.15), rond(0.5, 0.4, 0.12), moins(Trace::Trait { x1: 0.38, y1: 0.4, x2: 0.62, y2: 0.4, epaisseur: 0.02 }), moins(Trace::Trait { x1: 0.5, y1: 0.28, x2: 0.5, y2: 0.52, epaisseur: 0.02 }), trou(0.34, 0.68, 0.32, 0.04)], couleur: [0x3C, 0x5B, 0x8A], degrade: None },
    Dessin { libelle: "Lunettes de soleil", formes: &[trait_(0.08, 0.42, 0.04, 0.3, 0.03), trait_(0.92, 0.42, 0.96, 0.3, 0.03), poly(&[(0.08, 0.4), (0.44, 0.4), (0.4, 0.64), (0.14, 0.64)]), poly(&[(0.56, 0.4), (0.92, 0.4), (0.86, 0.64), (0.6, 0.64)]), trait_(0.42, 0.44, 0.58, 0.44, 0.04)], couleur: [0x7B, 0x7B, 0x9B], degrade: None },
    Dessin { libelle: "Épée", formes: &[poly(&[(0.46, 0.62), (0.46, 0.16), (0.5, 0.06), (0.54, 0.16), (0.54, 0.62)]), moins(Trace::Trait { x1: 0.5, y1: 0.18, x2: 0.5, y2: 0.58, epaisseur: 0.015 }), trait_(0.3, 0.64, 0.7, 0.64, 0.06), trait_(0.5, 0.66, 0.5, 0.84, 0.06), rond(0.5, 0.88, 0.05)], couleur: [0x7B, 0x8A, 0x9B], degrade: None },
    Dessin { libelle: "Brique de construction", formes: &[boite(0.12, 0.4, 0.76, 0.44), ronde(0.17, 0.28, 0.14, 0.14, 0.03), ronde(0.35, 0.28, 0.14, 0.14, 0.03), ronde(0.53, 0.28, 0.14, 0.14, 0.03), ronde(0.71, 0.28, 0.14, 0.14, 0.03)], couleur: [0xC9, 0x3C, 0x3C], degrade: None },
    Dessin { libelle: "Toupie", formes: &[trait_(0.5, 0.12, 0.5, 0.34, 0.06), ronde(0.16, 0.34, 0.68, 0.12, 0.06), poly(&[(0.16, 0.42), (0.84, 0.42), (0.5, 0.9)]), moins(Trace::Trait { x1: 0.24, y1: 0.52, x2: 0.76, y2: 0.52, epaisseur: 0.03 })], couleur: [0x9B, 0x5B, 0xC9], degrade: None },
    Dessin { libelle: "Cerf-volant", formes: &[poly(&[(0.5, 0.08), (0.8, 0.4), (0.5, 0.72), (0.2, 0.4)]), moins(Trace::Trait { x1: 0.5, y1: 0.08, x2: 0.5, y2: 0.72, epaisseur: 0.02 }), moins(Trace::Trait { x1: 0.2, y1: 0.4, x2: 0.8, y2: 0.4, epaisseur: 0.02 }), trait_(0.5, 0.72, 0.42, 0.82, 0.025), trait_(0.42, 0.82, 0.56, 0.88, 0.025), trait_(0.56, 0.88, 0.48, 0.96, 0.025), poly(&[(0.38, 0.78), (0.46, 0.82), (0.38, 0.86)]), poly(&[(0.6, 0.84), (0.52, 0.88), (0.6, 0.92)])], couleur: [0xC9, 0x5B, 0x4C], degrade: None },
];

/// Une couleur en `#RRGGBB` — la forme que `style.color` et un `fill` SVG acceptent.
fn couleur_hex(couleur: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", couleur[0], couleur[1], couleur[2])
}

/// **UN DESSIN DÉCRIT POUR LA PAGE** — ses formes, en clair.
///
/// ⚠️ **L'ÉCHELLE EST ×100, ET C'EST LA SEULE LICENCE.** Les coordonnées unitaires ont
/// deux décimales utiles (une épaisseur de trait vaut `0,05`), et un `viewBox` de
/// `100 × 100` les porte sans perte. Le dock, lui, rasterise en unités : les deux
/// décrivent donc **la même géométrie**, à l'échelle près.
///
/// ⛔ **CE QUI REND L'APERÇU HONNÊTE** : la page ne reçoit pas une image, elle reçoit
/// **la description** — et le masque SVG qu'elle en tire applique les opérations DANS
/// L'ORDRE, comme le rasteriseur (`Ajouter` éclaircit le masque, `Retirer` l'assombrit).
/// Un aperçu dessiné autrement montrerait autre chose que ce qu'il pose.
pub fn pour_la_page() -> serde_json::Value {
    serde_json::Value::Array(
        CATALOGUE
            .iter()
            .map(|dessin| {
                serde_json::json!({
                    "nom": dessin.libelle,
                    "couleur": couleur_hex(dessin.couleur),
                    "degrade": dessin.degrade.map(couleur_hex),
                    "formes": dessin.formes.iter().map(forme_decrite).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

/// Une forme, décrite pour la page — son opération et sa géométrie, en `viewBox` 100.
fn forme_decrite(forme: &Forme) -> serde_json::Value {
    const ECHELLE: f32 = 100.0;
    let operation = match forme.operation {
        Operation::Ajouter => "plus",
        Operation::Retirer => "moins",
    };
    let trace = match forme.trace {
        Trace::Disque { cx, cy, r } => serde_json::json!({
            "type": "disque", "cx": cx * ECHELLE, "cy": cy * ECHELLE, "r": r * ECHELLE,
        }),
        Trace::Boite { x, y, l, h, rayon } => serde_json::json!({
            "type": "boite", "x": x * ECHELLE, "y": y * ECHELLE,
            "l": l * ECHELLE, "h": h * ECHELLE, "rayon": rayon * ECHELLE,
        }),
        Trace::Trait { x1, y1, x2, y2, epaisseur } => serde_json::json!({
            "type": "trait", "x1": x1 * ECHELLE, "y1": y1 * ECHELLE,
            "x2": x2 * ECHELLE, "y2": y2 * ECHELLE, "epaisseur": epaisseur * ECHELLE,
        }),
        Trace::Polygone { points } => serde_json::json!({
            "type": "polygone",
            "points": points.iter().map(|(x, y)| [x * ECHELLE, y * ECHELLE]).collect::<Vec<_>>(),
        }),
    };
    serde_json::json!({ "op": operation, "trace": trace })
}

#[cfg(test)]
mod essais_de_fond {
    use super::*;
    use crate::ui::vignettes_dossiers::teinte_melangee as celle_des_glyphes;

    /// **LES DEUX FAMILLES FONDENT DANS LE MÊME SENS.** `teinte_melangee` est recopiée
    /// ici plutôt qu'importée (l'autre module dépend de celui-ci), et deux formules
    /// jumelles divergent à la première retouche : ce test les tient ensemble.
    #[test]
    fn les_deux_familles_fondent_dans_le_meme_sens() {
        let (a, b) = ([0x10, 0x40, 0x80], [0xF0, 0x80, 0x20]);
        for pas in 0..=10 {
            let t = pas as f32 / 10.0;
            assert_eq!(
                teinte_melangee(a, b, t),
                celle_des_glyphes(a, b, t),
                "les deux fondus divergent a t = {t}"
            );
        }
    }
}
