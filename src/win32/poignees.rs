//! **La géométrie d'une zone AJUSTABLE** — les huit poignées de
//! redimensionnement, le déplacement, et le placement d'une barre de boutons.
//!
//! Tout est **PUR** : aucun appel Win32, aucune fenêtre, aucun `thread_local`.
//! C'est ce qui rend le contrat entier éprouvable en `cargo test`, sans écran —
//! et c'est délibéré, parce que le chemin réel (une fenêtre plein écran pilotée à
//! la souris) n'est pas testable autrement.
//!
//! ⛔ **CE MODULE N'EST PAS UN SECOND TEST DE PROXIMITÉ.** Le dépôt en compte
//! déjà trois (`cef_couleurs`, `cef_caracteres`, `cef_visionneuse`), et ceux-là
//! répondent à une AUTRE question : « sur quel BORD de cette fenêtre est ce
//! point ? », pour `WM_NCHITTEST`. Ici la question est « ce point tombe-t-il sur
//! une POIGNÉE de cette zone, **et y a-t-il une zone du tout** ? » — la seconde
//! moitié n'existe pas là-bas.
//!
//! ⛔ **ET SURTOUT, LES TESTS DE BORD SONT « EXTÉRIEUR-INCLUSIFS ».** Dans
//! `cef_couleurs::code_pour_point`, `point.x - g < bande` est vrai pour TOUT
//! point à gauche du rectangle — sans conséquence pour un `WM_NCHITTEST`, qui
//! n'est interrogé que sur des points DE la fenêtre, mais **ruineux ici** : un
//! clic à 800 px à droite d'une zone posée répondrait « poignée droite », et
//! **plus aucune nouvelle sélection ne serait possible sur cette ligne**.
//! D'où `dans_la_zone` en premier, dans `poignee_sous`.

use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT,
};

/// Côté du carré DESSINÉ à chaque poignée, en pixels **logiques** (× DPI / 96).
///
/// ⚠️ C'est une aide VISUELLE : la prise réelle est plus large (voir
/// `BANDE_PRISE`). Un carré de 8 px qu'il faudrait viser au pixel près serait une
/// corvée — le dépôt a déjà tranché la question pour la visionneuse, où 6 px
/// physiques sont jugés « justes mais MINCES » et 12 px retenus.
pub(crate) const COTE_POIGNEE: i32 = 8;

/// Largeur de la bande de PRISE le long d'un bord, en pixels **logiques**.
///
/// ⚠️ **LA BANDE EST PLUS LARGE QUE LE CARRÉ DESSINÉ, ET C'EST VOULU** : toute la
/// longueur d'un bord est saisissable, pas seulement le milieu. Les carrés
/// servent d'indication, pas de cible — c'est ce que fait Windows pour ses
/// propres bordures. Le curseur (voir `capture_ecran::WM_SETCURSOR`) est ce qui
/// rend cette bande visible : sans lui, ce serait une affordance qu'on ne
/// découvre qu'en la heurtant.
pub(crate) const BANDE_PRISE: i32 = 8;

/// Côté minimal d'une zone, en pixels **logiques**.
///
/// ⚠️ **C'EST UN MINIMUM DE CÔTÉ, PAS UNE TAILLE PLAFOND** : voir
/// `redimensionner`, qui refuse d'AMENER un bord plus près que ça de l'opposé —
/// il ne fait jamais GRANDIR une zone qu'on n'a pas touchée.
pub(crate) const COTE_MINIMAL: i32 = 8;

/// `point` est-il DANS `zone` ? — bornes `right`/`bottom` **EXCLUSES**, comme
/// partout ailleurs dans le dépôt (voir `capture_ecran::rectangle_entre`).
///
/// ⛔ **C'EST LA GARDE QUI EMPÊCHE LE DÉSASTRE DÉCRIT DANS LA DOC DU MODULE.**
/// Sans elle, un point n'importe où sur le bureau serait « près d'un bord ».
pub(crate) fn dans_la_zone(zone: RECT, point: POINT) -> bool {
    point.x >= zone.left && point.x < zone.right && point.y >= zone.top && point.y < zone.bottom
}

