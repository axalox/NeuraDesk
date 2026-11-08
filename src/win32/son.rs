//! **JOUER UN SON — la première lecture audio du dépôt.**
//!
//! DEMANDE EXPLICITE (2026-09-24) : « j'aimerais que l'alarme parmi les fichiers
//! du dossier son soit sélectionnable ». Avant cette date, l'application n'avait
//! **jamais émis un seul son** : aucun `PlaySound`, aucun `mciSendString`, aucune
//! balise `<audio>` nulle part. Tout ce qui suit est donc neuf.
//!
//! ⚠️ **POURQUOI MCI ET PAS `PlaySoundW`.** Les fichiers de l'utilisateur sont des
//! **`.mp3`** (huit sur neuf), or `PlaySoundW` est **WAV uniquement** — il ne les
//! lit pas. MCI (`mciSendStringW`, dans `winmm.dll`) est le seul mécanisme Windows
//! qui lise un MP3 depuis un chemin **sans ajouter de dépendance** (pas de `rodio`,
//! pas de `symphonia`, pas de CEF). C'est ce qui a décidé, et c'est aussi ce qui
//! fixe la limite : **MCI ne lit pas l'OGG**, d'où le filtrage de `fichiers()`.
//!
//! ⚠️ **DEUX ALIAS, ET CHACUN EST REFERMÉ AVANT CHAQUE OUVERTURE.** MCI garde
//! l'état du lecteur dans un « alias » nommé : rouvrir deux fois le même alias
//! échoue, et un alias laissé ouvert tient le fichier. `jouer` referme donc
//! d'abord, toujours.
//!
//! ⛔ **ET IL Y EN A DEUX PARCE QU'UN SEUL ÉTAIT UN DÉFAUT RÉEL.** La sonnerie et
//! l'écoute du panneau des Réglages sont **deux lectures indépendantes**, et
//! `app::run` coupe la première à chaque frame où il n'y a plus rien à signaler
//! (voir sa branche `else`). Partageant un alias, l'écoute était donc refermée un
//! soixantième de seconde après avoir commencé : « les boutons de lecture ne
//! semblent pas fonctionner », alors que `jouer` rendait `Ok` **sept fois de
//! suite** et que la sonde `sonde_mci_joue_vraiment` rapportait un mode
//! « playing » et une position qui AVANCE. **Tout était vrai, et rien ne
//! s'entendait.**
//!
//! ⚠️ **LE SON N'EST PAS COUPÉ QUAND ON LE RELANCE** : c'est `app::run` qui décide
//! (voir `joue_encore`), pas ce module — une alarme qui joue un son de quatre
//! minutes ne doit pas le reprendre toutes les trente secondes.

use std::path::{Path, PathBuf};

use windows::core::PCWSTR;
use windows::Win32::Media::Multimedia::{mciGetErrorStringW, mciSendStringW};

/// **L'ALIAS MCI DE LA SONNERIE** — celui que `app::run` déclenche, et qu'il coupe
/// dès qu'il n'y a plus rien à signaler. C'est aussi celui que `joue_encore`
/// interroge.
const ALIAS: &str = "neuradesk_alarme";

/// **L'ALIAS MCI DE L'ÉCOUTE** — celui du bouton « ▶ » du panneau des Réglages.
///
/// ⛔ **DEUX ALIAS, ET C'EST LA CORRECTION D'UN DÉFAUT RÉEL** (voir la doc de
/// module) : la branche d'`app::run` qui coupe la sonnerie tourne **à chaque
/// frame**, donc un alias partagé faisait durer l'écoute un soixantième de
/// seconde. Séparés, la sonnerie peut couper la sienne à tout moment sans toucher
/// à ce que l'utilisateur écoute — et une écoute en cours ne fait plus croire à
/// `joue_encore` qu'une alarme joue déjà.
const ALIAS_ECOUTE: &str = "neuradesk_ecoute";

/// Envoie une commande MCI et rend la réponse, ou l'erreur que Windows donne.
///
/// `retour` : la taille du tampon de réponse. MCI écrit dedans pour `status`.
fn envoyer(commande: &str, taille_retour: usize) -> Result<String, String> {
    let cmd: Vec<u16> = commande.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buffer = vec![0u16; taille_retour.max(1)];
    let code = unsafe {
        mciSendStringW(
            PCWSTR(cmd.as_ptr()),
            if taille_retour == 0 { None } else { Some(&mut buffer) },
            None,
        )
    };
    if code != 0 {
        return Err(erreur_mci(code));
    }
    let fin = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Ok(String::from_utf16_lossy(&buffer[..fin]))
}

