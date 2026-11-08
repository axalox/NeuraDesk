//! **SONDE D'ÉTAPE 1 — PAYLOAD INJECTÉ DANS `explorer.exe`. LECTURE SEULE.**
//!
//! Elle ne déplace RIEN, ne réécrit RIEN, ne refuse RIEN. Elle observe et écrit
//! un journal. C'est délibéré : avant de changer quoi que ce soit dans le shell
//! de l'utilisateur, il faut savoir **si** et **quand** le shell positionne son
//! menu volant, et par quel chemin.
//!
//! # Pourquoi SOUS-CLASSER plutôt que crocheter `SetWindowPos`
//!
//! Un crochet de fonction (inline hook) patche des OCTETS DE CODE dans
//! `user32.dll` à l'intérieur d'explorer : il faut un trampoline, gérer la
//! réentrance, et une erreur d'un octet tue le shell. Le sous-classement obtient
//! la même information sans toucher une seule instruction :
//! `SetWindowLongPtrW(GWLP_WNDPROC)` détourne la **procédure de fenêtre**, et
//! Windows envoie `WM_WINDOWPOSCHANGING` **avant** d'appliquer une position —
//! donc exactement là où il faudra décider, à l'étape 2.
//!
//! # Ce qu'elle journalise
//!
//! Chaque `WM_WINDOWPOSCHANGING` reçu par une fenêtre de classe
//! `Xaml_WindowedPopupClass` : la position DEMANDÉE, la position ACTUELLE, et les
//! drapeaux. C'est ce qui dira si le shell reprend la pose lui-même (le message
//! le montrera) ou s'il passe par un autre chemin.
//!
//! # Les trois façons dont ce code peut mal tourner, et ce qui les couvre
//!
//! 1. **Une panique dans notre proc tuerait explorer** — `panic = "abort"` est
//!    absent du profil, et `subclass_proc` enveloppe tout dans `catch_unwind`.
//! 2. **Un proc de sous-classe orphelin** (notre DLL déchargée alors que la
//!    fenêtre pointe encore dessus) tuerait explorer à la première souris —
//!    d'où le recensement `SUBCLASSEES` et la remise en état AVANT tout
//!    déchargement.
//! 3. **Un verrou pris pendant que le proc d'origine est appelé** — réentrance
//!    par le même fil, donc interblocage. Le verrou est TOUJOURS relâché avant
//!    `CallWindowProcW` (voir `subclass_proc`).
//!
//! # ÉTAPE 2 : MESURÉE, ET ELLE S'ARRÊTE LÀ
//!
//! L'étape 2 réécrit `x`/`y` dans `WINDOWPOS` (voir `subclass_proc` et
//! `CIBLE`). **Mécaniquement, ça marche** : le journal montre la fenêtre atteindre
//! la cible imposée — `actuel (3366,1874)`, `actuel (2500,600)` — et le shell
//! lui-même redemande ensuite cette position à chaque réaffirmation.
//!
//! **Mais ce qui est DESSINÉ ne suit pas.** Test décisif : une cible à 400 px
//! au-dessus de la position du shell, donc un recouvrement **NUL par
//! construction** entre la fenêtre déplacée et le contenu resté à l'origine — et
//! le menu est apparu **à sa place habituelle**. La couche XAML compose ses
//! visuels **indépendamment du rectangle du `HWND`** : la fenêtre et le dessin
//! sont deux choses distinctes, et le journal comme l'œil disaient vrai tous les
//! deux.
//!
//! **Et l'effet de bord est pire que l'absence d'effet** : après manipulation, le
//! shell **cesse de rouvrir son menu** pour cette icône — même symptôme que la
//! voie `WM_CLOSE` mesurée côté NeuraDesk. La barre des tâches réelle, elle,
//! continue de fonctionner : le shell n'est pas abîmé, c'est notre demande qui est
//! refusée.
//!
//! **Conclusion : ce niveau d'injection ne peut pas atteindre l'objectif.** Agir
//! sur le dessin demande d'entrer dans l'**ARBRE XAML** — c'est ce que fait le mod
//! « Vertical Taskbar » de Windhawk, qui cherche des éléments (`TaskbarFrame`) et
//! leur applique des transformations via un `VisualTreeWatcher` obtenu par
//! `InitializeXamlDiagnosticsEx`. C'est un ordre de grandeur plus profond, donc
//! plus fragile, que tout ce qui est écrit ici. **Ne pas rouvrir cette route sans
//! en accepter le coût.**
//!
//! # Ce qui reste VÉRIFIÉ, et réutilisable
//!
//! - l'**injection** (`VirtualAllocEx` + `WriteProcessMemory` +
//!   `CreateRemoteThread(LoadLibraryW)`) — voir `main.rs` ;
//! - le **sous-classement** d'une fenêtre d'un autre processus, y compris les
//!   fenêtres CACHÉES (les popups XAML passent leur vie en `visible=false` et ne
//!   sont visibles que le temps de leur usage : les découvrir par sondage oblige à
//!   les prendre d'avance) ;
//! - et surtout l'**éjection propre** : fichier d'arrêt → défaire les sous-classes
//!   → `FreeLibraryAndExitThread` **depuis son propre fil**. C'est ce qui a rendu
//!   explorer intact à chaque essai, et c'est la partie à copier si un jour une
//!   injection revient dans ce dépôt.

