//! **L'ATTERRISSAGE** — DEMANDE EXPLICITE (2026-11-23) : « des super héros qui
//! arrivent de très loin dans le haut droit de l'écran et qui viennent se poser
//! en bas de l'écran au centre, puis qui partent d'un côté ou de l'autre en
//! marchant pour disparaître dans le bord de l'écran ».
//!
//! - **Trois planches par héros**, fabriquées par `tools/super_heros_vol/`
//!   (Wan 2.2, détourage BiRefNet) avec UN cadre et UNE échelle communs, si bien
//!   que les raccords tombent au pixel :
//!   `heros-NN-approche.webp` (vol face à nous, en boucle),
//!   `heros-NN-atterrissage.webp` (il se pose, se redresse, se tourne),
//!   `heros-NN-marche.webp` (pas de profil vers la droite, en boucle), et la
//!   fiche `heros-NN-atterrissage.txt` (nombres d'images, taille d'une case,
//!   ligne des pieds, foulée). 8 colonnes, autant de lignes qu'il faut.
//! - **Sur place** : les vidéos ne bougent pas, c'est la fenêtre qui fait le
//!   chemin. L'approche grossit d'un FACTEUR CONSTANT par seconde (une
//!   croissance qui paraît régulière à l'œil, de `ECHELLE_LOIN` à 1) ; la marche
//!   avance de la foulée MESURÉE sur la vidéo, pour que les pieds ne glissent pas.
//! - **Vers la gauche** : le miroir des trois planches ; le chemin part toujours
//!   du haut à droite.

use windows::Win32::Foundation::{POINT, RECT};

use std::borrow::Cow;

use super::{decouper, mettre_a_taille, miroir, IMAGES_PAR_SECONDE};

/// L'approche dure ce nombre de boucles de sa vidéo : elle FINIT sur sa
/// première image, qui est aussi la première de l'atterrissage.
const TOURS_APPROCHE: u32 = 2;
/// La taille du héros quand il paraît, en fraction de sa taille posée.
pub const ECHELLE_LOIN: f32 = 0.04;
/// Où il paraît, en fraction de l'aire de travail (depuis le coin haut-gauche).
const DEPART: (f32, f32) = (0.90, 0.07);
/// La marche va un peu plus vite que la foulée mesurée — DEMANDE EXPLICITE
/// (2026-11-23) : « qu'il se déplace plus vite de 7 % en marchant ».
pub const VITESSE_MARCHE: f32 = 1.07;
/// Le départ de la marche s'adoucit sur ce temps (de l'arrêt à la vitesse).
const ELAN: f32 = 0.25;

/// `heros-NN-atterrissage.txt`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fiche {
    pub approche: u32,
    pub atterrissage: u32,
    pub marche: u32,
    /// Taille d'une case (commune aux trois planches).
    pub largeur: u32,
    pub hauteur: u32,
    /// Ligne des pieds posés, depuis le haut de la case.
    pub pieds: u32,
    /// Hauteur du héros debout, en pixels de planche.
    pub debout: u32,
    /// Distance parcourue par boucle de marche, en pixels de planche.
    pub foulee: u32,
    /// L'image de l'atterrissage où il se met à marcher SUR PLACE (Wan cloue
    /// la fin du clip sur la première image de la marche) : c'est là qu'il
    /// doit déjà avancer. Absente de la fiche : à la fin du clip. Mesurée par
    /// `tools/super_heros_vol/debut_marche.py`.
    pub marche_des: u32,
}

pub fn lire_fiche(texte: &str) -> Option<Fiche> {
    let lire = |cle: &str| -> Option<u32> {
        texte.lines().find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == cle).then(|| v.trim().parse().ok()).flatten()
        })
    };
    let f = Fiche {
        approche: lire("approche")?,
        atterrissage: lire("atterrissage")?,
        marche: lire("marche")?,
        largeur: lire("largeur")?,
        hauteur: lire("hauteur")?,
        pieds: lire("pieds")?,
        debout: lire("debout")?,
        foulee: lire("foulee")?,
        marche_des: 0,
    };
    let f = Fiche { marche_des: lire("marche_des").unwrap_or(f.atterrissage).min(f.atterrissage), ..f };
    let valide = f.approche > 0 && f.atterrissage > 0 && f.marche > 0 && f.largeur > 0 && f.hauteur > 0
        && f.debout > 0 && f.pieds <= f.hauteur;
    valide.then_some(f)
}

