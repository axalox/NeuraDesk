//! **L'AMÉLIORATION DE CONTRASTE ET DE LUMINOSITÉ DES PAGES SCANNÉES.**
//!
//! DEMANDE DU 2026-10-27 : « dans scanner, quand une page est agrandie, des outils
//! d'amélioration d'image pour améliorer automatiquement ou manuellement le
//! contraste et la luminosité des pages ». Trois décisions posées avec la personne :
//! **les deux hôtes** (le mode « page » du panneau ET la fenêtre détachée), **les
//! niveaux automatiques + trois curseurs** (luminosité, contraste, saturation, comme
//! dans l'éditeur d'image), et **une copie écrite à la demande** — la page
//! numérisée n'est JAMAIS écrasée.
//!
//! ## LE PARTAGE QUI FAIT LA DESIGN
//!
//! Un réglage vit ICI, en Rust, et il est poussé aux deux hôtes. C'est la même
//! règle que `cef_page_scan::etat_page`, que `cef_scanner::etat_json` reprend
//! mot pour mot : les deux affichages ne peuvent pas montrer deux choses
//! différentes de la même page.
//!
//! ⭐ **L'APERÇU EST LE FILTRE CSS, LE FICHIER ÉCRIT EST LE MÊME CALCUL EN RUST.**
//! Les curseurs ne re-encodent rien (une page de 300 dpi pèse des mégaoctets, et le
//! relâcher d'un ascenseur arriverait soixante fois par seconde) : la page applique
//! `filter: brightness() contrast() saturate()`, et le bouton d'enregistrement
//! applique `appliquer()` aux pixels, dans le MÊME ORDRE et avec les MÊMES
//! formules. Un aperçu qui ne ressemble pas au fichier enregistré est le défaut que
//! ce module refuse — d'où les formules écrites une fois, ici.
//!
//! ⚠️ **L'ORDRE DES TROIS FILTRES EST PARTIE DU CONTRAT** : luminosité (une
//! multiplication), puis contraste (un pivot autour de 128), puis saturation.
//! `filtre_css` émet dans cet ordre et `appliquer_pixel` calcule dans cet ordre.
//! Les inverser donnerait un écart visible sur une page grise — le cas exact pour
//! lequel cette fonction existe.
//!
//! ## CE QUE L'AUTOMATIQUE NE FAIT PAS, et c'est délibéré
//!
//! `auto_niveaux` touche **luminosité et contraste seulement**. La saturation reste
//! à 0 : étirer l'histogramme de luminance ne dit rien de la colorimétrie d'un
//! scan, et une saturation automatique sur une photo de journal la rendrait
//! fluo. Le curseur reste à la main.

use image::RgbaImage;
use std::path::{Path, PathBuf};

/// **LA BORNE DES TROIS CURSEURS** — la même que `BORNE_REGLAGE_COULEUR` de
/// l'éditeur d'image (`assets/popups/editeur_geometrie.js`) : −100 à +100, **0 =
/// inchangé**, la valeur devenant le pourcentage d'un filtre (0 % à 200 %).
/// Deux bornes différentes entre l'éditeur et le scanner feraient deux « max » qui
/// ne veulent pas dire la même chose.
pub const BORNE: i32 = 100;

/// Les percentiles de l'étirement automatique. 2 % / 98 % : assez pour écarter le
/// noir et le blanc « noise » d'un scanner, pas assez pour qu'une petite tache
/// d'encre ou une vignette blanche décide de toute l'échelle.
pub const PERCENTILE_BAS: f32 = 0.02;
pub const PERCENTILE_HAUT: f32 = 0.98;

/// Une plage de luminances plus étroite que ça est **déjà plate** : l'étirer
/// posteriserait la page sans rien montrer. Ce seuil est un plancher de
/// TRACTABILITÉ, pas un réglage esthétique.
pub const PLAGE_MINIMALE: u8 = 16;

/// Une plage de luminances plus large que ça est **déjà étendue** : il reste moins
/// d'un dixième de l'échelle à gagner, et l'étirer coûterait des demi-teintes pour
/// un gain que l'œil ne voit pas.
///
/// ⚠️ **CE GARDE EST NÉ DU TEST, PAS D'UNE INTUITION.** Une page couvrant 5 à 250
/// calculait un contraste de **+4** — un nombre qui n'est rien à l'écran, mais qui
/// ré-encode quand même la page et posterise les deux nuances du bout.
pub const PLAGE_MAXIMALE: u8 = 240;

