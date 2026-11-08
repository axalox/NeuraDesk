//! **OUVRIR LES PROPRIÉTÉS SYSTÈME DE WINDOWS** (`sysdm.cpl`).
//!
//! DEMANDE EXPLICITE (2026-10-08) : « j'aimerais une nouvelle applet dans
//! Intégrés pour ouvrir sysdm.cpl ».
//!
//! ⚠️ **LA ZONE A ÉTÉ CORRIGÉE LE MÊME JOUR** : « en fait déplace la nouvelle
//! applet dans Réglages ». L'applet se pose donc dans **Réglages**, après les
//! fonctionnalités facultatives (voir `app::run`, et `config::
//! PROPRIETES_SYSTEME_REGLAGES`, dont le nom porte la zone). Le module, lui, n'a
//! jamais rien su de la zone — c'est `app::run` qui la décide.
//!
//! Une ACTION, comme les réglages de l'écran et les fonctionnalités
//! facultatives (voir `win32::reglages_ecran`) : un clic ouvre la fenêtre,
//! rien à relire ni à repeindre.
//!
//! ## ⛔ POURQUOI `control.exe`, ET NON `explorer.exe`
//!
//! Les deux autres ACTION passent par `explorer.exe` — mais elles lui donnent
//! une **URI** (`ms-settings:…`, `ms-actioncenter:`), que le shell résout comme
//! un protocole. `sysdm.cpl` n'est pas un protocole : c'est un **composant
//! enfichable du Panneau de configuration**, et le programme dont c'est le
//! métier de l'ouvrir s'appelle `control.exe` (`Control_RunDLL`). Il vit dans
//! `System32`, donc toujours résoluble.
//!
//! ⛔ **L'ÉLÉVATION A ÉTÉ VÉRIFIÉE, PAS SUPPOSÉE** — c'est le piège qui a
//! coûté le Gestionnaire de périphériques (voir
//! `win32::gestionnaire_peripheriques`) : le manifeste EMBARQUÉ de
//! `C:\Windows\System32\control.exe` a été relu sur la machine, et il déclare
//! `requestedExecutionLevel level="asInvoker"`. Il n'y a donc **pas** de
//! `ERROR_ELEVATION_REQUIRED` (740) à craindre, et `Command::spawn` suffit —
//! inutile d'invoquer `ShellExecuteW` et son invite UAC.
//!
//! ⚠️ **`sysdm.cpl` SANS ARGUMENT** ouvre la fenêtre sur son onglet par défaut
//! (`sysdm.cpl,,N` choisit un onglet : 0 = nom de l'ordinateur, 2 = matériel,
//! 3 = avancé, 4 = protection du système, 5 = accès distant). Aucun n'est
//! demandé : c'est la fenêtre entière que l'applet doit donner.

use std::process::Command;

/// Le composant enfichable du Panneau de configuration à ouvrir.
const COMPOSANT: &str = "sysdm.cpl";

/// Ouvre les Propriétés système.
///
/// Best-effort assumé, comme `reglages_ecran::ouvrir` : un échec de `spawn` ne
/// remonte pas, mais il est JOURNALISÉ — sans quoi un clic sans effet serait muet.
pub fn ouvrir() {
    crate::diag!("propriétés système : ouverture de {COMPOSANT}");
    if let Err(err) = Command::new("control.exe").arg(COMPOSANT).spawn() {
        crate::diag!("propriétés système : lancement impossible — {err}");
    }
}