use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;

use windows::Win32::Foundation::{HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, FreeLibraryAndExitThread};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, EnumWindows, GetClassNameW, GetWindowRect, IsWindow,
    IsWindowVisible, SetWindowLongPtrW, GWLP_WNDPROC, WM_SHOWWINDOW, WM_WINDOWPOSCHANGING, WNDPROC,
};

/// Classe des menus volants XAML du shell — MESURÉE dans NeuraDesk : c'est celle
/// du menu de l'icône « Réseau ».
const CLASSE_VISE: &str = "Xaml_WindowedPopupClass";

/// Journal et fichier d'arrêt. Sur `C:` et non dans `%TEMP%`, parce que les deux
/// côtés (le nôtre et explorer) doivent le voir au même endroit — et parce qu'un
/// fichier d'arrêt qu'on ne trouve pas est pire que pas de fichier d'arrêt.
const CHEMIN_JOURNAL: &str = r"C:\ndtarget\shell-hook.log";
const CHEMIN_ARRET: &str = r"C:\ndtarget\shell-hook.stop";

/// **OÙ POSER LE MENU** — un fichier de deux nombres, `x,y`. Y passer par un
/// fichier plutôt que par du code permet de changer la cible **sans reconstruire
/// ni réinjecter**, et de la relire à l'œil en cas de doute.
const CHEMIN_CIBLE: &str = r"C:\ndtarget\shell-hook.target";

/// Hauteur en dessous de laquelle une fenêtre n'est PAS un menu : les bandes
/// d'aide à la découpe d'écran d'explorer font 63 px de haut et partagent la même
/// classe (mesuré) — les viser poserait au mauvais endroit une fenêtre qui n'est
/// pas un menu.
const HAUTEUR_MINI_MENU: i32 = 80;

/// Où la sonde écrit la capture d'écran — la mesure « qu'est-ce qui est PEINT ».
const CHEMIN_CAPTURE: &str = r"C:\ndtarget\shell-hook-capture.bmp";

mod xaml_tap;

/// Le chemin complet de CETTE DLL. `InitializeXamlDiagnosticsEx` en a besoin pour
/// savoir QUOI charger et interroger : c'est lui qui nous rappellera, par
/// `DllGetClassObject`, pour obtenir l'objet `IObjectWithSite`.
pub fn chemin_de_la_dll() -> Option<String> {
    use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
    let module = MODULE.load(Ordering::SeqCst);
    if module == 0 {
        return None;
    }
    let mut tampon = [0u16; 1024];
    let longueur =
        unsafe { GetModuleFileNameW(Some(HMODULE(module as *mut _)), &mut tampon) };
    (longueur > 0).then(|| String::from_utf16_lossy(&tampon[..longueur as usize]))
}

/// La fenêtre visée et la position à lui imposer. `HWND` n'est pas `Send` : on
/// garde un `isize`.
static CIBLE: Mutex<Option<(isize, i32, i32)>> = Mutex::new(None);

