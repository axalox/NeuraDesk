//! Le NUMÉRO porté par une pastille de bureaux virtuels (voir
//! `ui::topbar::desktop_dot_instance`, qui dessine la pastille elle-même).
//!
//! DEMANDE EXPLICITE (2026-09-23) : « je voudrais qu'il y ait un numéro sur les
//! pastilles de l'applet changement de bureau ». Les pastilles seules sont
//! muettes : rien n'y dit lequel des bureaux est le troisième.
//!
//! **CE MODULE PORTE LE CACHE, ET CE N'EST PAS UNE OPTIMISATION FACULTATIVE.**
//! `gfx::text::render_text` fabrique une texture GDI/D3D à CHAQUE appel, et la
//! boucle de rendu tourne à 60 im/s : sans cache, afficher « 1 » à « 20 »
//! régénérerait vingt textures par frame. C'est le garde-fou double de
//! `dock_instance::CachedText` (texte ET taille de police) qui est réutilisé
//! ici — le même que l'horloge et les quatre textes média, et pour la même
//! raison.
//!
//! Les deux barres qui dessinent des pastilles s'en servent : la PRIMAIRE
//! (`app::run`) et les SECONDAIRES (`dock_instance::DockInstance::render`).
//! Chacune tient son propre `DesktopDotLabels`, à SA taille de pastille (voir
//! `Dock::desktop_dot_size`, réglable PAR ÉCRAN) — sans quoi un écran
//! secondaire garderait les numéros du primaire, exactement le défaut que
//! `CachedText` documente pour les textes de la barre du haut.

use windows::Win32::Graphics::Direct3D11::ID3D11Device;

use crate::dock_instance::CachedText;
use crate::gfx::texture::Texture;
use crate::ui::{color, topbar};

/// Taille de pastille en dessous de laquelle plus AUCUN numéro n'est dessiné
/// (DÉCISION UTILISATEUR, 2026-09-23 : « masquer sous un seuil » plutôt que de
/// laisser un chiffre illisible, ou de le rétrécir jusqu'à la tache).
///
/// Une pastille de quelques pixels ne peut pas porter un chiffre lisible ; à
/// 12 px de côté il ne resterait que des débris d'encre. Le seuil est mis à
/// 14 px : la valeur par DÉFAUT est 18 (`topbar::DEFAULT_DESKTOP_DOT_SIZE`),
/// donc les numéros sont là tant que l'utilisateur ne rétrécit pas
/// délibérément ses pastilles.
///
/// **La pastille, elle, continue d'être dessinée** — c'est le NUMÉRO qui
/// disparaît, pas le point. Le clic et le survol d'un bureau n'y perdent rien.
pub const MIN_DOT_SIZE_WITH_LABEL: f32 = 14.0;

/// Hauteur d'ENCRE visée pour un chiffre, en fraction de la taille de la
/// pastille — la contrainte du cas à UN chiffre.
const ONE_DIGIT_INK_RATIO: f32 = 0.55;

/// Largeur d'ENCRE visée pour le numéro, en fraction de la taille de la
/// pastille — la contrainte du cas à DEUX chiffres, qui est celle qui mord.
const TWO_DIGIT_INK_RATIO: f32 = 0.72;

/// Hauteur d'encre d'un chiffre de « Segoe UI » gras, en fraction de sa taille
/// de police — **MESURÉE** par la sonde `mesure_les_numeros_des_pastilles`
/// (encre de « 1 » : 7 px à la police 12, 11 px à 21, 5 px à 9 — soit 0,52 à
/// 0,58 ; le maximum est retenu, c'est lui qui doit tenir).
const INK_HEIGHT_PER_FONT_PX: f32 = 0.583;

/// Largeur d'encre de DEUX chiffres de « Segoe UI » gras, en fraction de leur
/// taille de police — **MESURÉE** par la même sonde (8 px à la police 9, 9 px à
/// 12, 13 px à 16, soit 0,75 à 0,89). Le maximum est retenu : l'arrondi de GDI
/// domine aux petites tailles, et c'est le pire cas qui doit passer.
const TWO_DIGIT_INK_WIDTH_PER_FONT_PX: f32 = 0.889;

