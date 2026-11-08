//! **LA TEXTURE DU FOND DES TERMINAUX** — ⭐ DEMANDE EXPLICITE (2026-10-15) :
//! « appliquer des textures au fond des terminaux mais pas forcément la même
//! texture que pour le reste de l'app ».
//!
//! ⛔ **LE SHADER DES PANNEAUX N'Y PEUT RIEN** : le fond d'un terminal est peint
//! par SA PAGE (`.terminal-hote`, la couleur du thème à 92 %, ou la couleur pleine
//! choisie au nuancier), par-dessus le fond que dessine `cef_popup` — la texture de
//! l'interface, posée dessous, n'en perce que 8 %, ou rien.
//!
//! **D'OÙ UNE IMAGE, CALCULÉE PAR LE MÊME SHADER** : `texture_interface::
//! calque_rgba` rend la texture des terminaux (`texture_interface::TERMINAUX`)
//! seule, en alpha droit, à la taille de l'écran ; elle est écrite en PNG dans le
//! dossier temporaire, et la page la pose SUR la couleur de chaque terminal
//! (`poserTextureDesTerminaux`, dans `terminal_theme.js`), sous le texte — mêmes
//! natures, même échelle que l'interface, au pixel près.
//!
//! **REFAITE SEULEMENT QUAND ELLE CHANGE** — réglages, composition personnalisée
//! DES TERMINAUX (`texture_perso::TERMINAUX`, la leur depuis 2026-10-16)
//! (hors de sa phase d'animation : l'image est FIXE) ou taille de l'écran — et
//! après `ATTENTE` sans nouveau changement : un curseur glissé ne régénère pas une
//! image par pas. L'appel à la page est RÉPÉTÉ toutes les `RAPPEL` : une page
//! (re)chargée après coup le reçoit quand même.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::thread::LocalKey;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};

use crate::gfx::texture_interface::{self, TERMINAUX};
use crate::win32::cef_popup::{self, PopupSlot};

/// Le calme à attendre avant de refaire l'image.
const ATTENTE: Duration = Duration::from_millis(250);
/// La cadence à laquelle la page reçoit à nouveau l'image en cours.
const RAPPEL: Duration = Duration::from_secs(1);

/// Ce dont l'image dépend : les constantes de la texture (en bits, pour l'égalité),
/// celles de la composition personnalisée si c'est elle, et la taille de l'écran.
#[derive(Debug, Clone, PartialEq)]
struct Cle {
    constantes: Vec<u32>,
    taille: (u32, u32),
}

#[derive(Default)]
struct Etat {
    /// L'image posée, et ce dont elle dépend.
    posee: Option<(Cle, Option<PathBuf>)>,
    /// Une clé différente vue depuis…
    attendue: Option<(Cle, Instant)>,
    rappel: Option<Instant>,
    numero: u32,
}

thread_local! {
    /// Un état par page (`terminal`, `ssh`).
    static ETATS: RefCell<HashMap<&'static str, Etat>> = RefCell::new(HashMap::new());
}

/// À appeler à chaque frame, après le rendu de la page `nom` : sans changement,
/// ne fait que mesurer l'écran.
pub fn tenir_a_jour(slot: &'static LocalKey<PopupSlot>, nom: &'static str) {
    let Some(Some(taille)) = cef_popup::avec_gpu(slot, |_, _, hwnd| taille_de_l_ecran(hwnd)) else { return };
    let voulue = cle(taille);
    let a_envoyer = ETATS.with(|e| {
        let mut etats = e.borrow_mut();
        let etat = etats.entry(nom).or_default();
        if etat.posee.as_ref().is_some_and(|(cle, _)| *cle == voulue) {
            etat.attendue = None;
            if etat.rappel.is_some_and(|t| t.elapsed() < RAPPEL) {
                return None;
            }
            etat.rappel = Some(Instant::now());
            return Some(etat.posee.as_ref().and_then(|(_, chemin)| chemin.clone()));
        }
        match &etat.attendue {
            Some((cle, depuis)) if *cle == voulue && depuis.elapsed() >= ATTENTE => {}
            Some((cle, _)) if *cle == voulue => return None,
            _ => {
                etat.attendue = Some((voulue.clone(), Instant::now()));
                return None;
            }
        }
        // Refaire l'image — la précédente est effacée une fois remplacée.
        let ancienne = etat.posee.take().and_then(|(_, chemin)| chemin);
        if etat.numero == 0 {
            nettoyer_les_anciennes(nom);
        }
        etat.numero += 1;
        let chemin = generer(slot, nom, etat.numero, taille);
        if let Some(ancienne) = ancienne {
            let _ = std::fs::remove_file(ancienne);
        }
        etat.posee = Some((voulue, chemin.clone()));
        etat.attendue = None;
        etat.rappel = Some(Instant::now());
        Some(chemin)
    });
    if let Some(chemin) = a_envoyer {
        cef_popup::execute_js(slot, &js_de(chemin.as_deref(), taille));
    }
}

fn cle(taille: (u32, u32)) -> Cle {
    let mut constantes: Vec<u32> = TERMINAUX.constantes().iter().flatten().map(|v| v.to_bits()).collect();
    if texture_interface::NATURES[TERMINAUX.nature().min(texture_interface::NATURES.len() - 1)].0 == "perso" {
        let mut perso = crate::gfx::texture_perso::TERMINAUX.constantes();
        // La PHASE de l'animation avance à chaque frame : l'image est fixe.
        if let Some(dernier) = perso.last_mut() {
            dernier[3] = 0.0;
        }
        constantes.extend(perso.iter().flatten().map(|v| v.to_bits()));
    }
    Cle { constantes, taille }
}

