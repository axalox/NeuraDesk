//! SONDE — **DWM dessine-t-il la vignette d'une fenêtre située sur un bureau
//! virtuel INACTIF ?** C'est la seule question qui reste avant d'écrire l'aperçu
//! d'un bureau tel que Windows le fait (voir `win32::desktop_preview`), et elle
//! se mesure, elle ne se déduit pas.
//!
//! **CE QUE LA SONDE FABRIQUE, et pourquoi de cette forme :**
//!
//! 1. une fenêtre SOURCE à nous, au contenu voyant (magenta plein + barre
//!    blanche) — notre fenêtre, donc nous la peignons nous-mêmes, et un motif
//!    franc se repère d'un coup d'œil ;
//! 2. elle est DÉPLACÉE sur un AUTRE bureau par
//!    `IVirtualDesktopManager::MoveWindowToDesktop` — l'API **documentée**, la
//!    même qui a servi à prouver la correspondance fenêtre -> bureau (voir
//!    `win32::desktops`). C'est ce déplacement qui crée le cas à mesurer ;
//! 3. une fenêtre DESTINATION à nous, sur le bureau courant ;
//! 4. `DwmRegisterThumbnail(destination, source)` + les propriétés qui la
//!    rendent visible et l'étirent sur toute la destination.
//!
//! **LE VERDICT SE LIT À L'ŒIL**, sur la fenêtre de destination : si le magenta
//! y apparaît, DWM dessine bien la vignette d'une fenêtre d'un bureau inactif —
//! et l'aperçu d'un bureau n'est plus que de la géométrie. Sinon, il n'y a rien
//! à construire par cette voie, et il faut le dire.
//!
//! **POURQUOI PAS UNE MESURE AUTOMATIQUE** : une vignette DWM est dessinée par
//! le COMPOSITEUR, pas par notre renderer — elle n'existe donc dans aucun de nos
//! tampons, et le harnais de capture (qui lit des back buffers) ne peut pas la
//! voir. La mesurer demanderait une capture d'écran de la destination, que nos
//! propres fenêtres excluent par ailleurs (voir `gfx::capture_visibility`). La
//! sonde rend donc la question VISIBLE au lieu de la mesurer, et c'est la même
//! décision que pour l'applet rognée : « une sonde qui peut répondre à côté est
//! pire qu'une question bien posée ».
//!
//! **INERTE SANS `NEURADESK_DEBUG_THUMB=1`** — aucune fenêtre n'est créée en
//! usage normal. Et **RIEN N'EST DÉPLACÉ QUI NE SOIT À NOUS** : la seule fenêtre
//! qui change de bureau est celle que cette sonde vient de créer, jamais une
//! fenêtre de l'utilisateur.

use windows::core::{w, GUID};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmRegisterThumbnail, DwmUpdateThumbnailProperties, DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY,
    DWM_TNP_RECTDESTINATION, DWM_TNP_VISIBLE,
};
use windows::Win32::Graphics::Gdi::{BeginPaint, CreateSolidBrush, EndPaint, FillRect, PAINTSTRUCT};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, PeekMessageW, RegisterClassExW, ShowWindow, TranslateMessage,
    MSG, PM_REMOVE, SW_SHOW, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW,
};

/// Position et taille des deux fenêtres de la sonde, en coordonnées écran.
/// La destination est à droite de la source pour qu'on puisse les comparer d'un
/// même regard — la source est de toute façon déplacée sur un autre bureau, donc
/// elle disparaîtra de l'écran : c'est bien le but.
const SOURCE_RECT: [i32; 4] = [120, 160, 320, 200];
const DEST_RECT: [i32; 4] = [520, 160, 420, 300];

/// Couleurs du motif — magenta franc et barre blanche pour la source, gris
/// sombre uni pour la destination (tout ce qui n'est PAS gris dans la
/// destination est la vignette).
const SOURCE_FILL: u32 = 0x00FF_00FF; // 0x00BBGGRR : magenta
const SOURCE_BAR: u32 = 0x00FF_FFFF; // blanc
const DEST_FILL: u32 = 0x0020_2020; // gris très sombre

