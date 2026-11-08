//! « Afficher le bureau » — cache toutes les fenêtres ouvertes pour montrer le
//! bureau, ou les rétablit (c'est une BASCULE). Même action que le bouton du
//! bord droit de la barre des tâches et que Win+D. Demandé pour l'icône de
//! l'extrémité gauche de la barre du haut (voir `ui::topbar::show_desktop_rect`
//! et son site de dessin dans `app::run`).
//!
//! **C'est l'API DOCUMENTÉE qui a décidé du choix** : `IShellDispatch4::
//! ToggleDesktop`
//! (<https://learn.microsoft.com/windows/win32/shell/ishelldispatch4-toggledesktop>)
//! — « It either hides all open windows to show the desktop or it hides the
//! desktop by showing all open windows ». L'alternative aurait été d'envoyer
//! Win+D : ce projet s'autorise déjà la synthèse d'entrée pour une action
//! déclenchée par un VRAI clic (voir `win32::desktops`, doc de module, qui en
//! donne la justification), mais quand une API documentée fait exactement la
//! même chose, c'est elle qu'on prend. **Rien n'est simulé ici.**
//!
//! **C'est une BASCULE D'ÉTAT, et l'état vit dans le shell, pas ici** : le
//! module ne sait pas si le bureau est actuellement affiché, il ne peut donc
//! être appelé que depuis un vrai clic — jamais par frame, jamais en boucle de
//! rattrapage. Corollaire à connaître : après un Win+D au clavier, un clic sur
//! l'icône RÉTABLIT les fenêtres (elle bascule, elle ne « cache » pas).
//!
//! COM : **rien à initialiser**. Le thread principal est déjà en STA (voir
//! `main`, doc de son `CoInitializeEx`) et c'est lui qui traite les clics ; le
//! serveur `Shell` est un serveur LOCAL, donc l'appel se marshalle et **bloque
//! ce thread** le temps que le shell réponde — quelques millisecondes, une fois
//! par clic. C'est précisément pourquoi ce module ne doit jamais être appelé
//! depuis la boucle de rendu.

use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
// `Shell` EST le `CLSID_Shell` de `Shldisp.h` : le crate `windows` l'expose sous
// le nom de sa coclasse, jamais sous celui de la constante C — chercher
// `CLSID_Shell` dans ses sources ne trouve donc RIEN, ce qui l'a fait croire
// absent et a failli faire redéclarer la valeur à la main (vérifié depuis :
// `pub const Shell: GUID = from_u128(0x13709620_c279_11ce_a49e_444553540000)`,
// soit exactement `{13709620-C279-11CE-A49E-444553540000}`, recoupé sur le
// `shell32_classes.idl` de Wine — coclass `Shell`, helpstring « Shell
// Automation Service », `InProcServer32` = `shell32.dll`, Apartment).
// Renommé à l'import pour que le site d'appel dise ce qu'il crée.
use windows::Win32::UI::Shell::{IShellDispatch4, Shell as CLSID_SHELL};

/// Bascule « afficher le bureau ».
///
/// Best-effort comme toutes les actions OS de ce dossier (voir `win32::power`,
/// doc de module) : un échec — création de l'objet COM ou bascule elle-même —
/// ne se signale pas à l'utilisateur, il n'y a rien de mieux à lui proposer, et
/// inventer ici un chemin d'erreur que personne ne consomme serait incohérent
/// avec le reste. Les deux sont donc avalés sur la même ligne.
pub fn toggle() {
    unsafe {
        let _ = CoCreateInstance::<_, IShellDispatch4>(&CLSID_SHELL, None, CLSCTX_ALL)
            .and_then(|shell| shell.ToggleDesktop());
    }
}