/// Les numéros des héros qui savent atterrir (`heros-NN-atterrissage.txt`), triés.
pub fn heros_atterrisseurs<'a>(noms: impl Iterator<Item = &'a str>) -> Vec<u32> {
    let mut v: Vec<u32> = noms
        .filter_map(|n| n.strip_prefix("heros-")?.strip_suffix("-atterrissage.txt")?.parse().ok())
        .collect();
    v.sort_unstable();
    v
}

/// Où en est l'animation, `ecoule` secondes après le départ.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// `avance` : 0 au départ, 1 au moment de se poser.
    Approche { image: usize, avance: f32 },
    /// `parcouru` : en pixels de planche depuis le point de pose (la marche
    /// commence avant la fin de l'atterrissage, à `marche_des`).
    Pose { image: usize, parcouru: f32 },
    Marche { image: usize, parcouru: f32 },
}

pub fn duree_approche(f: &Fiche) -> f32 {
    (TOURS_APPROCHE * f.approche) as f32 / IMAGES_PAR_SECONDE
}

/// Les pixels de planche parcourus `t` secondes après le début de la marche :
/// la foulée par boucle × `VITESSE_MARCHE`, atteinte en `ELAN` secondes.
pub fn distance_de_marche(t: f32, f: &Fiche) -> f32 {
    let vitesse = f.foulee as f32 * VITESSE_MARCHE * IMAGES_PAR_SECONDE / f.marche as f32;
    let t = t.max(0.0);
    if t < ELAN { vitesse * t * t / (2.0 * ELAN) } else { vitesse * (t - ELAN / 2.0) }
}

pub fn phase(ecoule: f32, f: &Fiche) -> Phase {
    let image = |s: f32, n: u32| (s.max(0.0) * IMAGES_PAR_SECONDE) as usize % n as usize;
    let approche = duree_approche(f);
    if ecoule < approche {
        return Phase::Approche { image: image(ecoule, f.approche), avance: ecoule / approche };
    }
    let pose = f.atterrissage as f32 / IMAGES_PAR_SECONDE;
    let s = ecoule - approche;
    let parcouru = distance_de_marche(s - f.marche_des as f32 / IMAGES_PAR_SECONDE, f);
    if s < pose {
        return Phase::Pose { image: ((s * IMAGES_PAR_SECONDE) as usize).min(f.atterrissage as usize - 1), parcouru };
    }
    Phase::Marche { image: image(s - pose, f.marche), parcouru }
}

/// Le coin haut-gauche de la case posée : centrée en bas de l'aire de travail,
/// les pieds sur son bord.
pub fn point_de_pose(zone: RECT, (l, _h): (u32, u32), pieds: u32) -> POINT {
    POINT { x: (zone.left + zone.right - l as i32) / 2, y: zone.bottom - pieds as i32 }
}

/// L'approche à `avance` (0..=1) : `(coin haut-gauche, échelle)`. Elle part,
/// minuscule, du haut à droite de `zone` et finit exactement sur la case posée.
pub fn approche(avance: f32, zone: RECT, (l, h): (u32, u32), pose: POINT) -> (POINT, f32) {
    let t = avance.clamp(0.0, 1.0);
    let echelle = ECHELLE_LOIN * (1.0 / ECHELLE_LOIN).powf(t);
    // Le chemin suit l'échelle : la perspective d'un vol en ligne droite.
    let w = (echelle - ECHELLE_LOIN) / (1.0 - ECHELLE_LOIN);
    let (zl, zh) = ((zone.right - zone.left) as f32, (zone.bottom - zone.top) as f32);
    let (dx, dy) = (zone.left as f32 + DEPART.0 * zl, zone.top as f32 + DEPART.1 * zh);
    let (ax, ay) = (pose.x as f32 + l as f32 / 2.0, pose.y as f32 + h as f32 / 2.0);
    let (cx, cy) = (dx + (ax - dx) * w, dy + (ay - dy) * w);
    let (el, eh) = (l as f32 * echelle, h as f32 * echelle);
    (POINT { x: (cx - el / 2.0).round() as i32, y: (cy - eh / 2.0).round() as i32 }, echelle)
}

/// Le coin haut-gauche pendant la marche ; `None` quand la case est
/// ENTIÈREMENT sortie de `ecran`.
pub fn marche(pose: POINT, parcouru: i32, l: u32, vers_la_droite: bool, ecran: RECT) -> Option<POINT> {
    let x = if vers_la_droite { pose.x + parcouru } else { pose.x - parcouru };
    (x < ecran.right && x + l as i32 > ecran.left).then_some(POINT { x, y: pose.y })
}

// ---------------------------------------------------------------- les images