/// Le carré dessiné à chaque poignée : les quatre coins, puis les quatre milieux
/// de bord.
///
/// ⚠️ Sur une zone plus petite que trois carrés, ils se chevauchent : c'est
/// **visuel seulement**. La prise, elle, applique la même réduction que
/// `poignee_sous`, donc les deux ne divergent jamais.
pub(crate) fn rects_poignees(zone: RECT, cote: i32) -> [(u32, RECT); 8] {
    let c = carre(zone, cote);
    let (g, h, d, b) = (zone.left, zone.top, zone.right, zone.bottom);
    let (mx, my) = ((g + d) / 2, (h + b) / 2);
    let demi = c / 2;
    let au = |x: i32, y: i32| RECT { left: x - demi, top: y - demi, right: x - demi + c, bottom: y - demi + c };
    [
        (HTTOPLEFT, au(g, h)),
        (HTTOP, au(mx, h)),
        (HTTOPRIGHT, au(d - 1, h)),
        (HTRIGHT, au(d - 1, my)),
        (HTBOTTOMRIGHT, au(d - 1, b - 1)),
        (HTBOTTOM, au(mx, b - 1)),
        (HTBOTTOMLEFT, au(g, b - 1)),
        (HTLEFT, au(g, my)),
    ]
}

/// Le côté RÉELLEMENT utilisable pour une zone donnée — voir `poignee_sous`.
///
/// ⛔ **LA PRISE NE PEUT PAS MANGER TOUTE LA ZONE** : avec une bande fixe, une
/// zone de quelques pixels n'aurait plus AUCUN intérieur, donc plus aucun
/// endroit pour la DÉPLACER. En se limitant au tiers, il reste toujours un
/// intérieur.
///
/// ⚠️ **`(l - 1) / 3`, ET PAS `l / 3`, ET PAS DE `.max(2)` DERRIÈRE** : sur une
/// zone de 4 px, `(l-1)/3` vaut 1 et laisse `x = 101, 102` libres ; `l/3` vaut
/// aussi 1 mais un `.max(2)` de confort ramenait la bande à 2 et **mangeait tout
/// l'intérieur** — exactement le piège qu'on veut éviter. Le plancher est donc
/// `1`, jamais plus.
fn carre(zone: RECT, cote: i32) -> i32 {
    let l = zone.right - zone.left;
    let h = zone.bottom - zone.top;
    cote.max(1).min((l - 1) / 3).min((h - 1) / 3).max(1)
}

/// La poignée sous `point`, ou `None` si le point n'est pas dans la zone.
pub(crate) fn poignee_sous(zone: RECT, point: POINT, bande: i32) -> Option<u32> {
    // ⛔ **D'ABORD DEDANS.** Voir la doc du module : sans cette ligne, tout le
    // bureau à gauche, à droite, au-dessus ou en dessous de la zone répondrait
    // « poignée », et on ne pourrait plus rien sélectionner ailleurs.
    if !dans_la_zone(zone, point) {
        return None;
    }
    let b = carre(zone, bande);
    let gauche = point.x - zone.left < b;
    let droite = zone.right - 1 - point.x < b;
    let haut = point.y - zone.top < b;
    let bas = zone.bottom - 1 - point.y < b;
    match (gauche, droite, haut, bas) {
        (true, _, true, _) => Some(HTTOPLEFT),
        (_, true, true, _) => Some(HTTOPRIGHT),
        (true, _, _, true) => Some(HTBOTTOMLEFT),
        (_, true, _, true) => Some(HTBOTTOMRIGHT),
        (true, _, _, _) => Some(HTLEFT),
        (_, true, _, _) => Some(HTRIGHT),
        (_, _, true, _) => Some(HTTOP),
        (_, _, _, true) => Some(HTBOTTOM),
        _ => None,
    }
}