/// Un réglage de trois curseurs, en unités de pourcentage (−100…+100, 0 = inchangé).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Reglage {
    pub luminosite: i32,
    pub contraste: i32,
    pub saturation: i32,
}

impl Reglage {
    /// Le réglage qui ne change RIEN — le « Réinitialiser », et l'état d'une page
    /// qu'on vient d'ouvrir.
    pub fn neutre() -> Self {
        Self::default()
    }

    pub fn est_neutre(&self) -> bool {
        *self == Self::neutre()
    }

    /// Une valeur hors borne est **planchée**, pas refusée : elle vient d'une page
    /// HTML (un ascenseur, une frappe), donc d'un code qui a pu diverger. La
    /// refuser en silence laisserait le curseur à l'écran et le fichier à Rust —
    /// deux moitiés qui ne se correspondent plus.
    pub fn borne(&self) -> Self {
        Self {
            luminosite: self.luminosite.clamp(-BORNE, BORNE),
            contraste: self.contraste.clamp(-BORNE, BORNE),
            saturation: self.saturation.clamp(-BORNE, BORNE),
        }
    }

    /// Le facteur d'un filtre : 0 % à 200 %.
    pub fn facteur(unite: i32) -> f32 {
        1.0 + unite.clamp(-BORNE, BORNE) as f32 / 100.0
    }

    /// **LE FILTRE CSS DE L'APERÇU** — `none` quand rien n'est réglé, pour que la
    /// page ne pose aucun filtre sur une page qu'on n'a pas touchée.
    ///
    /// ⚠️ **L'ORDRE EST LE CONTRAT** (voir le commentaire d'en-tête). Et les
    /// pourcentages sont ARRONDIS à l'entier : `filter` ne lit pas les décimales
    /// avec la même précision selon la version de Chromium, et un `118.5%` ne de
    /// rien de mieux qu'un `118 %` — il rend juste l'aperçu non comparable au
    /// fichier.
    pub fn filtre_css(&self) -> String {
        if self.est_neutre() {
            return "none".to_string();
        }
        let luminosite = (Reglage::facteur(self.luminosite) * 100.0).round();
        let contraste = (Reglage::facteur(self.contraste) * 100.0).round();
        let saturation = (Reglage::facteur(self.saturation) * 100.0).round();
        format!("brightness({luminosite}%) contrast({contraste}%) saturate({saturation}%)")
    }
}

/// **LA LUMINANCE**, aux poids Rec. 601 (les mêmes que `0.299 R + 0.587 G + 0.114 B`).
///
/// ⚠️ **CE N'EST PAS LA MOYENNE DES TROIS CANAUX** : une page numérisée en gris a
/// ses trois canaux égaux, donc les deux se ressemblent — mais sur un scan en
/// couleur (une page jaunie), la moyenne surestimerait le blanc et l'étirement
/// automatique centrerait mal.
pub fn luminance(r: u8, g: u8, b: u8) -> u8 {
    let v = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
    (v.round().clamp(0.0, 255.0)) as u8
}

/// L'histogramme de luminance d'une image — 256 cases, une par valeur.
pub fn histogramme(image: &RgbaImage) -> [u32; 256] {
    let mut h = [0u32; 256];
    for pixel in image.pixels() {
        h[luminance(pixel[0], pixel[1], pixel[2]) as usize] += 1;
    }
    h
}

/// **LA VALEUR DE LUMINANCE AU DESSUS DE `proportion` DES PIXELS** — pure, donc
/// testable sans image : `proportion` hors de 0..1 est bornée, et une histogramme
/// vide rend 0 plutôt que de paniquer sur une cumulative qui n'atteint jamais rien.
pub fn percentile(h: &[u32; 256], proportion: f32) -> u8 {
    let total: u64 = h.iter().map(|&n| n as u64).sum();
    if total == 0 {
        return 0;
    }
    let cible = (proportion.clamp(0.0, 1.0) as f64 * total as f64).ceil() as u64;
    let cible = cible.max(1);
    let mut cumulative = 0u64;
    for (valeur, &n) in h.iter().enumerate() {
        cumulative += n as u64;
        if cumulative >= cible {
            return valeur as u8;
        }
    }
    255
}