/// Cadence de la veille. Assez lente pour ne rien coûter, assez rapide pour
/// sous-classer un popup avant qu'il ait fini de naître.
const PERIODE_MS: u64 = 250;

static JOURNAL: Mutex<Option<std::fs::File>> = Mutex::new(None);
/// `HWND` → proc d'origine. `HWND` n'est pas `Send`, et un `isize` traverse tout.
static SUBCLASSEES: Mutex<Option<HashMap<isize, isize>>> = Mutex::new(None);
/// Le module de CETTE DLL, pour se décharger proprement.
static MODULE: AtomicIsize = AtomicIsize::new(0);

fn journaliser(message: &str) {
    let horodatage = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    if let Ok(mut garde) = JOURNAL.lock() {
        if let Some(fichier) = garde.as_mut() {
            let _ = writeln!(fichier, "[{horodatage}] {message}");
            let _ = fichier.flush();
        }
    }
}

fn nom_de_classe(fenetre: HWND) -> String {
    let mut tampon = [0u16; 256];
    let longueur = unsafe { GetClassNameW(fenetre, &mut tampon) };
    String::from_utf16_lossy(&tampon[..longueur.max(0) as usize])
}

/// **LA PROCÉDURE DE SOUS-CLASSE — elle n'entrave RIEN.**
///
/// Elle journalise ce qui l'intéresse puis transmet **toujours** au proc
/// d'origine. Le verrou est relâché AVANT l'appel : le proc d'origine peut
/// renvoyer des messages qui nous reviennent par le même fil, et le garder
/// provoquerait un interblocage.
unsafe extern "system" fn subclass_proc(
    fenetre: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Rien de ce que nous faisons ne doit pouvoir remonter dans explorer.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if message == WM_WINDOWPOSCHANGING {
            let position = lparam.0 as *mut windows::Win32::UI::WindowsAndMessaging::WINDOWPOS;
            if !position.is_null() {
                let position = &mut *position;
                // **LE CORRECTIF TIENT DANS CES QUATRE LIGNES.** Le shell redemande
                // sa position toutes les ~7 ms (mesuré) : en réécrivant `x`/`y` ici,
                // c'est SA réaffirmation qui porte NOTRE position — donc elle tient,
                // et sans clignotement, puisque rien ne se bat.
                //
                // `SWP_NOMOVE` : quand il est posé, le demandeur dit explicitement
                // « ne bouge pas » — c'est la démolition, pas la pose. Y réécrire
                // une position serait au mieux inutile, au pire un combat.
                const SWP_NOMOVE: u32 = 0x0002;
                let visee = CIBLE.lock().ok().and_then(|garde| *garde);
                if let Some((fenetre_visee, x, y)) = visee {
                    if fenetre_visee == fenetre.0 as isize && (position.flags.0 & SWP_NOMOVE) == 0 {
                        journaliser(&format!(
                            "RÉÉCRITURE {:?} : demandé ({},{}) -> ({x},{y})",
                            fenetre.0, position.x, position.y
                        ));
                        position.x = x;
                        position.y = y;
                    }
                }
                let mut actuelle = RECT::default();
                let _ = GetWindowRect(fenetre, &mut actuelle);
                journaliser(&format!(
                    "WINDOWPOSCHANGING {:?} : demandé ({},{}) {}x{} drapeaux {:#x} ; actuel ({},{}) {}x{}",
                    fenetre.0,
                    position.x,
                    position.y,
                    position.cx,
                    position.cy,
                    position.flags.0,
                    actuelle.left,
                    actuelle.top,
                    actuelle.right - actuelle.left,
                    actuelle.bottom - actuelle.top,
                ));
            }
        } else if message == WM_SHOWWINDOW {
            journaliser(&format!("SHOWWINDOW {:?} : visible={}", fenetre.0, wparam.0));
        }
    }));

    let original = SUBCLASSEES
        .lock()
        .ok()
        .and_then(|garde| garde.as_ref().and_then(|table| table.get(&(fenetre.0 as isize)).copied()));

    match original {
        Some(adresse) => {
            let proc: WNDPROC = std::mem::transmute::<isize, WNDPROC>(adresse);
            CallWindowProcW(proc, fenetre, message, wparam, lparam)
        }
        // Ne devrait JAMAIS arriver : on ne sous-classe qu'en enregistrant
        // l'original. Si ça arrive, on le DIT plutôt que de le taire.
        None => {
            journaliser("!! proc d'origine INTROUVABLE — repli sur DefWindowProcW");
            DefWindowProcW(fenetre, message, wparam, lparam)
        }
    }
}

