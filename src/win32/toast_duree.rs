//! **LA DURÉE DES NOTIFICATIONS DE WINDOWS — un réglage de NeuraDesk.**
//!
//! DEMANDE EXPLICITE (2026-11-02) : « pourrais-je régler la durée de la
//! notification ? », avec le choix explicite que NeuraDesk modifie AUSSI le
//! réglage de Windows pour les durées longues.
//!
//! # Deux leviers, et ils ne font pas la même chose
//!
//! 1. **Le réglage de Windows** (`SPI_SETMESSAGEDURATION`, Paramètres >
//!    Accessibilité > Effets visuels) — 5 s au moins. ⚠️ **GLOBAL ET PERMANENT** :
//!    il survit à la fermeture de NeuraDesk, et c'est le choix de l'utilisateur
//!    (l'indice des Réglages le dit). Posé SEULEMENT quand il choisit 5 s ou plus,
//!    jamais au lancement.
//! 2. **La fermeture par NeuraDesk** — au bout du délai, le bouton de fermeture de
//!    chaque toast de la pile est déclenché par UI Automation (`DismissButton`,
//!    mesuré le 2026-11-02 : bannière disparue en 667 ms, notification gardée au
//!    centre). C'est elle qui permet MOINS de 5 s, et qui rattrape les toasts qui
//!    ignorent le réglage de Windows (rappels). `WM_CLOSE` est ignoré par le shell.
//!
//! # ⭐ Le survol suspend le compte
//!
//! Pointeur sur la bannière, le compte repart de zéro : les boutons
//! (« Répondre »…) restent utilisables aussi longtemps qu'il le faut. Et un toast
//! EMPILÉ sur le précédent (la bannière grandit) a droit à son propre délai.
//! Les deux règles viennent de `toast-auto-dismiss` (Windhawk).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationInvokePattern, TreeScope_Descendants,
    UIA_InvokePatternId,
};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId, IsWindow, SetForegroundWindow, SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_GETMESSAGEDURATION,
    SPI_SETMESSAGEDURATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};
use windows_core::Interface;

/// Le fichier du réglage. Absent ou `0` : « comme Windows », NeuraDesk ne ferme rien.
pub const REGLAGE: &str = "duree_notifications";

/// **LES DURÉES PROPOSÉES**, en secondes. À partir de 5, ce sont EXACTEMENT les
/// valeurs du menu de Windows : une autre valeur y apparaîtrait vide.
pub const DUREES: [u32; 9] = [2, 3, 4, 5, 7, 15, 30, 60, 300];

/// Le plancher du réglage de Windows : en dessous, seul NeuraDesk agit.
pub const PLANCHER_WINDOWS: u32 = 5;

/// Entre deux tentatives de fermeture qui ont échoué.
const REESSAI_MS: u64 = 1_000;

/// Délai ajouté à la durée quand Windows l'applique déjà lui-même : NeuraDesk ne
/// ferme alors que les toasts qui l'ignorent (rappels) — voir `delai_ms`.
const RATTRAPAGE_MS: u64 = 3_000;

/// **PUR — AU BOUT DE COMBIEN DE TEMPS NEURADESK FERME-T-IL LUI-MÊME ?**
///
/// ⛔ **FERMER UN TOAST PAR SON ✕ LUI DONNE LE PREMIER PLAN** — DÉFAUT SIGNALÉ
/// (2026-11-18) : « quand il y a des notifications je ne peux plus interagir
/// avec les programmes et ça ferme les dashs ». MESURÉ (sonde
/// `GetForegroundWindow` toutes les 20 ms, toast de test, durée 7 s) : le
/// `CoreWindow` de `ShellExperienceHost` prend le premier plan À L'INSTANT de
/// notre `Invoke` sur `DismissButton`, 7,2 s après l'apparition, jamais avant.
/// Le programme perd le clavier, et nos panneaux se ferment (perte de focus).
///
/// Or à 5 s et plus, Windows applique LUI-MÊME la durée (`SPI_SETMESSAGEDURATION`,
/// posé par `choisir`) et ferme sans rien activer : notre fermeture ne sert plus
/// qu'aux toasts qui ignorent le réglage, d'où le rattrapage.
pub(crate) fn delai_ms(duree_s: u32, duree_windows_s: Option<u32>) -> u64 {
    let delai = u64::from(duree_s) * 1000;
    if duree_s >= PLANCHER_WINDOWS && duree_windows_s == Some(duree_s) {
        delai + RATTRAPAGE_MS
    } else {
        delai
    }
}

static DUREE_S: AtomicU32 = AtomicU32::new(0);
static COMPTE: Mutex<Option<Compte>> = Mutex::new(None);