/// La zone après avoir tiré `poignee` jusqu'à `curseur`.
///
/// **Seuls les bords que la poignée NOMME bougent** — tirer le bord gauche ne
/// touche pas au droit. `minimum` est un minimum de CÔTÉ : un bord ne peut pas
/// s'approcher de l'opposé à moins, donc **aucune inversion n'est possible**, et
/// une zone déjà plus petite que `minimum` n'est jamais AGRANDIE par surprise
/// (elle est seulement bornée dans ses déplacements).
///
/// Le résultat est borné à `ecran` — l'écran VIRTUEL entier, en coordonnées
/// client, pas un moniteur.
pub(crate) fn redimensionner(origine: RECT, poignee: u32, curseur: POINT, ecran: (i32, i32), minimum: i32) -> RECT {
    let mut g = origine.left;
    let mut h = origine.top;
    let mut d = origine.right;
    let mut b = origine.bottom;
    let (x, y) = (curseur.x, curseur.y);
    // ⛔ **LE MINIMUM SE LIT CONTRE LE BORD OPPOSÉ, ET JAMAIS PLUS QUE LA TAILLE
    // DÉJÀ LÀ.** Les deux moitiés comptent :
    // - contre le bord opposé ⇒ un bord ne peut pas DÉPASSER l'autre, quel que
    //   soit le sens du glisser, donc aucune inversion ;
    // - jamais plus que la taille actuelle ⇒ une zone DÉJÀ plus petite que le
    //   minimum n'est pas AGRANDIE par un frémissement de la souris. C'est le
    //   piège du garde-fou qui rend visible ce qu'il prétend border : une zone de
    //   3 px qui bondit à 8 dès qu'on l'effleure.
    let mini_l = minimum.min(d - g).max(1);
    let mini_h = minimum.min(b - h).max(1);
    if matches!(poignee, HTLEFT | HTTOPLEFT | HTBOTTOMLEFT) {
        g = x.min(d - mini_l);
    }
    if matches!(poignee, HTRIGHT | HTTOPRIGHT | HTBOTTOMRIGHT) {
        d = x.max(g + mini_l);
    }
    if matches!(poignee, HTTOP | HTTOPLEFT | HTTOPRIGHT) {
        h = y.min(b - mini_h);
    }
    if matches!(poignee, HTBOTTOM | HTBOTTOMLEFT | HTBOTTOMRIGHT) {
        b = y.max(h + mini_h);
    }
    // ⚠️ Le bornage à l'écran est MONOTONE : appliqué aux quatre valeurs, il ne
    // peut pas inverser ce que les gardes ci-dessus viennent d'établir.
    RECT {
        left: g.clamp(0, ecran.0),
        top: h.clamp(0, ecran.1),
        right: d.clamp(0, ecran.0),
        bottom: b.clamp(0, ecran.1),
    }
}

/// La zone translatée de `delta` — **TAILLE PRÉSERVÉE**, bornée à l'écran.
///
/// ⛔ **C'est un `clamp` SUR L'ORIGINE, pas sur chaque bord** : borner les bords
/// indépendamment RÉTRÉCIRAIT la zone contre un bord de l'écran, au lieu de
/// l'arrêter. La taille est calculée une fois et reportée telle quelle.
pub(crate) fn deplacer(origine: RECT, delta: (i32, i32), ecran: (i32, i32)) -> RECT {
    let l = origine.right - origine.left;
    let h = origine.bottom - origine.top;
    let g = (origine.left + delta.0).clamp(0, (ecran.0 - l).max(0));
    let t = (origine.top + delta.1).clamp(0, (ecran.1 - h).max(0));
    RECT { left: g, top: t, right: g + l, bottom: t + h }
}

/// Hauteur de la barre, en pixels **logiques**.
pub(crate) const BARRE_HAUTEUR: i32 = 28;
/// Marge entre la barre et la zone, et entre la barre et le bord de l'écran.
pub(crate) const BARRE_MARGE: i32 = 6;
/// Largeur de la case « L × H », en pixels logiques.
pub(crate) const ETIQUETTE_LARGEUR: i32 = 110;
/// Largeur d'un bouton, en pixels logiques.
pub(crate) const BOUTON_LARGEUR: i32 = 90;

/// **Où poser la barre `[ L × H ] [ Valider ] [ Texte ] [ Annuler ]`, et où sont ses
/// boutons.**
///
/// ⚠️ **SEULS LES TROIS BOUTONS SE PRENNENT** — `fond` et `etiquette` sont
/// décoratifs, et **laissent passer le glisser** : quand la barre se retrouve
/// DEDANS la zone, on doit encore pouvoir la déplacer en tirant sur sa case de
/// taille. (C'est aussi ce qui rend l'ordre des tests indifférent.)
pub(crate) struct Barre {
    pub fond: RECT,
    pub etiquette: RECT,
    pub valider: RECT,
    /// « Texte » : la zone part à la reconnaissance de caractères, et son texte
    /// au presse-papiers (voir `win32::texte_capture`).
    pub texte: RECT,
    pub annuler: RECT,
}