/// Le message de Windows pour un code d'erreur MCI — jamais un code nu : « MCI
/// error 263 » n'apprend rien, « Le périphérique MCI ne peut pas lire ce
/// fichier » dit quoi corriger.
fn erreur_mci(code: u32) -> String {
    let mut buffer = [0u16; 256];
    let ok = unsafe { mciGetErrorStringW(code, &mut buffer) };
    let fin = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    let texte = String::from_utf16_lossy(&buffer[..fin]);
    if ok.as_bool() && !texte.trim().is_empty() {
        format!("{texte} (MCI {code})")
    } else {
        format!("erreur MCI {code}")
    }
}

/// Referme l'alias s'il est ouvert — **sans jamais échouer** : refermer ce qui
/// n'est pas ouvert est une erreur MCI normale, pas un problème pour l'appelant.
fn refermer(alias: &str) {
    let _ = envoyer(&format!("close {alias}"), 0);
}

/// **JOUE LE SON DE LA SONNERIE**, en remplaçant ce qui jouait sur cet alias.
/// Rend `Err` avec le message de Windows si le fichier est illisible (format non
/// pris en charge, chemin perdu).
pub fn jouer(chemin: &Path) -> Result<(), String> {
    jouer_sur(ALIAS, chemin)
}

/// **JOUE UN SON POUR L'ÉCOUTE** — le bouton « ▶ » des Réglages. Même mécanisme,
/// **autre alias** : c'est ce qui empêche la sonnerie de le couper (voir
/// `ALIAS_ECOUTE`, qui porte le défaut mesuré).
///
/// ⚠️ Refermer d'abord son PROPRE alias, c'est ce qui fait qu'un second appui
/// rejoue depuis le début — ce qu'un bouton d'écoute doit faire.
pub fn jouer_ecoute(chemin: &Path) -> Result<(), String> {
    jouer_sur(ALIAS_ECOUTE, chemin)
}

fn jouer_sur(alias: &str, chemin: &Path) -> Result<(), String> {
    refermer(alias);
    // ⚠️ **LE CHEMIN EST ENTRE GUILLEMETS** : les noms de fichiers de l'utilisateur
    // portent des espaces et des parenthèses (« lesiakower-lo-fi-alarm-clock
    // (1).mp3 »), et MCI s'arrête au premier espace sans eux.
    // `type mpegvideo` est ce qui fait décoder un MP3 — sans lui, MCI cherche un
    // périphérique par extension et refuse le fichier.
    let chemin = chemin.display().to_string().replace('\\', "/");
    envoyer(&format!("open \"{chemin}\" type mpegvideo alias {alias}"), 0)?;
    envoyer(&format!("play {alias}"), 0)?;
    Ok(())
}

/// **ARRÊTE LA SONNERIE** — l'alias de `app::run` uniquement. L'écoute de
/// l'utilisateur a le sien (voir `ALIAS_ECOUTE`) : ce n'est PAS à la sonnerie de
/// couper ce qu'il a demandé à entendre.
pub fn arreter() {
    refermer(ALIAS);
}

/// **LE SON JOUE-T-IL ENCORE ?** — la seule question que `app::run` pose, et la
/// raison pour laquelle le module existe : sans elle, une alarme relancerait le
/// son toutes les trente secondes, y compris par-dessus lui-même.
///
/// Rend `false` quand rien n'est ouvert : `status` échoue alors, et ce n'est pas
/// une anomalie.
pub fn joue_encore() -> bool {
    match envoyer(&format!("status {ALIAS} mode"), 64) {
        Ok(mode) => mode.trim().eq_ignore_ascii_case("playing"),
        Err(_) => false,
    }
}

/// **LE DOSSIER DES SONS** — à côté de l'exécutable, comme `assets/popups` (voir
/// `cef_popup.rs`, qui porte le pourquoi du `current_exe()` plutôt que du dossier
/// courant).
///
/// ⚠️ Il est rempli par `build.rs` : c'est lui qui copie `son/` à côté de l'exe,
/// sans filtre d'extension.
pub fn dossier() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(|d| d.join("son"))
}

/// **LES SONS PROPOSABLES** — triés par nom, et **filtrés sur ce que MCI sait
/// lire**.
///
/// ⚠️ **L'OGG EST EXCLU, ET C'EST DÉLIBÉRÉ** : proposer un son qui ne jouerait pas
/// serait pire que de ne pas le proposer — l'utilisateur choisirait un silence et
/// croirait l'alarme cassée. Le `.ogg` de l'utilisateur n'apparaît donc pas.
pub fn fichiers() -> Vec<PathBuf> {
    let Some(dossier) = dossier() else {
        return Vec::new();
    };
    let Ok(entrees) = std::fs::read_dir(&dossier) else {
        return Vec::new();
    };
    let mut liste: Vec<PathBuf> = entrees
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && est_lisible(p))
        .collect();
    liste.sort();
    liste
}