/// Le héros prêt à atterrir : ses images BGRA prémultipliées, gardées à la
/// taille `source` (au plus celle de la planche) et mises à la `taille` de
/// l'écran image par image ; sa fiche ramenée à la taille de l'écran.
pub struct Atterrisseur {
    pub approche: Vec<Vec<u8>>,
    pub atterrissage: Vec<Vec<u8>>,
    pub marche: Vec<Vec<u8>>,
    pub source: (u32, u32),
    pub taille: (u32, u32),
    pub fiche: Fiche,
    /// Pieds et foulée à l'écran.
    pub pieds: u32,
    pub foulee: f32,
}

/// Décode le héros `n` pour qu'il mesure `debout` pixels physiques une fois
/// posé. Dans un fil à part : ~150 images.
pub fn preparer(dossier: &std::path::Path, n: u32, debout: u32, vers_la_droite: bool) -> Option<Atterrisseur> {
    let chemin = dossier.join(format!("heros-{n:02}-atterrissage.txt"));
    let texte = std::fs::read_to_string(&chemin)
        .map_err(|e| crate::diag!("[heros] {} illisible : {e}", chemin.display()))
        .ok()?;
    let Some(fiche) = lire_fiche(&texte) else {
        crate::diag!("[heros] {} mal formée", chemin.display());
        return None;
    };
    let k = debout as f32 / fiche.debout as f32;
    let a_l_echelle = |k: f32| (((fiche.largeur as f32 * k).round() as u32).max(8), ((fiche.hauteur as f32 * k).round() as u32).max(8));
    let taille = a_l_echelle(k);
    let source = a_l_echelle(k.min(1.0));
    let planche = |nom: &str, combien: u32| -> Option<Vec<Vec<u8>>> {
        let chemin = dossier.join(format!("heros-{n:02}-{nom}.webp"));
        let octets = std::fs::read(&chemin).map_err(|e| crate::diag!("[heros] {} illisible : {e}", chemin.display())).ok()?;
        let image = image::load_from_memory_with_format(&octets, image::ImageFormat::WebP)
            .map_err(|e| crate::diag!("[heros] {} non décodée : {e}", chemin.display()))
            .ok()?
            .to_rgba8();
        let (l, h) = image.dimensions();
        let (cl, ch) = (fiche.largeur, fiche.hauteur);
        if l < cl * super::COLONNES || h < ch * combien.div_ceil(super::COLONNES) {
            crate::diag!("[heros] {} : {l}×{h} trop petite pour {combien} cases de {cl}×{ch}", chemin.display());
            return None;
        }
        let images = decouper(image.as_raw(), l, (cl, ch), combien)
            .iter()
            .map(|rgba| {
                let bgra = crate::win32::epingle::premultiplier_bgra(rgba);
                let petite = crate::win32::epingle::reechantillonner(&bgra, cl, ch, source.0, source.1);
                if vers_la_droite { petite } else { miroir(&petite, source.0) }
            })
            .collect();
        Some(images)
    };
    Some(Atterrisseur {
        approche: planche("approche", fiche.approche)?,
        atterrissage: planche("atterrissage", fiche.atterrissage)?,
        marche: planche("marche", fiche.marche)?,
        source,
        taille,
        pieds: (fiche.pieds as f32 * k).round() as u32,
        foulee: fiche.foulee as f32 * k,
        fiche,
    })
}

/// Une image gardée, à la taille de l'écran (copiée seulement s'il faut l'agrandir).
fn a_l_ecran<'a>(a: &Atterrisseur, px: &'a [u8]) -> Cow<'a, [u8]> {
    if a.source == a.taille { Cow::Borrowed(px) } else { Cow::Owned(mettre_a_taille(px, a.source, a.taille)) }
}

