//! ⭐ **LE CENTRE DE LA BARRE DU HAUT** — DEMANDE EXPLICITE (2026-10-31) :
//! « ajouter une couleur de fond au centre de la barre du haut avec des
//! ombrages noirs ou de couleur qui s'étendent en fondu sur le reste de la
//! barre, comme ce qui est déjà disponible pour les autres barres ».
//!
//! Un bloc centré, de la hauteur de la barre du haut et d'une largeur en % de
//! la barre ; sa couleur vient du sélecteur, avec une transparence ; ses coins
//! sont carrés ou arrondis (au choix). De ses deux bords partent deux fondus,
//! réglés chacun à part comme les deux ombres des barres : un COLORÉ (la couleur
//! du bloc) et un NOIR, posé par-dessus. Leur largeur se règle en % de l'espace
//! entre le bloc et le bout de la barre (100 % : jusqu'au bout), et chacun a
//! une case par côté.
//!
//! Tout se compose dans le calque des ombres du dock (`ombre_barre`, voir
//! `Calque`) : le shader le pose sur les FONDS de barre, sous les icônes et le
//! texte, qui restent lisibles par-dessus le bloc.

/// Un fondu d'un côté à l'autre du bloc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fondu {
    /// En % de l'espace entre le bloc et le bout de la barre, `0..=100`.
    pub largeur: f32,
    /// `0..=1`.
    pub force: f32,
    pub gauche: bool,
    pub droite: bool,
}

impl Fondu {
    const DEFAUT: Self = Self { largeur: 50.0, force: 0.6, gauche: true, droite: true };

    fn bornes(self, defaut: Self) -> Self {
        Self { largeur: borne(self.largeur, 100.0, defaut.largeur), force: borne(self.force, 1.0, defaut.force), ..self }
    }

    /// « largeur,force,gauche,droite », les cases en 0/1.
    fn serialiser(&self) -> String {
        format!("{},{},{},{}", self.largeur, self.force, u8::from(self.gauche), u8::from(self.droite))
    }

    fn lire(contenu: &str, defaut: Self) -> Self {
        let champs: Vec<Option<f32>> = contenu.split(',').map(|c| c.trim().parse::<f32>().ok()).collect();
        let champ = |i: usize| champs.get(i).copied().flatten();
        let case = |i: usize, d: bool| champ(i).map_or(d, |v| v >= 0.5);
        Self {
            largeur: champ(0).unwrap_or(defaut.largeur),
            force: champ(1).unwrap_or(defaut.force),
            gauche: case(2, defaut.gauche),
            droite: case(3, defaut.droite),
        }
    }

    /// L'opacité à `d` pixels du bloc, du côté `a_gauche`, sur une portée de
    /// `espace` pixels (du bloc au bout de la barre) : la courbe des ombres des
    /// barres (`ombre_barre::Trait::alpha`), nulle au-delà de sa largeur.
    fn alpha(&self, d: f32, espace: f32, a_gauche: bool) -> f32 {
        let portee = espace * self.largeur / 100.0;
        if (a_gauche && !self.gauche) || (!a_gauche && !self.droite) || portee <= 0.0 || d < 0.0 {
            return 0.0;
        }
        let t = 1.0 - (d / portee).clamp(0.0, 1.0);
        self.force * t * t
    }
}

/// Les réglages du centre de la barre du haut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReglagesCentre {
    pub actif: bool,
    /// En % de la largeur de la barre, `0..=100`.
    pub largeur: f32,
    pub couleur: [u8; 3],
    /// `0..=1` : 0, le bloc est opaque.
    pub transparence: f32,
    /// Coins arrondis (en demi-cercle de la hauteur de la barre) ; sinon carrés.
    pub arrondi: bool,
    pub noire: Fondu,
    pub coloree: Fondu,
}

impl ReglagesCentre {
    /// Éteint ; un tiers de la barre, bleu nuit, coins arrondis, les deux fondus.
    pub const DEFAUT: Self = Self {
        actif: false,
        largeur: 33.0,
        couleur: [0x2a, 0x4a, 0x8a],
        transparence: 0.0,
        arrondi: true,
        noire: Fondu::DEFAUT,
        coloree: Fondu { largeur: 100.0, force: 0.8, ..Fondu::DEFAUT },
    };