/// Les extensions que MCI lit ici. **`.mp3` et `.wav` seulement** — voir la doc de
/// module pour l'OGG.
fn est_lisible(chemin: &Path) -> bool {
    chemin
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("mp3") || e.eq_ignore_ascii_case("wav"))
}

/// Le nom d'un fichier, pour l'afficher dans le sélecteur.
pub fn nom(chemin: &Path) -> String {
    chemin.file_name().and_then(|n| n.to_str()).unwrap_or("(sans nom)").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **LE FILTRE D'EXTENSION EST LA SEULE CHOSE VRAIMENT TESTABLE ICI** — le reste
    /// parle à `winmm.dll`. Il mérite pourtant son test : c'est lui qui décide ce
    /// que l'utilisateur voit dans le sélecteur, et proposer un `.ogg` que MCI ne
    /// lit pas donnerait une alarme silencieuse.
    #[test]
    fn seuls_les_formats_que_mci_lit_sont_proposes() {
        for bon in ["a.mp3", "b.MP3", "c.wav", "d.WAV"] {
            assert!(est_lisible(Path::new(bon)), "{bon} devrait être proposé");
        }
        // ⚠️ L'OGG EST LE CAS RÉEL : c'est le fichier que l'utilisateur a
        // réellement dans son dossier `son/`, et c'est celui qu'on écarte.
        for mauvais in ["e.ogg", "f.OGG", "g.flac", "h.m4a", "i", "j.mp3.txt"] {
            assert!(!est_lisible(Path::new(mauvais)), "{mauvais} ne devrait PAS être proposé");
        }
    }

    /// **MCI SAIT-IL DÉCODER NOS FICHIERS ?** — la question qui décide de tout le
    /// module, et elle se pose **sans jouer le son** : `open` puis
    /// `status length` font lire le fichier par le décodeur et rendent sa durée,
    /// sans qu'un haut-parleur s'allume. C'est la seule mesure non intrusive
    /// possible sur une lecture audio.
    ///
    /// ⚠️ **Le dossier est celui du DÉPÔT** (`CARGO_MANIFEST_DIR`), pas celui à
    /// côté de l'exécutable de test — qui vit dans `target/<profil>/deps/` et n'a
    /// pas de `son/`. Le test ne dépend donc pas de `build.rs`.
    ///
    /// ⚠️ Il est **sauté** si le dossier est absent (dépôt fraîchement cloné) : le
    /// module n'a rien à prouver dans ce cas, et un test qui échoue pour une
    /// raison extérieure au code ne rend service à personne.
    #[test]
    fn mci_sait_decoder_les_fichiers_du_dossier_son() {
        let dossier = Path::new(env!("CARGO_MANIFEST_DIR")).join("son");
        let Ok(entrees) = std::fs::read_dir(&dossier) else {
            eprintln!("pas de dossier son/ dans le dépôt — test sauté");
            return;
        };
        let mut proposes: Vec<PathBuf> =
            entrees.flatten().map(|e| e.path()).filter(|p| p.is_file() && est_lisible(p)).collect();
        proposes.sort();
        assert!(!proposes.is_empty(), "son/ doit contenir au moins un .mp3 ou .wav");
        for chemin in proposes {
            refermer(ALIAS);
            let c = chemin.display().to_string().replace('\\', "/");
            let ouvert = envoyer(&format!("open \"{c}\" type mpegvideo alias {ALIAS}"), 0);
            assert!(ouvert.is_ok(), "MCI refuse « {} » : {:?}", nom(&chemin), ouvert);
            let duree = envoyer(&format!("status {ALIAS} length"), 64);
            assert!(duree.is_ok(), "durée illisible pour « {} » : {:?}", nom(&chemin), duree);
            // ⚠️ **LA DURÉE N'EST PAS UN DÉTAIL** : un fichier accepté mais de 0 ms
            // serait un silence — précisément ce que l'utilisateur prendrait pour
            // une alarme cassée.
            let ms: u64 = duree.unwrap().trim().parse().unwrap_or(0);
            assert!(ms > 0, "« {} » fait 0 ms", nom(&chemin));
        }
        refermer(ALIAS);
    }

    #[test]
    fn le_nom_affiche_est_le_nom_du_fichier() {
        assert_eq!(nom(Path::new("/x/y/ding.mp3")), "ding.mp3");
        assert_eq!(nom(Path::new("C:\\son\\lesiakower-lo-fi-alarm.mp3")), "lesiakower-lo-fi-alarm.mp3");
    }

    /// Quand rien ne joue, `status` échoue sur un alias inexistant — et ce n'est
    /// **pas** une anomalie : c'est la réponse attendue, et elle doit valoir
    /// « non », jamais une panique.
    #[test]
    fn rien_qui_joue_ne_fait_pas_echouer_la_question() {
        arreter();
        assert!(!joue_encore(), "après `arreter`, plus rien ne joue");
    }

    /// Le premier son du dossier DU DÉPÔT, et non celui de l'exécutable :
    /// `fichiers()` cherche `son/` à côté de l'exe, or le binaire de TEST vit dans
    /// `debug/deps/`, où `build.rs` ne copie rien. Même contenu, chemin
    /// déterministe — et c'est de toute façon le fichier que l'application joue.
    ///
    /// Partagé par les deux sondes ci-dessous, qui l'une et l'autre ÉMETTENT DU SON.
    fn premier_son_du_depot() -> PathBuf {
        let dossier = Path::new(env!("CARGO_MANIFEST_DIR")).join("son");
        let mut liste: Vec<PathBuf> = std::fs::read_dir(&dossier)
            .map(|entrees| {
                entrees
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_file() && est_lisible(p))
                    .collect()
            })
            .unwrap_or_default();
        liste.sort();
        liste.into_iter().next().unwrap_or_else(|| {
            panic!("aucun son lisible dans {} — la sonde n'a rien à mesurer", dossier.display())
        })
    }

    /// ⛔ **L'ÉCOUTE SURVIT À LA FERMETURE DE LA SONNERIE** — c'est le défaut RÉEL
    /// du 2026-09-24, et ce test est le seul endroit où il pouvait être attrapé
    /// sans oreilles.
    ///
    /// `app::run` coupe la sonnerie **à chaque frame** où il n'y a rien à signaler
    /// (sa branche `else` appelle `arreter()`). Avec un alias MCI PARTAGÉ, cela
    /// refermait aussi l'ÉCOUTE, un soixantième de seconde après son début : le
    /// bouton « ▶ » des Réglages paraissait mort alors que `jouer` rendait `Ok`.
    ///
    /// `#[ignore]` : elle ÉMET DU SON, comme la sonde ci-dessus.
    ///
    ///     cargo test --bin neuradesk l_ecoute_survit -- --ignored --nocapture
    #[test]
    #[ignore]
    fn l_ecoute_survit_a_la_fermeture_de_la_sonnerie() {
        let chemin = premier_son_du_depot();
        jouer_ecoute(&chemin).expect("l'écoute doit démarrer");
        // Ce qu'`app::run` fait soixante fois par seconde :
        arreter();
        let mode = envoyer(&format!("status {ALIAS_ECOUTE} mode"), 64).expect("l'alias d'écoute existe");
        refermer(ALIAS_ECOUTE);
        assert_eq!(mode.trim(), "playing", "la sonnerie ne doit pas couper l'écoute");
    }

    /// **SONDE — MCI JOUE-T-IL VRAIMENT, OU DIT-IL SEULEMENT OUI ?**
    ///
    /// Volontairement `#[ignore]` : **elle ÉMET DU SON**, ce que les tests de ce
    /// dépôt ne font jamais (voir la doc du module, et le test de lisibilité qui
    /// prouve la lecture SANS jouer). Elle se lance à la main :
    ///
    ///     cargo test --bin neuradesk sonde_mci -- --ignored --nocapture
    ///
    /// ⚠️ **ELLE EXISTE PARCE QU'UN DÉFAUT RÉEL EST RESTÉ INVISIBLE** : « les
    /// boutons de lecture ne semblent pas fonctionner » — alors que `jouer` avait
    /// rendu `Ok` SEPT FOIS de suite (mesuré au journal de l'application). `open`
    /// et `play` réussissent et rien ne sort : le test de lisibilité ne pouvait pas
    /// le voir, puisqu'il ne va jamais jusqu'à `play`.
    ///
    /// **Ce qu'elle mesure** : le MODE et la POSITION que MCI rapporte, plusieurs
    /// fois de suite. Une position qui AVANCE prouve que MCI décode vraiment ; une
    /// position figée pendant que le mode dit « playing » prouve le contraire —
    /// et c'est ce qui départage « MCI a dit oui » de « MCI joue ».
    #[test]
    #[ignore]
    fn sonde_mci_joue_vraiment() {
        let chemin = premier_son_du_depot();
        println!("fichier    : {}", chemin.display());
        jouer(&chemin).expect("`jouer` doit accepter un fichier du dossier");
        for tour in 0..6 {
            let mode = envoyer(&format!("status {ALIAS} mode"), 64).unwrap_or_else(|e| format!("<{e}>"));
            let position = envoyer(&format!("status {ALIAS} position"), 64).unwrap_or_else(|e| format!("<{e}>"));
            println!("  t={tour}  mode={:?}  position={:?}", mode.trim(), position.trim());
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        println!("  joue_encore = {}", joue_encore());
        arreter();
    }
}