/// Place la barre : **sous** la zone si elle y tient, sinon **au-dessus**, sinon
/// **dedans par le bas**.
///
/// ⛔ **LES DEUX AXES SONT DES `clamp`, ET C'EST CE QUI REND « JAMAIS HORS
/// ÉCRAN » VRAI PAR CONSTRUCTION** — une échelle de trois cas ne le prouve pas,
/// elle énumère seulement ceux auxquels on a pensé.
///
/// ⚠️ **ANCRÉE AU BORD GAUCHE DE LA ZONE**, jamais centrée : centrée, elle
/// recouvrirait la poignée du bas-droit ou celle du bord droit, c'est-à-dire
/// précisément ce qu'on vient de dessiner pour être saisi.
///
/// ⚠️ Le cas réellement délicat n'est PAS la zone plein écran (qu'on ne peut de
/// toute façon pas déplacer) mais **une zone qui touche le bord bas sans être
/// pleine hauteur** : elle a une vraie intention de déplacement, « dessous » ne
/// tient pas, et c'est l'essai « au-dessus » qui la sauve. D'où cet ordre.
pub(crate) fn barre(zone: RECT, ecran: (i32, i32), dpi: i32) -> Barre {
    let e = |v: i32| v * dpi / 96;
    let (hb, marge, le, lb) = (e(BARRE_HAUTEUR), e(BARRE_MARGE), e(ETIQUETTE_LARGEUR), e(BOUTON_LARGEUR));
    let largeur = le + 3 * lb;
    let y = if zone.bottom + marge + hb + marge <= ecran.1 {
        zone.bottom + marge
    } else if zone.top - marge - hb >= 0 {
        zone.top - marge - hb
    } else {
        (zone.bottom - hb - marge).clamp(0, (ecran.1 - hb).max(0))
    };
    let x = zone.left.clamp(0, (ecran.0 - largeur).max(0));
    let fond = RECT { left: x, top: y, right: x + largeur, bottom: y + hb };
    Barre {
        etiquette: RECT { left: x, top: y, right: x + le, bottom: y + hb },
        valider: RECT { left: x + le, top: y, right: x + le + lb, bottom: y + hb },
        texte: RECT { left: x + le + lb, top: y, right: x + le + 2 * lb, bottom: y + hb },
        annuler: RECT { left: x + le + 2 * lb, top: y, right: x + largeur, bottom: y + hb },
        fond,
    }
}

/// `point` est-il dans `r` ? Bornes excluses.
pub(crate) fn dans(point: POINT, r: RECT) -> bool {
    point.x >= r.left && point.x < r.right && point.y >= r.top && point.y < r.bottom
}

#[cfg(test)]
mod tests {
    use super::*;

    const ECRAN: (i32, i32) = (1920, 1080);
    const ZONE: RECT = RECT { left: 400, top: 300, right: 800, bottom: 600 };

    fn p(x: i32, y: i32) -> POINT {
        POINT { x, y }
    }

    /// ⛔ **LE DÉFAUT QUI AURAIT TOUT CASSÉ** : un point HORS de la zone ne doit
    /// JAMAIS répondre une poignée. Les tests de bord du dépôt sont
    /// « extérieur-inclusifs » — `point.x - g < bande` est vrai pour tout point à
    /// gauche — ce qui est sans effet pour un `WM_NCHITTEST` mais interdit
    /// **toute nouvelle sélection** sur cette ligne si on le réutilise tel quel.
    #[test]
    fn un_point_hors_de_la_zone_n_est_jamais_une_poignee() {
        assert_eq!(poignee_sous(ZONE, p(0, 0), BANDE_PRISE), None, "loin en haut à gauche");
        assert_eq!(poignee_sous(ZONE, p(1600, 450), BANDE_PRISE), None, "800 px À DROITE : le cas fatal");
        assert_eq!(poignee_sous(ZONE, p(600, 900), BANDE_PRISE), None, "loin en dessous");
        assert_eq!(poignee_sous(ZONE, p(100, 450), BANDE_PRISE), None, "à gauche, à la hauteur du milieu");
    }

