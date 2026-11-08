//! **LA VEILLE DES TOASTS — le vrai toast sous la barre, sa trace dans l'historique.**
//!
//! DEMANDE EXPLICITE (2026-10-08) : « faire s'afficher les notifications en temps
//! réel sous l'horloge de la barre principale au lieu qu'elles s'affichent en bas
//! à droite de l'écran ».
//!
//! # ⛔ CE QUE CE MODULE FAISAIT, ET POURQUOI IL A CESSÉ (2026-11-02)
//!
//! Il coupait les popups du shell (`ToastEnabled = 0`, via `toast_garde`) et
//! réaffichait chaque toast dans le bandeau. **MESURÉ : une fois la valeur prise
//! en compte, Windows JETTE les toasts** — ni bannière, ni ligne dans
//! `wpndatabase`, donc rien à lire : le bandeau restait vide et les notifications
//! étaient PERDUES. Ça n'avait semblé marcher que parce que la valeur n'agit
//! qu'au redémarrage de `WpnUserService_*`.
//!
//! # Ce qu'il fait maintenant
//!
//! 1. **Le vrai toast est déplacé** sous la barre du haut, à la position du
//!    bandeau (`win32::toast_natif`). Rien n'est coupé, rien n'est écrit dans le
//!    registre : le toast garde ses boutons, sa durée, son clic.
//! 2. **Sa trace va dans l'historique** du bandeau (`bandeau::retenir`, clic
//!    droit sur l'horloge) — SANS ouvrir le bandeau, qui le montrerait en double.
//!
//! # ⭐ LE TITRE PORTE DÉJÀ L'ÉMETTEUR — ON NE DEVINE RIEN
//!
//! MESURÉ sur la vraie base (2026-10-08) : pour une notification relayée, le
//! **premier `<text>` EST le nom de l'émetteur** — `Discord`, `Telegram`,
//! `AliExpress`. Le nom est donc dans le contenu, et l'identifiant
//! `Microsoft.YourPhone_8wekyb3d8bbwe!YourPhoneNotifications_com.discord` n'a pas
//! à être décodé.
//!
//! **C'est heureux, parce que ce décodage est INTERDIT ailleurs** :
//! `notification::exe_named_by` ne rend un nom que pour un `.exe` et se tait sur
//! tout identifiant opaque — « ne JAMAIS deviner l'application à partir de leur
//! forme ». On ne fait donc AUCUNE inférence ici, sauf quand le titre est vide,
//! et le repli est alors le nom d'exécutable ou l'identifiant BRUT.
//!
//! # L'amorçage, et le défaut qu'il évite
//!
//! La PREMIÈRE lecture **ne retient rien** : elle ne note que l'arrivée la plus
//! récente (`ArrivalTime` — ⛔ pas `Order`, que SQLite réutilise). Sans cela,
//! tout l'historique du centre déferlerait dans l'historique au lancement —
//! même discipline que `notification::UnreadState::seed`.

use std::cell::Cell;
use std::time::{Duration, Instant};

use crate::win32::notification;

