//! ⭐ **DES PLACES D'ÉPINGLE RÉSERVÉES, POUR ÉPINGLER SANS RECHARGER.**
//!
//! DEMANDE (2026-10-10) : « lorsqu'un programme est ajouté dans une zone de la barre
//! de droite, qu'il n'y ait que cette zone qui se recharge, pas toute l'interface ».
//!
//! **POURQUOI ÉPINGLER RECHARGEAIT TOUT.** Les épingles du dock vivent à
//! `PINNED_ICON_BASE..`, et les applets juste après : `PINNED_ICON_BASE +
//! épingles + k`. `app::run` tient ces ~35 index en variables locales, calculées
//! UNE fois au lancement — une épingle de plus les décalait tous d'un cran, d'où le
//! redémarrage complet.
//!
//! **LE REMÈDE : DES PLACES LIBRES, MASQUÉES, ENTRE LES ÉPINGLES ET LES APPLETS.**
//! Les applets valent maintenant `PINNED_ICON_BASE + capacité + k` ; épingler
//! remplit la première place libre (on la démasque), et **aucun index ne bouge**
//! pendant que l'application tourne. Quand il n'y a plus de place, on retombe sur
//! l'ancien chemin (décaler d'un cran, recharger) — et le lancement suivant
//! regarnit la réserve.
//!
//! ⚠️ **AGRANDIR LA RÉSERVE DÉCALE LES ORDRES PERSISTÉS** — exactement comme
//! l'auraient fait `RESERVE` épingles ajoutées d'un coup : tout ce qui est cité dans
//! `[début, winpin_base[` avance de `décalage`, et rien au-delà (les remplisseurs
//! absorbent, voir `Dock::decaler_le_bloc_des_epingles`). C'est donc la même
//! mécanique que le chemin d'ajout qui tournait déjà, appliquée `décalage` fois.

/// Combien de places libres on garnit quand la réserve est vide.
pub const RESERVE: usize = 8;

/// **LA DÉCISION DU LANCEMENT** : la capacité retenue, et de combien il faut décaler
/// les ordres persistés pour la tenir (`0` = rien à réécrire).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reserve {
    pub capacite: usize,
    pub decalage: usize,
    /// `true` : la réserve ne tient pas sous `winpin_base` — tout ce qui est cité à
    /// partir de `debut` avance, épingles de la barre des tâches comprises, et
    /// `winpin_base` avec. C'est ce que fait déjà l'ajout d'une épingle quand aucun
    /// remplisseur n'est libre (`cef_dash::limite_du_decalage`).
    pub tout_decaler: bool,
}

/// **DÉCIDE LA CAPACITÉ, SANS RIEN ÉCRIRE** — fonction pure, pour être testée.
///
/// - `epingles` : le nombre d'épingles lues ;
/// - `sauvee` : la capacité pour laquelle les fichiers sont écrits (`None` = jamais
///   réservé : ils sont écrits pour `epingles`, comme avant ce module) ;
/// - `fin_des_applets` : la longueur d'`icons` après `Dock::new` À LA CAPACITÉ
///   COURANTE (épingles + applets, avant les remplisseurs) ;
/// - `cites` : tous les index cités par les quatre ordres primaires ;
/// - `limite` : `winpin_base` — `None` ⇒ on ne devine pas, on ne réserve rien.
///
/// ⛔ **ON NE RÉSERVE QUE S'IL Y A LA PLACE** : aucun index cité ne doit franchir
/// `limite` une fois décalé (il désignerait une épingle de la barre des tâches), et
/// les applets décalées doivent tenir sous elle.
pub fn decider(
    epingles: usize,
    sauvee: Option<usize>,
    fin_des_applets: usize,
    cites: impl IntoIterator<Item = usize>,
    debut: usize,
    limite: Option<usize>,
) -> Reserve {
    let courante = sauvee.unwrap_or(epingles).max(epingles);
    let garde = Reserve { capacite: courante, decalage: 0, tout_decaler: false };
    if courante > epingles {
        return garde;
    }
    let Some(limite) = limite else { return garde };
    let voulue = epingles + RESERVE;
    let decalage = voulue - courante;
    // ⭐ **SANS PLACE SOUS `winpin_base`, ON DÉCALE TOUT** (2026-10-18) — RÉGRESSION
    // SIGNALÉE : « quand j'ajoute un programme à la section applis toute la barre se
    // relance ». Refuser la réserve ici la refusait À JAMAIS : sans remplisseur
    // entre les applets et les épingles de la barre des tâches, `winpin_base` colle
    // aux applets à chaque lancement, et chaque ajout repassait par le rechargement.
    let deborde = fin_des_applets + decalage > limite
        || cites.into_iter().any(|i| i >= debut && i < limite && i + decalage >= limite);
    Reserve { capacite: voulue, decalage, tout_decaler: deborde }
}

