//! **LE FIL DE TRAVAIL DE LA ZONE DE NOTIFICATION.**
//!
//! **CE QU'IL RÉPARE, ET C'EST MESURÉ.** L'application saccadait. La cause :
//! `tray_icons::enumerate()` — **~64 ms de médiane pour l'arbre entier**, mesuré
//! le 2026-09-21 sur les deux fils (la doc de `tray_icons` annonçait 42 ms, un
//! chiffre **périmé**) — tournait **à chaque seconde sur le fil qui peint**.
//! À 60 Hz, cela fait **3 à 4 images perdues chaque seconde**, et jusqu'à
//! **348 ms** sur un pic mesuré.
//!
//! **POURQUOI LE DÉPLACEMENT EST POSSIBLE, ET CE N'EST PAS UNE INTUITION** :
//! `enumerate()` fait son **propre** `CoInitializeEx` et crée/détruit son
//! `IUIAutomation` **localement** (`tray_icons.rs:236-250`) — elle ne conserve
//! **aucun état COM** entre deux appels, et un objet COM ne pourrait pas
//! traverser un fil. Elle est donc déjà appelable d'ici. Ses résultats
//! (`Vec<TrayIcon>` : des `String`, des `RECT`, un enum, un `bool`) sont des
//! données plates, `Send`.
//!
//! **CESSE QUOI, ICI** : tout ce qui touche au D3D et au dock reste sur le fil
//! principal — `gfx.device` n'est pas `Send`, et **aucune ressource D3D n'est
//! créée hors du fil principal dans tout ce dépôt**. Ce module ne connaît donc ni
//! `Texture`, ni `Dock` : il rend des listes.
//!
//! **APPARTEMENT : STA**, comme `tray_icons::open_menu_of` (`:1919-1923`), qui
//! est le précédent éprouvé d'UI Automation sur un fil secondaire ici. Les fils de
//! `media` et `cef_dash` sont en MTA parce que WinRT l'exige
//! (`IAsyncOperation::join()`) — ce chemin n'appelle aucun WinRT.
//!
//! ⚠️ **L'appartement est UNE constante** (`APARTMENT`) pour que la bascule soit
//! d'une ligne si une mesure le demandait. Le risque est borné par construction :
//! **un fil de travail bloqué donne un miroir figé, plus une application morte.**
//!
//! **DPI** : `make_process_dpi_aware()` agit au **niveau processus** (`dpi.rs:18`)
//! et il n'existe aucun `SetThreadDpiAwarenessContext` dans ce dépôt — le fil
//! hérite donc de PER_MONITOR_AWARE_V2, et les captures gardent les pixels réels.
//! ⚠️ Si quelqu'un en ajoutait un jour un sur le fil principal, **les captures de
//! ce fil partiraient ailleurs en silence** : c'est la raison d'être de ce
//! paragraphe. (Et toute sonde `cargo test` doit poser le sien.)

use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::win32::tray_icons::{TrayIcon, TrayZone};

