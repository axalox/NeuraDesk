//! **OUVRIR LE CENTRE DE NOTIFICATIONS DE WINDOWS.**
//!
//! DEMANDE EXPLICITE (2026-10-04) : « une applet pour afficher les
//! notifications ».
//!
//! Une ACTION, jumelle des fonctionnalités facultatives (voir
//! `win32::fonctionnalites_facultatives`) : le volet se désigne par son URI
//! `ms-actioncenter:`, confié à `explorer.exe`, qui résout le protocole et ouvre
//! le centre de notifications (celui de Win+N) — sans simuler la touche, ce que
//! le dépôt s'interdit (c'est le bureau réel). Pas d'élévation.
//!
//! Les notifications ne sont PAS relues ici : `win32::notification` lit leur
//! base en lecture seule pour l'animation des icônes, mais seul le volet de
//! Windows sait les afficher et y répondre.

use std::process::Command;

/// Le centre de notifications.
const URI: &str = "ms-actioncenter:";

/// Ouvre le centre de notifications.
///
/// Best-effort assumé, comme `fonctionnalites_facultatives::ouvrir` : un échec de
/// `spawn` ne remonte pas, mais il est JOURNALISÉ — sans quoi un clic sans effet
/// serait muet.
pub fn ouvrir() {
    crate::diag!("centre de notifications : ouverture de {URI}");
    if let Err(err) = Command::new("explorer.exe").arg(URI).spawn() {
        crate::diag!("centre de notifications : lancement impossible — {err}");
    }
}