/// Crée les deux fenêtres et enregistre la vignette. Rend `true` si la sonde a
/// été montée — ce qui n'est PAS le verdict : le verdict se lit sur la fenêtre
/// de destination.
fn mount() -> bool {
    unsafe {
        let Ok(instance) = GetModuleHandleW(None) else { return false };
        let instance = instance.into();
        for (name, proc_) in [(w!("NeuraDeskThumbProbeSrc"), source_wndproc as unsafe extern "system" fn(_, _, _, _) -> _), (w!("NeuraDeskThumbProbeDst"), dest_wndproc as unsafe extern "system" fn(_, _, _, _) -> _)] {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc_),
                hInstance: instance,
                lpszClassName: name,
                ..Default::default()
            };
            let _ = RegisterClassExW(&wc);
        }

        // `WS_EX_TOOLWINDOW` sur les DEUX : elles ne doivent pas apparaître dans
        // la barre des tâches ni dans Alt+Tab, ce ne sont pas des fenêtres de
        // l'application mais des instruments de mesure. **Et `WS_EX_TOPMOST`**,
        // sans quoi elles naîtraient DERRIÈRE les fenêtres déjà ouvertes de
        // l'utilisateur — qui ne verrait rien et conclurait à un échec.
        let ex_style = WS_EX_TOOLWINDOW | WS_EX_TOPMOST;
        let source = CreateWindowExW(
            ex_style,
            w!("NeuraDeskThumbProbeSrc"),
            w!("NeuraDesk — sonde vignette (source)"),
            WS_OVERLAPPEDWINDOW,
            SOURCE_RECT[0],
            SOURCE_RECT[1],
            SOURCE_RECT[2],
            SOURCE_RECT[3],
            None,
            None,
            Some(instance),
            None,
        );
        let Ok(source) = source else {
            eprintln!("[neuradesk][sonde] fenêtre source NON créée");
            return false;
        };

        // LE DÉPLACEMENT SUR UN AUTRE BUREAU — tout l'objet de la sonde. Le
        // GUID du bureau est lu dans le registre, dans le MÊME ordre que les
        // pastilles (voir `win32::desktops`), et converti par une COPIE D'OCTETS
        // BRUTS : c'est la conversion inverse de `desktops::guid_bytes`, et elle
        // n'est légitime que parce que la sonde de ce module-là a établi que le
        // registre et COM partagent exactement la même disposition.
        let ids = crate::win32::desktops::raw_desktop_ids().unwrap_or_default();
        if ids.len() < 2 * 16 {
            eprintln!("[neuradesk][sonde] moins de DEUX bureaux virtuels : rien à déplacer, la question ne se pose pas");
        } else {
            let target: GUID = unsafe { std::ptr::read(ids[16..32].as_ptr() as *const GUID) };
            match unsafe { CoCreateInstance::<_, IVirtualDesktopManager>(&VirtualDesktopManager, None, CLSCTX_ALL) } {
                Ok(manager) => match unsafe { manager.MoveWindowToDesktop(source, &target) } {
                    Ok(()) => eprintln!("[neuradesk][sonde] fenêtre source DÉPLACÉE sur le bureau 1"),
                    Err(error) => eprintln!("[neuradesk][sonde] déplacement REFUSÉ : {error:?}"),
                },
                Err(error) => eprintln!("[neuradesk][sonde] IVirtualDesktopManager indisponible : {error:?}"),
            }
        }
        let _ = ShowWindow(source, SW_SHOW);

        let dest = CreateWindowExW(
            ex_style,
            w!("NeuraDeskThumbProbeDst"),
            w!("NeuraDesk — sonde vignette (destination)"),
            WS_OVERLAPPEDWINDOW,
            DEST_RECT[0],
            DEST_RECT[1],
            DEST_RECT[2],
            DEST_RECT[3],
            None,
            None,
            Some(instance),
            None,
        );
        let Ok(dest) = dest else {
            eprintln!("[neuradesk][sonde] fenêtre destination NON créée");
            return false;
        };
        let _ = ShowWindow(dest, SW_SHOW);

        // L'ENREGISTREMENT — et c'est ici que tout se joue. `rcDestination` est
        // la clientèle ENTIÈRE de la destination, donc si la vignette se dessine,
        // elle la remplit.
        match unsafe { DwmRegisterThumbnail(dest, source) } {
            Ok(thumbnail) => {
                let properties = DWM_THUMBNAIL_PROPERTIES {
                    dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY,
                    rcDestination: RECT { left: 0, top: 0, right: DEST_RECT[2], bottom: DEST_RECT[3] },
                    opacity: 255,
                    fVisible: true.into(),
                    ..Default::default()
                };
                match unsafe { DwmUpdateThumbnailProperties(thumbnail, &properties) } {
                    Ok(()) => eprintln!(
                        "[neuradesk][sonde] vignette ENREGISTRÉE (source {:#x} -> destination {:#x}) : regardez la fenêtre grise",
                        source.0 as usize, dest.0 as usize
                    ),
                    Err(error) => eprintln!("[neuradesk][sonde] propriétés de vignette REFUSÉES : {error:?}"),
                }
            }
            Err(error) => eprintln!("[neuradesk][sonde] DwmRegisterThumbnail a ÉCHOUÉ : {error:?}"),
        }
        true
    }
}

