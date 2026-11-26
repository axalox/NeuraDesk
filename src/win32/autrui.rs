//! **LES APPELS VERS UNE FENÊTRE D'UN AUTRE PROCESSUS, AVEC UN DÉLAI.**
//!
//! `ShowWindow`, `SetWindowPos` et `SetWindowLongPtr(GWL_EXSTYLE)` sur une
//! fenêtre qui appartient à un autre processus lui envoient un message et
//! **attendent qu'elle le lise**, sans limite. FluidWin a mesuré sur un
//! processus figé : **29 s** pour chacune des trois. Faites depuis notre boucle
//! de frame, elles gèlent TOUT le shell — barres, survol, horloge, animations —
//! et Windows finit par nous marquer « ne répond pas ».
//!
//! ⛔ `IsHungAppWindow` ne suffit pas : Windows ne déclare une fenêtre figée
//! qu'après 5 s sans message. Les cinq premières secondes d'un gel passent donc
//! inaperçues — et c'est exactement le moment où l'utilisateur bascule une app
//! en plein écran.
//!
//! Ce qui **n'attend pas** (mesuré par FluidWin, même processus figé) :
//! `SetLayeredWindowAttributes`, `SetPropW`, `RedrawWindow` sans
//! `RDW_UPDATENOW`, `SetForegroundWindow`, `SwitchToThisWindow`,
//! `ShowWindowAsync`, et `SetWindowPos` avec `SWP_ASYNCWINDOWPOS`. Ces appels-là
//! n'ont pas besoin de garde.
//!
//! # Comment s'en servir
//!
//! - On veut **savoir** si le déplacement a été appliqué (c'est le cas de
//!   `plein_ecran_zone`) : [`repond`] d'abord, puis l'appel SYNCHRONE.
//! - On veut seulement **demander** quelque chose sans jamais attendre (remise
//!   en place à la sortie, par exemple) : [`deplacer_sans_attendre`].
//!
//! Dans les deux cas, une fenêtre muette est **sautée** : on ne l'attend pas, on
//! réessaiera plus tard. Un délai qui expire n'est pas une erreur — c'est une
//! app qui a autre chose à faire.
//!
//! ⚠️ **CE QUI RESTE.** La sonde elle-même coûte son délai quand l'app est
//! lente, et le `SetWindowPos` SYNCHRONE qui la suit peut encore attendre si
//! l'app cesse de pomper ENTRE les deux, ou si son `WM_WINDOWPOSCHANGING` est
//! lent (réinitialisation de swapchain). La garde supprime le gel de 29 s
//! observé ; elle ne rend pas l'appel instantané.

use std::time::Duration;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    IsHungAppWindow, SendMessageTimeoutW, SetWindowPos, SMTO_ABORTIFHUNG, SMTO_ERRORONEXIT, SWP_ASYNCWINDOWPOS,
    SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER, WM_NULL,
};

/// Le délai accordé à une fenêtre pour dire qu'elle est vivante. La valeur de
/// FluidWin : quand l'app répond, la vérification ne coûte rien de plus que
/// l'appel qu'elle précède ; quand elle ne répond pas, 300 ms suffisent à la
/// déclarer sourde — borne valable pour une autre file d'attente (voir `repond`).
pub const DELAI_REPONSE: Duration = Duration::from_millis(300);

/// La fenêtre répond-elle à un message anodin dans `delai` ?
///
/// ⭐ **LE SUCCÈS SE LIT DANS LA VALEUR DE RETOUR, PAS DANS LE RÉSULTAT.**
/// MESURÉ le 2026-11-26 sur trois fenêtres vivantes d'autres processus :
/// `SendMessageTimeoutW(WM_NULL)` rend **1** et met **0** dans `lpdwresult`
/// (c'est le résultat de `DefWindowProc`, pas celui de la fonction). Contrôle
/// croisé avec `WM_GETTEXTLENGTH` : rend 1, et 26 dans `lpdwresult` pour un
/// titre de 26 caractères. La valeur de retour dit vivant/mort ; le résultat du
/// message ne dit rien.
///
/// ⛔ Ne PAS « corriger » en `retour != 0 || GetLastError() == 0` (ce que j'ai
/// écrit d'abord, en croyant `altrui.h` fautif — il avait raison). La doc dit
/// que `SendMessageTimeout` **ne pose pas toujours** de dernière erreur en cas
/// d'échec ; ce `||` déclarerait alors vivante une fenêtre morte, et le
/// `SetWindowPos` qui suit bloquerait la boucle — exactement ce que ce module
/// existe pour empêcher. Le test de la fenêtre muette ne l'attrape pas : sur ce
/// chemin, Windows pose `ERROR_TIMEOUT`.
///
/// ⚠️ Le délai n'est une borne que pour une fenêtre d'une AUTRE file d'attente
/// (toujours le cas ici : aucune `AttachThreadInput` dans le dépôt) — même
/// file, le message est traité directement et le délai est ignoré.
pub fn repond(fenetre: HWND, delai: Duration) -> bool {
    if fenetre.0.is_null() {
        return false;
    }
    // Déjà déclarée figée par Windows : inutile d'attendre le délai.
    if unsafe { IsHungAppWindow(fenetre) }.as_bool() {
        return false;
    }
    let mut resultat = 0usize;
    unsafe {
        SendMessageTimeoutW(
            fenetre,
            WM_NULL,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
            delai.as_millis() as u32,
            Some(&mut resultat),
        )
    }
    .0 != 0
}