    /// Les huit poignées tombent juste, et l'intérieur ne prend rien.
    #[test]
    fn les_huit_poignees_tombent_juste() {
        let au = |x: i32, y: i32| poignee_sous(ZONE, p(x, y), BANDE_PRISE);
        assert_eq!(au(400, 300), Some(HTTOPLEFT));
        assert_eq!(au(799, 300), Some(HTTOPRIGHT));
        assert_eq!(au(400, 599), Some(HTBOTTOMLEFT));
        assert_eq!(au(799, 599), Some(HTBOTTOMRIGHT));
        assert_eq!(au(600, 300), Some(HTTOP), "le milieu du bord haut");
        assert_eq!(au(600, 599), Some(HTBOTTOM));
        assert_eq!(au(400, 450), Some(HTLEFT));
        assert_eq!(au(799, 450), Some(HTRIGHT));
        assert_eq!(au(600, 450), None, "le centre est LIBRE : c'est le déplacement");
    }

    /// ⛔ **UNE ZONE MINUSCULE GARDE UN INTÉRIEUR.** Sans la réduction de la
    /// bande, une zone de 4 px n'aurait plus aucun point intérieur libre, donc
    /// plus aucun endroit pour la DÉPLACER — et une zone qu'on ne peut plus
    /// déplacer est un piège.
    #[test]
    fn une_zone_minuscule_garde_un_interieur_deplacable() {
        let minuscule = RECT { left: 100, top: 100, right: 104, bottom: 104 };
        assert_eq!(poignee_sous(minuscule, p(102, 102), BANDE_PRISE), None, "le centre reste libre");
    }

    /// Tirer un bord ne bouge QUE ce bord.
    #[test]
    fn tirer_un_bord_ne_bouge_que_lui() {
        let r = redimensionner(ZONE, HTLEFT, p(350, 999), ECRAN, COTE_MINIMAL);
        assert_eq!((r.left, r.top, r.right, r.bottom), (350, 300, 800, 600));
        let r = redimensionner(ZONE, HTBOTTOM, p(0, 700), ECRAN, COTE_MINIMAL);
        assert_eq!((r.left, r.top, r.right, r.bottom), (400, 300, 800, 700));
    }

    /// ⛔ **AUCUNE INVERSION, MÊME EN TRAVERSANT.** Tirer le bord gauche
    /// jusqu'à 2000 ne doit pas rendre un rectangle à l'envers — le résultat
    /// serait un PNG de largeur négative.
    #[test]
    fn un_bord_ne_traverse_jamais_l_oppose() {
        let r = redimensionner(ZONE, HTLEFT, p(2000, 450), ECRAN, COTE_MINIMAL);
        assert!(r.left < r.right, "{r:?}");
        assert_eq!(r.right - r.left, COTE_MINIMAL, "il s'arrête au minimum");
        let r = redimensionner(ZONE, HTBOTTOMRIGHT, p(0, 0), ECRAN, COTE_MINIMAL);
        assert!(r.right > r.left && r.bottom > r.top, "{r:?}");
    }

    /// ⚠️ **UN MINIMUM NE FAIT PAS GRANDIR UNE ZONE QU'ON N'A PAS TOUCHÉE.** Une
    /// zone déjà plus petite que lui ne doit pas bondir à la taille minimale sous
    /// un frémissement de la souris — le garde-fou rendrait alors visible ce
    /// qu'il prétend border.
    #[test]
    fn le_minimum_ne_fait_pas_grandir_une_zone_deja_petite() {
        let petite = RECT { left: 100, top: 100, right: 103, bottom: 103 };
        let r = redimensionner(petite, HTTOPLEFT, p(102, 102), ECRAN, COTE_MINIMAL);
        assert_eq!((r.left, r.top), (100, 100), "le coin ne BONDIT pas au minimum");
        assert_eq!((r.right - r.left, r.bottom - r.top), (3, 3), "et la zone n'a pas grandi");
        // Vers l'EXTÉRIEUR, en revanche, elle suit le curseur normalement.
        let r = redimensionner(petite, HTTOPLEFT, p(50, 50), ECRAN, COTE_MINIMAL);
        assert_eq!((r.left, r.top), (50, 50));
    }

    /// Le redimensionnement ne sort jamais de l'écran.
    #[test]
    fn le_redimensionnement_reste_dans_l_ecran() {
        let r = redimensionner(ZONE, HTTOPLEFT, p(-500, -500), ECRAN, COTE_MINIMAL);
        assert_eq!((r.left, r.top), (0, 0));
        let r = redimensionner(ZONE, HTBOTTOMRIGHT, p(9999, 9999), ECRAN, COTE_MINIMAL);
        assert_eq!((r.right, r.bottom), (1920, 1080));
    }

