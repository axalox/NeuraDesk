//! **LE TEXTE D'UNE CAPTURE** — le bouton « Texte » de la capture d'écran
//! (`capture_ecran::Resultat::Texte`) : la zone part à la reconnaissance de
//! caractères, et son texte au presse-papiers.
//!
//! DEMANDE (2026-10-24), d'après Snip (« Capture Text ») : « 1 » dans la liste
//! de ses fonctions à reprendre.
//!
//! ⭐ **LES DEUX MOTEURS DE `win32::ocr`, CELUI DE WINDOWS D'ABORD** : il est
//! toujours là, alors que Tesseract vient avec NAPS2. S'il échoue (aucune
//! langue de reconnaissance installée, par exemple), Tesseract prend le relais ;
//! l'échec des deux est DIT au bandeau, jamais tu.
//!
//! ⭐ **ET DEPUIS LE 2026-11-04, LE MOTEUR PEUT ÊTRE CHOISI** — demande : « depuis
//! le dash du presse-papiers et le dash des notes ainsi que depuis le dash de
//! l'éditeur d'images pouvoir lancer une reconnaissance de texte en ayant le
//! choix entre naps2 et copilot ». Le choix ne fait **pas** de repli : celui qui
//! demande NAPS2 et reçoit le texte de Copilot croirait avoir comparé les deux.
//! Le repli reste celui du bouton de la capture, qui n'a jamais demandé de choix.
//!
//! La reconnaissance tourne hors du fil principal (`ocr::Reconnaissance`) ;
//! `poll`, à chaque frame, en relève le résultat.

use std::cell::RefCell;
use std::path::PathBuf;

use crate::win32::ocr::{self, Etat, Moteur, Reconnaissance};

/// Au-delà, l'aperçu du bandeau s'arrête sur « … ».
const APERCU_MAX: usize = 80;

/// Le titre des bandeaux quand la reconnaissance part d'un panneau (presse-
/// papiers, notes, éditeur) — la capture d'écran, elle, garde « Capture de
/// texte », qui dit d'où l'image vient.
pub const TITRE_PANNEAU: &str = "Reconnaissance de texte";

struct EnCours {
    reconnaissance: Reconnaissance,
    moteur: Moteur,
    image: PathBuf,
    /// Le titre du bandeau, gardé pour les messages qui arrivent plus tard
    /// (`poll`) : l'échec doit se dire sous le même nom que le départ.
    titre: &'static str,
    /// Le repli a-t-il été permis au départ ? Un échec relevé en `poll` doit
    /// suivre la même règle que celui du lancement.
    repli: bool,
}

thread_local! {
    static EN_COURS: RefCell<Option<EnCours>> = const { RefCell::new(None) };
}

/// Le fichier de l'image à reconnaître — un seul : une reconnaissance chasse la
/// précédente.
fn chemin_image() -> PathBuf {
    std::env::temp_dir().join("neuradesk-texte-capture.png")
}

/// **DÉMARRE LA RECONNAISSANCE D'UNE CAPTURE D'ÉCRAN** — le geste d'origine,
/// sans choix de moteur : le choix automatique (Copilot d'abord, NAPS2 en
/// relais), sous le titre « Capture de texte ».
pub fn lancer(png: &[u8]) {
    lancer_avec(png, None, "Capture de texte");
}

/// Le même départ, avec le moteur et le titre DITS.
///
/// `moteur` : `None` = le choix automatique (Copilot d'abord, NAPS2 en relais) ;
/// `Some(m)` = **exactement celui-là, sans repli** — voir la doc de module.
/// `titre` : le titre des bandeaux, qui dit d'où vient l'image.
fn lancer_avec(png: &[u8], moteur: Option<Moteur>, titre: &'static str) {
    let image = chemin_image();
    if let Err(erreur) = std::fs::write(&image, png) {
        crate::win32::bandeau::montrer(titre, &format!("Image impossible à écrire : {erreur}"));
        return;
    }
    demarrer(image, moteur.unwrap_or(Moteur::Windows), titre, moteur.is_none());
}

/// **UNE IMAGE DE PANNEAU PASSE À LA RECONNAISSANCE, LE MOTEUR AYANT ÉTÉ CHOISI**
/// — ce que demandent le presse-papiers, les notes et l'éditeur d'images.
///
/// `nom` est celui que la page envoie (`"tesseract"`, `"windows"` — voir
/// `ocr::Moteur::nom`), jamais son libellé : la page montre « NAPS2 » et
/// « Copilot », le verbe porte le nom stable. Rend `false` si le nom est
/// inconnu — l'appelant n'a rien d'autre à dire, le journal suffit.
pub fn lancer_choisi(nom: &str, png: &[u8]) -> bool {
    let Some(moteur) = Moteur::depuis_nom(nom) else {
        crate::diag!("[texte-capture] moteur de reconnaissance inconnu : {nom:?}");
        return false;
    };
    lancer_avec(png, Some(moteur), TITRE_PANNEAU);
    true
}