/// **LES NIVEAUX AUTOMATIQUES** — l'étirement d'histogramme, exprimé dans les
/// unités des curseurs. Pure : elle ne lit que l'histogramme.
///
/// La cible est la MÊME que celle d'un « niveaux » d'un éditeur de photo, et elle
/// est choisie pour que le calcul se lise dans les DEUX sens :
///
/// 1. **centrer la médiane sur 128** — la luminosité est une multiplication, donc
///    `b = 128 / médiane` ; une page grise à 160 est trop claire et part vers le bas,
///    une page sombre à 60 monte.
/// 2. **étirer la plage après ça** — la luminosité a déjà changé la plage
///    (`(haut − bas) × b`), donc le contraste se calcule SUR LA PLAGE DÉJÀ
///    MULTIPLIÉE. Le calculer avant serait le défaut classique : les deux facteurs
///    se corrigeraient l'un l'autre et la page sortirait moins contrastée que prévu.
///
/// ⚠️ **DEUX PLAFONDS À 2×** (la borne des curseurs) : sans eux, une page
/// quasiment plate (plage de 3 nuances) demanderait un contraste de 80× et la
/// sortirait en deux couleurs. Automatiquement « un peu mieux » vaut mieux
/// qu'automatiquement détruit.
pub fn auto_niveaux(h: &[u32; 256]) -> Reglage {
    let total: u64 = h.iter().map(|&n| n as u64).sum();
    if total == 0 {
        return Reglage::neutre();
    }
    let bas = percentile(h, PERCENTILE_BAS);
    let haut = percentile(h, PERCENTILE_HAUT);
    let plage = haut.saturating_sub(bas);
    // ⛔ **PLATE ou DÉJÀ ÉTENDUE : RIEN.** Les deux bouts de l'échelle se refusent
    // pour la même raison — l'étirement n'y a rien à gagner, et il y coûte des
    // nuances.
    if plage < PLAGE_MINIMALE || plage >= PLAGE_MAXIMALE {
        return Reglage::neutre();
    }
    let mediane = ((bas as f32 + haut as f32) / 2.0).max(1.0);
    let b = (128.0 / mediane).clamp(0.5, 2.0);
    let etendue = (haut as f32 - bas as f32) * b;
    let c = (255.0 / etendue.max(1.0)).clamp(0.5, 2.0);
    Reglage {
        luminosite: ((b - 1.0) * 100.0).round() as i32,
        contraste: ((c - 1.0) * 100.0).round() as i32,
        saturation: 0,
    }
    .borne()
}

/// **UN PIXEL, LES TROIS FILTRES, DANS L'ORDRE DU CONTRAT.**
///
/// ⚠️ **ARRONDI, PAS TRONQUÉ, PUIS PLANCHÉ** : une multiplication laisse des
/// décimales, et `.round()` est ce qui fait que le Rust et le CSS tombent sur la
/// même valeur de 0 à 255. Un `as u8` sur un négatif aurait fait un wraparound —
/// une page très contrastée sortirait en négatif coloré (l'inverse du but).
pub fn appliquer_pixel(r: u8, g: u8, b: u8, reglage: &Reglage) -> (u8, u8, u8) {
    let fb = Reglage::facteur(reglage.luminosite);
    let fc = Reglage::facteur(reglage.contraste);
    let fs = Reglage::facteur(reglage.saturation);

    let mut v = [r as f32 * fb, g as f32 * fb, b as f32 * fb];
    for can in v.iter_mut() {
        *can = (*can - 128.0) * fc + 128.0;
    }
    if fs != 1.0 {
        let l = luminance(v[0] as u8, v[1] as u8, v[2] as u8) as f32;
        for can in v.iter_mut() {
            *can = l + (*can - l) * fs;
        }
    }
    let plancher = |x: f32| -> u8 {
        if x.is_nan() {
            return 0;
        }
        (x.round().clamp(0.0, 255.0)) as u8
    };
    (plancher(v[0]), plancher(v[1]), plancher(v[2]))
}

/// **APPLIQUER LE RÉGLAGE AUX PIXELS.** Un réglage neutre rend une copie identique,
/// octet pour octet — c'est le garde qui dit que « Enregistrer » sans réglage ne
/// dégrade pas la page pour rien.
pub fn appliquer(image: &RgbaImage, reglage: &Reglage) -> RgbaImage {
    if reglage.est_neutre() {
        return image.clone();
    }
    let borne = reglage.borne();
    let mut sortie = RgbaImage::new(image.width(), image.height());
    for (x, y, pixel) in image.enumerate_pixels() {
        let (r, g, b) = appliquer_pixel(pixel[0], pixel[1], pixel[2], &borne);
        sortie.put_pixel(x, y, image::Rgba([r, g, b, pixel[3]]));
    }
    sortie
}