/// **AU LANCEMENT** — lit le réglage, sans rien poser dans Windows (il y est
/// déjà, depuis le choix).
pub fn charger() {
    let duree = crate::config::load_u32_setting(REGLAGE).filter(|d| DUREES.contains(d)).unwrap_or(0);
    DUREE_S.store(duree, Ordering::SeqCst);
    if duree > 0 {
        crate::win32::toast_natif::assurer_le_crochet();
    }
}

/// La durée choisie dans NeuraDesk, `0` pour « comme Windows ».
pub fn duree() -> u32 {
    DUREE_S.load(Ordering::Relaxed)
}

/// **LA DURÉE QUE WINDOWS APPLIQUE**, lue — `None` si l'appel échoue.
pub fn duree_windows() -> Option<u32> {
    let mut secondes = 0u32;
    unsafe {
        SystemParametersInfoW(
            SPI_GETMESSAGEDURATION,
            0,
            Some(&mut secondes as *mut u32 as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .ok()
    .map(|()| secondes)
}

/// **LE CHOIX DE L'UTILISATEUR** — `false` pour une valeur inconnue, et rien
/// n'est écrit dans ce cas.
///
/// ⚠️ **À 5 S OU PLUS, LE RÉGLAGE DE WINDOWS EST POSÉ**, et RELU : la trace dit
/// ce que Windows a gardé, pas ce qu'on lui a demandé.
pub fn choisir(duree: u32) -> bool {
    if duree != 0 && !DUREES.contains(&duree) {
        return false;
    }
    crate::config::save_u32_setting(REGLAGE, duree);
    DUREE_S.store(duree, Ordering::SeqCst);
    if duree >= PLANCHER_WINDOWS {
        // ⚠️ **SUR UN FIL À PART** : `SPIF_SENDCHANGE` diffuse `WM_SETTINGCHANGE` à
        // TOUTES les fenêtres et attend — mesuré le 2026-11-02 : 1,6 s de gel de
        // l'interface quand l'appel partait du fil de la page.
        // `SPI_SETMESSAGEDURATION` prend la durée DANS `pvParam`, pas derrière un
        // pointeur (documentation de `SystemParametersInfo`).
        let _ = std::thread::Builder::new().name("toast-duree".into()).spawn(move || {
            let pose = unsafe {
                SystemParametersInfoW(
                    SPI_SETMESSAGEDURATION,
                    0,
                    Some(duree as usize as *mut _),
                    SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
                )
            };
            crate::diag!("[toast-duree] Windows : {duree} s demandees ({pose:?}), relu {:?}", duree_windows());
        });
    }
    if duree > 0 {
        crate::win32::toast_natif::assurer_le_crochet();
    }
    crate::diag!("[toast-duree] duree choisie : {duree} s");
    true
}

// ---------------------------------------------------------------------------
// LE COMPTE À REBOURS — pur, testé
// ---------------------------------------------------------------------------

/// Ce qu'on sait de la bannière suivie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Compte {
    hwnd: isize,
    hauteur: i32,
    depuis_ms: u64,
    essais: u32,
    dernier_essai_ms: u64,
}

/// Ce que le tic observe : la bannière (identité, hauteur, survol), ou rien.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Vue {
    pub hwnd: isize,
    pub hauteur: i32,
    pub survolee: bool,
}

/// **FAUT-IL FERMER MAINTENANT ?** — met `compte` à jour et rend `true` quand
/// la pile doit être fermée. L'appelant rapporte le succès par `fermee`.
pub(crate) fn decider(compte: &mut Option<Compte>, vue: Option<Vue>, maintenant_ms: u64, duree_ms: u64) -> bool {
    let Some(vue) = vue else {
        *compte = None;
        return false;
    };
    if duree_ms == 0 {
        *compte = None;
        return false;
    }
    let c = match compte {
        Some(c) if c.hwnd == vue.hwnd => c,
        _ => {
            *compte = Some(Compte { hwnd: vue.hwnd, hauteur: vue.hauteur, depuis_ms: maintenant_ms, essais: 0, dernier_essai_ms: 0 });
            return false;
        }
    };
    // Un toast EMPILÉ (la bannière grandit) a droit à son propre délai.
    if vue.hauteur > c.hauteur {
        c.depuis_ms = maintenant_ms;
        c.essais = 0;
    }
    c.hauteur = vue.hauteur;
    if vue.survolee {
        c.depuis_ms = maintenant_ms;
        return false;
    }
    if maintenant_ms.saturating_sub(c.depuis_ms) < duree_ms {
        return false;
    }
    if c.essais > 0 && maintenant_ms.saturating_sub(c.dernier_essai_ms) < REESSAI_MS {
        return false;
    }
    c.essais += 1;
    c.dernier_essai_ms = maintenant_ms;
    true
}

/// La pile est fermée : un toast arrivé PENDANT la fermeture garde la fenêtre
/// ouverte et repart avec un délai entier.
pub(crate) fn fermee(compte: &mut Option<Compte>, maintenant_ms: u64) {
    if let Some(c) = compte {
        c.depuis_ms = maintenant_ms;
        c.essais = 0;
    }
}

// ---------------------------------------------------------------------------
// LE TIC — appelé par le fil de `toast_natif`, quatre fois par seconde
// ---------------------------------------------------------------------------

thread_local! {
    /// UI Automation, créé au premier besoin SUR LE FIL du crochet (STA).
    static UIA: Option<IUIAutomation> = unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        windows::Win32::System::Com::CoCreateInstance(&CUIAutomation, None, windows::Win32::System::Com::CLSCTX_INPROC_SERVER).ok()
    };
}