/// Lance `moteur` sur `image`, ou le suivant s'il ne peut pas commencer — et
/// seulement si le repli est permis.
fn demarrer(image: PathBuf, moteur: Moteur, titre: &'static str, repli: bool) {
    match Reconnaissance::lancer(&image, moteur) {
        Ok(reconnaissance) => {
            crate::diag!("[texte-capture] reconnaissance lancée ({})", moteur.nom());
            EN_COURS.with(|e| *e.borrow_mut() = Some(EnCours { reconnaissance, moteur, image, titre, repli }));
        }
        Err(message) => echouer(image, moteur, titre, &message, repli),
    }
}

/// `moteur` a échoué : on passe au suivant, ou on le dit.
fn echouer(image: PathBuf, moteur: Moteur, titre: &'static str, message: &str, repli: bool) {
    crate::diag!("[texte-capture] {} a échoué : {message}", moteur.nom());
    match suivant(moteur, repli) {
        Some(autre) => demarrer(image, autre, titre, repli),
        None => {
            let _ = std::fs::remove_file(&image);
            crate::win32::bandeau::montrer(titre, message);
        }
    }
}

/// L'ordre des moteurs — PUR, voir la doc de module. `repli: false` = le moteur
/// a été CHOISI : il n'y a pas de suivant.
fn suivant(moteur: Moteur, repli: bool) -> Option<Moteur> {
    if !repli {
        return None;
    }
    match moteur {
        Moteur::Windows => Some(Moteur::Tesseract),
        Moteur::Tesseract => None,
    }
}

/// **RELÈVE LA RECONNAISSANCE EN COURS** — à chaque frame, sur le fil principal.
pub fn poll() {
    let fini = EN_COURS.with(|e| {
        let mut garde = e.try_borrow_mut().ok()?;
        let etat = garde.as_mut()?.reconnaissance.relever()?;
        let en_cours = garde.take()?;
        Some((etat, en_cours.moteur, en_cours.image, en_cours.titre, en_cours.repli))
    });
    // ⚠️ **APRÈS L'EMPRUNT** : `echouer` relance un moteur, donc réemprunte.
    let Some((etat, moteur, image, titre, repli)) = fini else { return };
    match etat {
        Etat::Pret(texte) => {
            let _ = std::fs::remove_file(&image);
            let message = annonce(&texte);
            if !texte.trim().is_empty() && !ocr::copier_le_texte(texte.trim()) {
                crate::win32::bandeau::montrer(titre, "Le presse-papiers est occupé par un autre programme.");
                return;
            }
            crate::diag!("[texte-capture] {} caractères reconnus ({})", texte.chars().count(), moteur.nom());
            crate::win32::bandeau::montrer(titre, &message);
        }
        // ⚠️ Un échec relevé ICI vient d'un moteur qui avait démarré : le repli
        // garde la permission qu'avait le départ. Le choix explicite n'en a
        // jamais eu, donc n'en reçoit pas.
        Etat::Echec(message) => echouer(image, moteur, titre, &message, repli),
        Etat::EnCours => {}
    }
}

/// Ce que dit le bandeau — PUR.
fn annonce(texte: &str) -> String {
    let texte = texte.trim();
    if texte.is_empty() {
        return "Aucun texte reconnu dans la zone.".to_string();
    }
    let ligne = texte.split_whitespace().collect::<Vec<_>>().join(" ");
    let apercu: String = ligne.chars().take(APERCU_MAX).collect();
    let suite = if ligne.chars().count() > APERCU_MAX { "…" } else { "" };
    format!("Texte copié : « {apercu}{suite} »")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_d_abord_puis_tesseract_puis_rien() {
        assert_eq!(suivant(Moteur::Windows, true), Some(Moteur::Tesseract));
        assert_eq!(suivant(Moteur::Tesseract, true), None);
    }

    /// ⛔ **UN MOTEUR CHOISI NE SE REPLIE PAS.** Celui qui demande NAPS2 et
    /// reçoit le texte de Copilot croirait avoir comparé les deux moteurs : le
    /// repli automatique n'appartient qu'au bouton de la capture.
    #[test]
    fn un_moteur_choisi_ne_se_replie_jamais() {
        for moteur in [Moteur::Windows, Moteur::Tesseract] {
            assert_eq!(suivant(moteur, false), None, "{moteur:?} s'est replié");
        }
    }

    #[test]
    fn l_annonce_resume_le_texte_sur_une_ligne() {
        assert_eq!(annonce("  \n "), "Aucun texte reconnu dans la zone.");
        assert_eq!(annonce("Bonjour\n  le monde "), "Texte copié : « Bonjour le monde »");
        let long = "a".repeat(APERCU_MAX + 5);
        assert!(annonce(&long).ends_with("… »"));
    }
}