/// L'appartement COM du fil. **Une seule constante, à dessein.**
///
/// ⚠️ **MTA — MAIS PAS POUR LA RAISON QU'ON CROYAIT.** L'idée première était STA,
/// par analogie avec `tray_icons::open_menu_of` (`:1919-1923`), qui fait de l'UI
/// Automation sur un fil secondaire en STA. Le premier cycle a rendu **585 ms
/// puis 2,00 s**, d'où la conclusion — **fausse** — qu'un STA sans pompe à
/// messages bloquait les appels COM inter-appartements, et la bascule en MTA.
///
/// **MTA a rendu 565 ms puis 1,69 s : aucun changement.** L'appartement n'était
/// donc pas la cause, et la mesure qui manquait alors a été prise depuis :
///
/// ```text
/// fil principal  : n=33  min 33,1 ms  médiane 63,9 ms  max 348,3 ms
/// fil de travail : n=60  min 24,9 ms  médiane 59,5 ms  max 317,7 ms
/// premier cycle  : 1,16 s sur le principal, 2,62 s sur le fil
/// ```
///
/// **Les deux fils sont indistinguables en régime** (63,9 contre 59,5 ms), et le
/// premier cycle est lent **sur les deux** : c'est un **échauffement** —
/// chargement d'UI Automation, premières traversées inter-processus — pas un
/// défaut d'appartement. Le choix est donc **indifférent au coût mesuré**.
///
/// ⚠️ **LA LEÇON, ET ELLE A COÛTÉ UNE SESSION** : les 585 ms et les 2,00 s
/// étaient les **deux premiers cycles**. Juger un coût sur les premiers cycles
/// d'un fil qui vient de naître, c'est mesurer l'échauffement et l'appeler un
/// régime — d'où un « 13 à 40× plus lent » qui n'existait pas. **Seule une
/// mesure en régime, sur des dizaines de cycles, tranche.**
///
/// MTA est par ailleurs le motif majoritaire du dépôt pour un fil de travail
/// (`media.rs`, `cef_dash.rs`) — et il n'a pas les contraintes qui les forcent,
/// ce chemin n'appelant aucun WinRT. **Conséquence à connaître** : sur un fil
/// MTA, le `CoInitializeEx(COINIT_APARTMENTTHREADED)` **interne** d'`enumerate()`
/// (`tray_icons.rs:244-249`) échoue en `RPC_E_CHANGED_MODE` et est ignoré
/// (`let _ =`) — l'UI Automation tourne donc ici en **MTA**. Mesuré : même coût,
/// mêmes 45 entrées.
const APARTMENT: windows::Win32::System::Com::COINIT =
    windows::Win32::System::Com::COINIT_MULTITHREADED;

/// Filet de sécurité du fil : si le fil principal cessait de publier, le fil
/// continue à son propre rythme **au lieu de mourir**.
///
/// ⚠️ **IL NE SERT PAS À CADENCER — ET À 1 s IL COURSAIT LE TOP DÉPART.** Le fil
/// attend 1 s **après** avoir fini son cycle (~60 ms), donc son délai expirait
/// **juste avant** le tick suivant : le tick qui arrivait alors relançait
/// aussitôt un second cycle. **Mesuré : 60 cycles du fil pour 33 publications du
/// principal en 33 s — ~1,8 énumération par seconde au lieu d'une.** C'est du
/// travail payé deux fois, et rien ne le signalait : le fil était *plus* occupé
/// qu'avant le déménagement, tout en le faisant au bon endroit.
///
/// **2 s place le filet hors de portée** du tick à 1 Hz : il ne peut plus
/// expirer qu'après deux ticks manqués — c'est-à-dire quand le principal s'est
/// vraiment tu, sa seule fonction.
const FILET: Duration = Duration::from_secs(2);

/// **LE DÉLAI DE GRÂCE D'UNE ZONE ÉTEINTE** (voir `zone_eteinte`).
///
/// Passé ce délai, on cesse de tenir : une zone qui reste vide 30 s n'est plus
/// un transitoire de reconstruction du shell, c'est la réalité — et prétendre le
/// contraire figerait le miroir sur des icônes disparues depuis longtemps.
///
/// ⚠️ **EN TEMPS, PAS EN CYCLES** — à dessein : la cadence de sondage n'est pas
/// encore décidée, et la grâce doit valoir la même durée réelle à 1 Hz comme à
/// 1/min.
const GRACE_ZONE_ETEINTE: Duration = Duration::from_secs(30);

/// **CE QU'ON RETIENT D'UN CYCLE : le nombre d'entrées de chaque zone, et rien
/// d'autre.**
///
/// Retenir la liste entière coûterait la copie de ~45 `String` par seconde pour
/// la seule information qui serve ici — savoir si une zone s'est ÉTEINTE.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Composition {
    visibles: usize,
    masquees: usize,
}

impl Composition {
    fn de(entrees: &[TrayIcon]) -> Self {
        Self {
            visibles: entrees.iter().filter(|e| e.zone == TrayZone::Visible).count(),
            masquees: entrees.iter().filter(|e| e.zone == TrayZone::Hidden).count(),
        }
    }
}

