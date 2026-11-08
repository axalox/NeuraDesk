//! **OÙ L'ON ÉCRIT** — la dernière fenêtre ÉTRANGÈRE au premier plan, et la frappe
//! qui s'y adresse.
//!
//! ⭐ **CE MODULE EXISTE PARCE QU'IL Y A MAINTENANT DEUX FENÊTRES QUI ÉCRIVENT** : les
//! caractères accentués (`cef_caracteres`) et les kaomojis (`cef_kaomoji`). La cible
//! est une propriété du **BUREAU** — « dans quelle fenêtre on écrit » ne dépend pas de
//! la fenêtre d'où l'on clique — donc elle a **un seul traqueur**, ici. Deux copies
//! auraient voulu dire deux réponses à la même question, et une divergence à la
//! première retouche.
//!
//! ## Pourquoi il faut RETENIR la cible, alors que ça semble inutile
//!
//! Les deux fenêtres PRENNENT LE FOCUS (choix informé de l'utilisateur : il les a
//! voulues « comme la visionneuse »). Au moment du clic, c'est donc ELLES qui ont le
//! premier plan, et la fenêtre où l'utilisateur écrivait est déjà derrière. La seule
//! façon de la retrouver est de la noter **tant qu'on peut encore la voir** — à chaque
//! frame, depuis `app::run`.
//!
//! ⚠️ **CETTE VOIE EST CELLE QUE CE DÉPÔT A ABANDONNÉE AILLEURS**, pour une raison
//! mesurée (`cef_clipboard::handle_paste` : « le collage part alors dans la fenêtre
//! élue par la restauration, pas dans celle que l'utilisateur regarde »). Elle est
//! moins sûre que le non-focus, et c'est assumé : **le CLIC DROIT — la copie — est la
//! voie garantie.** Si la frappe n'arrive pas, elle n'arrive pas *en silence*, et le
//! journal le dit.
//!
//! ⚠️ **ET WINDOWS EN REFUSE UNE ENTRÉE SUR SORTE** : un processus non élevé ne peut
//! pas frapper dans une fenêtre ÉLEVÉE (les onglets administrateur du Terminal, entre
//! autres). Ce refus est silencieux lui aussi — d'où le clic droit.

use std::cell::Cell;

use windows::Win32::Foundation::HWND;

thread_local! {
    /// **LA DERNIÈRE FENÊTRE ÉTRANGÈRE AU PREMIER PLAN** — celle où l'utilisateur
    /// écrivait avant de venir dans l'une de nos deux fenêtres. C'est la SEULE cible
    /// plausible d'une frappe, et elle se note à chaque frame.
    static CIBLE: Cell<Option<HWND>> = const { Cell::new(None) };
}

/// **CE QUI A LE PREMIER PLAN, QUAND CE N'EST PAS NOUS.** À appeler à CHAQUE frame
/// (voir `app::run`) — c'est la cible d'une frappe, et elle se note tant qu'on peut
/// encore la voir.
///
/// ⚠️ **ON IGNORE NOS PROPRES FENÊTRES** : sans ce filtre, la cible serait tour à tour
/// notre barre, notre panneau, notre infobulle — et la frappe partirait dans
/// l'application NeuraDesk au lieu de celle de l'utilisateur. Même filtre que
/// `foreground_fullscreen_window`, et pour la même raison.
pub fn noter_la_derniere_fenetre_etrangere() {
    unsafe {
        let premier = windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow();
        if premier.is_invalid() {
            return;
        }
        let mut pid = 0u32;
        windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(premier, Some(&mut pid));
        if pid != std::process::id() {
            CIBLE.set(Some(premier));
        }
    }
}

/// **ÉCRIT UNE CHAÎNE DANS LA FENÊTRE RETENUE**, en la remettant au premier plan le
/// temps de la frappe.
///
/// ⚠️ **UNE CHAÎNE, PAS UN CARACTÈRE** : un kaomoji fait jusqu'à vingt unités, et il
/// doit partir d'UN SEUL geste — vingt envois séparés laisseraient une autre
/// application s'intercaler, et le kaomoji arriverait en morceaux.
///
/// ⚠️ **`SetForegroundWindow` NE RÉUSSIT ICI QU'À UNE CONDITION** : que NOUS ayons le
/// premier plan au moment de l'appel — ce qui est justement le cas, l'utilisateur
/// vient de cliquer dans l'une de nos fenêtres. C'est précisément la situation où
/// Windows l'autorise ; ailleurs, il l'ignore en silence.
pub fn frapper_dans_la_cible(texte: &str) {
    let Some(cible) = CIBLE.get() else {
        crate::diag!("[frappe] aucune fenetre cible retenue — le clic droit copie, lui, toujours");
        return;
    };
    unsafe {
        // ⚠️ Le résultat est IGNORÉ À DESSEIN : Windows refuse sans le dire, et
        // `SendInput` juste après rendra zéro événement — c'est CE compte qui parle.
        let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(cible);
    }
    if !super::caracteres::frapper_texte(texte) {
        crate::diag!("[frappe] REFUSEE (fenetre elevee ?) — utiliser le clic droit");
    } else {
        crate::diag!("[frappe] envoyee : {texte:?}");
    }
}