/// Corps du numéro à UN chiffre — **DÉDUIT DE LA MESURE, PAS CALCULÉ À L'ŒIL** :
/// c'est l'encre qui doit tenir dans la pastille, donc la taille de police se
/// divise par le rapport d'encre mesuré (`INK_HEIGHT_PER_FONT_PX`).
///
/// **LA CONTRAINTE N'EST PAS LA MÊME POUR UN ET DEUX CHIFFRES, ET C'EST CE QUI
/// JUSTIFIE DEUX PALIERS** (DÉCISION UTILISATEUR, 2026-09-23 : « la police
/// s'adapte ») : un chiffre seul est limité par la HAUTEUR (il est étroit), deux
/// chiffres par la LARGEUR (ils ont la même hauteur mais occupent le double).
/// Le dixième bureau est donc bien le moment où le corps doit baisser.
///
/// ⚠️ CE QUI A CHANGÉ APRÈS MESURE, et pourquoi c'est écrit ici : le premier
/// réglage était un 0,66 « au jugé » pour les deux cas, au motif qu'« une
/// pastille de 18 px, donc « 10 » tient mais serré ». **C'était faux** : à la
/// police 9, « 10 » n'occupe que 8 px de large — un tiers de la pastille. La
/// mesure a donné au chiffre SEUL le droit d'être plus grand qu'on ne le
/// croyait (il n'était limité par rien d'autre que sa hauteur).
const ONE_DIGIT_FONT_RATIO: f32 = ONE_DIGIT_INK_RATIO / INK_HEIGHT_PER_FONT_PX;

/// Corps du numéro à DEUX chiffres (« 10 » et au-delà) — même déduction, par la
/// largeur.
const TWO_DIGIT_FONT_RATIO: f32 = TWO_DIGIT_INK_RATIO / TWO_DIGIT_INK_WIDTH_PER_FONT_PX;

/// Police des numéros — la même famille que l'horloge et les autres textes de
/// la barre du haut (`ui::topbar::CLOCK_FONT_NAME` et consorts), mais une
/// constante À PART, comme eux : c'est ce qui permettra de la changer ici sans
/// toucher à l'horloge.
pub const LABEL_FONT_NAME: &str = "Segoe UI";

/// Le numéro du bureau `index` (0-based) — c'est-à-dire `index + 1`, parce
/// qu'un utilisateur compte ses bureaux à partir de UN.
pub fn label(index: usize) -> String {
    (index + 1).to_string()
}

/// Taille de police du numéro du bureau `index`, pour une pastille de
/// `dot_size` pixels de côté.
pub fn font_px(dot_size: f32, index: usize) -> i32 {
    let ratio = if index + 1 >= 10 { TWO_DIGIT_FONT_RATIO } else { ONE_DIGIT_FONT_RATIO };
    (dot_size * ratio).round().max(1.0) as i32
}

/// Remontée du numéro, en fraction de sa taille de police — correction du
/// centrage, voir `rect_for`.
///
/// **MESURÉ SUR UNE CAPTURE, PAS SUPPOSÉ** : sur la barre primaire (pastilles de
/// 36 px, police 34), l'encre du chiffre tombait à 11 px du bord haut de sa
/// pastille pour 8 px du bord bas — soit 1,5 px trop bas, c'est-à-dire 4 % de la
/// hauteur de la pastille, ce qui se voit. La sonde
/// `mesure_les_numeros_des_pastilles` donne la cause : l'encre d'un chiffre n'est
/// pas centrée dans la boîte que `render_text` renvoie, parce que
/// `GetTextExtentPoint32W` rend la hauteur de LIGNE — qui réserve un jambage
/// qu'un chiffre n'utilise jamais. L'écart mesuré à la sonde (0,5 px à la police
/// 17, 1,5 px à 30) vaut 0,03 à 0,05 fois la police ; la valeur retenue est le
/// haut de cette fourchette, celui du plus gros cas réel.
const INK_BASELINE_LIFT: f32 = 0.045;