thread_local! {
    /// L'`ArrivalTime` le plus récent DÉJÀ VU (voir `a_montrer`). `None` = on
    /// n'a pas encore amorcé.
    static DERNIER: Cell<Option<i64>> = const { Cell::new(None) };
    /// L'instant du dernier relevé — voir `CADENCE`.
    static DERNIER_RELEVE: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// **LE RÉGLAGE QUI ACTIVE LA FONCTIONNALITÉ**, dans le dossier de configuration
/// — **désactivé par défaut**. Le NOM est gardé : qui l'avait coché retrouve la
/// fonctionnalité, par la nouvelle voie.
pub const REGLAGE: &str = "toasts_sous_l_horloge";

/// **LA CADENCE DU RELEVÉ.** La base est en WAL et s'ouvre/ferme à chaque
/// scrutation (voir `notification`, dont c'est la doctrine) : une seconde suffit
/// à un historique.
const CADENCE: Duration = Duration::from_secs(1);

/// **ACTIVE LA FONCTIONNALITÉ SI LE RÉGLAGE LE DEMANDE** — une fois au lancement,
/// après la réparation de `toast_garde`.
pub fn activer_si_demande() {
    crate::win32::toast_natif::set_actif(crate::config::load_bool_setting(REGLAGE));
}

/// **L'INTERRUPTEUR, POUR LA CASE DES RÉGLAGES** — enregistre ET applique à
/// chaud. Éteindre laisse le prochain toast en bas à droite ; celui déjà
/// déplacé finit sa vie où il est.
pub fn set_enabled(actif: bool) {
    crate::config::save_bool_setting(REGLAGE, actif);
    crate::win32::toast_natif::set_actif(actif);
    crate::diag!("[toast] deplacement des toasts : {actif}");
}

/// La fonctionnalité est-elle active ?
pub fn est_actif() -> bool {
    crate::win32::toast_natif::est_actif()
}

/// **LE RELEVÉ, À CHAQUE FRAME** — il ne touche la base qu'une fois par
/// `CADENCE`, et ne fait que RETENIR : le toast, lui, est déjà à l'écran.
pub fn surveiller() {
    if !est_actif() {
        return;
    }
    let maintenant = Instant::now();
    let trop_tot = DERNIER_RELEVE.with(|cell| {
        matches!(cell.get(), Some(dernier) if maintenant.duration_since(dernier) < CADENCE)
    });
    if trop_tot {
        return;
    }
    DERNIER_RELEVE.with(|cell| cell.set(Some(maintenant)));

    let dernier = DERNIER.with(Cell::get);
    let Some(toasts) = notification::toasts_depuis(dernier.unwrap_or(0)) else {
        // Base illisible (service arrêté, verrou) : on réessaiera — transitoire.
        return;
    };
    let (curseur, montres) = a_montrer(dernier, &toasts);
    DERNIER.with(|cell| cell.set(curseur));

    for toast in montres {
        // Un toast sans texte (une image seule) n'a rien à laisser dans une liste.
        if toast.titre.is_empty() && toast.message.is_empty() {
            continue;
        }
        let titre = if toast.titre.is_empty() {
            nom_de_repli(&toast.aumid)
        } else {
            toast.titre.clone()
        };
        // ⚠️ **LA TRACE NE PORTE QUE DES LONGUEURS** : le journal est un fichier,
        // le contenu des messages n'a rien à y faire.
        crate::diag!(
            "[toast] retenu : ordre {}, titre {} car., message {} car.",
            toast.order,
            titre.chars().count(),
            toast.message.chars().count()
        );
        crate::win32::bandeau::retenir(&titre, &toast.message);
    }
}

/// **LA DÉCISION DU RELEVÉ, PURE** — rend le nouveau curseur (un `ArrivalTime`)
/// et les toasts à montrer, dans l'ordre d'arrivée.
///
/// ⭐ **LE PREMIER RELEVÉ AMORCE** (`curseur == None`) : il retient l'arrivée la
/// plus récente sans rien montrer, sinon tout l'historique du centre
/// déferlerait au lancement. ⚠️ **Une base VIDE amorce aussi** (à `0`) : sans
/// cela le premier toast de la session servirait d'amorce, et serait perdu.
///
/// ⛔ **LE CURSEUR EST L'ARRIVÉE, PAS `Order`**, que SQLite réutilise après un
/// effacement (voir `notification::QUERY_CONTENU`).
fn a_montrer(
    curseur: Option<i64>,
    toasts: &[notification::ToastWindows],
) -> (Option<i64>, Vec<&notification::ToastWindows>) {
    let plus_recente = toasts.iter().map(|toast| toast.arrivee).max();
    let Some(seuil) = curseur else {
        return (Some(plus_recente.unwrap_or(0)), Vec::new());
    };
    let montres: Vec<_> = toasts.iter().filter(|toast| toast.arrivee > seuil).collect();
    (Some(plus_recente.map_or(seuil, |p| p.max(seuil))), montres)
}

/// **LE NOM À MONTRER QUAND LE TITRE EST VIDE** — et c'est un REPLI, pas une
/// identification.
///
/// Il ne sert qu'à ne pas afficher une carte sans en-tête : on prend le nom
/// d'exécutable si l'identifiant en nomme un (`notification::exe_named_by`, la
/// seule lecture que ce dépôt s'autorise), et **l'identifiant BRUT** sinon.
///
/// ⛔ **ON NE DECODE PAS L'IDENTIFIANT.** `com.discord` se lirait bien, mais
/// `com.alibaba.aliexpresshd` donnerait « alibaba » — et la règle du dépôt est
/// explicite : « ne JAMAIS deviner l'application à partir de leur forme ». Un
/// identifiant brut est laid ; il est vrai.
pub fn nom_de_repli(aumid: &str) -> String {
    match notification::exe_named_by(aumid) {
        Some(exe) => exe.to_string(),
        None => aumid.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une application de bureau : l'identifiant est un chemin terminé par son
    /// exécutable, et `notification::exe_named_by` le nomme — c'est la SEULE
    /// lecture que ce dépôt s'autorise.
    #[test]
    fn un_identifiant_qui_nomme_un_exe_rend_son_nom() {
        assert_eq!(
            nom_de_repli(r"{GUID}\WindowsPowerShell\v1.0\powershell.exe"),
            "powershell.exe"
        );
    }

    /// ⛔ **ET UN IDENTIFIANT OPAQUE EST RENDU TEL QUEL.** Le décoder se
    /// lirait bien pour celui-ci — mais `com.alibaba.aliexpresshd` donnerait
    /// « alibaba », et la règle du dépôt est explicite : « ne JAMAIS deviner
    /// l'application à partir de leur forme ». Un identifiant brut est laid ; il
    /// est vrai.
    ///
    /// ⚠️ **CE TEST ÉCHOUE SUR UN CODE QUI DÉCODERAIT** : il attend la chaîne
    /// ENTIÈRE, pas `discord`.
    #[test]
    fn un_identifiant_opaque_est_rendu_tel_quel() {
        let aumid = "Microsoft.YourPhone_8wekyb3d8bbwe!YourPhoneNotifications_com.discord";
        assert_eq!(nom_de_repli(aumid), aumid);
        assert_ne!(nom_de_repli(aumid), "discord");
    }

    /// Le cas de l'application elle-même — son identifiant n'est ni un chemin ni
    /// un paquet, et il se rend tel quel.
    #[test]
    fn l_identifiant_de_l_application_se_rend_tel_quel() {
        assert_eq!(nom_de_repli("NeuraDesk"), "NeuraDesk");
    }

    fn toast(order: i64, arrivee: i64) -> notification::ToastWindows {
        notification::ToastWindows {
            aumid: "App".into(),
            order,
            arrivee,
            titre: "T".into(),
            message: "M".into(),
        }
    }

    /// Le premier relevé amorce : il ne montre RIEN et retient la plus récente.
    #[test]
    fn le_premier_releve_amorce_sans_rien_montrer() {
        let base = [toast(10, 100), toast(11, 200)];
        let (curseur, montres) = a_montrer(None, &base);
        assert_eq!(curseur, Some(200));
        assert!(montres.is_empty());
    }

    /// ⛔ **LE DÉFAUT DU 2026-11-02.** Après un effacement du centre, SQLite
    /// REPREND les `Order` plus bas : le toast neuf porte un `Order` INFÉRIEUR à
    /// celui déjà vu, mais il est arrivé APRÈS. Il doit être montré. L'ancien
    /// curseur sur `Order` le taisait.
    #[test]
    fn un_order_reutilise_apres_effacement_est_quand_meme_montre() {
        let deja_vu = toast(11752, 1_000);
        let (curseur, _) = a_montrer(None, std::slice::from_ref(&deja_vu));
        let neuf = toast(11703, 2_000);
        let (curseur, montres) = a_montrer(curseur, std::slice::from_ref(&neuf));
        assert_eq!(montres, vec![&neuf]);
        assert_eq!(curseur, Some(2_000));
    }

    /// ⛔ **UNE BASE VIDE AU LANCEMENT AMORCE QUAND MÊME.** Sinon le PREMIER
    /// toast de la session servirait d'amorce et ne serait jamais montré.
    #[test]
    fn une_base_vide_amorce_et_le_premier_toast_est_montre() {
        let (curseur, montres) = a_montrer(None, &[]);
        assert!(montres.is_empty());
        let premier = toast(1, 500);
        let (_, montres) = a_montrer(curseur, std::slice::from_ref(&premier));
        assert_eq!(montres, vec![&premier]);
    }

    /// Un toast déjà montré (arrivée ≤ curseur) ne se rejoue pas, et le curseur
    /// ne recule jamais.
    #[test]
    fn rien_ne_se_rejoue_et_le_curseur_ne_recule_pas() {
        let base = [toast(5, 900), toast(6, 1_000)];
        let (curseur, montres) = a_montrer(Some(1_000), &base);
        assert!(montres.is_empty());
        assert_eq!(curseur, Some(1_000));
    }
}