/// **LE NOM D'UNE COPIE AMÉLIORÉE, QUI NE COLLISIONNE JAMAIS AVEC UNE PAGE.**
///
/// `page-12-03.jpg` devient `page-12-03.ameliore.jpg`, puis `.ameliore-2.jpg` si la
/// première existe déjà — parce qu'améliorer deux fois la même page est le geste
/// normal de quelqu'un qui cherche ses curseurs, et écraser la copie précédente
/// serait une perte silencieuse.
///
/// ⛔ **ET LE CHEMIN NE PEUT PAS ÊTRE CELUI DE LA PAGE** : c'est la règle du module
/// (une écriture ne coûte jamais la numérisation), vérifiée par un test.
pub fn nom_ameliore(page: &Path) -> PathBuf {
    let stem = page.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "page".to_string());
    let extension = page.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "jpg".to_string());
    for numero in 0..=99 {
        // ⚠️ **LE PREMIER NUMÉRO EST « 2 », PAS « 1 »** : la copie sans suffixe est
        // déjà la première, donc `page-1.jpg` déjà copié une fois donne
        // `.ameliore-2.jpg`. Un `-1` à côté de l'absence de suffixe ferait deux noms
        // pour la même idée, et « quelle est la deuxième ? » ne se répondrait plus.
        let nom = if numero == 0 {
            format!("{stem}.ameliore.{extension}")
        } else {
            format!("{stem}.ameliore-{}.{}", numero + 1, extension)
        };
        let candidat = page.with_file_name(nom);
        if candidat != page && !candidat.exists() {
            return candidat;
        }
    }
    // 100 copies porteuses du même nom : on horodate plutôt que d'écraser.
    page.with_file_name(format!("{stem}.ameliore-horodate-{}.jpg", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)))
}