    /// ⛔ **LE DÉPLACEMENT PRÉSERVE LA TAILLE**, y compris plaqué contre un bord.
    /// Borner les quatre bords indépendamment RÉTRÉCIRAIT la zone.
    #[test]
    fn le_deplacement_preserve_la_taille_meme_contre_un_bord() {
        let (l, h) = (ZONE.right - ZONE.left, ZONE.bottom - ZONE.top);
        for delta in [(0, 0), (-9999, 0), (9999, 0), (0, -9999), (0, 9999), (-9999, -9999), (9999, 9999)] {
            let r = deplacer(ZONE, delta, ECRAN);
            assert_eq!((r.right - r.left, r.bottom - r.top), (l, h), "delta {delta:?}");
            assert!(r.left >= 0 && r.top >= 0 && r.right <= ECRAN.0 && r.bottom <= ECRAN.1, "delta {delta:?} : {r:?}");
        }
    }

    /// ⛔ **LA BARRE RESTE ENTIÈREMENT DANS L'ÉCRAN, ET LES DEUX BOUTONS NE SE
    /// CHEVAUCHENT PAS** — sur toute la matrice de zones plausible : chaque coin,
    /// chaque bord, pleine largeur, plein écran, et minuscule.
    #[test]
    fn la_barre_ne_sort_jamais_de_l_ecran() {
        let zones = [
            RECT { left: 0, top: 0, right: 200, bottom: 100 },
            RECT { left: 1720, top: 0, right: 1920, bottom: 100 },
            RECT { left: 0, top: 980, right: 200, bottom: 1080 },
            RECT { left: 1720, top: 980, right: 1920, bottom: 1080 },
            RECT { left: 0, top: 0, right: 1920, bottom: 100 },
            RECT { left: 0, top: 980, right: 1920, bottom: 1080 },
            RECT { left: 0, top: 0, right: 100, bottom: 1080 },
            RECT { left: 1820, top: 0, right: 1920, bottom: 1080 },
            RECT { left: 0, top: 0, right: 1920, bottom: 1080 },
            RECT { left: 500, top: 500, right: 502, bottom: 502 },
        ];
        for z in zones {
            for dpi in [96, 144, 192] {
                let b = barre(z, ECRAN, dpi);
                for (nom, r) in [("fond", b.fond), ("valider", b.valider), ("texte", b.texte), ("annuler", b.annuler)] {
                    assert!(
                        r.left >= 0 && r.top >= 0 && r.right <= ECRAN.0 && r.bottom <= ECRAN.1,
                        "{nom} sort de l'écran — zone {z:?}, dpi {dpi} : {r:?}"
                    );
                }
                assert!(
                    b.valider.right == b.texte.left && b.texte.right == b.annuler.left,
                    "les trois boutons se touchent sans se chevaucher"
                );
                assert!(b.annuler.right <= b.fond.right, "les boutons tiennent dans la barre");
            }
        }
    }

    /// ⚠️ **LE CAS QUI A FAILLI PASSER** : une zone qui touche le bord BAS sans
    /// être pleine hauteur. Elle a une vraie intention de déplacement, « dessous »
    /// ne tient pas — c'est l'essai « au-dessus » qui la sauve, et il doit passer
    /// AVANT le repli « dedans ».
    #[test]
    fn une_zone_qui_touche_le_bas_met_la_barre_au_dessus() {
        let z = RECT { left: 300, top: 700, right: 900, bottom: 1080 };
        let b = barre(z, ECRAN, 96);
        assert!(b.fond.bottom <= z.top, "la barre est AU-DESSUS de la zone : {:?}", b.fond);
    }

    /// Les carrés dessinés tombent sur les bonnes poignées, et chacun reste
    /// saisissable à l'endroit où il est PEINT.
    #[test]
    fn les_carres_dessines_correspondent_a_leur_poignee() {
        for (code, carre) in rects_poignees(ZONE, COTE_POIGNEE) {
            let centre = p((carre.left + carre.right) / 2, (carre.top + carre.bottom) / 2);
            assert_eq!(poignee_sous(ZONE, centre, BANDE_PRISE), Some(code), "carré {code} en {carre:?}");
        }
    }
}
