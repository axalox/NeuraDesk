//! « Garder éveillé » — idée n°7 de FluidWin (`comodita.h`) : tant que c'est
//! actif, l'ordinateur ne se met plus en veille et l'écran ne s'éteint plus,
//! exactement ce que fait un lecteur vidéo en cours de lecture.
//!
//! `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED |
//! ES_DISPLAY_REQUIRED)` — la même API que FluidWin, documentée par Microsoft.
//! FluidWin propose une heure, deux heures, ou jusqu'à ce qu'on l'arrête : ce
//! sont les trois durées du menu Alimentation (voir `DUREES_MIN` et
//! `assets/popups/power.html`).
//!
//! ## ⚠️ L'ÉTAT EST CELUI DU FIL QUI APPELLE — et c'est pour ça qu'il marche
//!
//! `SetThreadExecutionState` s'applique au fil appelant, et une demande
//! `ES_CONTINUOUS` reste posée tant que ce fil vit (elle meurt avec lui). Ce
//! module est donc appelé UNIQUEMENT depuis la boucle principale (`app::run`,
//! `tick` à chaque frame et le dispatch du menu Alimentation, tous deux sur le
//! fil principal) : appeler `demarrer` depuis un fil de travail jetable ferait
//! une demande qui s'évapore à la fin du fil, sans erreur.
//!
//! ## Ce qui n'est PAS fait, volontairement
//!
//! - **Aucune persistance.** Comme FluidWin : au redémarrage de l'app, tout est
//!   éteint. Une veille « collée » après un redémarrage surprendrait.
//! - **L'écran aussi.** `ES_SYSTEM_REQUIRED` seul laisserait l'écran
//!   s'éteindre ; le libellé « Garder éveillé » promet les deux, comme FluidWin.
//! - **Pas de réponse dans `powercfg /requests`** : cette commande exige des
//!   droits d'administrateur (mesuré le 2026-11-26), elle ne peut donc pas
//!   servir de vérification automatique.

use std::cell::Cell;
use std::time::{Duration, Instant};

use serde_json::json;
use windows::Win32::System::Power::{
    SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
};

/// Les trois durées du menu, en minutes — **0 = sans limite** (voir
/// `demarrer`). L'ordre est celui des boutons de `assets/popups/power.html`.
pub const DUREES_MIN: [u32; 3] = [60, 120, 0];

/// L'état de la demande. `Arrete` = aucune demande posée auprès de Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Etat {
    Arrete,
    /// Fin de la demande (`Instant` monotone : insensible au changement d'heure).
    JusquA(Instant),
    SansLimite,
}

thread_local! {
    /// ⚠️ PAR FIL, comme la demande Windows elle-même (voir la doc de module) —
    /// lu et écrit seulement depuis le fil principal.
    static ETAT: Cell<Etat> = const { Cell::new(Etat::Arrete) };
}

/// Démarre (ou remplace) la demande. `minutes == 0` = sans limite.
pub fn demarrer(minutes: u32) {
    let etat = if minutes == 0 {
        Etat::SansLimite
    } else {
        Etat::JusquA(Instant::now() + Duration::from_secs(u64::from(minutes) * 60))
    };
    ETAT.with(|e| e.set(etat));
    appliquer(true);
}

/// Annule la demande. Sans effet si rien n'était demandé (et n'appelle alors
/// pas Windows inutilement).
pub fn arreter() {
    let courant = ETAT.with(|e| e.replace(Etat::Arrete));
    if courant != Etat::Arrete {
        appliquer(false);
    }
}

/// L'état COURANT, une échéance dépassée étant d'abord soldée (voir `tick`).
pub fn etat() -> Etat {
    tick();
    ETAT.with(Cell::get)
}

pub fn actif() -> bool {
    etat() != Etat::Arrete
}

/// Temps restant avant l'échéance — `None` si arrêté **ou sans limite** (les
/// deux cas où il n'y a rien à décompter).
pub fn restant() -> Option<Duration> {
    match etat() {
        Etat::JusquA(fin) => Some(fin.saturating_duration_since(Instant::now())),
        Etat::Arrete | Etat::SansLimite => None,
    }
}

/// **À APPELER À CHAQUE FRAME** (voir `app::run`) : quand l'échéance est
/// passée, la demande est retirée auprès de Windows (`ES_CONTINUOUS` sans
/// autre drapeau, la remise à zéro documentée).
pub fn tick() {
    let maintenant = Instant::now();
    let courant = ETAT.with(Cell::get);
    let suivant = expirer(courant, maintenant);
    if suivant != courant {
        ETAT.with(|e| e.set(suivant));
        appliquer(false);
    }
}

/// **PURE** — l'échéance dépassée vaut `Arrete` ; tout le reste est inchangé.
fn expirer(etat: Etat, maintenant: Instant) -> Etat {
    match etat {
        Etat::JusquA(fin) if maintenant >= fin => Etat::Arrete,
        autre => autre,
    }
}

/// Minutes restantes, ARRONDIES AU SUPÉRIEUR (61 s → 2 min : « reste 1 min »
/// serait déjà faux), 0 quand il n'y a pas de compte à rebours.
fn reste_minutes(restant: Option<Duration>) -> u32 {
    match restant {
        Some(d) => {
            let s = d.as_secs() + u64::from(d.subsec_nanos() > 0);
            u32::try_from(s.div_ceil(60)).unwrap_or(u32::MAX)
        }
        None => 0,
    }
}