/// **ÉCRIRE LA COPIE AMÉLIORÉE DE LA PAGE** — à côté, jamais dedans.
///
/// ⛔ **LA LEÇON DU 2026-09-25 EST ICI, ET ELLE EST LA MÊME QUE POUR LA ROTATION**
/// (`cef_page_scan::tourner_fichier`) : `RgbaImage::save` vers un `.jpg` **échoue
/// parce que le codec JPEG n'a pas de canal alpha**, et il **tronque la destination
/// avant d'échouer**. Donc :
///
/// 1. on convertit **en RGB** avant de sauver ;
/// 2. on écrit dans un **temporaire porteur de la bonne extension**, puis on le
///    renomme — la page ne bouge pas, elle n'a même pas besoin d'être relisible.
///
/// ⚠️ **L'EXTENSION DU TEMPORAIRE SUIT CELLE DE LA CIBLE** : `save` choisit le
/// format par l'extension, et écrire un JPEG sous un `.png` le ferait échouer.
pub fn ecrire_amelioration(page: &Path, reglage: &Reglage, cible: &Path) -> Result<PathBuf, String> {
    if cible == page {
        return Err("la copie améliorée ne peut pas porter le nom de la page".to_string());
    }
    let image = image::open(page)
        .map_err(|erreur| format!("{} n'a pas pu être relue : {erreur}", page.display()))?
        .to_rgba8();
    let amelioree = appliquer(&image, reglage);
    let rgb = image::DynamicImage::ImageRgba8(amelioree).to_rgb8();

    let extension = cible.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "jpg".to_string());
    let temporaire = cible.with_extension(format!("amelioration-neuve.{extension}"));
    if let Err(erreur) = rgb.save(&temporaire) {
        let _ = std::fs::remove_file(&temporaire);
        return Err(format!("la copie améliorée n'a pas pu être écrite : {erreur}"));
    }
    if let Err(erreur) = std::fs::rename(&temporaire, cible) {
        let _ = std::fs::remove_file(&temporaire);
        return Err(format!("la copie améliorée n'a pas pu être posée : {erreur}"));
    }
    Ok(cible.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une image de test dont la luminance va de `bas` à `haut`, en paliers de un.
    fn rampe(bas: u8, haut: u8) -> RgbaImage {
        let pas = (haut - bas).max(1) as u32;
        let mut image = RgbaImage::new(pas + 1, 4);
        for (x, _y, pixel) in image.enumerate_pixels_mut() {
            let v = bas + x as u8;
            *pixel = image::Rgba([v, v, v, 255]);
        }
        image
    }

    #[test]
    fn le_reglage_neutre_laisse_chaque_pixel_octet_pour_octet() {
        let image = rampe(0, 255);
        let sortie = appliquer(&image, &Reglage::neutre());
        assert_eq!(sortie, image, "un réglage neutre ne doit rien changer, pas même d'un octet");
        assert_eq!(Reglage::neutre().filtre_css(), "none");
    }

    #[test]
    fn un_scan_gris_monte_en_contraste_et_se_centre_sur_128() {
        // Le cas pour lequel la demande existe : une page numérisée trop claire et
        // trop plate (160 ± 10).
        let image = rampe(150, 170);
        let reglage = auto_niveaux(&histogramme(&image));
        assert!(reglage.contraste > 0, "une page plate doit GAGNER du contraste : {reglage:?}");
        assert!(reglage.luminosite < 0, "une médiane à 160 est TROP CLAIRE, elle descend : {reglage:?}");
        assert_eq!(reglage.saturation, 0, "les niveaux ne touchent pas la saturation");

        let h_ameliore = histogramme(&appliquer(&image, &reglage));
        let avant = (percentile(&histogramme(&image), 0.98) - percentile(&histogramme(&image), 0.02)) as i32;
        let apres = (percentile(&h_ameliore, 0.98) - percentile(&h_ameliore, 0.02)) as i32;
        assert!(apres > avant, "la plage doit s'ÉLARGIR : {avant} -> {apres}");

        let mediane = (percentile(&h_ameliore, 0.5) as i32 - 128).abs();
        assert!(mediane <= 8, "la médiane doit être ramenée près de 128, pas à {mediane} octets de côté");
    }

    #[test]
    fn une_page_deja_etendue_ne_recoit_rien() {
        let reglage = auto_niveaux(&histogramme(&rampe(0, 255)));
        assert_eq!(reglage, Reglage::neutre(), "une page qui couvre déjà tout ne doit pas être posterisée");
    }

    #[test]
    fn une_page_quasi_plate_ne_detruit_rien() {
        // Trois nuances seulement : l'étirer à 2× ne montrerait rien de plus, ça
        // détruirait les demi-teintes restantes.
        let reglage = auto_niveaux(&histogramme(&rampe(120, 122)));
        assert_eq!(reglage, Reglage::neutre(), "une plage sous le plancher doit rester inchangée");
    }

    #[test]
    fn un_histogramme_vide_ne_panique_pas() {
        assert_eq!(percentile(&[0u32; 256], 0.5), 0);
        assert_eq!(auto_niveaux(&[0u32; 256]), Reglage::neutre());
    }

    #[test]
    fn le_filtre_css_porte_les_trois_reglages_arrondis() {
        let reglage = Reglage { luminosite: 20, contraste: 18, saturation: -12 };
        assert_eq!(reglage.filtre_css(), "brightness(120%) contrast(118%) saturate(88%)");
        // L'ordre est le contrat : contraste avant luminosité ne donne pas la même image.
        assert!(reglage.filtre_css().find("brightness").unwrap() < reglage.filtre_css().find("contrast").unwrap());
    }

    #[test]
    fn une_valeur_hors_borne_est_planchee_et_le_filtre_suiv() {
        let hors = Reglage { luminosite: 900, contraste: -900, saturation: 7 };
        let borne = hors.borne();
        assert_eq!(borne.luminosite, BORNE);
        assert_eq!(borne.contraste, -BORNE);
        assert_eq!(borne.saturation, 7, "ce qui est dans la borne ne bouge pas");
        // `facteur` planche aussi, pour qu'un appelant qui oublie `borne()` ne
        // fabrique pas un filtre à 1000 %.
        assert_eq!(Reglage::facteur(900), 2.0);
    }

    #[test]
    fn le_contraste_pivot_autour_de_128_et_non_autour_de_zero() {
        // 128 (le milieu) ne doit PAS bouger sous un contraste pur — c'est ce qui
        // distingue un contraste d'une simple multiplication.
        let sombre = appliquer_pixel(0, 0, 0, &Reglage { contraste: 100, ..Reglage::neutre() });
        let milieu = appliquer_pixel(128, 128, 128, &Reglage { contraste: 100, ..Reglage::neutre() });
        let clair = appliquer_pixel(255, 255, 255, &Reglage { contraste: 100, ..Reglage::neutre() });
        assert_eq!(sombre, (0, 0, 0), "le noir ne peut pas descendre sous 0");
        assert_eq!(milieu, (128, 128, 128), "le milieu du pivot ne bouge pas");
        assert_eq!(clair, (255, 255, 255), "le blanc ne peut pas monter au-dessus de 255");
    }

    #[test]
    fn une_luminosite_negative_ne_fait_pas_un_wraparound() {
        // Sans le plancher, `-128 as u8` remonterait à 128 : une page assombrie
        // ressortirait à moitié blanche.
        let (r, g, b) = appliquer_pixel(10, 10, 10, &Reglage { luminosite: -100, ..Reglage::neutre() });
        assert_eq!((r, g, b), (0, 0, 0), "−100 % doit assombrir jusqu'à 0, pas faire le tour");
    }

    #[test]
    fn le_nom_ameliore_ne_collisionne_jamais_avec_la_page() {
        let dossier = std::env::temp_dir().join(format!("nd-amelioration-nom-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dossier);
        let page = dossier.join("page-1.jpg");
        std::fs::write(&page, b"pas une image, mais un nom").expect("ecriture du temoin");

        let premier = nom_ameliore(&page);
        assert_eq!(premier, dossier.join("page-1.ameliore.jpg"));
        assert_ne!(premier, page, "la copie ne peut pas porter le nom de la numérisation");

        std::fs::write(&premier, b"x").expect("premiere copie posee");
        let second = nom_ameliore(&page);
        assert_eq!(second, dossier.join("page-1.ameliore-2.jpg"), "deuxième copie : un autre nom, pas un écrasement");

        std::fs::remove_file(&premier).ok();
        std::fs::remove_file(&page).ok();
        std::fs::remove_dir(&dossier).ok();
    }

    #[test]
    fn ecrire_amelioration_ecrit_a_cote_et_laisse_la_page_entiere() {
        let dossier = std::env::temp_dir().join(format!("nd-amelioration-fichier-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dossier);
        let page = dossier.join("page-2.jpg");

        // Un vrai JPEG, écrit par le même codec que celui de la lecture : une page
        // du scanner est un JPEG, et un PNG ne montrerait pas le canal alpha qui a
        // coûté le défaut du 2026-09-25.
        let originelle = rampe(150, 170);
        image::DynamicImage::ImageRgba8(originelle.clone())
            .to_rgb8()
            .save(&page)
            .expect("temoin jpeg ecrit");
        let avant = std::fs::read(&page).expect("relue");

        let reglage = auto_niveaux(&histogramme(&originelle));
        let cible = nom_ameliore(&page);
        let ecrit = ecrire_amelioration(&page, &reglage, &cible).expect("copie amelioree ecrite");

        assert_eq!(ecrit, cible);
        assert_eq!(std::fs::read(&page).expect("page relue"), avant, "la numérisation n'a pas bougé");
        assert!(image::open(&cible).is_ok(), "la copie doit être relisible");

        // ⚠️ **RELISIBLE NE VEUT PAS DIRE AMÉLIORÉE.** Sans cette comparaison, un
        // `ecrire_amelioration` qui recopierait les octets de la page passerait : la
        // personne aurait un fichier de plus, identique, et le bouton « Enregistrer la
        // copie » ne ferait que dupliquer en dégradé.
        let copie = image::open(&cible).expect("copie relue").to_rgb8();
        let originelle_lue = image::open(&page).expect("page relue").to_rgb8();
        assert_ne!(copie.as_raw(), originelle_lue.as_raw(), "la copie porte le réglage, pas la page recopiée");

        // ⛔ Le refus du même nom, et il doit venir AVANT toute écriture.
        assert!(ecrire_amelioration(&page, &reglage, &page).is_err(), "ecrire dans la page est refuse");
        assert_eq!(std::fs::read(&page).expect("page toujours entiere"), avant, "le refus n'a rien tronque");

        std::fs::remove_file(&cible).ok();
        std::fs::remove_file(&page).ok();
        std::fs::remove_dir(&dossier).ok();
    }

    /// FALSIFICATION : ce test rougit si `auto_niveaux` calcule le contraste AVANT
    /// la luminosité (les deux facteurs se corrigeraient l'un l'autre).
    #[test]
    fn le_contraste_se_calcule_sur_la_plage_deja_multipliee() {
        let image = rampe(150, 170);
        let reglage = auto_niveaux(&histogramme(&image));
        let b = Reglage::facteur(reglage.luminosite);
        let attendue = (255.0 / ((20.0) * b)).clamp(0.5, 2.0);
        assert!(
            (Reglage::facteur(reglage.contraste) - attendue).abs() < 0.02,
            "contraste = 255 / (plage × luminosité) : attendu {attendue:.3}, obtenu {:.3}",
            Reglage::facteur(reglage.contraste)
        );
    }
}