/// **LE PRÉDICAT : UNE ZONE ENTIÈRE NE S'ÉTEINT PAS EN UN CYCLE.**
///
/// MESURÉ, et c'est ce qui l'a fait écrire : pendant une session, le journal a
/// montré `45 → 34 → 45` entrées en **dix secondes**, sans qu'aucune icône n'ait
/// bougé. La sonde `tray_probe::counts_what_each_tray_zone_contributes` a donné
/// l'arithmétique : **`45 = 11 visibles + 34 masquées`**, et la liste de 34
/// commençait par « WhatsApp, DeepL… » — donc **la zone VISIBLE s'était tue**,
/// pas la masquée comme la première lecture le laissait croire.
///
/// Et elle s'est tue **en silence**, ce qui localise le défaut : `enumerate_zone`
/// a bien un témoin pour la passerelle introuvable (« passerelle de composition
/// introuvable sous Shell_TrayWnd »), mais **il n'a pas parlé**. La passerelle
/// était trouvée ; c'est la boucle d'éléments qui a tout rendu vide, et là
/// **chaque échec est un `continue` muet** (`tray_icons.rs:303-324`) —
/// `GetElement` raté, classe hors préfixe, `keep` qui exclut,
/// `CurrentBoundingRectangle` en échec. Le seul témoin du fichier est placé
/// **avant** l'endroit où la panne se produit réellement.
///
/// D'où ce prédicat, qui ne cherche pas la cause mais **refuse la conclusion** :
/// une zone pleine qui rend zéro d'un cycle à l'autre n'est pas une composition,
/// c'est une lecture incomplète — et publier ça ferait disparaître 11 icônes
/// pour rien.
///
/// ⚠️ **IL S'ANCRE SUR LE CYCLE PRÉCÉDENT**, donc il ne peut pas se déclencher
/// sur un état légitime : une zone réellement vidée se vide par étapes, jamais
/// d'un bloc. C'est ce qui le rend sûr là où une règle du genre « la zone
/// visible n'est jamais vide » serait une supposition.
fn zone_eteinte(precedent: Composition, courant: Composition) -> Option<TrayZone> {
    if precedent.visibles > 0 && courant.visibles == 0 {
        return Some(TrayZone::Visible);
    }
    if precedent.masquees > 0 && courant.masquees == 0 {
        return Some(TrayZone::Hidden);
    }
    None
}

/// Ce que le fil principal envoie au fil de travail.
pub enum TrayInput {
    /// **LE TOP DÉPART DU CYCLE.** Publié par le fil principal dans le bloc qui
    /// vient de calculer les fenêtres ouvertes, pour que les deux fils restent
    /// alignés à 1 Hz. (Il portera la projection `Send` des fenêtres ouvertes à
    /// l'étape 3, quand la résolution des icônes déménagera.)
    Tick,
    /// Arrêt propre — déposé par `Drop`.
    Stop,
}

/// **LE FIL, ET SON ARRÊT, DANS UN SEUL OBJET.**
///
/// Détenu comme une locale de `app::run`, **déclaré APRÈS `GpuCacheGuard`** — donc
/// détruit AVANT lui, puisque les locales se détruisent en ordre inverse. C'est
/// l'ordre correct : le fil ne doit plus exister quand le device D3D meurt.
///
/// Son `Drop` **ne détient aucune ressource GPU**, ce qui est ce qui rend son
/// `join()` sans danger — contrairement aux panneaux CEF, dont la destruction
/// prématurée a déjà coûté un plantage à la sortie (voir `GpuCacheGuard`).
pub struct TrayWorker {
    entree: SyncSender<TrayInput>,
    sortie: Receiver<Vec<TrayIcon>>,
    fil: Option<JoinHandle<()>>,
}