/// Boîte à DESSINER du numéro : sa bitmap (`text_w` × `text_h`) centrée dans le
/// rectangle de la pastille, puis remontée de `INK_BASELINE_LIFT`.
///
/// `dot_rect` est le rectangle PRÉSENTÉ de la pastille — celui que
/// `ui::topbar::desktop_dot_instance` a réellement posé dans son
/// `QuadInstance::rect`, donc déjà grossi par le survol. Centrer dans la boîte
/// AU REPOS ferait sauter le numéro hors de sa pastille dès que la souris
/// l'approche ; c'est la même règle que `ui::topbar::desktop_dot_at`, qui teste
/// lui aussi ce qui est DESSINÉ.
///
/// Centrage sur la BOÎTE de la police (celle que `render_text` rend, voir sa
/// doc), comme l'horloge — et non sur l'encre du glyphe, qui demanderait les
/// métriques de la police.
///
/// ⚠️ **LE CENTRAGE VERTICAL EST CORRIGÉ, ET LA CORRECTION EST MESURÉE** : centrer
/// la BOÎTE laisse le chiffre 1,5 px trop bas sur une pastille de 36 px (voir
/// `INK_BASELINE_LIFT`, qui porte la mesure et sa cause). Ne pas retirer cette
/// correction au motif qu'un centrage « devrait » suffire : il ne suffit pas,
/// c'est constaté sur capture.
///
/// `font_px` : la taille de police du numéro, pour doser la correction — elle
/// est proportionnelle, l'écart étant celui du jambage de la police.
pub fn rect_for(dot_rect: [f32; 4], text_w: f32, text_h: f32, font_px: i32) -> [f32; 4] {
    [
        dot_rect[0] + (dot_rect[2] - text_w) * 0.5,
        dot_rect[1] + (dot_rect[3] - text_h) * 0.5 - font_px as f32 * INK_BASELINE_LIFT,
        text_w,
        text_h,
    ]
}

/// Couleur du numéro : celle des deux quasi-blanc/quasi-noir de
/// `ui::color::readable_text_rgb` qui offre le MEILLEUR contraste WCAG contre
/// ce que l'œil voit RÉELLEMENT sous le chiffre.
///
/// DÉCISION UTILISATEUR, 2026-09-23 : « automatique » — un chiffre d'une
/// couleur fixe est invisible quelque part, la pastille du bureau ACTIF étant
/// en couleur d'accentuation pleine (un blanc sur le jaune #FFB900 de
/// `WINDOWS_COLORS` ne se lit pas). C'est exactement le bug que
/// `readable_text_rgb` a été écrit pour corriger, et sa doc le dit déjà : on
/// réutilise son remède plutôt que d'en inventer un second.
///
/// **LE FOND À JUGER N'EST PAS LA COULEUR DE LA PASTILLE, MAIS CE QU'ON VOIT
/// D'ELLE** : une pastille inactive est translucide
/// (`ui::topbar::DESKTOP_DOT_INACTIVE_ALPHA`, 35 %), donc le chiffre y est posé
/// sur le MÉLANGE de la pastille et du fond de la barre. Juger la couleur
/// brute donnerait noir sur une pastille noire à 35 % — illisible — là où le
/// mélange est un gris moyen, qui appelle du blanc.
///
/// `backdrop_rgb` : le fond de la barre sous la pastille, DÉJÀ composé par
/// l'appelant (voir `ui::color::soften_tint`, qui est ce que la barre teinte
/// réellement). C'est une approximation assumée : le fond réel de la barre est
/// le bureau FLOUTÉ teinté (voir `gfx::backdrop`), donc il varie avec le papier
/// peint. L'erreur reste sans conséquence parce que `soften_tint` ramène 85 %
/// de la teinte vers le neutre du thème — c'est le THÈME qui décide, ici comme
/// partout dans cette barre.
pub fn color_for(active: bool, accent: [f32; 4], dark_mode: bool, backdrop_rgb: [f32; 3]) -> [f32; 3] {
    let dot = topbar::desktop_dot_color(active, accent, dark_mode);
    color::readable_text_rgb(color::composite_over(dot, backdrop_rgb))
}

/// Les numéros d'un écran, mémorisés — un `CachedText` par bureau, dans
/// l'ordre des bureaux.
///
/// Voir la doc de module : le cache n'est pas facultatif. `CachedText` porte le
/// garde-fou double (texte ET taille de police), donc :
///
/// * le numéro d'un bureau ne se régénère JAMAIS tant que ni son texte ni la
///   taille de sa pastille ne changent — soit jamais, en pratique ;
/// * glisser le curseur de taille de pastille d'un écran régénère les numéros
///   DE CET ÉCRAN, et seulement eux : c'est `font_px`, dérivé de `dot_size`,
///   qui invalide.
pub struct DesktopDotLabels {
    labels: Vec<CachedText>,
}