unsafe extern "system" fn enumerer(fenetre: HWND, lparam: LPARAM) -> windows::core::BOOL {
    let sortie = &mut *(lparam.0 as *mut Vec<HWND>);
    sortie.push(fenetre);
    windows::core::BOOL(1)
}

/// **SOUS-CLASSE TOUT POPUP XAML VISIBLE QUI NE L'EST PAS ENCORE**, et défait
/// tout si le fichier d'arrêt apparaît.
fn surveiller() {
    // Deux traces qui ne coûtent rien et qui tranchent : la veille tourne-t-elle,
    // et voit-elle seulement des fenêtres de la classe visée ? Sans elles, un
    // journal vide se lirait aussi bien « le fil n'a pas démarré » que « aucune
    // fenêtre n'existe » — deux causes opposées.
    journaliser("veille démarrée");
    sonder_xaml();
    // ÉTAPE 3 — on demande au shell de nous parler de son arbre XAML.
    xaml_tap::demarrer();
    let mut dernier_compte = usize::MAX;
    let mut dernier_xaml = u32::MAX;
    loop {
        if std::path::Path::new(CHEMIN_ARRET).exists() {
            journaliser("fichier d'arrêt vu — défaire les sous-classes puis se décharger");
            defaire_tout();
            let module = MODULE.load(Ordering::SeqCst);
            if module != 0 {
                // Se décharger SOI-MÊME, depuis SON fil : la seule façon sûre.
                unsafe { FreeLibraryAndExitThread(HMODULE(module as *mut _), 0) };
            }
            return;
        }

        purger_les_mortes();
        let mut fenetres: Vec<HWND> = Vec::new();
        let _ = unsafe {
            EnumWindows(Some(enumerer), LPARAM(&mut fenetres as *mut _ as isize))
        };
        let visees = fenetres
            .iter()
            .filter(|fenetre| nom_de_classe(**fenetre).contains(CLASSE_VISE))
            .count();
        if visees != dernier_compte {
            journaliser(&format!(
                "{visees} fenêtre(s) {CLASSE_VISE} (sur {} au premier niveau)",
                fenetres.len()
            ));
            dernier_compte = visees;
        }
        // Le compteur XAML est lu ICI, sur notre fil — jamais écrit ni lu depuis
        // le récepteur, qui tourne sur le fil d'interface du shell.
        let evenements = xaml_tap::evenements_recus();
        if evenements != dernier_xaml {
            journaliser(&format!("XAML : {evenements} événement(s) d'arbre reçus"));
            dernier_xaml = evenements;
        }
        // Relue à CHAQUE tour : c'est ce qui permet de changer la cible sans
        // reconstruire ni réinjecter, et de la corriger en la relisant à l'œil.
        let position_cible = std::fs::read_to_string(CHEMIN_CIBLE).ok().and_then(|texte| {
            let mut champs = texte.trim().split(',');
            let x = champs.next()?.trim().parse::<i32>().ok()?;
            let y = champs.next()?.trim().parse::<i32>().ok()?;
            Some((x, y))
        });

        for fenetre in fenetres {
            // **ON SOUS-CLASSE AUSSI LES FENÊTRES CACHÉES, ET C'EST ESSENTIEL.**
            // Les popups XAML du shell sont créés UNE FOIS puis réaffichés : ils
            // passent leur vie en `visible=false` (mesuré : quatre fenêtres, toutes
            // cachées, en permanence). Attendre qu'ils deviennent visibles, c'est
            // arriver APRÈS la pose — donc ne rien voir de ce qu'on cherche à voir.
            // Sous-classer d'avance coûte une entrée de table, et rend le proc
            // présent au moment où Windows envoie `WM_WINDOWPOSCHANGING`.
            if !nom_de_classe(fenetre).contains(CLASSE_VISE) {
                continue;
            }
            // **LE REPLI EST `false`, ET C'EST LE FOND DU CORRECTIF.** Écrire
            // `unwrap_or(true)` ici — « en cas de doute, considère que c'est déjà
            // fait » — avait l'air prudent et désactivait TOUT : au démarrage la
            // table est `None` (vide), donc chaque fenêtre était sautée et le
            // journal restait muet alors qu'il voyait bien les quatre popups.
            // Un repli qui désactive en silence est un repli qui ment.
            // Seul un verrou EMPOISONNÉ justifie de ne rien toucher.
            let deja = match SUBCLASSEES.lock() {
                Ok(garde) => garde
                    .as_ref()
                    .is_some_and(|table| table.contains_key(&(fenetre.0 as isize))),
                Err(_) => true,
            };
            if deja {
                continue;
            }
            // Les POIGNÉES D'ÉCRITURE D'ÉCRAN sont des popups XAML d'explorer
            // comme les autres, mais elles n'ont rien à voir avec la zone de
            // notification : on journalise la classe et le titre pour pouvoir
            // les reconnaître, sans les exclure — étape 1, on veut TOUT voir.
            // `WNDPROC` est un `Option<fn>` — donc un pointeur, optimisé à null —
            // et c'est `isize` qui traverse : il n'est pas question de le stocker
            // autrement, la table doit rester `Send`.
            let notre_proc = unsafe { std::mem::transmute::<WNDPROC, isize>(Some(subclass_proc)) };
            let ancien = unsafe { SetWindowLongPtrW(fenetre, GWLP_WNDPROC, notre_proc) };
            if ancien == 0 {
                journaliser(&format!(
                    "sous-classement REFUSÉ pour {:?} ({})",
                    fenetre.0,
                    nom_de_classe(fenetre)
                ));
                continue;
            }
            if let Ok(mut garde) = SUBCLASSEES.lock() {
                garde.get_or_insert_with(HashMap::new).insert(fenetre.0 as isize, ancien);
            }
            let mut rect = RECT::default();
            let _ = unsafe { GetWindowRect(fenetre, &mut rect) };
            let visible = unsafe { IsWindowVisible(fenetre) }.as_bool();
            let hauteur = rect.bottom - rect.top;
            journaliser(&format!(
                "sous-classé {:?} ({}) — proc d'origine à {ancien:#x}, visible={visible}, rect ({},{}) {}x{hauteur}",
                fenetre.0,
                nom_de_classe(fenetre),
                rect.left,
                rect.top,
                rect.right - rect.left,
            ));

            // **ON ARME LA CIBLE SUR CE QUI VIENT D'APPARAÎTRE.** Une fenêtre
            // VISIBLE au moment où on la découvre est un popup qui s'ouvre à
            // l'instant ; les quatre permanentes sont toujours cachées (mesuré), et
            // les bandes de découpe d'écran font 63 px de haut, sous le seuil.
            if visible && hauteur >= HAUTEUR_MINI_MENU {
                match position_cible {
                    Some((x, y)) => {
                        if let Ok(mut garde) = CIBLE.lock() {
                            *garde = Some((fenetre.0 as isize, x, y));
                        }
                        journaliser(&format!("CIBLE ARMÉE sur {:?} -> ({x},{y})", fenetre.0));
                        // ET ON CAPTURE CE QUI SERA PEINT — c'est la seule mesure
                        // qui distingue « la fenêtre a bougé » de « le dessin a
                        // bougé », et elle doit être prise pendant que le menu est
                        // ouvert, donc d'ici.
                        std::thread::spawn(capturer_apres_delai);
                    }
                    None => journaliser(
                        "pas de cible : aucun fichier shell-hook.target — on observe sans agir",
                    ),
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(PERIODE_MS));
    }
}

/// SONDE — **LA VOIE XAML EST-ELLE SEULEMENT OUVERTE ?**
///
/// Avant d'écrire la moindre plomberie COM — le crate `windows` n'expose AUCUN
/// type `Windows.UI.Xaml.*` (vérifié : zéro fonctionnalité `UI_Xaml*`), donc IID
/// et dispositions de vtable seraient à écrire à la main — une question qui coûte
/// dix lignes : le module XAML est-il chargé dans explorer, et l'export
/// `InitializeXamlDiagnosticsEx` — celui par lequel on obtient un
/// `VisualTreeWatcher` — **existe-t-il encore sur ce build** ? Il n'est pas
/// documenté : rien ne garantit qu'il ait survécu à une mise à jour de Windows.
///
/// Répondre « non » ici ferme la voie pour un coût dérisoire. Y aller d'abord
/// aurait coûté des heures de COM pour rien.
fn sonder_xaml() {
    use windows::core::{s, PCWSTR};
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

    for nom in [
        "Windows.UI.Xaml.dll",
        "Windows.UI.Xaml.Controls.dll",
        "CoreMessaging.dll",
        "Windows.UI.Xaml.Phone.dll",
    ] {
        let large: Vec<u16> = nom.encode_utf16().chain(std::iter::once(0)).collect();
        let module = unsafe { GetModuleHandleW(PCWSTR(large.as_ptr())) };
        match module {
            Ok(module) if !module.0.is_null() => {
                let entree = unsafe { GetProcAddress(module, s!("InitializeXamlDiagnosticsEx")) };
                match entree {
                    Some(adresse) => journaliser(&format!(
                        "XAML : {nom} CHARGÉ — InitializeXamlDiagnosticsEx à {:#x}",
                        adresse as usize
                    )),
                    None => journaliser(&format!(
                        "XAML : {nom} chargé, mais InitializeXamlDiagnosticsEx ABSENT"
                    )),
                }
            }
            _ => journaliser(&format!("XAML : {nom} non chargé dans ce processus")),
        }
    }
}

/// SONDE — **CE QUI EST RÉELLEMENT PEINT À L'ÉCRAN.**
///
/// C'est la mesure qui manquait, et celle qui a coûté une manche : le journal
/// disait où la FENÊTRE était (`actuel (3366,1874)`), l'utilisateur disait que le
/// MENU était ailleurs — et les deux avaient raison, parce que la fenêtre et le
/// dessin sont deux choses distinctes. Les pixels, eux, ne peuvent pas avoir
/// raison tous les deux.
///
/// Écrit en BMP : aucun encodeur à écrire, la conversion se fait ensuite à
/// l'extérieur, où l'on peut aussi la mesurer.
fn capturer_ecran(chemin: &str) {
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    let largeur = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let hauteur = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    unsafe {
        let ecran = GetDC(None);
        let memoire = CreateCompatibleDC(Some(ecran));
        let entete = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: largeur,
            // Négatif : lignes du HAUT vers le bas, comme le veut un BMP écrit
            // d'un bloc depuis un tampon lu dans l'ordre naturel.
            biHeight: -hauteur,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let info = BITMAPINFO { bmiHeader: entete, ..Default::default() };
        let mut pixels: *mut std::ffi::c_void = std::ptr::null_mut();
        let bitmap = match CreateDIBSection(Some(memoire), &info, DIB_RGB_COLORS, &mut pixels, None, 0)
        {
            Ok(bitmap) if !pixels.is_null() => bitmap,
            _ => {
                journaliser("capture : CreateDIBSection a échoué");
                let _ = DeleteDC(memoire);
                ReleaseDC(None, ecran);
                return;
            }
        };
        let ancien = SelectObject(memoire, bitmap.into());
        let _ = BitBlt(memoire, 0, 0, largeur, hauteur, Some(ecran), 0, 0, SRCCOPY);
        let _ = SelectObject(memoire, ancien);

        let octets =
            std::slice::from_raw_parts(pixels as *const u8, (largeur * hauteur * 4) as usize);
        let mut sortie: Vec<u8> = Vec::with_capacity(octets.len() + 54);
        sortie.extend_from_slice(b"BM");
        sortie.extend_from_slice(&(54u32 + octets.len() as u32).to_le_bytes());
        sortie.extend_from_slice(&[0u8; 4]);
        sortie.extend_from_slice(&54u32.to_le_bytes());
        sortie.extend_from_slice(&40u32.to_le_bytes());
        sortie.extend_from_slice(&largeur.to_le_bytes());
        sortie.extend_from_slice(&(-hauteur).to_le_bytes());
        sortie.extend_from_slice(&1u16.to_le_bytes());
        sortie.extend_from_slice(&32u16.to_le_bytes());
        sortie.extend_from_slice(&0u32.to_le_bytes());
        sortie.extend_from_slice(&(octets.len() as u32).to_le_bytes());
        sortie.extend_from_slice(&[0u8; 16]);
        sortie.extend_from_slice(octets);
        match std::fs::write(chemin, &sortie) {
            Ok(()) => journaliser(&format!("capture écrite : {chemin} ({largeur}x{hauteur})")),
            Err(erreur) => journaliser(&format!("capture NON écrite : {erreur}")),
        }
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memoire);
        ReleaseDC(None, ecran);
    }
}

/// La capture, différée pour laisser le menu se peindre.
fn capturer_apres_delai() {
    std::thread::sleep(std::time::Duration::from_millis(600));
    capturer_ecran(CHEMIN_CAPTURE);
}

/// **RETIRE DE LA TABLE LES FENÊTRES QUI N'EXISTENT PLUS.**
///
/// Windows **réutilise les `HWND`**. Une entrée laissée par une fenêtre détruite
/// ferait croire qu'un numéro est déjà sous-classé ; la fenêtre suivante qui
/// hériterait de ce numéro serait donc ignorée — et **le popup qu'on veut voir
/// passerait inaperçu, en silence**. C'est exactement la classe de défaut que ce
/// dépôt paie à répétition.
fn purger_les_mortes() {
    let Ok(mut garde) = SUBCLASSEES.lock() else { return };
    let Some(table) = garde.as_mut() else { return };
    let mortes: Vec<isize> = table
        .keys()
        .copied()
        .filter(|fenetre| !unsafe { IsWindow(Some(HWND(*fenetre as *mut _))) }.as_bool())
        .collect();
    for fenetre in mortes {
        table.remove(&fenetre);
        journaliser(&format!("fenêtre {fenetre:#x} disparue — entrée retirée de la table"));
    }
}

/// Rend à chaque fenêtre son proc d'origine. **À faire AVANT tout déchargement** :
/// une fenêtre qui pointerait encore sur notre code après le déchargement tuerait
/// explorer à la première souris.
fn defaire_tout() {
    let table = match SUBCLASSEES.lock() {
        Ok(mut garde) => garde.take(),
        Err(_) => None,
    };
    let Some(table) = table else { return };
    for (fenetre, original) in table {
        unsafe { SetWindowLongPtrW(HWND(fenetre as *mut _), GWLP_WNDPROC, original) };
        journaliser(&format!("sous-classe défaite pour {fenetre:#x}"));
    }
    journaliser("toutes les sous-classes défaites");
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "system" fn DllMain(module: HINSTANCE, raison: u32, _reserve: *mut std::ffi::c_void) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    const DLL_PROCESS_DETACH: u32 = 0;

    match raison {
        DLL_PROCESS_ATTACH => {
            MODULE.store(module.0 as isize, Ordering::SeqCst);
            // Le fichier d'arrêt d'une session PRÉCÉDENTE ne doit pas nous faire
            // sortir aussitôt : on le retire nous-mêmes au démarrage.
            let _ = std::fs::remove_file(CHEMIN_ARRET);
            let fichier = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(CHEMIN_JOURNAL)
                .ok();
            if let Ok(mut garde) = JOURNAL.lock() {
                *garde = fichier;
            }
            journaliser(&format!(
                "=== DLL chargée — pid {} (explorer) — {CLASSE_VISE} surveillée ===",
                std::process::id()
            ));
            unsafe { let _ = DisableThreadLibraryCalls(HMODULE(module.0)); }
            // Le fil démarre ICI et pas ailleurs : `DllMain` est le seul point
            // d'entrée d'une DLL. Il ne fait RIEN d'autre que lancer ce fil, donc
            // il ne peut pas bloquer le verrou du chargeur.
            std::thread::spawn(surveiller);
        }
        DLL_PROCESS_DETACH => {
            journaliser("=== DLL déchargée ===");
        }
        _ => {}
    }
    1
}