    fn bornes(self) -> Self {
        let d = Self::DEFAUT;
        Self {
            largeur: borne(self.largeur, 100.0, d.largeur),
            transparence: borne(self.transparence, 1.0, d.transparence),
            noire: self.noire.bornes(d.noire),
            coloree: self.coloree.bornes(d.coloree),
            ..self
        }
    }

    /// « actif,largeur,RRGGBB,transparence,arrondi;<noire>;<colorée> ».
    pub fn serialiser(&self) -> String {
        let [r, g, b] = self.couleur;
        format!(
            "{},{},{r:02x}{g:02x}{b:02x},{},{};{};{}",
            u8::from(self.actif),
            self.largeur,
            self.transparence,
            u8::from(self.arrondi),
            self.noire.serialiser(),
            self.coloree.serialiser()
        )
    }

    /// Relit `serialiser` ; un champ absent ou illisible garde sa valeur par défaut.
    pub fn charger(contenu: &str) -> Self {
        let d = Self::DEFAUT;
        let mut parts = contenu.trim().split(';');
        let tete: Vec<&str> = parts.next().unwrap_or("").split(',').map(str::trim).collect();
        let nombre = |i: usize| tete.get(i).and_then(|c| c.parse::<f32>().ok());
        let case = |i: usize, defaut: bool| nombre(i).map_or(defaut, |v| v >= 0.5);
        Self {
            actif: case(0, d.actif),
            largeur: nombre(1).unwrap_or(d.largeur),
            couleur: tete.get(2).and_then(|h| hex(h)).unwrap_or(d.couleur),
            transparence: nombre(3).unwrap_or(d.transparence),
            arrondi: case(4, d.arrondi),
            noire: Fondu::lire(parts.next().unwrap_or(""), d.noire),
            coloree: Fondu::lire(parts.next().unwrap_or(""), d.coloree),
        }
        .bornes()
    }

    /// Le bloc à peindre sur une barre du haut de `largeur` × `hauteur` pixels ;
    /// `None` : éteint, rien à peindre (bloc nul), ou pas de barre du haut.
    pub fn bloc(&self, largeur: i32, hauteur: i32) -> Option<Bloc> {
        let large = (largeur as f32 * self.largeur / 100.0).round() as i32;
        (self.actif && largeur > 0 && hauteur > 0 && large > 0).then_some(Bloc { reglages: *self, largeur, hauteur, large })
    }
}

/// Le centre d'UNE barre du haut : ses réglages et sa géométrie, en pixels de
/// la fenêtre de dock (la barre du haut en occupe le haut, de `0` à `hauteur`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bloc {
    reglages: ReglagesCentre,
    largeur: i32,
    hauteur: i32,
    /// La largeur du bloc, en pixels.
    large: i32,
}

impl Bloc {
    /// Le rectangle `[x0, y0, x1, y1]` qu'il touche : toute la barre du haut.
    pub fn rect(&self) -> [i32; 4] {
        [0, 0, self.largeur, self.hauteur]
    }

    /// La distance signée du centre de pixel `(x, y)` au bord du bloc (négative
    /// dedans) : un rectangle à coins arrondis, du rayon d'une demi-hauteur.
    fn distance(&self, x: f32, y: f32) -> f32 {
        let (l, h) = (self.largeur as f32, self.hauteur as f32);
        let (demi_l, demi_h) = (self.large as f32 / 2.0, h / 2.0);
        let r = if self.reglages.arrondi { demi_h.min(demi_l) } else { 0.0 };
        let (qx, qy) = ((x - l / 2.0).abs() - (demi_l - r), (y - h / 2.0).abs() - (demi_h - r));
        let dehors = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
        dehors + qx.max(qy).min(0.0) - r
    }

    /// Le pixel `(px, py)` en BGRA PRÉMULTIPLIÉ : le bloc (lissé d'un pixel au
    /// bord), puis le fondu coloré hors de lui, puis le noir par-dessus.
    pub fn pixel(&self, px: i32, py: i32) -> [u8; 4] {
        self.pixel_texture(px, py, [0.0; 3])
    }

    /// `pixel`, la couleur du bloc décalée de `ecart` (RGB, en 0..=255, voir
    /// `peindre`) : la texture de l'interface, que le shader ajoute aux fonds.
    fn pixel_texture(&self, px: i32, py: i32, ecart: [f32; 3]) -> [u8; 4] {
        let (couleur, noire) = self.couches(px as f32 + 0.5, py as f32 + 0.5);
        let alpha = noire + couleur * (1.0 - noire);
        let part = couleur * (1.0 - noire);
        let [r, g, b] = self.reglages.couleur;
        let canal = |c: u8, e: f32| ((c as f32 + e).clamp(0.0, 255.0) * part).round().clamp(0.0, alpha * 255.0) as u8;
        [canal(b, ecart[2]), canal(g, ecart[1]), canal(r, ecart[0]), (alpha * 255.0).round().clamp(0.0, 255.0) as u8]
    }