impl DesktopDotLabels {
    pub fn new() -> Self {
        Self { labels: Vec::new() }
    }

    /// Met à jour les `count` numéros pour une pastille de `dot_size` pixels,
    /// en ne régénérant que ce qui a réellement changé.
    ///
    /// `dot_size` sous `MIN_DOT_SIZE_WITH_LABEL` : la liste est VIDÉE, ce qui
    /// libère les textures — un numéro masqué ne coûte alors plus rien, et en
    /// agrandir de nouveau les pastilles les régénère.
    pub fn sync(
        &mut self,
        device: &ID3D11Device,
        count: usize,
        dot_size: f32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if dot_size < MIN_DOT_SIZE_WITH_LABEL {
            self.labels.clear();
            return Ok(());
        }
        // Un bureau FERMÉ emporte son numéro : sans ce retrait, les textures
        // des bureaux disparus resteraient allouées pour toujours.
        self.labels.truncate(count);
        for index in 0..count {
            if self.labels.len() <= index {
                self.labels.push(CachedText::new());
            }
            self.labels[index].sync(device, &label(index), font_px(dot_size, index), LABEL_FONT_NAME, true)?;
        }
        Ok(())
    }

    /// Bitmap du numéro du bureau `index`, `None` tant qu'il n'a pas été
    /// synchronisé (ou qu'il est masqué, voir `sync`).
    pub fn texture(&self, index: usize) -> Option<(&Texture, f32, f32)> {
        self.labels.get(index).and_then(|cached| cached.texture())
    }
}

impl Default for DesktopDotLabels {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Taille de pastille par DÉFAUT (`topbar::DEFAULT_DESKTOP_DOT_SIZE`) — les
    // tests qui suivent portent sur le cas réel de l'utilisateur, pas sur une
    // valeur inventée.
    const DEFAULT_DOT: f32 = 18.0;

    #[test]
    fn le_numero_est_le_rang_plus_un() {
        // Un utilisateur compte ses bureaux à partir de 1, pas de 0.
        assert_eq!(label(0), "1");
        assert_eq!(label(9), "10");
        assert_eq!(label(19), "20");
    }

    #[test]
    fn le_seuil_masque_les_numeros_sur_les_petites_pastilles() {
        assert!(DEFAULT_DOT >= MIN_DOT_SIZE_WITH_LABEL, "les numéros sont visibles par défaut");
        assert!(12.0 < MIN_DOT_SIZE_WITH_LABEL, "une pastille de 12 px n'en porte plus");
        assert!(8.0 < MIN_DOT_SIZE_WITH_LABEL);
        // Juste au seuil : visibles. C'est la seule valeur de la frontière, et
        // elle doit tomber du côté VISIBLE.
        assert!(MIN_DOT_SIZE_WITH_LABEL >= MIN_DOT_SIZE_WITH_LABEL);
    }

    #[test]
    fn la_police_retrecit_a_partir_du_dixieme_bureau() {
        let un_chiffre = font_px(DEFAULT_DOT, 0);
        let neuf = font_px(DEFAULT_DOT, 8);
        let dix = font_px(DEFAULT_DOT, 9);

        assert_eq!(un_chiffre, neuf, "de 1 à 9, même corps — le rang ne change rien");
        assert!(dix < neuf, "« 10 » doit tenir dans la même largeur que « 9 » : corps réduit");
        // Jamais zéro, jamais négatif : `render_text` recevrait une taille
        // impossible, et une police de 0 px ne dessine rien. 20 = la borne du
        // dépôt (`win32::desktops::MAX_DESKTOPS`), recopiée parce que cette
        // constante est privée à son module et qu'un test n'est pas une raison
        // d'élargir une visibilité.
        for index in 0..20 {
            for dot in [MIN_DOT_SIZE_WITH_LABEL, DEFAULT_DOT, 30.0, 60.0] {
                assert!(font_px(dot, index) >= 1, "taille de police toujours dessinable");
            }
        }
    }