// **LA SONDE TOURNE COMME UN TEST, PAS DANS L'APPLICATION**, et c'est un choix
// qui a deux vertus : elle n'oblige pas à fermer l'application en cours (les
// binaires de test se lient même quand `neuradesk.exe` tourne), et elle ne peut
// pas se déclencher en usage normal — le module entier est `#[cfg(test)]`.
//
// Elle POMPE ELLE-MÊME ses messages (voir `pump`), parce qu'un thread de test
// n'a pas de boucle principale : sans cela, ses deux fenêtres ne recevraient
// jamais leur `WM_PAINT` et resteraient... grises, c'est-à-dire exactement le
// verdict « négatif » qu'on cherche à distinguer d'un vrai.

unsafe extern "system" fn source_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    const WM_PAINT: u32 = 0x000F;
    const WM_ERASEBKGND: u32 = 0x0014;
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(hwnd, &mut ps) };
            let mut client = RECT::default();
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut client);
                let fill = CreateSolidBrush(windows::Win32::Foundation::COLORREF(SOURCE_FILL));
                FillRect(dc, &client, fill);
                let _ = windows::Win32::Graphics::Gdi::DeleteObject(fill.into());
                // La BARRE BLANCHE : elle rend le motif asymétrique, donc
                // reconnaissable même réduit et même si la vignette est
                // partiellement rognée.
                let bar = RECT { left: 0, top: client.bottom / 3, right: client.right, bottom: client.bottom * 2 / 3 };
                let bar_brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(SOURCE_BAR));
                FillRect(dc, &bar, bar_brush);
                let _ = windows::Win32::Graphics::Gdi::DeleteObject(bar_brush.into());
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Pompe à messages du THREAD DE TEST — voir l'en-tête de `mount` : sans elle,
/// les deux fenêtres ne recevraient jamais leur `WM_PAINT` et la destination
/// resterait grise, ce qui se lirait exactement comme le verdict négatif qu'on
/// cherche à distinguer d'un vrai.
fn pump() {
    unsafe {
        let mut message = MSG::default();
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **SONDE — DWM dessine-t-il la vignette d'une fenêtre située sur un autre
    /// bureau virtuel ?**
    ///
    /// `cargo test --bins -- --ignored --nocapture thumb_probe`
    ///
    /// Elle monte les deux fenêtres (voir `mount`) puis laisse l'utilisateur
    /// REGARDER pendant vingt secondes. **Elle ne juge pas elle-même, et ne le
    /// peut pas** : une vignette DWM est dessinée par le compositeur, donc elle
    /// n'existe dans aucun tampon qu'on saurait relire (voir le doc de module).
    #[test]
    #[ignore = "ouvre deux fenetres sur le bureau de l'utilisateur et attend son verdict"]
    fn shows_a_thumbnail_of_a_window_on_another_desktop() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        if !mount() {
            println!("!! la sonde n'a pas pu etre montee");
            return;
        }
        println!("== REGARDEZ la fenetre GRISE : le magenta et sa barre blanche y apparaissent-ils ? ==");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while std::time::Instant::now() < deadline {
            pump();
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        println!("== fin de la sonde ==");
    }
}

unsafe extern "system" fn dest_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    const WM_PAINT: u32 = 0x000F;
    const WM_ERASEBKGND: u32 = 0x0014;
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(hwnd, &mut ps) };
            let mut client = RECT::default();
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut client);
                let fill = CreateSolidBrush(windows::Win32::Foundation::COLORREF(DEST_FILL));
                FillRect(dc, &client, fill);
                let _ = windows::Win32::Graphics::Gdi::DeleteObject(fill.into());
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