/// L'état pour `assets/popups/power.html` (voir `cef_test_window::push_state`).
pub fn json() -> serde_json::Value {
    let etat = etat();
    json!({
        "actif": etat != Etat::Arrete,
        "sansLimite": etat == Etat::SansLimite,
        "resteMin": reste_minutes(restant()),
    })
}

/// La clé de déduplication de `push_state` : elle ne change qu'à la minute
/// près (et à chaque changement d'état), pas à chaque frame. **PURE**, et
/// prend `etat` en paramètre pour être testable.
pub fn cle_pour(etat: Etat, restant: Option<Duration>) -> String {
    let (actif, sans_limite) = match etat {
        Etat::Arrete => (false, false),
        Etat::JusquA(_) => (true, false),
        Etat::SansLimite => (true, true),
    };
    format!("{actif}|{sans_limite}|{}", reste_minutes(restant))
}

pub fn cle() -> String {
    cle_pour(etat(), restant())
}

/// Pose (ou retire) la demande auprès de Windows. Les deux appels sont
/// IDEMPOTENTS — inutile de suivre l'état déjà appliqué.
fn appliquer(actif: bool) {
    let drapeaux = if actif {
        ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
    } else {
        ES_CONTINUOUS
    };
    // Le retour est l'état PRÉCÉDENT du fil, et vaut légitimement 0 au premier
    // appel réussi : il ne dit rien du succès (la doc le réserve à l'échec
    // seulement dans un sens ambigu). Rien à faire de plus : cette API n'a pas
    // d'autre contrôle d'erreur utile.
    unsafe {
        let _ = SetThreadExecutionState(drapeaux);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sans_limite_ne_se_termine_pas() {
        demarrer(0);
        assert_eq!(etat(), Etat::SansLimite);
        assert!(actif());
        assert_eq!(restant(), None);
        assert_eq!(
            json(),
            json!({ "actif": true, "sansLimite": true, "resteMin": 0 })
        );
        arreter();
        assert!(!actif());
    }

    #[test]
    fn une_duree_pose_une_echeance() {
        demarrer(120);
        let reste = restant().expect("deux heures, ce n'est pas « sans limite »");
        assert!(reste <= Duration::from_secs(120 * 60));
        assert!(reste > Duration::from_secs(120 * 60 - 5));
        assert_eq!(reste_minutes(Some(reste)), 120);
        arreter();
        assert_eq!(etat(), Etat::Arrete);
        assert_eq!(restant(), None);
    }

    #[test]
    fn une_echeance_passee_s_arrete_toute_seule() {
        let maintenant = Instant::now();
        let passe = Etat::JusquA(maintenant - Duration::from_secs(1));
        assert_eq!(expirer(passe, maintenant), Etat::Arrete);
        let futur = Etat::JusquA(maintenant + Duration::from_secs(60));
        assert_eq!(expirer(futur, maintenant), futur);
        assert_eq!(expirer(Etat::SansLimite, maintenant), Etat::SansLimite);
        assert_eq!(expirer(Etat::Arrete, maintenant), Etat::Arrete);
    }

    #[test]
    fn les_minutes_restantes_sont_arrondies_au_superieur() {
        for (secondes, attendu) in [(1, 1), (59, 1), (60, 1), (61, 2), (3599, 60), (3600, 60), (3601, 61)] {
            assert_eq!(
                reste_minutes(Some(Duration::from_secs(secondes))),
                attendu,
                "{secondes} s"
            );
        }
        assert_eq!(reste_minutes(None), 0);
    }

    #[test]
    fn la_cle_ne_change_qu_au_changement_visible() {
        let maintenant = Instant::now();
        let fin = Etat::JusquA(maintenant + Duration::from_secs(90));
        let a = cle_pour(fin, Some(Duration::from_secs(90)));
        let b = cle_pour(fin, Some(Duration::from_secs(75)));
        assert_eq!(a, b, "même minute (2 min) → même clé : pas de poussée inutile");
        let c = cle_pour(fin, Some(Duration::from_secs(59)));
        assert_ne!(a, c);
        assert_eq!(cle_pour(Etat::Arrete, None), "false|false|0");
        assert_eq!(cle_pour(Etat::SansLimite, None), "true|true|0");
    }

    #[test]
    fn les_durees_du_menu_sont_trois_et_finissent_par_sans_limite() {
        assert_eq!(DUREES_MIN, [60, 120, 0]);
    }

    /// Vérification RÉELLE (ignorée par défaut) : la demande est acceptée par
    /// Windows sur ce fil. `powercfg /requests` ne peut pas servir de juge ici
    /// — il exige des droits d'administrateur (mesuré le 2026-11-26).
    #[test]
    #[ignore]
    fn essai_reel_windows_accepte_la_demande() {
        use windows::Win32::Foundation::{GetLastError, SetLastError, ERROR_SUCCESS};

        let debut = Instant::now();
        demarrer(1);
        println!("demande posée en {} ms", debut.elapsed().as_millis());
        assert!(actif());
        assert!(restant().is_some());

        // ⚠️ Le retour de `SetThreadExecutionState` est l'état PRÉCÉDENT et vaut
        // 0 au premier appel réussi : il ne dit rien. La dernière erreur, si.
        let (retour, erreur) = unsafe {
            SetLastError(ERROR_SUCCESS);
            let r = SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED);
            (r, GetLastError())
        };
        println!("état précédent 0x{:X}, erreur {}", retour.0, erreur.0);
        assert_eq!(erreur, ERROR_SUCCESS);

        let debut = Instant::now();
        arreter();
        println!("demande retirée en {} ms", debut.elapsed().as_millis());
        assert!(!actif());
        assert_eq!(restant(), None);
    }
}