    #[test]
    fn l_encre_tient_dans_la_pastille_a_toutes_les_tailles() {
        // Confronte la DÉDUCTION des deux ratios aux rapports d'encre MESURÉS
        // (`INK_HEIGHT_PER_FONT_PX`, `TWO_DIGIT_INK_WIDTH_PER_FONT_PX`), sur
        // toute la plage utile — seuil compris, c'est là que la marge est la
        // plus mince. C'est le calcul qui doit tenir ; c'est la sonde
        // `mesure_les_numeros_des_pastilles` qui le confirme sur les vrais
        // glyphes, et les deux doivent dire la même chose.
        for dot_size in [MIN_DOT_SIZE_WITH_LABEL, 16.0, 18.0, 24.0, 32.0, 64.0] {
            let un_chiffre = font_px(dot_size, 0) as f32;
            let deux_chiffres = font_px(dot_size, 9) as f32;

            let hauteur_encre = un_chiffre * INK_HEIGHT_PER_FONT_PX;
            let largeur_encre = deux_chiffres * TWO_DIGIT_INK_WIDTH_PER_FONT_PX;
            assert!(hauteur_encre <= dot_size, "un chiffre tient en HAUTEUR dans {dot_size} px (encre {hauteur_encre})");
            assert!(largeur_encre <= dot_size, "deux chiffres tiennent en LARGEUR dans {dot_size} px (encre {largeur_encre})");
            assert!(deux_chiffres < un_chiffre, "et le dixième bureau est bien RÉDUIT dans {dot_size} px");
        }
    }

    #[test]
    fn le_numero_est_centre_dans_la_pastille_presentee() {
        // Pastille 18×18 en (100, 10), bitmap 8×14, police 17.
        let dot = [100.0, 10.0, 18.0, 18.0];
        let (w, h, font) = (8.0, 14.0, 17);
        let rect = rect_for(dot, w, h, font);

        // HORIZONTAL : centrage exact, sans correction — c'est ce qui fait
        // suivre l'agrandissement au survol sans formule à part.
        assert_eq!(rect[0] + rect[2] * 0.5, dot[0] + dot[2] * 0.5);
        assert_eq!(rect[2], w, "la boîte garde la taille de la bitmap");

        // VERTICAL : la BOÎTE est remontée, c'est voulu (voir
        // `INK_BASELINE_LIFT`)...
        let centre_boite = rect[1] + rect[3] * 0.5;
        let centre_pastille = dot[1] + dot[3] * 0.5;
        assert!(centre_boite < centre_pastille, "la boîte est remontée, pas centrée");

        // ...et c'est l'ENCRE qui tombe au centre. L'encre est basse de
        // `INK_BASELINE_LIFT` dans sa boîte (mesuré) : la remontée de la boîte
        // doit donc l'annuler exactement.
        let centre_encre = centre_boite + font as f32 * INK_BASELINE_LIFT;
        assert!((centre_encre - centre_pastille).abs() < 0.01, "l'encre est centrée, pas la boîte : {centre_encre} vs {centre_pastille}");
    }

    #[test]
    fn le_numero_s_oppose_a_ce_qu_on_voit_de_la_pastille() {
        // Barre SOMBRE (le neutre de `color::NEUTRAL_DARK`), thème sombre.
        let barre_sombre = [0.09, 0.09, 0.10];
        let accent_fonce = [0.05, 0.20, 0.60, 1.0];
        let accent_jaune = [1.0, 0.725, 0.0, 1.0]; // #FFB900, `WINDOWS_COLORS`

        // Bureau ACTIF sur une accentuation FONCÉE : numéro clair.
        let sur_fonce = color_for(true, accent_fonce, true, barre_sombre);
        assert!(sur_fonce[0] > 0.5, "clair sur accent foncé, reçu {sur_fonce:?}");

        // Bureau ACTIF sur l'accentuation JAUNE : le cas que la couleur fixe
        // ratait — le numéro doit passer SOMBRE.
        let sur_jaune = color_for(true, accent_jaune, true, barre_sombre);
        assert!(sur_jaune[0] < 0.5, "sombre sur accent jaune, reçu {sur_jaune:?}");

        // Et le numéro se lit vraiment : contraste WCAG au-dessus du minimum
        // AA pour du texte normal (4.5:1) dans les deux cas.
        for (vu, choisi) in [(accent_fonce, sur_fonce), (accent_jaune, sur_jaune)] {
            let vu = color::composite_over(vu, barre_sombre);
            assert!(color::contrast_ratio(vu, choisi) >= 4.5, "contraste AA respecté : {vu:?} / {choisi:?}");
        }
    }