/// Replace une fenêtre d'un autre processus **sans jamais attendre qu'elle
/// lise** : `SWP_ASYNCWINDOWPOS` poste la demande à sa file et rend la main
/// aussitôt. La fenêtre se déplacera quand elle pompera ses messages, ou jamais
/// si elle ne se réveille pas — ce n'est plus notre boucle qui attend.
///
/// ⚠️ Le résultat n'est donc pas connu : un refus (jeu lancé en administrateur,
/// UIPI) ne se voit pas ici. L'appelant qui doit le constater relit le
/// rectangle à la frame suivante (voir `plein_ecran_zone::rediriger`).
///
/// Rend `true` si la demande est partie.
pub fn deplacer_sans_attendre(fenetre: HWND, x: i32, y: i32, largeur: i32, hauteur: i32) -> bool {
    if fenetre.0.is_null() {
        return false;
    }
    unsafe {
        SetWindowPos(
            fenetre,
            None,
            x,
            y,
            largeur,
            hauteur,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_ASYNCWINDOWPOS,
        )
    }
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Instant;
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DispatchMessageW, GetMessageW, GetWindowThreadProcessId, PostThreadMessageW, TranslateMessage, MSG,
        WINDOW_EX_STYLE, WM_QUIT, WS_POPUP,
    };

    /// Une fenêtre `STATIC` sur un AUTRE fil, qui **pompe ses messages** comme
    /// une app vivante — sinon `SendMessageTimeout` serait traité directement
    /// (même fil) et l'essai ne prouverait rien du chemin réel.
    fn fenetre_vivante() -> (HWND, u32, std::thread::JoinHandle<()>) {
        let (rendue, attente) = mpsc::channel::<isize>();
        let fil = std::thread::spawn(move || {
            let fenetre = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    w!("autrui vivante"),
                    WS_POPUP,
                    0,
                    0,
                    10,
                    10,
                    None,
                    None,
                    None,
                    None,
                )
                .expect("fenêtre d'essai")
            };
            let _ = rendue.send(fenetre.0 as isize);
            let mut message = MSG::default();
            unsafe {
                while GetMessageW(&mut message, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&message);
                    let _ = DispatchMessageW(&message);
                }
            }
        });
        let fenetre = HWND(attente.recv().expect("fenêtre d'essai") as _);
        let proprietaire = unsafe { GetWindowThreadProcessId(fenetre, None) };
        (fenetre, proprietaire, fil)
    }

    /// Une fenêtre qui appartient à un fil qui **ne pompe jamais** ses messages
    /// — l'app figée de la mesure. Le fil reste vivant jusqu'au `drop` du canal,
    /// et Windows détruit sa fenêtre quand il sort.
    fn fenetre_muette() -> (HWND, mpsc::Sender<()>, std::thread::JoinHandle<()>) {
        let (fin, recu) = mpsc::channel::<()>();
        let (rendue, attente) = mpsc::channel::<isize>();
        let fil = std::thread::spawn(move || {
            let fenetre = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    w!("autrui muette"),
                    WS_POPUP,
                    0,
                    0,
                    10,
                    10,
                    None,
                    None,
                    None,
                    None,
                )
                .expect("fenêtre d'essai")
            };
            let _ = rendue.send(fenetre.0 as isize);
            let _ = recu.recv(); // dort sans jamais pomper
        });
        let fenetre = HWND(attente.recv().expect("fenêtre d'essai") as _);
        (fenetre, fin, fil)
    }

    /// Une fenêtre qui pompe est dite vivante, et tout de suite. C'est aussi
    /// l'essai qui fixe la lecture de la valeur de retour : `repond` rend vrai
    /// parce que `SendMessageTimeout` rend **1**, pas parce que le résultat du
    /// message (`lpdwresult`, toujours 0 pour `WM_NULL`) serait non nul.
    #[test]
    fn une_fenetre_vivante_repond() {
        let (fenetre, proprietaire, fil) = fenetre_vivante();
        let debut = Instant::now();
        assert!(repond(fenetre, DELAI_REPONSE));
        assert!(debut.elapsed() < Duration::from_secs(1));
        unsafe {
            let _ = PostThreadMessageW(proprietaire, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        let _ = fil.join();
    }

    /// ⛔ **L'ESSAI QUI COMPTE.** Une fenêtre figée ne fait plus attendre ni la
    /// garde ni le déplacement. Sans la garde, l'appel synchrone attendait
    /// qu'elle lise — jusqu'à 29 s mesurées. ⚠️ Les DEUX appels tournent sur un
    /// fil à part : un retour en arrière doit faire ÉCHOUER l'essai au lieu de
    /// le laisser pendre indéfiniment.
    #[test]
    fn une_fenetre_muette_ne_fait_attendre_personne() {
        let (fenetre, fin, fil) = fenetre_muette();

        let (dite, ecoute) = mpsc::channel::<bool>();
        let brut = fenetre.0 as isize;
        let debut = Instant::now();
        std::thread::spawn(move || {
            let _ = dite.send(repond(HWND(brut as _), Duration::from_millis(100)));
        });
        let vivante = ecoute
            .recv_timeout(Duration::from_secs(1))
            .expect("la garde a attendu une fenêtre qui ne pompe pas");
        let attente = debut.elapsed();
        assert!(!vivante, "une fenêtre qui ne pompe pas ne répond pas");
        assert!(attente < Duration::from_secs(1), "la garde a attendu {attente:?}");

        let (fait, recu) = mpsc::channel::<()>();
        let brut = fenetre.0 as isize;
        std::thread::spawn(move || {
            assert!(deplacer_sans_attendre(HWND(brut as _), 0, 0, 50, 50));
            let _ = fait.send(());
        });
        assert!(
            recu.recv_timeout(Duration::from_secs(1)).is_ok(),
            "le déplacement a attendu une fenêtre qui ne pompe pas"
        );

        drop(fin);
        let _ = fil.join();
    }
}