    /// Au point `(x, y)` (fenêtre) : l'opacité de la couleur (le bloc, lissé
    /// d'un pixel au bord, et son fondu coloré) et celle du fondu noir.
    fn couches(&self, x: f32, y: f32) -> (f32, f32) {
        let r = &self.reglages;
        let d = self.distance(x, y);
        let couvert = (0.5 - d).clamp(0.0, 1.0);
        let dehors = 1.0 - couvert;
        let a_gauche = x < self.largeur as f32 / 2.0;
        let espace = (self.largeur - self.large) as f32 / 2.0;
        let opacite = 1.0 - r.transparence;
        // Le fondu coloré prolonge le bloc : à son bord, à pleine force, il a
        // son opacité — pas de marche entre les deux.
        let bloc = opacite * couvert;
        let coloree = opacite * r.coloree.alpha(d.max(0.0), espace, a_gauche) * dehors;
        let noire = r.noire.alpha(d.max(0.0), espace, a_gauche) * dehors;
        ((bloc + coloree).min(1.0), noire)
    }

    /// ⭐ **L'OMBRE COLORÉE DE LA BARRE DU HAUT PREND LE CENTRE** — DEMANDE
    /// EXPLICITE (2026-10-31) : la nouvelle couleur et ses fondus doivent passer
    /// dans l'ombre de couleur de la barre du haut — « oui [le fondu noir] sur la
    /// barre du haut, non sur l'ombrage de la barre du haut ».
    ///
    /// Le profil de la barre du haut (`N` segments sur toute sa largeur, voir
    /// `gfx::releve_barres::profil`) recomposé avec le bloc et son fondu COLORÉ,
    /// lus à mi-hauteur au milieu de chaque segment — SANS le fondu noir.
    pub fn teinter<const N: usize>(&self, profil: &[[u8; 3]; N]) -> [[u8; 3]; N] {
        let y = self.hauteur as f32 / 2.0;
        std::array::from_fn(|k| {
            let x = (k as f32 + 0.5) / N as f32 * self.largeur as f32;
            let (a, _) = self.couches(x, y);
            std::array::from_fn(|c| (profil[k][c] as f32 * (1.0 - a) + self.reglages.couleur[c] as f32 * a).round() as u8)
        })
    }

    /// Le peint dans `pixels` (BGRA prémultiplié, fenêtre de `taille`), en
    /// remplaçant ce qui y était — à poser AVANT les ombres.
    ///
    /// ⛔ **LA TEXTURE DISPARAISSAIT SOUS LE BLOC** — DÉFAUT SIGNALÉ (2026-10-31) :
    /// « je ne vois pas la texture sous la zone de couleur de la barre du haut ».
    /// Le shader ajoute la texture au FOND de la barre, puis pose le calque
    /// par-dessus : un bloc opaque la couvrait entière. `texture` : le calque de
    /// la texture sur `rect` (RGBA à alpha droit, `QuadInstance::
    /// new_calque_texture` — l'écart d'un canal vaut `(2·c/255 − 1)·a`), ajouté à
    /// la couleur du bloc comme le shader l'ajoute aux fonds.
    pub fn peindre(&self, pixels: &mut [u8], taille: (i32, i32), texture: Option<&[u8]>) {
        let [x0, y0, x1, y1] = self.rect();
        for y in y0..y1.min(taille.1) {
            for x in x0..x1.min(taille.0) {
                let t = ((y * self.largeur + x) * 4) as usize;
                let ecart = texture
                    .and_then(|tx| tx.get(t..t + 4))
                    .map_or([0.0; 3], |p| std::array::from_fn(|k| (2.0 * p[k] as f32 / 255.0 - 1.0) * p[3] as f32));
                let d = ((y * taille.0 + x) * 4) as usize;
                pixels[d..d + 4].copy_from_slice(&self.pixel_texture(x, y, ecart));
            }
        }
    }
}

fn borne(v: f32, max: f32, defaut: f32) -> f32 {
    if v.is_finite() { v.clamp(0.0, max) } else { defaut }
}