/// `banniere` : la fenêtre et son rectangle si elle est encore une bannière.
pub(crate) fn tic(banniere: Option<(HWND, RECT)>) {
    let maintenant = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() };
    let vue = banniere.map(|(hwnd, rect)| Vue { hwnd: hwnd.0 as isize, hauteur: rect.bottom - rect.top, survolee: survolee(&rect) });
    let mut compte = COMPTE.lock().unwrap_or_else(|e| e.into_inner());
    // Lu à la DEMANDE (et pas mis en cache) : l'utilisateur peut changer la
    // durée dans les Paramètres de Windows. Seulement quand une bannière est là.
    let duree_windows = if vue.is_some() { duree_windows() } else { None };
    if !decider(&mut compte, vue, maintenant, delai_ms(duree(), duree_windows)) {
        return;
    }
    let Some((hwnd, _)) = banniere else { return };
    let avant = unsafe { GetForegroundWindow() };
    let fermes = fermer_la_pile(hwnd);
    rendre_le_premier_plan(avant, hwnd);
    crate::diag!("[toast-duree] fermeture apres {} s : {fermes} toast(s)", duree());
    if fermes > 0 {
        fermee(&mut compte, maintenant);
    }
}

/// Si notre fermeture a donné le premier plan au toast (voir `delai_ms`), le
/// rend à la fenêtre qui l'avait. ⚠️ Windows refuse `SetForegroundWindow` à un
/// processus qui n'est pas au premier plan : on s'attache, le temps de l'appel,
/// à la file d'entrée du fil qui l'a pris — même technique que
/// `cef_popup::force_foreground`, sans son `HWND_TOP` ni son `SetFocus` (on rend
/// le premier plan, on ne remonte rien).
fn rendre_le_premier_plan(avant: HWND, toast: HWND) {
    // Laisse au shell le temps de traiter le clic sur le ✕.
    std::thread::sleep(std::time::Duration::from_millis(60));
    unsafe {
        let apres = GetForegroundWindow();
        if avant.is_invalid() || apres == avant || !IsWindow(Some(avant)).as_bool() {
            return;
        }
        // Seulement si c'est le TOAST qui l'a pris (même fil que la bannière) :
        // un premier plan changé par l'utilisateur entre-temps ne se touche pas.
        let fil_toast = GetWindowThreadProcessId(toast, None);
        let fil_apres = GetWindowThreadProcessId(apres, None);
        if fil_apres == 0 || fil_apres != fil_toast {
            return;
        }
        let nous = GetCurrentThreadId();
        let attache = AttachThreadInput(nous, fil_apres, true).as_bool();
        let rendu = SetForegroundWindow(avant).as_bool();
        if attache {
            let _ = AttachThreadInput(nous, fil_apres, false);
        }
        crate::diag!("[toast-duree] premier plan rendu a {:#x} : {rendu}", avant.0 as usize);
    }
}

fn survolee(rect: &RECT) -> bool {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.is_ok()
        && (rect.left..rect.right).contains(&point.x)
        && (rect.top..rect.bottom).contains(&point.y)
}

/// L'identifiant UIA du ✕ d'un toast (mesuré par la sonde).
const BOUTON_FERMER: &str = "DismissButton";