    #[test]
    fn une_pastille_inactive_translucide_se_juge_sur_son_melange() {
        // Barre CLAIRE, thème clair : la pastille inactive est NOIRE à 35 %
        // (`DESKTOP_DOT_INACTIVE_ALPHA`). Jugée à sa couleur BRUTE, elle
        // demanderait un numéro blanc — illisible sur ce qui est en réalité un
        // gris moyen. Jugée sur son MÉLANGE, elle demande un numéro sombre.
        let barre_claire = [0.94, 0.94, 0.95];
        let choisi = color_for(false, [0.1, 0.2, 0.3, 1.0], false, barre_claire);
        let vu = color::composite_over(topbar::desktop_dot_color(false, [0.1, 0.2, 0.3, 1.0], false), barre_claire);
        assert!(vu[0] > 0.5, "le mélange est bien CLAIR malgré une pastille noire : {vu:?}");
        assert!(choisi[0] < 0.5, "donc le numéro est sombre, reçu {choisi:?}");
        assert!(color::contrast_ratio(vu, choisi) >= 4.5, "contraste AA respecté");
    }

    /// SONDE — imprime, pour chaque numéro et chaque taille de pastille
    /// utile, la BOÎTE rendue par `render_text` et l'ENCRE réellement dessinée
    /// dedans. Deux mesures que le calcul ne peut pas donner :
    ///
    /// * `ONE_DIGIT_FONT_RATIO`/`TWO_DIGIT_FONT_RATIO` sont réglés par la
    ///   LARGEUR mesurée, pour que le numéro tienne dans la pastille avec sa
    ///   marge — « ça devrait tenir » n'est pas une mesure ;
    /// * le centrage se fait sur la BOÎTE (la ligne de la police), or l'encre
    ///   d'un chiffre n'y est pas centrée : elle n'a pas de jambage. L'écart
    ///   imprimé ici est celui qu'il faudrait corriger si le numéro paraît
    ///   trop haut — `rect_for` ne le corrige pas.
    ///
    /// Lancer : `cargo test --bins mesure_les_numeros_des_pastilles -- --ignored --nocapture`
    #[test]
    #[ignore = "sonde de mesure : imprime des dimensions, n'affirme rien"]
    fn mesure_les_numeros_des_pastilles() {
        for dot_size in [MIN_DOT_SIZE_WITH_LABEL, 16.0, 18.0, 24.0, 32.0] {
            println!("--- pastille {dot_size} px ---");
            for index in [0usize, 8, 9, 10, 19] {
                let texte = label(index);
                let px = font_px(dot_size, index);
                match crate::gfx::text::rendre_rgba(&texte, px, LABEL_FONT_NAME, true) {
                    Ok((largeur, hauteur, rgba)) => {
                        let Some((x0, y0, x1, y1)) = crate::gfx::text::boite_de_l_encre(&rgba, largeur, hauteur) else {
                            println!("  « {texte} » police {px} px → AUCUNE ENCRE");
                            continue;
                        };
                        let (encre_l, encre_h) = ((x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32);
                        // Le verdict qui compte : l'encre tient-elle dans la
                        // pastille ? C'est ce que `l_encre_tient_dans_la_pastille_
                        // a_toutes_les_tailles` calcule, ici mesuré sur le glyphe.
                        let verdict = if encre_l <= dot_size && encre_h <= dot_size { "TIENT" } else { "DÉBORDE" };
                        println!(
                            "  « {texte} » police {px} px → boîte {largeur}×{hauteur}, encre {encre_l}×{encre_h} en ({x0},{y0}) \
                             = {:.0}% largeur / {:.0}% hauteur → {verdict}",
                            encre_l / dot_size * 100.0,
                            encre_h / dot_size * 100.0,
                        );
                    }
                    Err(e) => println!("  « {texte} » police {px} px → ÉCHEC de rendu : {e}"),
                }
            }
        }
    }
}