/// « RRGGBB » (avec ou sans `#`).
fn hex(texte: &str) -> Option<[u8; 3]> {
    let t = texte.trim().trim_start_matches('#');
    if t.len() != 6 || !t.is_ascii() {
        return None;
    }
    let octet = |i: usize| u8::from_str_radix(&t[i..i + 2], 16).ok();
    Some([octet(0)?, octet(2)?, octet(4)?])
}

static REGLAGES: std::sync::Mutex<ReglagesCentre> = std::sync::Mutex::new(ReglagesCentre::DEFAUT);

pub fn reglages() -> ReglagesCentre {
    *REGLAGES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Change les réglages (bornés), et rend ceux retenus — à enregistrer.
pub fn modifier(modifier: impl FnOnce(&mut ReglagesCentre)) -> ReglagesCentre {
    let mut reglages = REGLAGES.lock().unwrap_or_else(|e| e.into_inner());
    modifier(&mut reglages);
    *reglages = reglages.bornes();
    *reglages
}

/// Applique un réglage venu de la page : « actif|arrondi » en 0/1, « largeur »
/// et « transparence » en %, « noire-… » et « coloree-… » (largeur, force en %,
/// gauche, droite). `false` : réglage inconnu ou valeur illisible.
pub fn regler(r: &mut ReglagesCentre, reglage: &str, valeur: &str) -> bool {
    let Some(v) = valeur.parse::<f32>().ok().filter(|v| v.is_finite()) else { return false };
    let case = v >= 0.5;
    let (fondu, champ) = match reglage.split_once('-') {
        Some(("noire", champ)) => (Some(&mut r.noire), champ),
        Some(("coloree", champ)) => (Some(&mut r.coloree), champ),
        _ => (None, reglage),
    };
    match (fondu, champ) {
        (None, "actif") => r.actif = case,
        (None, "arrondi") => r.arrondi = case,
        (None, "largeur") => r.largeur = v,
        (None, "transparence") => r.transparence = v / 100.0,
        (Some(f), "largeur") => f.largeur = v,
        (Some(f), "force") => f.force = v / 100.0,
        (Some(f), "gauche") => f.gauche = case,
        (Some(f), "droite") => f.droite = case,
        _ => return false,
    }
    true
}

/// La couleur du bloc, du sélecteur (`apply_color_to_target`, cible `centre_haut`).
pub fn poser_couleur(rgb: [f32; 3]) -> ReglagesCentre {
    modifier(|r| r.couleur = rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actif() -> ReglagesCentre {
        ReglagesCentre {
            actif: true,
            largeur: 20.0,
            couleur: [200, 100, 50],
            transparence: 0.0,
            arrondi: false,
            noire: Fondu { largeur: 0.0, ..Fondu::DEFAUT },
            coloree: Fondu { largeur: 0.0, ..Fondu::DEFAUT },
        }
    }

    #[test]
    fn eteint_ou_nul_il_ne_peint_rien() {
        assert_eq!(ReglagesCentre::DEFAUT.bloc(1000, 40), None);
        assert_eq!(ReglagesCentre { largeur: 0.0, ..actif() }.bloc(1000, 40), None);
        assert_eq!(actif().bloc(1000, 0), None);
        assert!(actif().bloc(1000, 40).is_some());
    }

    #[test]
    fn le_bloc_est_centre_et_prend_sa_largeur_en_pourcent() {
        // 20 % de 1000 : de 400 à 600.
        let bloc = actif().bloc(1000, 40).expect("bloc");
        assert_eq!(bloc.pixel(500, 20), [50, 100, 200, 255], "dedans : la couleur en BGRA");
        assert_eq!(bloc.pixel(400, 20)[3], 255, "premier pixel du bloc");
        assert_eq!(bloc.pixel(399, 20)[3], 0, "juste avant le bloc");
        assert_eq!(bloc.pixel(599, 20)[3], 255, "dernier pixel du bloc");
        assert_eq!(bloc.pixel(600, 20)[3], 0, "juste après le bloc");
    }

    #[test]
    fn la_transparence_reste_premultipliee() {
        let bloc = ReglagesCentre { transparence: 0.5, ..actif() }.bloc(1000, 40).expect("bloc");
        let [b, g, r, a] = bloc.pixel(500, 20);
        assert_eq!(a, 128);
        assert_eq!([r, g, b], [100, 50, 25], "la couleur multipliée par l'opacité");
    }

    #[test]
    fn les_coins_arrondis_laissent_les_angles_vides() {
        let carre = actif().bloc(1000, 40).expect("bloc");
        let rond = ReglagesCentre { arrondi: true, ..actif() }.bloc(1000, 40).expect("bloc");
        assert_eq!(carre.pixel(400, 0)[3], 255, "coin carré : plein");
        assert_eq!(rond.pixel(400, 0)[3], 0, "coin arrondi : vide");
        assert!(rond.pixel(400, 20)[3] > 240, "au milieu de la hauteur, l'arc touche le bord (lissé)");
        assert_eq!(rond.pixel(500, 0)[3], 255, "le haut du bloc reste plein");
    }

    #[test]
    fn le_fondu_decroit_vers_le_bout_de_la_barre() {
        // Espace de 400 px de chaque côté ; fondu sur 50 % : 200 px.
        let r = ReglagesCentre { noire: Fondu { largeur: 50.0, force: 1.0, gauche: true, droite: true }, ..actif() };
        let bloc = r.bloc(1000, 40).expect("bloc");
        let alpha = |x: i32| bloc.pixel(x, 20)[3];
        assert!(alpha(399) > 240, "contre le bloc : presque plein ({})", alpha(399));
        assert!(alpha(399) > alpha(350) && alpha(350) > alpha(250), "décroissant");
        assert_eq!(alpha(199), 0, "au-delà de 200 px : rien");
        assert_eq!(bloc.pixel(399, 20)[..3], [0, 0, 0], "noir");
        assert_eq!(alpha(600), alpha(399), "symétrique");
    }

    #[test]
    fn le_fondu_colore_prend_la_couleur_du_bloc_et_va_jusqu_au_bout() {
        let r = ReglagesCentre { coloree: Fondu { largeur: 100.0, force: 1.0, gauche: true, droite: true }, ..actif() };
        let bloc = r.bloc(1000, 40).expect("bloc");
        let [b, g, rr, a] = bloc.pixel(300, 20);
        assert!(a > 0 && a < 255);
        // Prémultiplié : chaque canal est la couleur × alpha.
        let attendu = |c: u8| (c as f32 * a as f32 / 255.0).round() as i32;
        assert!((rr as i32 - attendu(200)).abs() <= 1 && (g as i32 - attendu(100)).abs() <= 1 && (b as i32 - attendu(50)).abs() <= 1);
        assert!(bloc.pixel(1, 20)[3] <= 1, "il s'éteint au bout de la barre");
    }

    #[test]
    fn chaque_cote_se_coche_a_part() {
        let r = ReglagesCentre { noire: Fondu { largeur: 100.0, force: 1.0, gauche: false, droite: true }, ..actif() };
        let bloc = r.bloc(1000, 40).expect("bloc");
        assert_eq!(bloc.pixel(390, 20)[3], 0, "gauche décochée");
        assert!(bloc.pixel(610, 20)[3] > 200, "droite cochée");
    }

    #[test]
    fn la_noire_passe_par_dessus_la_coloree() {
        let plein = Fondu { largeur: 100.0, force: 1.0, gauche: true, droite: true };
        let r = ReglagesCentre { noire: plein, coloree: plein, ..actif() };
        let [b, g, rr, a] = r.bloc(1000, 40).expect("bloc").pixel(399, 20);
        assert!(a > 250);
        assert!(rr < 10 && g < 10 && b < 10, "contre le bloc la noire à pleine force couvre la couleur : {rr},{g},{b}");
    }

    #[test]
    fn peindre_remplace_toute_la_barre_du_haut_et_rien_dessous() {
        let bloc = actif().bloc(100, 10).expect("bloc");
        let mut pixels = vec![9u8; 100 * 20 * 4];
        bloc.peindre(&mut pixels, (100, 20), None);
        assert_eq!(pixels[(5 * 100 + 50) * 4 + 3], 255, "le bloc");
        assert_eq!(pixels[(5 * 100 + 5) * 4 + 3], 0, "hors du bloc, effacé");
        assert_eq!(pixels[(15 * 100 + 50) * 4 + 3], 9, "sous la barre du haut : intact");
    }

    #[test]
    fn l_ombre_coloree_prend_le_bloc_et_son_fondu_colore_mais_pas_le_noir() {
        let gris = [[50u8, 50, 50]; 10];
        let fondu = Fondu { largeur: 100.0, force: 1.0, gauche: true, droite: true };
        // 20 % de 1000 : les segments 4 et 5 (centres à 450 et 550) sont dans le bloc.
        let seul = actif().bloc(1000, 40).expect("bloc").teinter(&gris);
        assert_eq!(seul[4], [200, 100, 50]);
        assert_eq!(seul[5], [200, 100, 50]);
        assert_eq!(seul[0], [50, 50, 50], "sans fondu coloré, la barre garde sa couleur hors du bloc");
        let avec = ReglagesCentre { coloree: fondu, ..actif() }.bloc(1000, 40).expect("bloc").teinter(&gris);
        assert!(avec[3][0] > avec[1][0] && avec[1][0] > 50, "le fondu coloré passe vers la couleur de la barre : {:?}", avec);
        let noire = ReglagesCentre { noire: fondu, ..actif() }.bloc(1000, 40).expect("bloc").teinter(&gris);
        assert_eq!(noire, seul, "le fondu noir ne touche pas l'ombre colorée");
        let transparent = ReglagesCentre { transparence: 0.5, ..actif() }.bloc(1000, 40).expect("bloc").teinter(&gris);
        assert_eq!(transparent[4], [125, 75, 50], "à demi transparent : à mi-chemin");
    }

    #[test]
    fn la_texture_se_voit_sur_le_bloc() {
        let bloc = actif().bloc(100, 10).expect("bloc");
        // Un calque de texture uniforme : +40 sur le rouge, −40 sur le vert.
        let mut texture = vec![0u8; 100 * 10 * 4];
        for p in texture.chunks_exact_mut(4) {
            p.copy_from_slice(&[255, 0, 128, 40]);
        }
        let mut pixels = vec![0u8; 100 * 10 * 4];
        bloc.peindre(&mut pixels, (100, 10), Some(&texture));
        let d = (5 * 100 + 50) * 4;
        // BGRA : bleu 50 (+0), vert 100 − 40, rouge 200 + 40.
        assert_eq!(pixels[d..d + 4], [50, 60, 240, 255]);
        let mut sans = vec![0u8; 100 * 10 * 4];
        bloc.peindre(&mut sans, (100, 10), None);
        assert_eq!(sans[d..d + 4], [50, 100, 200, 255], "sans texture : la couleur telle quelle");
        assert_eq!(pixels[(5 * 100 + 5) * 4 + 3], 0, "hors du bloc : rien");
    }

    #[test]
    fn les_reglages_font_l_aller_retour() {
        let r = ReglagesCentre {
            actif: true,
            largeur: 42.0,
            couleur: [1, 0xab, 0xff],
            transparence: 0.25,
            arrondi: false,
            noire: Fondu { largeur: 70.0, force: 0.3, gauche: false, droite: true },
            coloree: Fondu { largeur: 10.0, force: 0.9, gauche: true, droite: false },
        };
        assert_eq!(ReglagesCentre::charger(&r.serialiser()), r);
    }

    #[test]
    fn un_fichier_illisible_garde_les_defauts_et_les_bornes() {
        assert_eq!(ReglagesCentre::charger(""), ReglagesCentre::DEFAUT);
        assert_eq!(ReglagesCentre::charger("zz;;"), ReglagesCentre::DEFAUT);
        let r = ReglagesCentre::charger("1,250,nope,-3,1;500,7,1,1");
        assert_eq!((r.largeur, r.transparence, r.couleur), (100.0, 0.0, ReglagesCentre::DEFAUT.couleur));
        assert_eq!((r.noire.largeur, r.noire.force), (100.0, 1.0));
    }

    #[test]
    fn la_page_regle_chaque_champ() {
        let mut r = ReglagesCentre::DEFAUT;
        for (reglage, valeur) in [("actif", "1"), ("arrondi", "0"), ("largeur", "55"), ("transparence", "40"), ("noire-largeur", "80"), ("noire-force", "50"), ("noire-gauche", "0"), ("coloree-droite", "0")] {
            assert!(regler(&mut r, reglage, valeur), "{reglage}");
        }
        assert!(r.actif && !r.arrondi);
        assert_eq!((r.largeur, r.transparence), (55.0, 0.4));
        assert_eq!((r.noire.largeur, r.noire.force, r.noire.gauche), (80.0, 0.5, false));
        assert!(!r.coloree.droite && r.coloree.gauche);
        assert!(!regler(&mut r, "inconnu", "1"));
        assert!(!regler(&mut r, "noire-inconnu", "1"));
        assert!(!regler(&mut r, "largeur", "abc"));
    }
}