/// **FERME TOUS LES TOASTS DE LA PILE** par leur propre bouton — rend combien.
fn fermer_la_pile(hwnd: HWND) -> usize {
    UIA.with(|uia| {
        let Some(uia) = uia else { return 0 };
        let Ok(racine) = (unsafe { uia.ElementFromHandle(hwnd) }) else { return 0 };
        let Ok(condition) = (unsafe { uia.CreateTrueCondition() }) else { return 0 };
        let Ok(boutons) = (unsafe { racine.FindAll(TreeScope_Descendants, &condition) }) else { return 0 };
        let mut fermes = 0;
        for rang in 0..unsafe { boutons.Length() }.unwrap_or(0) {
            let Ok(bouton) = (unsafe { boutons.GetElement(rang) }) else { continue };
            if unsafe { bouton.CurrentAutomationId() }.map(|id| id.to_string()).as_deref() != Ok(BOUTON_FERMER) {
                continue;
            }
            let invoque = unsafe { bouton.GetCurrentPattern(UIA_InvokePatternId) }
                .and_then(|motif| motif.cast::<IUIAutomationInvokePattern>())
                .and_then(|motif| unsafe { motif.Invoke() });
            if invoque.is_ok() {
                fermes += 1;
            }
        }
        fermes
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vue(hwnd: isize, hauteur: i32, survolee: bool) -> Option<Vue> {
        Some(Vue { hwnd, hauteur, survolee })
    }

    #[test]
    fn la_banniere_se_ferme_apres_la_duree_et_pas_avant() {
        let mut c = None;
        assert!(!decider(&mut c, vue(1, 165, false), 0, 3_000));
        assert!(!decider(&mut c, vue(1, 165, false), 2_999, 3_000));
        assert!(decider(&mut c, vue(1, 165, false), 3_000, 3_000));
    }

    #[test]
    fn le_survol_repousse_la_fermeture() {
        let mut c = None;
        decider(&mut c, vue(1, 165, false), 0, 3_000);
        assert!(!decider(&mut c, vue(1, 165, true), 2_500, 3_000));
        assert!(!decider(&mut c, vue(1, 165, false), 5_000, 3_000));
        assert!(decider(&mut c, vue(1, 165, false), 5_500, 3_000));
    }

    #[test]
    fn un_toast_empile_a_son_propre_delai() {
        let mut c = None;
        decider(&mut c, vue(1, 165, false), 0, 3_000);
        assert!(!decider(&mut c, vue(1, 330, false), 2_000, 3_000));
        assert!(!decider(&mut c, vue(1, 330, false), 4_000, 3_000));
        assert!(decider(&mut c, vue(1, 330, false), 5_000, 3_000));
    }

    #[test]
    fn un_echec_est_reessaye_une_seconde_plus_tard() {
        let mut c = None;
        decider(&mut c, vue(1, 165, false), 0, 2_000);
        assert!(decider(&mut c, vue(1, 165, false), 2_000, 2_000));
        assert!(!decider(&mut c, vue(1, 165, false), 2_500, 2_000));
        assert!(decider(&mut c, vue(1, 165, false), 3_000, 2_000));
    }

    #[test]
    fn apres_une_fermeture_reussie_le_delai_repart() {
        let mut c = None;
        decider(&mut c, vue(1, 165, false), 0, 2_000);
        assert!(decider(&mut c, vue(1, 165, false), 2_000, 2_000));
        fermee(&mut c, 2_000);
        assert!(!decider(&mut c, vue(1, 165, false), 3_500, 2_000));
        assert!(decider(&mut c, vue(1, 165, false), 4_000, 2_000));
    }

    /// « Comme Windows » ne ferme jamais rien, et une bannière disparue oublie tout.
    #[test]
    fn sans_duree_ou_sans_banniere_rien_ne_se_ferme() {
        let mut c = None;
        assert!(!decider(&mut c, vue(1, 165, false), 0, 0));
        assert!(!decider(&mut c, vue(1, 165, false), 99_000, 0));
        decider(&mut c, vue(1, 165, false), 0, 2_000);
        assert!(!decider(&mut c, None, 5_000, 2_000));
        assert_eq!(c, None);
    }

    /// ⛔ Le défaut signalé : fermer nous-mêmes un toast que Windows ferme déjà
    /// lui VOLAIT le premier plan. Quand Windows applique la durée, NeuraDesk ne
    /// ferme qu'en RATTRAPAGE (rappels qui l'ignorent).
    #[test]
    fn quand_windows_applique_la_duree_neuradesk_ne_ferme_qu_en_rattrapage() {
        assert_eq!(delai_ms(7, Some(7)), 7_000 + RATTRAPAGE_MS);
        assert_eq!(delai_ms(5, Some(5)), 5_000 + RATTRAPAGE_MS);
        // Windows réglé autrement (changé hors de NeuraDesk) : on ferme à l'heure.
        assert_eq!(delai_ms(7, Some(5)), 7_000);
        assert_eq!(delai_ms(7, None), 7_000);
        // Sous 5 s, Windows ne sait pas faire : NeuraDesk seul.
        assert_eq!(delai_ms(3, Some(5)), 3_000);
        assert_eq!(delai_ms(0, Some(7)), 0, "« comme Windows »");
    }

    /// À partir de 5 s, les durées sont celles du menu de Windows.
    #[test]
    fn les_durees_longues_sont_celles_de_windows() {
        let longues: Vec<u32> = DUREES.iter().copied().filter(|d| *d >= PLANCHER_WINDOWS).collect();
        assert_eq!(longues, vec![5, 7, 15, 30, 60, 300]);
    }
}