impl TrayWorker {
    /// Démarre le fil. Ne bloque pas, ne peut pas échouer.
    pub fn start() -> Self {
        // `sync_channel(1)` : si le fil est encore occupé au cycle suivant, la
        // publication est ABANDONNÉE plutôt que mise en file — la suivante, une
        // seconde plus tard, portera une information plus fraîche. Jamais bloquant.
        let (entree_tx, entree_rx) = std::sync::mpsc::sync_channel::<TrayInput>(1);
        let (sortie_tx, sortie_rx) = std::sync::mpsc::channel::<Vec<TrayIcon>>();

        let fil = std::thread::spawn(move || {
            // COM sur CE fil. `enumerate()` le redemande de toute façon
            // (« déjà initialisé » est un échec bénin, voir `tray_icons.rs:233`),
            // mais l'initialiser ici rend l'appartement explicite et le rend
            // effectif pour tout ce que le fil fera ensuite.
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(None, APARTMENT);
            }

            let mut premier = true;
            // Ce qu'on retient du dernier cycle PUBLIÉ, et la zone éteinte en
            // cours — voir `zone_eteinte`, qui porte la mesure et la raison.
            let mut precedent = Composition::default();
            let mut extinction: Option<(TrayZone, Instant)> = None;
            loop {
                let reveil = match entree_rx.recv_timeout(FILET) {
                    Ok(TrayInput::Tick) => true,
                    // Filet : le principal s'est tu. On travaille quand même.
                    Err(RecvTimeoutError::Timeout) => true,
                    // Le principal a disparu : on sort, plutôt que de boucler à vide.
                    Err(RecvTimeoutError::Disconnected) => false,
                    Ok(TrayInput::Stop) => false,
                };
                if !reveil {
                    break;
                }

                let debut = Instant::now();
                // ⚠️ **`enumerate_avec_le_panneau`, PAS `enumerate`** : c'est le
                // MIROIR, donc la seule voie qui a le droit d'ouvrir le panneau de
                // débordement pour lire les icônes masquées — la fenêtre qui les
                // porte n'existe pas au repos. Le coût est borné par
                // `PEREMPTION_MASQUEES` (un aller-retour ~750 ms toutes les
                // 30 min), donc il reste sur CE fil et jamais sur celui qui peint.
                let entries = crate::win32::tray_icons::enumerate_avec_le_panneau();
                let duree = debut.elapsed();

                // **LES DEUX OBSERVABLES DU FIL.** Le premier cycle est toujours
                // écrit — sans lui, on ne saurait pas que le fil a démarré. Ensuite
                // on ne parle QUE si le cycle pèse, pour que le journal reste muet
                // en régime normal et bruyant exactement quand le fil est le
                // problème (même esprit que `tray_icons::warn_once`).
                if premier {
                    crate::diag!(
                        "[tray_worker] fil démarré — premier cycle : énumération {:?}, \
                         {} entrée(s)",
                        duree,
                        entries.len()
                    );
                    premier = false;
                } else if duree.as_millis() >= 100 {
                    crate::diag!(
                        "[tray_worker] cycle lent : énumération {:?}, {} entrée(s)",
                        duree,
                        entries.len()
                    );
                }

                // ⚠️ **ON NE PUBLIE PAS UNE LECTURE OÙ UNE ZONE ENTIÈRE S'EST
                // ÉTEINTE** (voir `zone_eteinte`). Ne rien publier n'est pas
                // perdre un cycle : le fil principal garde sa dernière liste
                // **COMPLÈTE**, donc le miroir cesse de clignoter — et c'est
                // exactement ce qu'on veut, puisque les icônes de la zone
                // éteinte sont toujours là, dans la barre, simplement
                // illisibles le temps que le shell se reconstruise.
                let courant = Composition::de(&entries);
                if let Some(zone) = zone_eteinte(precedent, courant) {
                    match extinction {
                        Some((z, debut)) if z == zone => {
                            if debut.elapsed() < GRACE_ZONE_ETEINTE {
                                continue;
                            }
                            // Au-delà de la grâce : on accepte la lecture telle
                            // qu'elle vient, et on republie normalement.
                        }
                        _ => {
                            crate::diag!(
                                "[tray_worker] zone {zone:?} ÉTEINTE — {courant:?} après \
                                 {precedent:?} : on ne publie pas, le fil principal garde sa \
                                 dernière liste complète (grâce de {GRACE_ZONE_ETEINTE:?})"
                            );
                            extinction = Some((zone, Instant::now()));
                            continue;
                        }
                    }
                } else if let Some((zone, debut)) = extinction.take() {
                    crate::diag!(
                        "[tray_worker] zone {zone:?} RALLUMÉE après {:?} — publication reprise",
                        debut.elapsed()
                    );
                }
                precedent = courant;

                // `send` sur un canal déconnecté rend `Err` et NE PANIQUE PAS :
                // on sort simplement.
                if sortie_tx.send(entries).is_err() {
                    break;
                }
            }

            unsafe { windows::Win32::System::Com::CoUninitialize() };
        });