/// Ce qu'il faut montrer à l'instant `ecoule` : `(clé de l'image, coin
/// haut-gauche, taille, pixels)` ; `None` quand il est sorti de l'écran. La clé
/// change dès que les pixels changent (image ou taille).
pub fn cadre(
    a: &Atterrisseur,
    ecoule: f32,
    zone: RECT,
    ecran: RECT,
    vers_la_droite: bool,
) -> Option<(u64, POINT, (u32, u32), Cow<'_, [u8]>)> {
    let pose = point_de_pose(zone, a.taille, a.pieds);
    match phase(ecoule, &a.fiche) {
        Phase::Approche { image, avance } => {
            let (coin, echelle) = approche(avance, zone, a.taille, pose);
            let l = ((a.taille.0 as f32 * echelle).round() as u32).max(2);
            let h = ((a.taille.1 as f32 * echelle).round() as u32).max(2);
            let px = mettre_a_taille(&a.approche[image], a.source, (l, h));
            let cle = (1 << 48) | ((image as u64) << 32) | ((l as u64) << 16) | h as u64;
            Some((cle, coin, (l, h), Cow::Owned(px)))
        }
        Phase::Pose { image, parcouru } => {
            let parcouru = (parcouru * a.foulee / a.fiche.foulee.max(1) as f32).round() as i32;
            let coin = marche(pose, parcouru, a.taille.0, vers_la_droite, ecran)?;
            Some(((2 << 48) | image as u64, coin, a.taille, a_l_ecran(a, &a.atterrissage[image])))
        }
        Phase::Marche { image, parcouru } => {
            let parcouru = (parcouru * a.foulee / a.fiche.foulee.max(1) as f32).round() as i32;
            let coin = marche(pose, parcouru, a.taille.0, vers_la_droite, ecran)?;
            Some(((3 << 48) | image as u64, coin, a.taille, a_l_ecran(a, &a.marche[image])))
        }
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    const FICHE: Fiche =
        Fiche { approche: 32, atterrissage: 81, marche: 32, largeur: 254, hauteur: 260, pieds: 247, debout: 240, foulee: 193, marche_des: 65 };

    #[test]
    fn la_fiche_se_lit_et_refuse_l_incomplet() {
        let texte = "approche=32\natterrissage=81\nmarche=32\nlargeur=254\nhauteur=260\npieds=247\ndebout=240\nfoulee=193\nmarche_des=65\n";
        assert_eq!(lire_fiche(texte), Some(FICHE));
        assert_eq!(lire_fiche(&texte.replace("foulee=193\n", "")), None, "une clé manque");
        let sans = lire_fiche(&texte.replace("marche_des=65\n", "")).unwrap();
        assert_eq!(sans.marche_des, 81, "sans `marche_des` : il marche à la fin du clip");
        assert_eq!(lire_fiche(&texte.replace("marche_des=65", "marche_des=500")).unwrap().marche_des, 81, "borné au clip");
        assert_eq!(lire_fiche(&texte.replace("marche=32", "marche=0")), None, "aucune image");
        assert_eq!(lire_fiche(&texte.replace("pieds=247", "pieds=300")), None, "pieds hors de la case");
    }

    #[test]
    fn seules_les_fiches_d_atterrissage_comptent() {
        let noms = ["heros-02-atterrissage.txt", "heros-02-marche.webp", "heros-07.webp", "heros-11-atterrissage.txt", "heros-x-atterrissage.txt"];
        assert_eq!(heros_atterrisseurs(noms.into_iter()), [2, 11]);
        // Et les planches d'atterrissage ne passent pas pour des traversées.
        let noms = ["heros-02-approche.webp", "heros-02-atterrissage.webp", "heros-02-marche.webp"];
        assert!(super::super::heros_presents(noms.into_iter()).is_empty());
    }

    #[test]
    fn les_phases_s_enchainent_sur_les_bonnes_images() {
        let approche = duree_approche(&FICHE);
        assert_eq!(approche, 4.0, "deux boucles de 32 images à 16 i/s");
        assert_eq!(phase(0.0, &FICHE), Phase::Approche { image: 0, avance: 0.0 });
        // Juste avant de se poser : la DERNIÈRE image de la boucle, qui précède la première.
        assert!(matches!(phase(approche - 0.01, &FICHE), Phase::Approche { image: 31, .. }));
        assert_eq!(phase(approche, &FICHE), Phase::Pose { image: 0, parcouru: 0.0 });
        // Il ne bouge pas avant `marche_des`...
        let des = approche + 65.0 / IMAGES_PAR_SECONDE;
        assert_eq!(phase(des - 0.01, &FICHE), Phase::Pose { image: 64, parcouru: 0.0 });
        // ... et avance DÉJÀ pendant la fin de l'atterrissage (il y marche sur place).
        let pose = FICHE.atterrissage as f32 / IMAGES_PAR_SECONDE;
        match phase(approche + pose - 0.01, &FICHE) {
            Phase::Pose { image: 80, parcouru } => assert!((80.0..100.0).contains(&parcouru), "une seconde de marche moins l'élan : {parcouru}"),
            autre => panic!("{autre:?}"),
        }
        // Sans à-coup au passage à la planche de marche.
        let (Phase::Pose { parcouru: avant, .. }, Phase::Marche { image: 0, parcouru: apres }) =
            (phase(approche + pose - 0.001, &FICHE), phase(approche + pose, &FICHE))
        else {
            panic!("passage mal placé")
        };
        assert!((apres - avant).abs() < 0.5, "{avant} → {apres}");
    }

    #[test]
    fn la_marche_va_sept_pour_cent_plus_vite_que_la_foulee_apres_son_elan() {
        let boucle = FICHE.marche as f32 / IMAGES_PAR_SECONDE;
        // Après l'élan, une boucle de marche = une foulée × 1,07.
        let d = distance_de_marche(1.0 + boucle, &FICHE) - distance_de_marche(1.0, &FICHE);
        assert!((d - 193.0 * 1.07).abs() < 0.01, "{d}");
        // L'élan part de l'arrêt, sans saut, et rejoint la vitesse sans saut.
        assert_eq!(distance_de_marche(0.0, &FICHE), 0.0);
        assert_eq!(distance_de_marche(-1.0, &FICHE), 0.0);
        let v = 193.0 * 1.07 / boucle;
        let pente = (distance_de_marche(ELAN + 0.001, &FICHE) - distance_de_marche(ELAN - 0.001, &FICHE)) / 0.002;
        assert!((pente - v).abs() < 1.0, "{pente} contre {v}");
    }

    #[test]
    fn l_approche_part_minuscule_du_haut_droit_et_finit_sur_la_pose() {
        let zone = RECT { left: 1920, top: 40, right: 1920 + 3840, bottom: 2100 };
        let taille = (254, 260);
        let pose = point_de_pose(zone, taille, 247);
        assert_eq!(pose, POINT { x: 1920 + (3840 - 254) / 2, y: 2100 - 247 }, "centrée, les pieds sur le bas");
        let (debut, e0) = approche(0.0, zone, taille, pose);
        assert!((e0 - ECHELLE_LOIN).abs() < 1e-6);
        assert!(debut.x > 1920 + 3840 * 8 / 10 && debut.y < 40 + 2060 / 5, "en haut à droite : {debut:?}");
        let (fin, e1) = approche(1.0, zone, taille, pose);
        assert!((e1 - 1.0).abs() < 1e-4);
        assert_eq!(fin, pose, "finit exactement sur la case posée");
        // Croissance régulière : le même facteur à chaque pas.
        let (_, a) = approche(0.25, zone, taille, pose);
        let (_, b) = approche(0.5, zone, taille, pose);
        let (_, c) = approche(0.75, zone, taille, pose);
        assert!((b / a - c / b).abs() < 1e-3);
        assert!(a > 0.08, "déjà visible au quart du chemin : {a}");
    }

    #[test]
    fn la_marche_sort_entierement_par_le_bord_choisi() {
        let ecran = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        let pose = POINT { x: 833, y: 800 };
        assert_eq!(marche(pose, 100, 254, true, ecran), Some(POINT { x: 933, y: 800 }));
        assert_eq!(marche(pose, 100, 254, false, ecran), Some(POINT { x: 733, y: 800 }));
        assert_eq!(marche(pose, 1920 - 833 - 1, 254, true, ecran).map(|p| p.x), Some(1919), "un pixel encore dedans");
        assert_eq!(marche(pose, 1920 - 833, 254, true, ecran), None, "sorti à droite");
        assert_eq!(marche(pose, 833 + 253, 254, false, ecran).map(|p| p.x), Some(-253), "un pixel encore dedans");
        assert_eq!(marche(pose, 833 + 254, 254, false, ecran), None, "sorti à gauche");
    }

    /// Les héros livrés : chaque fiche se lit et ses trois planches se découpent
    /// en images non vides.
    #[test]
    fn chaque_atterrisseur_livre_se_decoupe() {
        let dossier = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/popups/mascottes");
        let noms: Vec<String> =
            std::fs::read_dir(&dossier).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        for n in heros_atterrisseurs(noms.iter().map(String::as_str)) {
            let a = preparer(&dossier, n, 10_000, true).unwrap_or_else(|| panic!("héros {n} : illisible"));
            assert_eq!(a.source, (a.fiche.largeur, a.fiche.hauteur), "gardé à la taille de la planche, agrandi à l'affichage");
            assert!(a.taille.1 > a.source.1 * 10);
            for (nom, images) in [("approche", &a.approche), ("atterrissage", &a.atterrissage), ("marche", &a.marche)] {
                for (k, im) in images.iter().enumerate() {
                    assert!(im.chunks_exact(4).any(|p| p[3] > 128), "héros {n} : {nom} {k} vide");
                }
            }
        }
    }
}