/// **AVANCE DE `decalage` TOUT CE QUI EST CITÉ DANS `[debut, limite[`** — le décalage
/// que `decider` a validé. `limite = usize::MAX` pour un écran secondaire (il n'a pas
/// d'épingles de barre des tâches, voir `config::decaler_les_ordres_secondaires`).
pub fn decaler(ordres: [&mut Vec<usize>; 4], debut: usize, limite: usize, decalage: usize) {
    for ordre in ordres {
        for entree in ordre.iter_mut().filter(|e| **e >= debut && **e < limite) {
            *entree += decalage;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::PINNED_ICON_BASE;
    use crate::ui::dock::Dock;

    /// Le montage mesuré sur la machine le 2026-10-10 : 21 épingles, applets
    /// jusqu'à 56, `winpin_base` 92, et `icon_order.txt` ne cite rien entre 57 et 91.
    const EPINGLES: usize = 21;
    const LIMITE: usize = 92;

    fn fin_des_applets(capacite: usize) -> usize {
        Dock::new([0.0; 4], capacite).icons.len()
    }

    #[test]
    fn le_premier_lancement_garnit_la_reserve() {
        let r = decider(EPINGLES, None, fin_des_applets(EPINGLES), [24, 40, 56, 92, 141], PINNED_ICON_BASE + EPINGLES, Some(LIMITE));
        assert_eq!(r, Reserve { capacite: EPINGLES + RESERVE, decalage: RESERVE, tout_decaler: false });
    }

    #[test]
    fn une_place_libre_ne_reecrit_rien() {
        let r = decider(EPINGLES, Some(EPINGLES + 3), 0, [], 0, Some(LIMITE));
        assert_eq!(r, Reserve { capacite: EPINGLES + 3, decalage: 0, tout_decaler: false });
    }

    #[test]
    fn une_reserve_pleine_se_regarnit() {
        let cap = EPINGLES + RESERVE;
        let r = decider(cap, Some(cap), fin_des_applets(cap), [56 + RESERVE], PINNED_ICON_BASE + cap, Some(LIMITE));
        assert_eq!(r, Reserve { capacite: cap + RESERVE, decalage: RESERVE, tout_decaler: false });
    }

    /// Un espace cité près de `winpin_base` le franchirait : la réserve décale tout,
    /// épingles de la barre des tâches comprises, au lieu d'être refusée.
    #[test]
    fn un_index_cite_qui_franchirait_la_limite_fait_tout_decaler() {
        let r = decider(EPINGLES, None, fin_des_applets(EPINGLES), [LIMITE - 2], PINNED_ICON_BASE + EPINGLES, Some(LIMITE));
        assert_eq!(r, Reserve { capacite: EPINGLES + RESERVE, decalage: RESERVE, tout_decaler: true });
    }

    /// ⭐ Le montage de la machine le 2026-10-18 : 2 épingles, et les épingles de la
    /// barre des tâches collées aux applets (`winpin_base` = fin des applets).
    #[test]
    fn sans_remplisseur_la_reserve_decale_tout() {
        let fin = fin_des_applets(2);
        let r = decider(2, Some(2), fin, [fin, fin + 3], PINNED_ICON_BASE + 2, Some(fin));
        assert_eq!(r, Reserve { capacite: 2 + RESERVE, decalage: RESERVE, tout_decaler: true });
    }

    #[test]
    fn pas_de_reserve_sans_limite_connue() {
        let r = decider(EPINGLES, None, fin_des_applets(EPINGLES), [], PINNED_ICON_BASE + EPINGLES, None);
        assert_eq!(r, Reserve { capacite: EPINGLES, decalage: 0, tout_decaler: false });
    }

    /// Des épingles ajoutées à la main dans `config.toml` dépassent la capacité
    /// sauvée : la capacité ne descend jamais sous le nombre d'épingles.
    #[test]
    fn la_capacite_ne_descend_jamais_sous_les_epingles() {
        let r = decider(30, Some(25), 0, [], 0, None);
        assert_eq!(r.capacite, 30);
    }

    #[test]
    fn le_decalage_ne_touche_que_la_plage() {
        let mut a = vec![0, 2, 24, 56, 91, 92, 141];
        let (mut b, mut c, mut d) = (vec![], vec![25], vec![]);
        decaler([&mut a, &mut b, &mut c, &mut d], 24, 92, 8);
        assert_eq!(a, vec![0, 2, 32, 64, 99, 92, 141]);
        assert_eq!(c, vec![33]);
    }

    /// ⭐ **LA PROPRIÉTÉ QUI REND L'ÉPINGLAGE À CHAUD POSSIBLE** : remplir une place
    /// réservée ne déplace AUCUN index — ni les applets, ni la longueur du dock.
    #[test]
    fn remplir_une_place_reservee_ne_deplace_rien() {
        let cap = EPINGLES + RESERVE;
        let mut dock = Dock::new([0.0; 4], cap);
        dock.masquer_les_places_libres(EPINGLES, cap);
        let longueur = dock.icons.len();
        let premiere_applet = PINNED_ICON_BASE + cap;
        assert!(dock.icons[PINNED_ICON_BASE + EPINGLES].hidden, "la place libre est masquee");
        assert!(!dock.icons[PINNED_ICON_BASE + EPINGLES - 1].hidden, "une vraie epingle reste visible");
        assert!(!dock.icons[premiere_applet].hidden, "l'applet n'est pas masquee");

        dock.set_icon_hidden(PINNED_ICON_BASE + EPINGLES, false);

        assert_eq!(dock.icons.len(), longueur);
        assert!(!dock.icons[PINNED_ICON_BASE + EPINGLES].hidden);
        assert!(!dock.icons[premiere_applet].is_spacer, "l'applet est toujours a son index");
    }

    /// ⭐ **RETIRER À CHAUD** : les épingles suivantes reculent d'un cran, l'icône
    /// retirée devient une place libre masquée, et les applets ne bougent pas.
    #[test]
    fn retirer_une_epingle_rend_sa_place_a_la_reserve() {
        let cap = 6;
        let mut dock = Dock::new([0.0; 4], cap);
        dock.masquer_les_places_libres(4, cap);
        for rang in 0..4 {
            dock.icons[PINNED_ICON_BASE + rang].label = char::from(b'a' + rang as u8);
            dock.icons[PINNED_ICON_BASE + rang].external_texture = true;
        }
        let applet = dock.icons[PINNED_ICON_BASE + cap].label;
        let longueur = dock.icons.len();

        assert!(dock.retirer_une_epingle(1, 4, cap));

        let labels: String = (0..3).map(|r| dock.icons[PINNED_ICON_BASE + r].label).collect();
        assert_eq!(labels, "acd", "les suivantes reculent d'un cran");
        for rang in 3..cap {
            let icon = &dock.icons[PINNED_ICON_BASE + rang];
            assert!(icon.hidden && !icon.external_texture, "rang {rang} : place libre");
        }
        assert_eq!(dock.icons[PINNED_ICON_BASE + cap].label, applet, "l'applet n'a pas bouge");
        assert_eq!(dock.icons.len(), longueur);
    }

    #[test]
    fn retirer_hors_des_epingles_est_refuse() {
        let mut dock = Dock::new([0.0; 4], 6);
        assert!(!dock.retirer_une_epingle(4, 4, 6), "rang = nombre d'epingles");
        assert!(!dock.retirer_une_epingle(0, 7, 6), "plus d'epingles que de places");
    }

    /// ⛔ **DÉFAUT RÉEL (2026-10-10) : « la zone de notification est vide »** après une
    /// épingle posée à chaud. `set_icon_order`, écrit pour le LANCEMENT, recevait
    /// l'ordre persistable (sans le miroir) : il retirait les icônes de notification
    /// de la droite et les reversait, avec les dossiers de la barre du bas, dans la
    /// GAUCHE. Reposé à chaud, l'ordre doit garder les plages d'exécution en place.
    #[test]
    fn reposer_l_ordre_a_chaud_garde_le_miroir_et_les_dossiers() {
        let mut dock = Dock::new([0.0; 4], 4);
        let dossiers = [crate::config::ElementBarre::Dossier(crate::config::DossierEpingle {
            chemin: std::path::PathBuf::from(r"C:\Temp"),
            nom: None,
            vignette: None,
        })];
        let base_dossiers = dock.set_dossiers(&dossiers);
        let base_miroir = dock.set_tray_count(3);
        let (l, r, tl, tr) = dock.icon_order();

        dock.set_icon_order(l, r, tl, tr);

        let [gauche, droite, _, _] = dock.icon_order_vivant();
        for i in base_miroir..base_miroir + 3 {
            assert!(droite.contains(&i), "l'icone de notification {i} reste a droite");
            assert!(!gauche.contains(&i), "l'icone de notification {i} ne passe pas a gauche");
        }
        assert!(!gauche.contains(&base_dossiers), "le dossier reste dans la barre du bas");
    }
}