        Self {
            entree: entree_tx,
            sortie: sortie_rx,
            fil: Some(fil),
        }
    }

    /// **LE TOP DÉPART DU CYCLE** — appelé une fois par seconde par le fil
    /// principal. Ne bloque jamais : si le fil est encore occupé, le message est
    /// abandonné et le cycle suivant suffira.
    pub fn publish(&self) {
        match self.entree.try_send(TrayInput::Tick) {
            Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
        }
    }

    /// **CE QUE LE FIL A PRODUIT, S'IL Y A QUELQUE CHOSE** — appelé à chaque
    /// frame, ne bloque jamais. `None` signifie « rien de neuf », et le fil
    /// principal garde alors la liste qu'il détient déjà : **il n'y a rien à
    /// recopier, et surtout rien à attendre.**
    pub fn poll(&self) -> Option<Vec<TrayIcon>> {
        match self.sortie.try_recv() {
            Ok(entries) => Some(entries),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            // Le fil est mort : plus rien n'arrivera. On rend `None` pour
            // toujours — l'appelant garde sa dernière liste, ce qui vaut mieux
            // qu'un miroir qui se vide.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
        }
    }
}

impl Drop for TrayWorker {
    fn drop(&mut self) {
        // Trois étages : on demande l'arrêt, on ferme la porte, on attend.
        // Le `join` dure **au plus un cycle** — le fil ne fait rien d'illimité ;
        // au pire il est dans une énumération de ~64 ms. S'il devait traîner, le
        // détachement serait sûr, précisément parce que ce fil ne détient aucune
        // ressource GPU.
        let _ = self.entree.send(TrayInput::Stop);
        if let Some(fil) = self.fil.take() {
            let _ = fil.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le couple `(visibles, masquées)` — **et non la liste entière** : c'est
    /// tout ce que le prédicat regarde, donc c'est tout ce que le test a besoin
    /// de fabriquer. Pas d'écran, pas d'UI Automation, pas de shell.
    fn c(visibles: usize, masquees: usize) -> Composition {
        Composition { visibles, masquees }
    }

    /// **Le cas MESURÉ** : la zone visible (11 entrées) s'éteint d'un cycle à
    /// l'autre, la masquée (34) survit — c'est exactement ce que le journal
    /// d'hier a montré sous la forme `45 -> 34`.
    #[test]
    fn a_zone_that_goes_dark_in_one_cycle_is_recognised() {
        assert_eq!(zone_eteinte(c(11, 34), c(0, 34)), Some(TrayZone::Visible));
        assert_eq!(zone_eteinte(c(11, 34), c(11, 0)), Some(TrayZone::Hidden));
    }

    /// **LE CONTRÔLE NÉGATIF — c'est lui qui protège l'utilisateur.** Une zone
    /// qui se vide par étapes est un usage normal : personne ne doit voir ses
    /// icônes figées parce qu'il en a fermé une.
    #[test]
    fn a_step_by_step_disappearance_or_an_already_dead_zone_does_not_trigger() {
        assert_eq!(zone_eteinte(c(11, 34), c(10, 34)), None);
        assert_eq!(zone_eteinte(c(11, 34), c(11, 33)), None);
        assert_eq!(zone_eteinte(c(1, 34), c(0, 34)), Some(TrayZone::Visible));
        // Une zone DÉJÀ éteinte ne se « re-détecte » pas : le cycle précédent
        // retenu est celui de la dernière publication, il n'est pas mis à jour
        // tant qu'on tient.
        assert_eq!(zone_eteinte(c(0, 34), c(0, 34)), None);
    }

    /// **Le démarrage** : rien de connu avant, donc rien à tenir. Sans ce cas, la
    /// première lecture d'une session serait refusée si une zone est vide au
    /// lancement — ce qui est le cas normal d'une zone masquée jamais ouverte.
    #[test]
    fn the_first_cycle_never_holds_anything() {
        assert_eq!(zone_eteinte(c(0, 0), c(11, 34)), None);
        assert_eq!(zone_eteinte(c(0, 0), c(0, 34)), None);
        assert_eq!(zone_eteinte(c(0, 0), c(0, 0)), None);
    }

    /// Les deux zones s'éteignent ensemble (reconstruction complète du shell) :
    /// on en nomme une, et c'est suffisant — le cycle ne publie pas.
    #[test]
    fn both_zones_going_dark_at_once_names_the_visible_one() {
        assert_eq!(zone_eteinte(c(11, 34), c(0, 0)), Some(TrayZone::Visible));
    }
}