/// La taille, en pixels physiques, de l'écran où est la fenêtre.
fn taille_de_l_ecran(hwnd: HWND) -> Option<(u32, u32)> {
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    let ecran = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    unsafe { GetMonitorInfoW(ecran, &mut info) }.as_bool().then_some(())?;
    let r = info.rcMonitor;
    let taille = ((r.right - r.left).max(0) as u32, (r.bottom - r.top).max(0) as u32);
    (taille.0 > 0 && taille.1 > 0).then_some(taille)
}

fn dossier() -> PathBuf {
    std::env::temp_dir().join("NeuraDesk")
}

/// Les images laissées par une session précédente.
fn nettoyer_les_anciennes(nom: &str) {
    let Ok(entrees) = std::fs::read_dir(dossier()) else { return };
    let prefixe = format!("texture-{nom}-");
    for entree in entrees.flatten() {
        if entree.file_name().to_string_lossy().starts_with(&prefixe) {
            let _ = std::fs::remove_file(entree.path());
        }
    }
}

/// L'image de la texture des terminaux, écrite en PNG. `None` : aucune texture,
/// ou un échec (journalisé).
fn generer(slot: &'static LocalKey<PopupSlot>, nom: &str, numero: u32, (largeur, hauteur): (u32, u32)) -> Option<PathBuf> {
    if TERMINAUX.nature() == 0 {
        return None;
    }
    let debut = Instant::now();
    let rgba = cef_popup::avec_gpu(slot, |device, ctx, _| texture_interface::calque_rgba(device, ctx, &TERMINAUX, &crate::gfx::texture_perso::TERMINAUX, largeur, hauteur)).flatten()?;
    let chemin = dossier().join(format!("texture-{nom}-{numero}.png"));
    if let Err(err) = ecrire_png(&chemin, &rgba, largeur, hauteur) {
        crate::diag!("[texture_terminaux] {} : {err}", chemin.display());
        return None;
    }
    crate::diag!("[texture_terminaux] {nom} : {largeur}×{hauteur} en {} ms", debut.elapsed().as_millis());
    Some(chemin)
}

fn ecrire_png(chemin: &Path, rgba: &[u8], largeur: u32, hauteur: u32) -> Result<(), String> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder;
    std::fs::create_dir_all(dossier()).map_err(|e| e.to_string())?;
    let fichier = std::fs::File::create(chemin).map_err(|e| e.to_string())?;
    let encodeur = PngEncoder::new_with_quality(std::io::BufWriter::new(fichier), CompressionType::Fast, FilterType::Sub);
    encodeur.write_image(rgba, largeur, hauteur, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())
}

/// L'URL `file:///` d'un chemin Windows, ses caractères gênants échappés.
fn url_de_fichier(chemin: &Path) -> String {
    let mut url = String::from("file:///");
    for c in chemin.to_string_lossy().chars() {
        match c {
            '\\' => url.push('/'),
            ' ' | '#' | '%' | '?' | '"' | '\'' | '(' | ')' => url.push_str(&format!("%{:02X}", c as u32)),
            c => url.push(c),
        }
    }
    url
}

/// L'appel de la page : l'image et sa taille en pixels physiques, ou `null`.
fn js_de(chemin: Option<&Path>, (largeur, hauteur): (u32, u32)) -> String {
    match chemin {
        Some(chemin) => format!(
            "window.poserTextureDesTerminaux && poserTextureDesTerminaux(\"{}\", {largeur}, {hauteur});",
            url_de_fichier(chemin)
        ),
        None => "window.poserTextureDesTerminaux && poserTextureDesTerminaux(null);".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un chemin Windows devient une URL que CSS accepte entre guillemets.
    #[test]
    fn le_chemin_devient_une_url_de_fichier() {
        let url = url_de_fichier(Path::new(r"C:\Users\Jean Dupont\AppData\Local\Temp\NeuraDesk\texture-ssh-2.png"));
        assert_eq!(url, "file:///C:/Users/Jean%20Dupont/AppData/Local/Temp/NeuraDesk/texture-ssh-2.png");
        assert_eq!(url_de_fichier(Path::new(r"C:\a'b(c)")), "file:///C:/a%27b%28c%29", "rien qui ferme la chaîne CSS");
    }

    /// La page reçoit l'image et sa taille PHYSIQUE, ou `null` pour l'effacer — et
    /// rien ne casse si `terminal_theme.js` n'est pas encore chargé.
    #[test]
    fn l_appel_de_la_page() {
        assert_eq!(
            js_de(Some(Path::new(r"C:\t\x.png")), (3840, 2160)),
            "window.poserTextureDesTerminaux && poserTextureDesTerminaux(\"file:///C:/t/x.png\", 3840, 2160);"
        );
        assert_eq!(js_de(None, (1, 1)), "window.poserTextureDesTerminaux && poserTextureDesTerminaux(null);");
    }

    /// La même taille d'écran donne la même clé (l'image n'est pas refaite) ; un
    /// autre écran, une autre.
    #[test]
    fn la_cle_suit_la_taille_de_l_ecran() {
        assert_eq!(cle((100, 50)), cle((100, 50)));
        assert_ne!(cle((100, 50)), cle((100, 51)), "un autre écran : une autre image");
    }
}
