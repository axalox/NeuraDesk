//! **SONDE, et uniquement une sonde** — même statut que `thumb_probe` : ce
//! module ne contient AUCUN code de production, n'existe que dans les binaires
//! de test, et n'alourdit donc pas l'application.
//!
//! **CE QU'ELLE MESURE, ET POURQUOI ELLE EXISTE.** On veut afficher un miroir de
//! la ZONE DE NOTIFICATION de Windows (les icônes de la barre des tâches :
//! volume, réseau, applications en arrière-plan) dans une zone du dock. Or
//! **Windows n'a AUCUNE API publique pour l'énumérer** : les deux voies connues
//! sont la lecture de la mémoire d'`explorer.exe` (avec le risque documenté de
//! faire planter la barre des tâches) ou UI Automation (documentée, sans risque,
//! mais qui ne rend aucun pixel).
//!
//! Avant de choisir, il faut savoir ce que la machine expose RÉELLEMENT. Cette
//! sonde répond à trois questions, **en lecture seule et sans aucun risque** :
//!
//! 1. la **hiérarchie** réelle sous `Shell_TrayWnd` — la littérature annonce
//!    `TrayNotifyWnd` → `SysPager` → `ToolbarWindow32`, mais rien ne le garantit
//!    sur cette version de Windows ;
//! 2. le **nombre de boutons** du toolbar (`TB_BUTTONCOUNT`), c'est-à-dire le
//!    nombre d'icônes visibles — obtenu par `SendMessageTimeoutW` avec un délai
//!    court, JAMAIS par un `SendMessage` bloquant : une boucle de rendu qui
//!    attend explorer est le défaut le plus facile à créer ici ;
//! 3. le **coût en temps** de ces appels, qui décidera de la cadence de
//!    rafraîchissement d'un futur miroir.
//!
//! ⚠️ **ELLE N'ÉCRIT RIEN, ET NE DOIT RIEN ÉCRIRE.** Pas de `VirtualAllocEx`,
//! pas de `ReadProcessMemory`, pas de `TB_GETBUTTON` : c'est précisément la
//! partie dont la littérature dit qu'elle peut faire planter explorer, et elle
//! n'est pas prise sans un accord explicite (voir le plan). La mesure d'abord,
//! la décision ensuite.
//!
//! Mesure : `cargo test --release --bins -- --ignored --nocapture
//! prints_the_real_notification_area_hierarchy`

use std::time::Instant;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::UI::Controls::TB_BUTTONCOUNT;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowW, GetClassNameW, GetWindow, GetWindowRect, InternalGetWindowText,
    GetWindowThreadProcessId, IsWindowVisible, SendMessageTimeoutW, GW_CHILD,
    GW_HWNDNEXT, SMTO_ABORTIFHUNG,
};

/// Délai accordé à chaque interrogation d'explorer. Court À DESSEIN : au-delà,
/// c'est notre propre fil qui attend, et un explorer pendu retarderait tout le
/// dock. Un miroir de production devra tenir la même discipline.
const PROBE_TIMEOUT_MS: u32 = 1000;

fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

/// Sans rien envoyer à la fenêtre : voir `app_identity::collect_window`.
fn window_title(hwnd: HWND) -> String {
    let mut buffer = [0u16; 512];
    let len = unsafe { InternalGetWindowText(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

fn owning_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

/// **Y A-T-IL UN POPUP XAML RESTÉ AFFICHÉ ?**
///
/// MESURÉ : après un `WM_CLOSE` sur le popup d'une icône du shell, celui-ci ne
/// rouvre plus le menu de CETTE icône (« AUCUNE fenêtre nouvelle », trois fois).
/// La première hypothèse à écarter est une fenêtre restée **VISIBLE** : elle
/// serait alors dans l'ensemble « avant » de la détection par différence, donc
/// invisible pour elle, et aucun nouveau menu ne serait jamais trouvé — un défaut
/// de NOTRE côté, pas de celui d'explorer.
///
/// Mesure : `cargo test --release --bins -- --ignored --nocapture
/// prints_visible_xaml_popups`
#[test]
#[ignore]
fn prints_visible_xaml_popups() {
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let out = &mut *(lparam.0 as *mut Vec<HWND>);
        out.push(hwnd);
        windows::core::BOOL(1)
    }
    let mut tous: Vec<HWND> = Vec::new();
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&mut tous as *mut _ as isize)) };

    let mut trouves = 0usize;
    for hwnd in tous {
        let classe = class_name(hwnd);
        if !classe.contains("Popup") && !classe.contains("popup") {
            continue;
        }
        let mut rect = RECT::default();
        let _ = unsafe { GetWindowRect(hwnd, &mut rect) };
        println!(
            "  {classe} visible={} ({},{}) {}x{} pid={} titre={:?}",
            unsafe { IsWindowVisible(hwnd) }.as_bool(),
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            owning_pid(hwnd),
            window_title(hwnd),
        );
        trouves += 1;
    }
    println!("{trouves} fenêtre(s) de type popup au premier niveau");
    println!("(bureau : {}x{})", unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
            windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN,
        )
    }, unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
            windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN,
        )
    });
}

/// Promène la hiérarchie **ENFANT PAR ENFANT** — et non par `EnumChildWindows`,
/// qui aplatit TOUS les descendants et perd donc la profondeur. C'est justement
/// la profondeur qu'on veut voir, puisque la littérature décrit un CHEMIN.
fn walk(hwnd: HWND, depth: usize, out: &mut Vec<(HWND, usize)>) {
    let mut child = unsafe { GetWindow(hwnd, GW_CHILD) }.unwrap_or_default();
    while !child.0.is_null() {
        out.push((child, depth));
        walk(child, depth + 1, out);
        child = unsafe { GetWindow(child, GW_HWNDNEXT) }.unwrap_or_default();
    }
}

#[test]
#[ignore = "lit la hiérarchie réelle de la barre des tâches de la machine"]
fn prints_the_real_notification_area_hierarchy() {
    let started = Instant::now();
    let tray = unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.unwrap_or_default();
    println!("\n=== Shell_TrayWnd : {:?}   (trouvé en {:?}) ===", tray.0, started.elapsed());
    if tray.0.is_null() {
        println!("ABSENT — cette version de Windows n'a pas de Shell_TrayWnd. Rien à mesurer.");
        return;
    }

    let mut nodes: Vec<(HWND, usize)> = Vec::new();
    walk(tray, 0, &mut nodes);
    println!("{} descendant(s) :\n", nodes.len());
    for (hwnd, depth) in &nodes {
        let pid = owning_pid(*hwnd);
        let exe = super::app_identity::process_image_name(pid).map(|(name, _)| name);
        println!(
            "{:indent$}{:<28} hwnd={:?} visible={:<5} pid={:<6} {:?} titre={:?}",
            "",
            class_name(*hwnd),
            hwnd.0,
            unsafe { IsWindowVisible(*hwnd) }.as_bool(),
            pid,
            exe,
            window_title(*hwnd),
            indent = depth * 2,
        );
    }

    // Les toolbars — c'est EUX qui portent les icônes de notification. On
    // l'interroge avec un délai, et on mesure ce qu'il coûte.
    let toolbars: Vec<HWND> = nodes
        .iter()
        .map(|(hwnd, _)| *hwnd)
        .filter(|hwnd| class_name(*hwnd).eq_ignore_ascii_case("ToolbarWindow32"))
        .collect();
    println!("\n=== {} ToolbarWindow32 ===", toolbars.len());
    if toolbars.is_empty() {
        println!(
            "AUCUN. La voie « toolbar » de la littérature ne s'applique pas telle quelle ici —\n\
             c'est une information, pas un échec : elle redirige vers UI Automation."
        );
    }
    for hwnd in toolbars {
        let pid = owning_pid(hwnd);
        let mut rect = windows::Win32::Foundation::RECT::default();
        let has_rect = unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok();
        let called = Instant::now();
        let mut count: usize = 0;
        let status = unsafe {
            SendMessageTimeoutW(
                hwnd,
                TB_BUTTONCOUNT,
                WPARAM(0),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                PROBE_TIMEOUT_MS,
                Some(&mut count),
            )
        };
        println!(
            "  hwnd={:?} pid={} visible={} rect={:?}\n    TB_BUTTONCOUNT -> {count}  \
             (LRESULT={}, en {:?})",
            hwnd.0,
            pid,
            unsafe { IsWindowVisible(hwnd) }.as_bool(),
            has_rect.then(|| (rect.left, rect.top, rect.right, rect.bottom)),
            status.0,
            called.elapsed(),
        );
    }

    println!("\n(coût total de la promenade + des interrogations : {:?})\n", started.elapsed());

    // ---- UI AUTOMATION -------------------------------------------------
    // Seule voie restante depuis la mesure ci-dessus (aucun `ToolbarWindow32`).
    // On part de `Shell_TrayWnd` et on descend TOUT l'arbre, en imprimant ce que
    // UIA rend de chaque élément dont le nom n'est pas vide — c'est ainsi qu'on
    // verra ce que la zone de notification expose RÉELLEMENT : noms, types,
    // rectangles, PID.
    println!("=== UI Automation depuis Shell_TrayWnd ===");
    unsafe {
        // Best-effort : le thread de test n'est pas forcément initialisé, et
        // l'échec (déjà initialisé, mode différent) n'est pas une erreur ici.
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }
    let automation: windows::Win32::UI::Accessibility::IUIAutomation = match unsafe {
        windows::Win32::System::Com::CoCreateInstance(
            &windows::Win32::UI::Accessibility::CUIAutomation,
            None,
            windows::Win32::System::Com::CLSCTX_ALL,
        )
    } {
        Ok(value) => value,
        Err(error) => {
            println!("CoCreateInstance(CUIAutomation) a échoué : {error:?}\n");
            return;
        }
    };
    let root: windows::Win32::UI::Accessibility::IUIAutomationElement = match unsafe {
        automation.ElementFromHandle(tray)
    } {
        Ok(element) => element,
        Err(error) => {
            println!("ElementFromHandle a échoué : {error:?}\n");
            return;
        }
    };
    let condition = unsafe { automation.CreateTrueCondition() };
    let Ok(condition) = condition else {
        println!("CreateTrueCondition a échoué\n");
        return;
    };
    let uia_started = Instant::now();
    // **LA FENÊTRE QUI EXPOSE L'ARBRE XAML DE LA BARRE DES TÂCHES EST LA
    // PASSERELLE DE COMPOSITION** — MESURÉ, et contre-intuitif : depuis
    // `Shell_TrayWnd` UIA ne rend que 2 éléments, et `TrayNotifyWnd`,
    // `ReBarWindow32`, `Start` n'en rendent AUCUN. C'est
    // `Windows.UI.Composition.DesktopWindowContentBridge` qui porte les 57
    // éléments, dont les boutons de la barre. `Shell_TrayWnd` n'est plus qu'une
    // enveloppe : la barre est en XAML depuis Windows 11.
    const BRIDGE_CLASS: &str = "Windows.UI.Composition.DesktopWindowContentBridge";
    let Some(bridge) = nodes
        .iter()
        .map(|(hwnd, _)| *hwnd)
        .find(|hwnd| class_name(*hwnd).eq_ignore_ascii_case(BRIDGE_CLASS))
    else {
        println!(
            "AUCUNE {BRIDGE_CLASS} sous Shell_TrayWnd — l'arbre XAML de la barre\n\
             n'est pas là où cette sonde le cherche. C'est une information."
        );
        return;
    };
    let element = unsafe { automation.ElementFromHandle(bridge) };
    let Ok(element) = element else {
        println!("ElementFromHandle sur la passerelle a échoué\n");
        return;
    };
    let Ok(children) =
        (unsafe { element.FindAll(windows::Win32::UI::Accessibility::TreeScope_Descendants, &condition) })
    else {
        println!("FindAll sur la passerelle a échoué\n");
        return;
    };
    let count = unsafe { children.Length() }.unwrap_or(0);
    // TOUT est imprimé, trié par ABSCISSE : la zone de notification est à
    // l'extrémité DROITE de la barre, donc en fin de liste. Un plafond
    // masquerait exactement ce qu'on vient chercher.
    let mut named: Vec<(i32, String)> = Vec::new();
    for index in 0..count {
        let Ok(child) = (unsafe { children.GetElement(index) }) else { continue };
        let name = unsafe { child.CurrentName() }.map(|n| n.to_string()).unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let pid = unsafe { child.CurrentProcessId() }.unwrap_or(0);
        let control_type = unsafe { child.CurrentControlType() }.map(|t| t.0).unwrap_or(0);
        let rect = unsafe { child.CurrentBoundingRectangle() }.ok();
        let class = unsafe { child.CurrentClassName() }.map(|c| c.to_string()).unwrap_or_default();
        let left = rect.map(|r| r.left).unwrap_or(0);
        named.push((
            left,
            format!(
                "[{control_type:>3}] x={left:>5}  {name:<44} classe={class:<24} pid={pid}",
            ),
        ));
    }
    named.sort_by_key(|(left, _)| *left);
    println!("=== {BRIDGE_CLASS} hwnd={:?} : {count} descendant(s), {} nommé(s) ===", bridge.0, named.len());
    for (_, line) in &named {
        println!("  {line}");
    }
    println!("(coût de l'énumération UIA : {:?})\n", uia_started.elapsed());

    // **LA LISTE DE PRODUCTION.** C'est `tray_icons::enumerate()` que le dock
    // utilisera — donc c'est ELLE qu'il faut voir, et pas une copie écrite dans
    // cette sonde : la sonde doit éprouver le vrai chemin, sinon elle ne prouve
    // rien sur ce qui sera livré.
    let production = Instant::now();
    let icons = super::tray_icons::enumerate();
    println!(
        "=== tray_icons::enumerate() : {} icône(s) de notification en {:?} ===",
        icons.len(),
        production.elapsed()
    );
    for icon in &icons {
        // Le nom est multi-ligne (l'infobulle complète) : on n'en montre que la
        // première ligne, la seule qui identifie l'entrée.
        let first = icon.name.lines().next().unwrap_or("").trim();
        println!("  x={:>5}  {:<26} {first}", icon.rect.left, icon.class);
    }
    println!();
}

/// Éléments de la zone de notification : `(nom, rectangle, classe)`, triés par
/// abscisse — la zone est à l'extrémité DROITE de la barre.
///
/// **LE DISCRIMINANT EST LA CLASSE, ET ELLE EST NETTE** : mesuré, tous les
/// éléments de la zone portent une classe préfixée `SystemTray.`
/// (`NormalButton`, `AccentButton`, `AccentText`, `OmniButtonLeft`,
/// `OmniButtonRight`, `ShowDesktopButton`) — alors que le reste de la barre
/// porte `Taskbar.TaskListButtonAutomationPeer` ou `ToggleButton`.
fn notification_elements(
    automation: &windows::Win32::UI::Accessibility::IUIAutomation,
    bridge: HWND,
) -> Vec<(String, windows::Win32::Foundation::RECT, String)> {
    let mut found = Vec::new();
    let Ok(element) = (unsafe { automation.ElementFromHandle(bridge) }) else { return found };
    let Ok(condition) = (unsafe { automation.CreateTrueCondition() }) else { return found };
    let Ok(children) = (unsafe {
        element.FindAll(windows::Win32::UI::Accessibility::TreeScope_Descendants, &condition)
    }) else {
        return found;
    };
    for index in 0..unsafe { children.Length() }.unwrap_or(0) {
        let Ok(child) = (unsafe { children.GetElement(index) }) else { continue };
        let class = unsafe { child.CurrentClassName() }.map(|c| c.to_string()).unwrap_or_default();
        if !class.starts_with("SystemTray.") {
            continue;
        }
        let name = unsafe { child.CurrentName() }.map(|n| n.to_string()).unwrap_or_default();
        let Ok(rect) = (unsafe { child.CurrentBoundingRectangle() }) else { continue };
        found.push((name, rect, class));
    }
    found.sort_by_key(|(_, rect, _)| rect.left);
    found
}

/// **CAPTURE LA BANDE DE NOTIFICATION ET L'ÉCRIT EN PNG** — pour JUGER le fond,
/// pas pour le décrire. Toute la question des pixels tient à une chose : le fond
/// de la barre des tâches est-il ASSEZ UNI pour qu'on puisse le détacher ? Si oui,
/// un miroir peut afficher les VRAIES icônes ; sinon, il faut y renoncer et le
/// dire. Un histogramme répondrait mal — regarder l'image répond mieux.
#[test]
#[ignore = "ecrit un PNG dans C:\\ndtarget et lit l'ecran de l'utilisateur"]
fn saves_a_shot_of_the_notification_area() {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
    };
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }
    let Ok(tray) = (unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }) else {
        println!("pas de Shell_TrayWnd");
        return;
    };
    let mut nodes = Vec::new();
    walk(tray, 0, &mut nodes);
    let bridge = nodes
        .iter()
        .map(|(hwnd, _)| *hwnd)
        .find(|hwnd| class_name(*hwnd).starts_with("Windows.UI.Composition.DesktopWindowContentBridge"));
    let Some(bridge) = bridge else {
        println!("pas de passerelle de composition");
        return;
    };
    let automation: windows::Win32::UI::Accessibility::IUIAutomation = match unsafe {
        windows::Win32::System::Com::CoCreateInstance(
            &windows::Win32::UI::Accessibility::CUIAutomation,
            None,
            windows::Win32::System::Com::CLSCTX_ALL,
        )
    } {
        Ok(value) => value,
        Err(error) => {
            println!("CUIAutomation indisponible : {error:?}");
            return;
        }
    };
    // La liste de PRODUCTION, pas une copie : c'est elle que le dock affichera.
    let icons = super::tray_icons::enumerate();
    if icons.is_empty() {
        println!("aucune icône de notification (voir la sonde de hiérarchie)");
        return;
    }
    let elements: Vec<_> = icons.iter().map(|i| (i.name.clone(), i.rect)).collect();
    // **OÙ EST LA BARRE DES TÂCHES, ET VOIT-ON QUELQUE CHOSE LÀ OÙ UIA LA
    // PLACE ?** La première capture a rendu du papier peint — soit le piège DPI
    // (ce processus n'est pas DPI-aware, il lit un écran VIRTUALISÉ pendant
    // qu'UIA rend des coordonnées physiques), soit une barre en AUTO-MASQUAGE
    // (elle existe, mais elle est hors écran). Ces trois chiffres tranchent.
    let mut taskbar_rect = windows::Win32::Foundation::RECT::default();
    let has_taskbar_rect = unsafe { GetWindowRect(tray, &mut taskbar_rect) }.is_ok();
    println!(
        "écran vu par CE processus : {}x{}  (auto-masquage = {})",
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
            windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN) },
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
            windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN) },
        super::taskbar_autohide::is_enabled(),
    );
    println!(
        "rectangle de Shell_TrayWnd : {}",
        if has_taskbar_rect {
            format!("({}, {}) .. ({}, {})", taskbar_rect.left, taskbar_rect.top, taskbar_rect.right, taskbar_rect.bottom)
        } else {
            "illisible".to_string()
        }
    );
    // La bande ENTIÈRE, de la première à la dernière icône, sur toute la hauteur
    // de la barre : c'est ainsi qu'on verra le fond autour des icônes.
    let left = elements.iter().map(|(_, r)| r.left).min().unwrap_or(0);
    let right = elements.iter().map(|(_, r)| r.right).max().unwrap_or(0);
    let top = elements.iter().map(|(_, r)| r.top).min().unwrap_or(0);
    let bottom = elements.iter().map(|(_, r)| r.bottom).max().unwrap_or(0);
    println!("bande : x {left}..{right}, y {top}..{bottom} ({} éléments)", elements.len());

    let width = right - left;
    let height = bottom - top;
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        // NÉGATIF : DIB « top-down », donc les lignes se lisent dans l'ordre
        // naturel — pas besoin de les retourner à la main.
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let info = BITMAPINFO { bmiHeader: header, ..Default::default() };
    let mut pixels: *mut std::ffi::c_void = std::ptr::null_mut();
    let screen = unsafe { GetDC(None) };
    let memory = unsafe { CreateCompatibleDC(Some(screen)) };
    let Ok(bitmap) =
        (unsafe { CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut pixels, None, 0) })
    else {
        println!("CreateDIBSection a échoué");
        unsafe {
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
        }
        return;
    };
    let previous = unsafe { SelectObject(memory, bitmap.into()) };
    let copied =
        unsafe { BitBlt(memory, 0, 0, width, height, Some(screen), left, top, SRCCOPY) }.is_ok();
    unsafe {
        SelectObject(memory, previous);
    }
    if !copied || pixels.is_null() {
        println!("BitBlt a échoué");
        unsafe {
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
        }
        return;
    }
    // BGRA -> RGBA, et on écrit le PNG.
    let count = (width * height) as usize;
    let raw = unsafe { std::slice::from_raw_parts(pixels as *const u8, count * 4) };
    let mut rgba = Vec::with_capacity(count * 4);
    for pixel in raw.chunks_exact(4) {
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
    let Some(image) = image::RgbaImage::from_raw(width as u32, height as u32, rgba) else {
        println!("RgbaImage::from_raw a refusé les dimensions");
        return;
    };
    let path = r"C:\ndtarget\tray-strip.png";
    match image.save(path) {
        Ok(()) => println!("écrit : {path} ({width}x{height})"),
        Err(error) => println!("écriture impossible : {error}"),
    }
    // Et la mesure qui répond à la question : combien de couleurs DISTINCTES
    // dans une bande de fond prise entre deux icônes ? Un fond uni en donne une
    // poignée ; un fond translucide en donne des centaines.
    let mut colors = std::collections::BTreeSet::new();
    for y in 0..height.min(6) {
        for x in 0..width {
            let offset = ((y * width + x) * 4) as usize;
            colors.insert((raw[offset + 2], raw[offset + 1], raw[offset]));
        }
    }
    println!("couleurs distinctes dans les 6 premières lignes (fond) : {}", colors.len());
    unsafe {
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
    }
}

/// **LES ICÔNES MASQUÉES** — celles derrière le chevron. Elles vivent dans une
/// autre fenêtre que la barre, et la littérature dit qu'elle **n'existe pas tant
/// que le chevron n'a pas été ouvert** : c'est la première chose à mesurer, et
/// elle décide si un miroir peut les afficher.
#[test]
#[ignore = "lit la fenetre des icones masquees de la machine"]
fn prints_the_hidden_notification_icons() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }
    // Le nom de classe relevé par la recherche (Windows 11, refonte XAML) —
    // `NotifyIconOverflowWindow` est l'ancien, et ne sert plus à rien.
    let overflow = unsafe {
        FindWindowW(w!("TopLevelWindowForOverflowXamlIsland"), PCWSTR::null())
    }
    .unwrap_or_default();
    println!("\n=== TopLevelWindowForOverflowXamlIsland : {:?} ===", overflow.0);
    if overflow.0.is_null() {
        println!(
            "ABSENTE. La littérature prévient : elle n'est créée qu'à l'OUVERTURE du\n\
             chevron. Un miroir ne peut donc PAS compter sur elle — c'est une information,\n\
             pas un échec, et elle oriente vers une autre source."
        );
        return;
    }
    let mut rect = windows::Win32::Foundation::RECT::default();
    let has_rect = unsafe { GetWindowRect(overflow, &mut rect) }.is_ok();
    println!(
        "présente : visible={} rect={:?}",
        unsafe { IsWindowVisible(overflow) }.as_bool(),
        has_rect.then(|| (rect.left, rect.top, rect.right, rect.bottom))
    );
    // La passerelle de composition INTERNE, comme pour la barre.
    let bridge_class = "Windows.UI.Composition.DesktopWindowContentBridge";
    let mut nodes = Vec::new();
    walk(overflow, 0, &mut nodes);
    let Some(bridge) = nodes
        .iter()
        .map(|(hwnd, _)| *hwnd)
        .find(|hwnd| class_name(*hwnd).starts_with(bridge_class))
    else {
        println!("pas de passerelle de composition sous cette fenêtre");
        return;
    };
    let automation: windows::Win32::UI::Accessibility::IUIAutomation = match unsafe {
        windows::Win32::System::Com::CoCreateInstance(
            &windows::Win32::UI::Accessibility::CUIAutomation,
            None,
            windows::Win32::System::Com::CLSCTX_ALL,
        )
    } {
        Ok(value) => value,
        Err(error) => {
            println!("CUIAutomation indisponible : {error:?}");
            return;
        }
    };
    let Ok(element) = (unsafe { automation.ElementFromHandle(bridge) }) else {
        println!("ElementFromHandle a échoué");
        return;
    };
    let Ok(condition) = (unsafe { automation.CreateTrueCondition() }) else { return };
    let Ok(children) = (unsafe {
        element.FindAll(windows::Win32::UI::Accessibility::TreeScope_Descendants, &condition)
    }) else {
        return;
    };
    let total = unsafe { children.Length() }.unwrap_or(0);
    println!("{total} descendant(s) :");
    for index in 0..total {
        let Ok(child) = (unsafe { children.GetElement(index) }) else { continue };
        let class = unsafe { child.CurrentClassName() }.map(|c| c.to_string()).unwrap_or_default();
        let name = unsafe { child.CurrentName() }.map(|n| n.to_string()).unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        println!("  {:<28} {}", class, name.lines().next().unwrap_or("").trim());
    }
    println!();
}

/// SONDE — **COMBIEN DE FENÊTRES DE DÉBORDEMENT, ET LESQUELLES ?**
///
/// MESURÉ : la capture de la région rendue par
/// `FindWindowW(TopLevelWindowForOverflowXamlIsland)` montrait le **panneau
/// d'emojis**, alors que l'arbre de CETTE fenêtre porte bien les icônes masquées.
/// L'hypothèse à écarter avant tout le reste : **il en existe plusieurs**, et
/// `FindWindowW` rend la PREMIÈRE — pas celle qu'on croit. Une seule ligne de
/// journal suffit à le dire.
#[test]
#[ignore = "lit les fenetres de debordement de la machine"]
fn prints_every_overflow_window() {
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let out = &mut *(lparam.0 as *mut Vec<HWND>);
        out.push(hwnd);
        windows::core::BOOL(1)
    }
    let mut toutes: Vec<HWND> = Vec::new();
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&mut toutes as *mut _ as isize)) };

    let mut trouvees = 0usize;
    for hwnd in toutes {
        let classe = class_name(hwnd);
        if !classe.contains("Overflow") {
            continue;
        }
        trouvees += 1;
        let mut rect = RECT::default();
        let _ = unsafe { GetWindowRect(hwnd, &mut rect) };
        let mut enfants = Vec::new();
        walk(hwnd, 0, &mut enfants);
        println!(
            "  {:?} classe={classe} visible={} rect=({},{}) {}x{} pid={} descendants={}",
            hwnd.0,
            unsafe { IsWindowVisible(hwnd) }.as_bool(),
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            owning_pid(hwnd),
            enfants.len(),
        );
    }
    println!("{trouvees} fenetre(s) dont la classe contient « Overflow »");
    // Et ce que rend `FindWindowW` — la « première », celle que le code prend.
    let premiere = unsafe { FindWindowW(w!("TopLevelWindowForOverflowXamlIsland"), PCWSTR::null()) }
        .unwrap_or_default();
    println!("FindWindowW rend : {:?}", premiere.0);
}

/// **CAPTURER LE PANNEAU DE DÉBORDEMENT — le pas qui manquait.**
///
/// MESURE À FAIRE : les icônes MASQUÉES ne sont peintes que lorsque le chevron
/// est ouvert (voir `prints_the_hidden_notification_icons`), et **trois d'entre
/// elles n'ont aucun nom dans UI Automation** — donc ni image, ni identité, et
/// rien pour les rapprocher d'un instantané du registre, qui se fait PAR NOM.
///
/// Une capture de ce panneau leur donnerait leur **vraie image**, et cette image
/// permettrait de les identifier par RESSEMBLANCE avec les neuf instantanés
/// candidats (voir la sonde des exécutables qui tournent) — sans rien deviner.
///
/// ⚠️ **LE PANNEAU SE REFERME DÈS QUE LA SOURIS S'EN ÉCARTE.** La sonde ATTEND
/// donc qu'il devienne visible — soixante secondes — et capture **à cet
/// instant-là**, sans quoi elle ne photographierait que le bureau.
///
/// Mesure : `cargo test --release --bins -- --ignored --nocapture
/// saves_a_shot_of_the_overflow_panel`
#[test]
#[ignore = "attend que le chevron soit ouvert, puis capture le panneau"]
fn saves_a_shot_of_the_overflow_panel() {
    // **LE PROCESSUS DE TEST DOIT ÊTRE DPI-AWARE, SINON ON CAPTURE AILLEURS.**
    // MESURÉ, et c'est le piège DPI déjà documenté dans ce dépôt : sans cet
    // appel, `GetWindowRect` rend des coordonnées VIRTUALISÉES (un bureau de
    // 2560x1440 au lieu de 3840x2160, soit un facteur 1,5) alors que
    // `GetDC(None)` capture l'écran RÉEL. La capture tombait donc à un tout autre
    // endroit — le terminal de l'utilisateur, quatre fois de suite. L'application
    // est DPI-aware ; seule la sonde ne l'était pas.
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
    // Cinq minutes : une fenêtre de soixante secondes s'est révélée trop courte
    // pour que l'utilisateur la saisisse — la mesure n'a pas eu lieu, et une
    // sonde qui ne mesure rien coûte plus cher qu'une sonde lente.
    const ATTENTE_S: u32 = 300;
    println!(
        "\n*** OUVREZ LE CHEVRON DE LA ZONE DE NOTIFICATION (barre des taches) ***\n\
         *** et LAISSEZ LA SOURIS DESSUS : la sonde attend jusqu'a {ATTENTE_S} s.   ***\n"
    );
    let overflow = unsafe {
        FindWindowW(w!("TopLevelWindowForOverflowXamlIsland"), PCWSTR::null())
    }
    .unwrap_or_default();
    if overflow.0.is_null() {
        println!("fenetre de debordement ABSENTE — elle n'existe pas encore.");
        return;
    }
    // **PAS DE SIGNAL DE FENÊTRE — MESURÉ, IL N'Y EN A PAS.** `IsWindowVisible`
    // rend VRAI en permanence : la fenêtre du panneau existe et est « visible »
    // même chevron fermé, elle n'est simplement pas PEINTE. Chercher un signal
    // dans l'arbre (les rectangles des icônes changent, mais d'une façon que deux
    // relevés ne suffisent pas à qualifier) serait deviner.
    //
    // On fait donc plus simple et plus sûr : on CAPTURE EN CONTINU et on garde
    // les images qui CHANGENT. Le panneau qui s'ouvre est un changement franc de
    // toute la région ; c'est lui qu'on veut, et il sera dans les fichiers.
    let mut rect = windows::Win32::Foundation::RECT::default();
    if unsafe { GetWindowRect(overflow, &mut rect) }.is_err() {
        println!("rectangle du panneau illisible");
        return;
    }
    println!(
        "region du panneau : ({},{}) .. ({},{})",
        rect.left, rect.top, rect.right, rect.bottom
    );
    let mut precedente: Option<Vec<u8>> = None;
    let mut gardees = 0u32;
    let limite = std::time::Instant::now() + std::time::Duration::from_secs(ATTENTE_S as u64);
    while std::time::Instant::now() < limite && gardees < 40 {
        std::thread::sleep(std::time::Duration::from_millis(400));
        let Some((largeur, hauteur, rgba)) = super::tray_icons::grab_screen(rect) else { continue };
        let ecart = match &precedente {
            None => u32::MAX,
            Some(avant) if avant.len() == rgba.len() => avant
                .iter()
                .zip(rgba.iter())
                .step_by(4)
                .map(|(a, b)| a.abs_diff(*b) as u32)
                .sum::<u32>()
                / (rgba.len() as u32 / 4),
            Some(_) => u32::MAX,
        };
        precedente = Some(rgba.clone());
        // Le panneau qui s'ouvre change FRANCHEMENT la région ; un défilement de
        // texte derrière, beaucoup moins. Le seuil a été relevé après un premier
        // essai où quatre images sur quatre montraient le bureau et le terminal.
        if ecart > 30 {
            gardees += 1;
            let chemin = format!(r"C:\ndtarget\panneau-{gardees:02}.png");
            if let Some(image) = image::RgbaImage::from_raw(largeur, hauteur, rgba) {
                let _ = image.save(&chemin);
                println!("  image {gardees} : ecart {ecart} -> {chemin} ({largeur}x{hauteur})");
            }
        }
    }
    println!("{gardees} image(s) gardee(s) — ouvrez le chevron pendant l'attente.");
    // Les icônes qu'il contient, avec leur rectangle — c'est ce qui permettra de
    // découper chacune dans la capture, puis de la comparer aux instantanés.
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }
    for (nom, zone) in super::tray_icons::enumerate()
        .iter()
        .filter(|icon| icon.zone == super::tray_icons::TrayZone::Hidden)
        .map(|icon| (icon.name.lines().next().unwrap_or("").trim().to_string(), icon.rect))
    {
        println!(
            "  icone {:<44} rect ({},{},{},{})",
            if nom.is_empty() { "<SANS NOM>".to_string() } else { nom },
            zone.left,
            zone.top,
            zone.right,
            zone.bottom
        );
    }
}

///
/// L'artwork du shell Windows 11 n'est pas dans des ressources de DLL : c'est
/// une POLICE, *Segoe Fluent Icons* (`SegoeIcons.ttf`, présente sur la machine).
/// Si les icônes système de la zone de notification viennent de là, alors elles
/// sont à portée d'un rendu de texte — et le dock sait déjà rendre du texte.
///
/// Les points de code ci-dessous sont des CANDIDATS : cette planche sert
/// précisément à confirmer ou réfuter chacun en le regardant, plutôt qu'à faire
/// confiance à un tableau de correspondance recopié de mémoire.
#[test]
#[ignore = "ecrit un PNG de glyphes dans C:\\ndtarget"]
fn saves_a_sheet_of_candidate_glyphs() {
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DeleteDC, DeleteObject,
        FillRect, GetDC, ReleaseDC, SelectObject, SetBkMode, SetTextColor, TextOutW, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH,
        DIB_RGB_COLORS, FF_DONTCARE, OUT_DEFAULT_PRECIS, PROOF_QUALITY, TRANSPARENT,
    };
    use windows::Win32::Foundation::RECT;
    use windows::core::w;

    /// (point de code, ce qu'on CROIT qu'il représente) — à confirmer à l'œil.
    const CANDIDATES: &[(u16, &str)] = &[
        // Les DEUX seuls déjà retenus par la table (vérifiés une première fois).
        (0xE767, "Volume (retenu)"),
        (0xE7BA, "Avertissement (retenu)"),
        // Protection / Sécurité — pour « Vous êtes protégé » et « Sécurité
        // Windows », qui n'ont AUCUN instantané dans le registre.
        (0xE83D, "bouclier ?"),
        (0xEA18, "bouclier ?"),
        (0xE72E, "bouclier ?"),
        (0xE1F6, "bouclier ?"),
        (0xE8D7, "cadenas ?"),
        // Synchronisation — pour « Synchronisation terminée avec succès » et
        // « Steam - Synchronisation ».
        (0xE895, "sync ?"),
        (0xE117, "sync ?"),
        (0xE72C, "sync ?"),
        (0xE9E9, "sync ?"),
        // Mobile / téléphone — pour « Mobile connecté - Connecté ».
        (0xE8EA, "téléphone ?"),
        (0xE1C9, "téléphone ?"),
        (0xEA8B, "téléphone ?"),
        (0xE717, "téléphone ?"),
        // Le repli GÉNÉRIQUE, pour une entrée sans nom du tout.
        (0xE9CE, "générique ?"),
        (0xE946, "info ?"),
        (0xE915, "point ?"),
        (0xE10C, "point ?"),
        (0xE76C, "point ?"),
    ];

    const COLS: i32 = 5;
    const CELL_W: i32 = 190;
    const CELL_H: i32 = 130;
    let rows = (CANDIDATES.len() as i32 + COLS - 1) / COLS;
    let width = COLS * CELL_W;
    let height = rows * CELL_H;

    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let info = BITMAPINFO { bmiHeader: header, ..Default::default() };
    let mut pixels: *mut std::ffi::c_void = std::ptr::null_mut();
    let screen = unsafe { GetDC(None) };
    let memory = unsafe { CreateCompatibleDC(Some(screen)) };
    let Ok(bitmap) =
        (unsafe { CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut pixels, None, 0) })
    else {
        println!("CreateDIBSection a échoué");
        return;
    };
    let previous = unsafe { SelectObject(memory, bitmap.into()) };

    // Fond sombre, comme la barre du dock en thème sombre.
    let background = unsafe { CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00241E20)) };
    let full = RECT { left: 0, top: 0, right: width, bottom: height };
    unsafe { FillRect(memory, &full, background) };

    let glyph_font = unsafe {
        CreateFontW(
            64, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS, PROOF_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32, w!("Segoe Fluent Icons"),
        )
    };
    let label_font = unsafe {
        CreateFontW(
            18, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS, PROOF_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32, w!("Segoe UI"),
        )
    };
    unsafe {
        SetBkMode(memory, TRANSPARENT);
        SetTextColor(memory, windows::Win32::Foundation::COLORREF(0x00E6E6E6));
    }
    for (index, (code, label)) in CANDIDATES.iter().enumerate() {
        let cell_x = (index as i32 % COLS) * CELL_W;
        let cell_y = (index as i32 / COLS) * CELL_H;
        let glyph: Vec<u16> = vec![*code];
        unsafe {
            SelectObject(memory, glyph_font.into());
            TextOutW(memory, cell_x + 70, cell_y + 16, &glyph);
            SelectObject(memory, label_font.into());
            let caption: Vec<u16> = format!("U+{code:04X}  {label}").encode_utf16().collect();
            TextOutW(memory, cell_x + 8, cell_y + CELL_H - 26, &caption);
        }
    }
    unsafe {
        SelectObject(memory, previous);
    }

    let count = (width * height) as usize;
    let raw = unsafe { std::slice::from_raw_parts(pixels as *const u8, count * 4) };
    let mut rgba = Vec::with_capacity(count * 4);
    for pixel in raw.chunks_exact(4) {
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
    let Some(image) = image::RgbaImage::from_raw(width as u32, height as u32, rgba) else {
        println!("RgbaImage::from_raw a refusé les dimensions");
        return;
    };
    let path = r"C:\ndtarget\glyphes-segoe.png";
    match image.save(path) {
        Ok(()) => println!("écrit : {path} ({width}x{height})"),
        Err(error) => println!("écriture impossible : {error}"),
    }
    unsafe {
        let _ = DeleteObject(glyph_font.into());
        let _ = DeleteObject(label_font.into());
        let _ = DeleteObject(background.into());
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
    }
}

/// **WGC ACCEPTE-T-IL LES FENÊTRES DE LA ZONE DE NOTIFICATION ?**
///
/// Question binaire, et c'est elle qui décide si les VRAIS pixels sont
/// atteignables. Mes deux tentatives précédentes étaient en **GDI** (`BitBlt`),
/// qui a rendu du papier peint — la barre étant en auto-masquage, rien n'est
/// dessiné à l'écran. La recherche dit que **Windows.Graphics.Capture capture la
/// sortie du COMPOSITEUR**, donc une fenêtre XAML même hors écran ou réduite.
/// Elle dit aussi que `CreateForWindow` **échoue en `E_INVALIDARG`** sur une
/// fenêtre réellement invisible, non listée dans Alt-Tab.
///
/// Les deux cas sont donc ouverts, et seule la mesure tranche.
#[test]
#[ignore = "sonde WGC sur les fenetres de la zone de notification de la machine"]
fn tries_wgc_on_the_notification_area_windows() {
    let Ok(gfx) = crate::gfx::GfxDevice::new() else {
        println!("pas de device D3D11");
        return;
    };
    println!("\nWGC supporté : {}", super::wgc::is_supported());

    let mut candidates: Vec<(String, HWND)> = Vec::new();
    for (label, class) in [
        ("barre", "Shell_TrayWnd"),
        ("débordement", "TopLevelWindowForOverflowXamlIsland"),
    ] {
        let wide: Vec<u16> = class.encode_utf16().chain(std::iter::once(0)).collect();
        let root = unsafe { FindWindowW(PCWSTR(wide.as_ptr()), PCWSTR::null()) }.unwrap_or_default();
        if root.0.is_null() {
            println!("{label} : fenêtre ABSENTE");
            continue;
        }
        candidates.push((label.to_string(), root));
        // …et sa passerelle de composition, qui est la surface XAML elle-même.
        let mut nodes = Vec::new();
        walk(root, 0, &mut nodes);
        if let Some(bridge) = nodes.iter().map(|(hwnd, _)| *hwnd).find(|hwnd| {
            class_name(*hwnd).starts_with("Windows.UI.Composition.DesktopWindowContentBridge")
        }) {
            candidates.push((format!("{label} (passerelle)"), bridge));
        }
    }

    for (label, hwnd) in candidates {
        let visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
        let started = std::time::Instant::now();
        match super::wgc::CaptureSession::start(&gfx.device, hwnd) {
            Some(session) => println!(
                "  {label:<24} hwnd={:?} visible={visible} -> WGC ACCEPTE, taille {:?} (en {:?})",
                hwnd.0,
                session.size(),
                started.elapsed()
            ),
            None => println!(
                "  {label:<24} hwnd={:?} visible={visible} -> WGC REFUSE (en {:?})",
                hwnd.0,
                started.elapsed()
            ),
        }
    }
    println!();
}

/// **UNE FRAME DE LA BARRE, RELUE ET ÉCRITE EN PNG** — l'étape qui tranche
/// vraiment : ouvrir une session WGC ne prouve pas que la surface contient
/// quelque chose, et la barre est HORS ÉCRAN (auto-masquage). Si cette image
/// montre les icônes, les vrais pixels sont atteignables ; si elle est vide, ils
/// ne le sont pas et le registre reste la seule source.
#[test]
#[ignore = "ecrit un PNG de la barre des taches dans C:\\ndtarget"]
fn saves_a_wgc_frame_of_the_taskbar() {
    use windows::core::Interface;
    let Ok(gfx) = crate::gfx::GfxDevice::new() else {
        println!("pas de device D3D11");
        return;
    };
    let tray = unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.unwrap_or_default();
    if tray.0.is_null() {
        println!("pas de Shell_TrayWnd");
        return;
    }
    let Some(mut session) = super::wgc::CaptureSession::start(&gfx.device, tray) else {
        println!("WGC a refusé la barre");
        return;
    };
    println!("taille de la surface : {:?}", session.size());
    // Une frame peut demander quelques tours : on insiste, mais pas indéfiniment
    // — une fenêtre hors écran peut ne JAMAIS produire de frame, et c'est
    // précisément ce que cette sonde doit montrer.
    let mut got_frame = false;
    for attempt in 0..40 {
        if session.poll(&gfx.device) {
            println!("frame obtenue au tour {attempt}");
            got_frame = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if !got_frame {
        println!(
            "AUCUNE frame en 40 tours (2 s). C'est une réponse : la barre masquée ne produit\n\
             pas de sortie de composition, donc WGC ne peut rien en tirer."
        );
        return;
    }
    let Some(srv) = session.srv() else {
        println!("pas de SRV dans la session");
        return;
    };
    let Ok(resource) = (unsafe { srv.GetResource() }) else {
        println!("GetResource a échoué");
        return;
    };
    let Ok(texture) = resource.cast::<windows::Win32::Graphics::Direct3D11::ID3D11Texture2D>() else {
        println!("la ressource n'est pas une texture");
        return;
    };
    match crate::gfx::capture::capture_rgba(&gfx.device, &gfx.context, &texture) {
        Ok((width, height, rgba)) => {
            let path = r"C:\ndtarget\barre-wgc.png";
            match image::RgbaImage::from_raw(width, height, rgba) {
                Some(image) => match image.save(path) {
                    Ok(()) => println!("écrit : {path} ({width}x{height})"),
                    Err(error) => println!("écriture impossible : {error}"),
                },
                None => println!("dimensions refusées par RgbaImage"),
            }
        }
        Err(error) => println!("relecture impossible : {error}"),
    }
}

/// identifiant d'automatisation, interface héritée…
///
/// Sert à répondre à une question précise : **peut-on savoir À QUEL PROGRAMME
/// appartient une entrée dont le nom ne le dit pas ?** C'est ce qui déciderait
/// si les icônes manquantes peuvent être rattrapées automatiquement (aller
/// chercher l'icône de l'application reconnue) ou s'il faut se contenter d'un
/// glyphe. Le nom seul ne suffit pas : « Vous êtes protégé » ou « Synchronisation
/// terminée avec succès » ne nomment aucun produit.
#[test]
#[ignore = "lit les proprietes UI Automation des icones de notification de la machine"]
fn prints_the_extra_uia_properties_of_notification_icons() {
    use windows::Win32::UI::Accessibility::{
        IUIAutomationLegacyIAccessiblePattern, UIA_LegacyIAccessiblePatternId,
    };
    // `cast` vient d'un trait, pas de la structure — apporté par le crate
    // `windows-core` (voir l'erreur du compilateur : « items from traits can only
    // be used if the trait is in scope »).
    use windows_core::Interface;
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }
    let Ok(tray) = (unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }) else {
        println!("pas de Shell_TrayWnd");
        return;
    };
    let automation: windows::Win32::UI::Accessibility::IUIAutomation = match unsafe {
        windows::Win32::System::Com::CoCreateInstance(
            &windows::Win32::UI::Accessibility::CUIAutomation,
            None,
            windows::Win32::System::Com::CLSCTX_ALL,
        )
    } {
        Ok(value) => value,
        Err(error) => {
            println!("CUIAutomation indisponible : {error:?}");
            return;
        }
    };
    // Les DEUX zones : la barre et la fenêtre de débordement.
    for (label, root) in [("barre", tray), ("débordement", unsafe {
        FindWindowW(w!("TopLevelWindowForOverflowXamlIsland"), PCWSTR::null())
    }
    .unwrap_or_default())] {
        if root.0.is_null() {
            continue;
        }
        let mut nodes = Vec::new();
        walk(root, 0, &mut nodes);
        let Some(bridge) = nodes.iter().map(|(hwnd, _)| *hwnd).find(|hwnd| {
            class_name(*hwnd).starts_with("Windows.UI.Composition.DesktopWindowContentBridge")
        }) else {
            continue;
        };
        let Ok(element) = (unsafe { automation.ElementFromHandle(bridge) }) else { continue };
        let Ok(condition) = (unsafe { automation.CreateTrueCondition() }) else { continue };
        let Ok(children) = (unsafe {
            element.FindAll(windows::Win32::UI::Accessibility::TreeScope_Descendants, &condition)
        }) else {
            continue;
        };
        println!("\n=== {label} ===");
        for index in 0..unsafe { children.Length() }.unwrap_or(0) {
            let Ok(child) = (unsafe { children.GetElement(index) }) else { continue };
            let class = unsafe { child.CurrentClassName() }.map(|c| c.to_string()).unwrap_or_default();
            if !class.starts_with("SystemTray.") {
                continue;
            }
            let name = unsafe { child.CurrentName() }.map(|n| n.to_string()).unwrap_or_default();
            let help = unsafe { child.CurrentHelpText() }.map(|n| n.to_string()).unwrap_or_default();
            let automation_id =
                unsafe { child.CurrentAutomationId() }.map(|n| n.to_string()).unwrap_or_default();
            let status = unsafe { child.CurrentItemStatus() }.map(|n| n.to_string()).unwrap_or_default();
            let framework =
                unsafe { child.CurrentFrameworkId() }.map(|n| n.to_string()).unwrap_or_default();
            print!(
                "  {:<40} aide={help:?} id={automation_id:?} etat={status:?} cadre={framework:?}",
                name.lines().next().unwrap_or("").trim()
            );
            // L'interface HÉRITÉE porte souvent ce que les propriétés modernes
            // ont perdu — c'est précisément ce qu'on cherche ici.
            if let Ok(pattern) = unsafe { child.GetCurrentPattern(UIA_LegacyIAccessiblePatternId) } {
                if let Ok(legacy) = pattern.cast::<IUIAutomationLegacyIAccessiblePattern>() {
                    let lname = unsafe { legacy.CurrentName() }.map(|n| n.to_string()).unwrap_or_default();
                    let ldesc =
                        unsafe { legacy.CurrentDescription() }.map(|n| n.to_string()).unwrap_or_default();
                    let lvalue =
                        unsafe { legacy.CurrentValue() }.map(|n| n.to_string()).unwrap_or_default();
                    print!(" hérité(nom={lname:?} desc={ldesc:?} valeur={lvalue:?})");
                }
            }
            println!();
        }
    }
}

/// **UNE ICÔNE DE NOTIFICATION EST-ELLE *INVOCABLE* ?** — la question qui décide
/// si un clic GAUCHE sur notre miroir peut se traiter comme le clic DROIT se
/// traite déjà (`ShowContextMenu`) : par UI Automation, sans simuler de souris.
///
/// ⚠️ **LECTURE SEULE** : on interroge des propriétés, on n'appelle ni `Invoke`
/// ni quoi que ce soit d'autre. Aucun effet sur le bureau de l'utilisateur.
///
/// Rend, par icône : la classe, si le motif Invoke est DISPONIBLE, et le nom de
/// l'action par défaut que porte le motif hérité (« Ouvrir », « Afficher », …) —
/// c'est ce dernier qui dit ce qu'un déclenchement FERAIT.
#[test]
#[ignore = "lit les motifs UI Automation des icones de notification de la machine"]
fn une_icone_de_notification_est_elle_invocable() {
    use windows::Win32::UI::Accessibility::{
        IUIAutomationInvokePattern, IUIAutomationLegacyIAccessiblePattern, UIA_InvokePatternId,
        UIA_LegacyIAccessiblePatternId,
    };
    use windows_core::Interface;
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }
    let Ok(tray) = (unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }) else {
        println!("pas de Shell_TrayWnd");
        return;
    };
    let automation: windows::Win32::UI::Accessibility::IUIAutomation = match unsafe {
        windows::Win32::System::Com::CoCreateInstance(
            &windows::Win32::UI::Accessibility::CUIAutomation,
            None,
            windows::Win32::System::Com::CLSCTX_ALL,
        )
    } {
        Ok(value) => value,
        Err(error) => {
            println!("CUIAutomation indisponible : {error:?}");
            return;
        }
    };
    for (label, root) in [
        ("barre", tray),
        (
            "débordement",
            unsafe { FindWindowW(w!("TopLevelWindowForOverflowXamlIsland"), PCWSTR::null()) }
                .unwrap_or_default(),
        ),
    ] {
        if root.0.is_null() {
            continue;
        }
        let mut nodes = Vec::new();
        walk(root, 0, &mut nodes);
        let Some(bridge) = nodes.iter().map(|(hwnd, _)| *hwnd).find(|hwnd| {
            class_name(*hwnd).starts_with("Windows.UI.Composition.DesktopWindowContentBridge")
        }) else {
            continue;
        };
        let Ok(element) = (unsafe { automation.ElementFromHandle(bridge) }) else { continue };
        let Ok(condition) = (unsafe { automation.CreateTrueCondition() }) else { continue };
        let Ok(children) = (unsafe {
            element.FindAll(windows::Win32::UI::Accessibility::TreeScope_Descendants, &condition)
        }) else {
            continue;
        };
        println!("\n=== {label} ===");
        let mut invocables = 0;
        let mut total = 0;
        for index in 0..unsafe { children.Length() }.unwrap_or(0) {
            let Ok(child) = (unsafe { children.GetElement(index) }) else { continue };
            let class = unsafe { child.CurrentClassName() }.map(|c| c.to_string()).unwrap_or_default();
            if !class.starts_with("SystemTray.") {
                continue;
            }
            total += 1;
            let name = unsafe { child.CurrentName() }.map(|n| n.to_string()).unwrap_or_default();
            // Le motif lui-même : sa présence EST la réponse, et elle évite de
            // déballer un `VARIANT` (dont le booléen est un `VARIANT_BOOL`).
            let motif = unsafe { child.GetCurrentPattern(UIA_InvokePatternId) }
                .ok()
                .and_then(|p| p.cast::<IUIAutomationInvokePattern>().ok())
                .is_some();
            if motif {
                invocables += 1;
            }
            // 3. L'action par défaut du motif hérité — ce qu'un déclenchement FERAIT.
            let defaut = unsafe { child.GetCurrentPattern(UIA_LegacyIAccessiblePatternId) }
                .ok()
                .and_then(|p| p.cast::<IUIAutomationLegacyIAccessiblePattern>().ok())
                .and_then(|legacy| unsafe { legacy.CurrentDefaultAction() }.ok())
                .map(|a| a.to_string())
                .unwrap_or_default();
            println!(
                "  {:<44} {:<28} invoke={:<9} defaut={defaut:?}",
                name.lines().next().unwrap_or("").trim(),
                class,
                if motif { "OUI" } else { "non" }
            );
        }
        println!("  -> {invocables} invocable(s) sur {total} élément(s) SystemTray.*");
    }
}

/// ⛔ **POURQUOI LA ZONE VISIBLE REND ZÉRO ?** — la question du 2026-10-06, après
/// que l'utilisateur a signalé « je ne vois plus du tout les icônes de Windows »
/// alors que SA barre des tâches les affichait toujours.
///
/// On ne se contente pas de compter : on **nomme la fenêtre trouvée et son
/// propriétaire** (c'est ce qui démasque une fenêtre IMPOSTEUR homonyme — voir
/// [[neuradesk-tray-copydata-protocole-vivant]]), on descend jusqu'à la
/// passerelle de composition, et on relève **toutes** les classes du sous-arbre.
/// Compter les seules `SystemTray.*` ne dirait pas si l'arbre est vide ou si
/// c'est le filtre qui les écarte.
#[test]
#[ignore = "lit l'arbre de la barre des taches de la machine"]
fn pourquoi_la_zone_visible_rend_zero() {

    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }
    let barre = unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) };
    match barre {
        Ok(hwnd) => {
            let (proprietaire, _) = crate::win32::app_identity::window_owner(hwnd);
            println!("Shell_TrayWnd            : 0x{:x} — appartient à {proprietaire}", hwnd.0 as isize);
        }
        Err(e) => {
            println!("Shell_TrayWnd            : INTROUVABLE ({e:?})");
            return;
        }
    }
    let hwnd = barre.unwrap();
    // TOUT le sous-arbre, pas seulement les enfants directs.
    println!("--- fenêtres descendantes de Shell_TrayWnd ---");
    let mut tous = Vec::new();
    walk(hwnd, 0, &mut tous);
    println!("  {} fenêtre(s) descendante(s)", tous.len());
    for (h, profondeur) in tous.iter() {
        let (proprietaire, _) = crate::win32::app_identity::window_owner(*h);
        println!("    [prof {profondeur}] 0x{:x} classe={:?} <- {proprietaire}", h.0 as isize, class_name(*h));
    }
    // ⭐ **TOUTES LES PASSERELLES, ET CE QUE CHACUNE CONTIENT.** C'est la mesure
    // qui tranche : s'il y en a plusieurs, prendre « la première » est un pari —
    // et ce pari est perdu aujourd'hui.
    let passerelles: Vec<HWND> = tous
        .iter()
        .map(|(h, _)| *h)
        .filter(|h| class_name(*h).starts_with("Windows.UI.Composition.DesktopWindowContentBridge"))
        .collect();
    println!("PASSERELLES               : {}", passerelles.len());
    let automation: windows::Win32::UI::Accessibility::IUIAutomation = match unsafe {
        windows::Win32::System::Com::CoCreateInstance(
            &windows::Win32::UI::Accessibility::CUIAutomation,
            None,
            windows::Win32::System::Com::CLSCTX_ALL,
        )
    } {
        Ok(v) => v,
        Err(e) => {
            println!("CUIAutomation indisponible : {e:?}");
            return;
        }
    };
    // ⭐ **OÙ EST PASSÉE L'ÎLE DE LA ZONE DE NOTIFICATION ?** La seule passerelle
    // sous `Shell_TrayWnd` porte la LISTE DES APPLICATIONS. On balaie donc TOUTES
    // les fenêtres de premier niveau d'`explorer.exe` et on compte les
    // `SystemTray.*` de chacune — c'est la seule façon de trouver l'île qui les
    // porte AUJOURD'HUI, sans supposer qu'elle n'a pas bougé.
    println!("--- SystemTray.* par fenêtre de PREMIER NIVEAU d'explorer.exe ---");
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let out = &mut *(lparam.0 as *mut Vec<HWND>);
        out.push(hwnd);
        windows::core::BOOL(1)
    }
    let mut premier_niveau: Vec<HWND> = Vec::new();
    let _ = unsafe {
        EnumWindows(Some(collect), LPARAM(&mut premier_niveau as *mut _ as isize))
    };
    for h in premier_niveau {
        let classe = class_name(h);
        let (proprietaire, _) = crate::win32::app_identity::window_owner(h);
        if !proprietaire.to_ascii_lowercase().contains("explorer") {
            continue;
        }
        let Ok(element) = (unsafe { automation.ElementFromHandle(h) }) else { continue };
        let Ok(condition) = (unsafe { automation.CreateTrueCondition() }) else { continue };
        let Ok(children) = (unsafe {
            element.FindAll(windows::Win32::UI::Accessibility::TreeScope_Descendants, &condition)
        }) else {
            continue;
        };
        let total = unsafe { children.Length() }.unwrap_or(0);
        let mut sys = 0usize;
        let mut noms: Vec<String> = Vec::new();
        for index in 0..total {
            let Ok(child) = (unsafe { children.GetElement(index) }) else { continue };
            let c = unsafe { child.CurrentClassName() }.map(|c| c.to_string()).unwrap_or_default();
            if c.starts_with("SystemTray.") {
                sys += 1;
                if noms.len() < 5 {
                    noms.push(
                        unsafe { child.CurrentName() }
                            .map(|n| n.to_string())
                            .unwrap_or_default()
                            .lines()
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_string(),
                    );
                }
            }
        }
        if sys > 0 || classe.contains("Bridge") || classe.contains("CoreWindow") || classe.contains("Tray") {
            println!(
                "  0x{:x} {classe:<45} {total:>4} élém., {sys:>3} SystemTray.*  {noms:?}",
                h.0 as isize
            );
        }
    }
}

///
/// Sert à juger le RAPPROCHEMENT en le regardant : quand l'infobulle d'un
/// instantané est vide, le rapprochement s'est fait par l'EXÉCUTABLE (le
/// troisième niveau de `match_snapshot`), et seule l'image permet de dire si
/// c'est la bonne. Un journal ne le dirait pas — il ne montre pas les pixels.
#[test]#[ignore = "ecrit un PNG de verification dans C:\\ndtarget"]
fn saves_a_sheet_of_matched_icons() {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DeleteDC, DeleteObject,
        FillRect, GetDC, ReleaseDC, SelectObject, SetBkMode, SetTextColor, TextOutW, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH,
        DIB_RGB_COLORS, FF_DONTCARE, OUT_DEFAULT_PRECIS, PROOF_QUALITY, TRANSPARENT,
    };
    use windows::core::w;

    let live = super::tray_icons::enumerate();
    let snapshots = super::tray_icons::read_snapshots();
    println!("{} entrée(s) vivante(s), {} instantané(s)", live.len(), snapshots.len());
    if live.is_empty() {
        return;
    }

    const COLS: i32 = 3;
    const CELL_W: i32 = 430;
    const CELL_H: i32 = 58;
    let rows = (live.len() as i32 + COLS - 1) / COLS;
    let width = COLS * CELL_W;
    let height = rows * CELL_H;

    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let info = BITMAPINFO { bmiHeader: header, ..Default::default() };
    let mut pixels: *mut std::ffi::c_void = std::ptr::null_mut();
    let screen = unsafe { GetDC(None) };
    let memory = unsafe { CreateCompatibleDC(Some(screen)) };
    let Ok(bitmap) =
        (unsafe { CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut pixels, None, 0) })
    else {
        println!("CreateDIBSection a échoué");
        return;
    };
    let previous = unsafe { SelectObject(memory, bitmap.into()) };
    let background = unsafe { CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00241E20)) };
    let full = RECT { left: 0, top: 0, right: width, bottom: height };
    unsafe { FillRect(memory, &full, background) };
    let font = unsafe {
        CreateFontW(
            16, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
            PROOF_QUALITY, (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32, w!("Segoe UI"),
        )
    };
    unsafe {
        SetBkMode(memory, TRANSPARENT);
        SetTextColor(memory, windows::Win32::Foundation::COLORREF(0x00E6E6E6));
        SelectObject(memory, font.into());
    }

    // Le tampon du DIB, en BGRA, pour y déposer les icônes décodées.
    let count = (width * height) as usize;
    let canvas = unsafe { std::slice::from_raw_parts_mut(pixels as *mut u8, count * 4) };
    let mut stats = Vec::new();
    for (index, icon) in live.iter().enumerate() {
        let cell_x = (index as i32 % COLS) * CELL_W;
        let cell_y = (index as i32 / COLS) * CELL_H;
        let matched = super::tray_icons::match_snapshot(&icon.name, &snapshots);
        let provenance = match matched {
            Some(i) => format!(
                "snapshot tooltip={:?} exe={}",
                snapshots[i].tooltip,
                snapshots[i].executable.rsplit('\\').next().unwrap_or("")
            ),
            None => "AUCUNE correspondance".to_string(),
        };
        stats.push(format!(
            "{:<46} <- {provenance}",
            icon.name.lines().next().unwrap_or("").trim()
        ));
        if let Some(i) = matched {
            if let Ok(image) = image::load_from_memory(&snapshots[i].png) {
                let rgba = image.to_rgba8();
                for (x, y, pixel) in rgba.enumerate_pixels() {
                    let dx = cell_x + 6 + x as i32;
                    let dy = cell_y + 12 + y as i32;
                    if dx < 0 || dy < 0 || dx >= width || dy >= height {
                        continue;
                    }
                    let offset = ((dy * width + dx) * 4) as usize;
                    canvas[offset] = pixel[2];
                    canvas[offset + 1] = pixel[1];
                    canvas[offset + 2] = pixel[0];
                    canvas[offset + 3] = 255;
                }
            }
        }
        let name: Vec<u16> = icon
            .name
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .chars()
            .take(46)
            .collect::<String>()
            .encode_utf16()
            .collect();
        unsafe { TextOutW(memory, cell_x + 46, cell_y + 10, &name) };
        let detail: Vec<u16> = provenance.chars().take(54).collect::<String>().encode_utf16().collect();
        unsafe { TextOutW(memory, cell_x + 46, cell_y + 30, &detail) };
    }
    unsafe {
        SelectObject(memory, previous);
    }

    let mut rgba = Vec::with_capacity(count * 4);
    for pixel in canvas.chunks_exact(4) {
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
    let Some(image) = image::RgbaImage::from_raw(width as u32, height as u32, rgba) else {
        println!("RgbaImage::from_raw a refusé les dimensions");
        return;
    };
    let path = r"C:\ndtarget\rapprochements.png";
    match image.save(path) {
        Ok(()) => println!("écrit : {path} ({width}x{height})"),
        Err(error) => println!("écriture impossible : {error}"),
    }
    println!("\n--- provenances ---\n{}", stats.join("\n"));
    unsafe {
        let _ = DeleteObject(font.into());
        let _ = DeleteObject(background.into());
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
    }
}

/// **LE SHELL SAIT-IL OÙ SE TROUVE L'ICÔNE D'UN GUID QUI NE NOUS APPARTIENT
/// PAS ?**
///
/// **CE QU'ELLE MESURE, ET POURQUOI ELLE EXISTE.** L'identification d'une
/// entrée SANS NOM bute sur le rapprochement : on sait CAPTURER l'icône
/// affichée, et le registre `HKCU\Control Panel\NotifyIconSettings` porte pour
/// chaque entrée un `ExecutablePath` et un `IconSnapshot` — mais il faut les
/// RELIER. Le rapprochement par image échoue quand l'instantané a vieilli
/// (Discord : cercle blanc à l'écran, logo au registre), et l'élimination
/// refuse à plus d'un candidat (21, mesuré).
///
/// Or ce même registre porte, pour **38 des 142 entrées**, une valeur
/// `IconGuid` — l'identité que l'application a donnée à son icône par
/// `NIF_GUID`. Et `Shell_NotifyIconGetRect` est une API **DOCUMENTÉE** qui
/// prend un `NOTIFYICONIDENTIFIER`, donc **accepte un GUID**, et rend le
/// rectangle de l'icône.
///
/// Si le shell répond pour des GUIDs qu'on ne possède pas, alors la chaîne est
/// complète **sans jamais toucher au shell** : GUID du registre → position à
/// l'écran → l'icône affichée à cette position → `ExecutablePath`. Discord,
/// Phone Link et Copilot sont tous trois identifiés par GUID.
///
/// ⚠️ **ELLE EST EN LECTURE SEULE** : elle interroge, elle ne modifie rien,
/// n'ajoute ni ne retire aucune icône, et n'installe aucune fenêtre. C'est
/// l'inverse exact de `tray_spy_probe`.
///
/// ⚠️ **SANS `SetProcessDpiAwarenessContext`, LES RECTANGLES MENTENT** — un
/// processus non DPI-aware reçoit des coordonnées **virtualisées** (2560×1440)
/// alors que le bureau est en 3840×2160. C'est le piège déjà payé deux fois
/// dans ce dépôt (voir `saves_a_shot_of_the_overflow_panel`).
///
/// Mesure : `cargo test --release --bins -- --ignored --nocapture
/// asks_the_shell_where_each_guid_identified_icon_is`
#[test]
#[ignore]
fn asks_the_shell_where_each_guid_identified_icon_is() {
    use windows::Win32::System::Registry::HKEY_CURRENT_USER;
    use windows::Win32::UI::Shell::{Shell_NotifyIconGetRect, NOTIFYICONIDENTIFIER};

    // Le piège DPI, désarmé AVANT toute mesure : sans cela les coordonnées
    // rendues sont virtualisées et ne se comparent à rien.
    let _ = unsafe {
        windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )
    };

    // ⚠️ **LE PANNEAU DE DÉBORDEMENT DOIT ÊTRE OUVERT, ET C'EST MESURÉ.**
    //
    // Panneau FERMÉ, `enumerate()` rend un rectangle **DÉGÉNÉRÉ** (`0,0 0x0`)
    // pour les 45 entrées masquées : elles n'ont aucune position tant que le
    // panneau n'est pas peint. Le croisement est donc **impossible** sans lui.
    //
    // L'ouverture est le fait de L'UTILISATEUR : ce dépôt ne simule aucune
    // entrée (voir la fiche `neuradesk-live-input-safety`). La sonde attend, et
    // abandonne au bout du délai plutôt que de mesurer un panneau fermé en
    // croyant mesurer autre chose.
    const ATTENTE_PANNEAU_S: u64 = 300;
    println!(
        "En attente du panneau de débordement — OUVRE le chevron des icônes masquées \
         (abandon dans {ATTENTE_PANNEAU_S} s)…"
    );
    let debut = Instant::now();
    loop {
        let ouvert = unsafe {
            FindWindowW(w!("TopLevelWindowForOverflowXamlIsland"), PCWSTR::null())
        }
        .map(|fenetre| unsafe { IsWindowVisible(fenetre) }.as_bool())
        .unwrap_or(false);
        if ouvert {
            break;
        }
        if debut.elapsed().as_secs() > ATTENTE_PANNEAU_S {
            println!("Panneau jamais ouvert — mesure abandonnée, RIEN n'a été relevé.");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    println!("panneau OUVERT — mesure.\n");

    const BASE: &str = r"Control Panel\NotifyIconSettings";
    let cles = crate::win32::registry_util::enumerate_subkeys(HKEY_CURRENT_USER, BASE);
    println!("{} entrée(s) sous « {BASE} »", cles.len());

    // **LA GÉOMÉTRIE RÉELLE, POUR POUVOIR INTERPRÉTER.** Cinq GUIDs différents
    // rendant le MÊME rectangle ne situent aucune icône — reste à savoir de quoi
    // ce rectangle est le rectangle. Le comparer à la barre, à la zone de
    // notification et au panneau de débordement tranche entre « une position
    // réelle mais partagée » et « un repli générique ».
    unsafe {
        let mut r = RECT::default();
        if let Ok(barre) = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) {
            let _ = GetWindowRect(barre, &mut r);
            println!(
                "Shell_TrayWnd     {} {} {}x{}",
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top
            );
        }
        if let Ok(debord) = FindWindowW(w!("TopLevelWindowForOverflowXamlIsland"), PCWSTR::null()) {
            let mut d = RECT::default();
            let _ = GetWindowRect(debord, &mut d);
            println!(
                "panneau débord.   {} {} {}x{}  visible={}",
                d.left,
                d.top,
                d.right - d.left,
                d.bottom - d.top,
                IsWindowVisible(debord).as_bool()
            );
        }
    }

    let mut avec_guid = 0usize;
    let mut localisees = 0usize;
    let mut par_uid = 0usize;
    // Les GUIDs que le shell a situés, gardés pour le CROISEMENT qui suit.
    let mut situes: Vec<(String, RECT)> = Vec::new();

    for cle in cles {
        let sous = format!(r"{BASE}\{cle}");
        let Some(brut) =
            crate::win32::registry_util::read_string_value(HKEY_CURRENT_USER, &sous, "IconGuid")
        else {
            if crate::win32::registry_util::read_string_value(
                HKEY_CURRENT_USER,
                &sous,
                "ExecutablePath",
            )
            .is_some()
            {
                par_uid += 1;
            }
            continue;
        };
        let exe =
            crate::win32::registry_util::read_string_value(HKEY_CURRENT_USER, &sous, "ExecutablePath")
                .unwrap_or_default();
        let nom = exe
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(&exe)
            .to_string();

        // **LA FONCTION DE PRODUCTION, PAS UNE COPIE** : garder deux décodeurs
        // ferait mesurer à cette sonde autre chose que ce que fait le logiciel
        // livré. Le sens de la dépendance va de la sonde vers la production
        // (`tray_probe` est `#[cfg(test)]`), jamais l'inverse.
        let Some(guid) = crate::win32::tray_icons::guid_from_registry_text(&brut) else {
            println!("  {nom:<28} GUID ILLISIBLE : {brut:?}");
            continue;
        };
        avec_guid += 1;

        let identifiant = NOTIFYICONIDENTIFIER {
            cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: HWND::default(),
            uID: 0,
            guidItem: guid,
        };
        match unsafe { Shell_NotifyIconGetRect(&identifiant) } {
            Ok(rect) => {
                localisees += 1;
                situes.push((nom.clone(), rect));
                println!(
                    "  {nom:<28} LOCALISÉE  x={} y={} {}x{}",
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top
                );
            }
            Err(erreur) => println!("  {nom:<28} {erreur:?}"),
        }
    }

    // ══ LE CROISEMENT — ce qui décide si la voie sert à quelque chose ════════
    //
    // `enumerate()` est appelé **dos à dos** avec les `GetRect` ci-dessus : les
    // deux jeux de rectangles doivent décrire la MÊME disposition, sinon la
    // comparaison ne veut rien dire.
    let icones = crate::win32::tray_icons::enumerate();
    println!(
        "\n{} icône(s) énumérée(s) par UI Automation :",
        icones.len()
    );
    for icone in &icones {
        println!(
            "    {:<28} zone={:?} shell={} x={} y={} {}x{}",
            format!("{:?}", icone.name.lines().next().unwrap_or("").trim()),
            icone.zone,
            icone.from_shell,
            icone.rect.left,
            icone.rect.top,
            icone.rect.right - icone.rect.left,
            icone.rect.bottom - icone.rect.top
        );
    }

    // Combien de GUIDs réclament le MÊME rectangle. **C'est la mesure qui
    // justifie la règle d'unicité** : un rectangle réclamé par plusieurs GUIDs
    // est ambigu (c'est le chevron), et il doit être ÉCARTÉ — jamais deviné par
    // une géométrie codée en dur, qui serait fausse ailleurs qu'ici, ni par
    // l'exclusion par NOM de `EXCLUDED_NAME_PREFIXES`, qui est française.
    let identiques = |a: &RECT, b: &RECT| {
        a.left == b.left && a.top == b.top && a.right == b.right && a.bottom == b.bottom
    };

    let mut sur_visible = 0usize;
    let mut sur_masquee = 0usize;
    let mut ecarts: Vec<(String, i32, i32, i32, i32)> = Vec::new();

    for (nom, rect) in &situes {
        let partage = situes.iter().filter(|(_, a)| identiques(a, rect)).count();
        let mut meilleur: Option<(usize, i32, i32, i32, i32)> = None;
        for (index, icone) in icones.iter().enumerate() {
            let ecart = (
                rect.left - icone.rect.left,
                rect.top - icone.rect.top,
                (rect.right - rect.left) - (icone.rect.right - icone.rect.left),
                (rect.bottom - rect.top) - (icone.rect.bottom - icone.rect.top),
            );
            let distance = ecart.0.abs() + ecart.1.abs() + ecart.2.abs() + ecart.3.abs();
            if meilleur.is_none_or(|(_, _, _, _, d)| distance < d) {
                meilleur = Some((index, ecart.0, ecart.1, ecart.2, ecart.3));
            }
        }
        let Some((index, dx, dy, dw, dh)) = meilleur else {
            continue;
        };
        if dx == 0 && dy == 0 && dw == 0 && dh == 0 {
            match icones[index].zone {
                crate::win32::tray_icons::TrayZone::Visible => sur_visible += 1,
                crate::win32::tray_icons::TrayZone::Hidden => sur_masquee += 1,
            }
            println!(
                "  {nom:<28} ÉGALITÉ EXACTE  zone={:?} nom UIA={:?} partagé par {partage} GUID(s)",
                icones[index].zone, icones[index].name
            );
        } else {
            ecarts.push((nom.clone(), dx, dy, dw, dh));
            println!(
                "  {nom:<28} AUCUNE ÉGALITÉ  plus proche : dx={dx} dy={dy} dL={dw} dH={dh}  partagé par {partage} GUID(s)"
            );
        }
    }

    println!("\n══ CE QUE LE CROISEMENT ÉTABLIT ══");
    println!("égalité EXACTE sur une entrée VISIBLE  : {sur_visible}");
    println!("égalité EXACTE sur une entrée MASQUÉE  : {sur_masquee}");
    println!("situé mais AUCUNE égalité              : {}", ecarts.len());
    if !ecarts.is_empty() {
        let plus_petit = ecarts
            .iter()
            .map(|(_, dx, dy, dw, dh)| dx.abs() + dy.abs() + dw.abs() + dh.abs())
            .min()
            .unwrap_or(0);
        let plus_grand = ecarts
            .iter()
            .map(|(_, dx, dy, dw, dh)| dx.abs() + dy.abs() + dw.abs() + dh.abs())
            .max()
            .unwrap_or(0);
        println!("  écart minimal / maximal (somme)      : {plus_petit} / {plus_grand}");
        println!("  → si ces écarts sont PETITS et constants, l'égalité stricte est trop");
        println!("    stricte et il faut une tolérance ; s'ils sont GRANDS, ce sont d'autres");
        println!("    icônes et il ne faut PAS de tolérance.");
    }


    println!(
        "\n{avec_guid} entrée(s) identifiée(s) par GUID, {par_uid} par (hWnd, uID)"
    );
    println!("{localisees} localisée(s) par le shell");
    println!("\n── LECTURE ──");
    if localisees > 0 {
        println!("Le shell SITUE des icônes par GUID : la chaîne GUID → position →");
        println!("icône affichée → ExecutablePath est ouverte, et elle ne touche PAS");
        println!("au shell (aucune fenêtre imposteur, aucune interception).");
    } else if avec_guid > 0 {
        println!("Aucune localisation. Le shell refuse de situer une icône qu'on ne");
        println!("possède pas — la voie est fermée, et c'est une mesure.");
    } else {
        println!("Aucun GUID lu : vérifier le chemin de la clé.");
    }
}

// Le décodeur `{GUID}` de cette sonde a été SUPPRIMÉ : il faisait double emploi
// avec `tray_icons::guid_from_registry_text`, et il tolérait en plus une forme
// sans tirets que le registre ne produit jamais. La sonde appelle désormais la
// fonction de production — mesurer sur une copie, c'est mesurer un mensonge.

// ═══════════════════════════════════════════════════════════════════════════
// **LE PARTAGE DES DEUX ZONES — 11 VISIBLES, 34 MASQUÉES.**
//
// MESURÉ le 2026-09-21, et c'est cette sonde qui a résolu l'énigme du journal :
// `enumerate()` concatène DEUX zones indépendantes (`tray_icons.rs:263-267`),
// les VISIBLES sous `Shell_TrayWnd` et les MASQUÉES sous
// `TopLevelWindowForOverflowXamlIsland`, et **chacune peut rendre vide sans
// rien signaler**.
//
// Un `45 → 34 → 45` en dix secondes, sans qu'aucune icône n'ait bougé, ne
// s'expliquait que par là — encore fallait-il savoir **laquelle** des deux
// s'était tue. La réponse est contre-intuitive : c'est **la VISIBLE** (11
// entrées), la masquée (34) ayant survécu intacte. D'où la règle de
// `tray_worker::zone_eteinte`, qui refuse de publier une lecture où une zone
// entière s'est éteinte.
//
// **SONDE DURABLE** — c'est la première chose à relancer si le miroir reperd des
// icônes : le partage 11/34 est ce qui distingue « une zone s'est tue » de « des
// icônes ont réellement bougé ». Et si un jour elle rend un total différent de
// 45, ce sont les zones elles-mêmes qui ont changé de forme.
//
// Mesure : `cargo test --release --bins -- --ignored --nocapture
// counts_what_each_tray_zone_contributes`
// ═══════════════════════════════════════════════════════════════════════════
#[test]
#[ignore]
fn counts_what_each_tray_zone_contributes() {
    use crate::win32::tray_icons::{TrayIcon, TrayZone};

    // Le piège DPI, désarmé AVANT toute mesure — troisième fois que ce dépôt le
    // paie, donc il est posé ici sans commentaire inutile.
    let _ = unsafe {
        windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )
    };

    let racine: Vec<u16> = "TopLevelWindowForOverflowXamlIsland\0".encode_utf16().collect();
    let fenetre = unsafe {
        windows::Win32::UI::WindowsAndMessaging::FindWindowW(
            windows::core::PCWSTR(racine.as_ptr()),
            windows::core::PCWSTR::null(),
        )
    };

    println!("\n── LA FENÊTRE QUI PORTE LES MASQUÉES ──");
    match fenetre {
        Ok(h) if !h.0.is_null() => println!("  FindWindowW : TROUVÉE ({:?})", h.0),
        Ok(_) => println!("  FindWindowW : handle NUL"),
        Err(e) => println!("  FindWindowW : ÉCHEC {e:?} ⇒ la zone masquée rend VIDE, en silence"),
    }

    let noms = |liste: &[TrayIcon], zone: TrayZone, n: usize| -> Vec<String> {
        liste
            .iter()
            .filter(|x| x.zone == zone)
            .take(n)
            .map(|x| x.name.lines().next().unwrap_or("").trim().to_string())
            .collect()
    };

    println!("\n── SIX RELEVÉS CONSÉCUTIFS ──");
    for i in 0..6 {
        let debut = Instant::now();
        let icones = crate::win32::tray_icons::enumerate();
        let duree = debut.elapsed();
        let visibles = icones.iter().filter(|x| x.zone == TrayZone::Visible).count();
        let masquees = icones.iter().filter(|x| x.zone == TrayZone::Hidden).count();
        println!(
            "  #{i}  {duree:>10.2?}  total {:>3}  =  visibles {:>3}  +  masquées {:>3}",
            icones.len(),
            visibles,
            masquees
        );
        if i == 0 {
            println!("      6 premières VISIBLES : {:?}", noms(&icones, TrayZone::Visible, 6));
            println!("      6 premières MASQUÉES : {:?}", noms(&icones, TrayZone::Hidden, 6));
        }
        std::thread::sleep(std::time::Duration::from_millis(1200));
    }
    println!();
}

/// ⛔ **L'ÉTAT RÉEL DES FENÊTRES, ET SANS RIEN OUVRIR NI TOUCHER.**
///
/// Écrite le 2026-10-08 pour trancher un relevé contradictoire : `enumerate()`
/// rendait 0 visible **et** 0 masquée, et `enumerate_avec_le_panneau` rendait la
/// main en **850 µs** — donc sans faire l'aller-retour de 750 ms. Or la seule
/// sortie rapide est « le panneau est DÉJÀ visible ». Deux hypothèses restaient,
/// et il fallait les départager au lieu de choisir :
///
/// 1. le panneau de débordement est **ouvert** (⇒ plus de passerelle sous
///    `Shell_TrayWnd`, donc 0 visible — défaut documenté l. 2317) ;
/// 2. cette machine porte **plusieurs** fenêtres de classe `Shell_TrayWnd` —
///    notre dock en installe une (voir `tray_flux`) — et c'est la nôtre qu'une
///    sonde trouve en premier, parce qu'elle n'a pas de `NOTRE_HWND` à sauter.
#[test]
#[ignore]
fn etat_des_fenetres_de_la_zone_de_notification() {
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, FindWindowW, GetWindow, GetWindowThreadProcessId, IsWindowVisible, GW_CHILD,
        GW_HWNDNEXT,
    };

    let _ = unsafe {
        windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )
    };

    let enfants = |fenetre| -> usize {
        let mut n = 0usize;
        let mut enfant = unsafe { GetWindow(fenetre, GW_CHILD) }.unwrap_or_default();
        while !enfant.0.is_null() {
            n += 1;
            enfant = unsafe { GetWindow(enfant, GW_HWNDNEXT) }.unwrap_or_default();
        }
        n
    };

    let proprietaire = |fenetre| -> u32 {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(fenetre, Some(&mut pid)) };
        pid
    };

    println!("\n── LE PANNEAU DE DÉBORDEMENT ──");
    match unsafe { FindWindowW(w!("TopLevelWindowForOverflowXamlIsland"), windows::core::PCWSTR::null()) } {
        Ok(fenetre) if !fenetre.0.is_null() => println!(
            "  PRÉSENTE {:?} — visible={} — pid={} — {} enfant(s) direct(s)",
            fenetre.0,
            unsafe { IsWindowVisible(fenetre) }.as_bool(),
            proprietaire(fenetre),
            enfants(fenetre),
        ),
        Ok(_) => println!("  handle NUL"),
        Err(_) => println!("  ABSENTE (FindWindowW a échoué) — l'état de repos normal, panneau fermé"),
    }

    println!("\n── TOUTES LES FENÊTRES DE CLASSE Shell_TrayWnd ──");
    let mut courante =
        unsafe { FindWindowW(w!("Shell_TrayWnd"), windows::core::PCWSTR::null()) }.unwrap_or_default();
    let nous = std::process::id();
    let mut rang = 0usize;
    while !courante.0.is_null() {
        let pid = proprietaire(courante);
        println!(
            "  #{rang} {:?} — pid={pid}{} — visible={} — {} enfant(s) direct(s)",
            courante.0,
            if pid == nous { " (CETTE SONDE)" } else { "" },
            unsafe { IsWindowVisible(courante) }.as_bool(),
            enfants(courante),
        );
        rang += 1;
        match unsafe {
            FindWindowExW(None, Some(courante), w!("Shell_TrayWnd"), windows::core::PCWSTR::null())
        } {
            Ok(suivante) if !suivante.0.is_null() => courante = suivante,
            _ => break,
        }
    }
    if rang == 0 {
        println!("  AUCUNE — la barre des tâches n'est pas trouvée par son nom");
    }
    println!("  (notre dock tourne sous le pid {nous} s'il est lancé depuis cette machine)");
    println!();
}

/// ⭐ **LA MESURE QUI MANQUAIT : LE PANNEAU OUVERT REND-IL LES ICÔNES MASQUÉES ?**
///
/// C'est la question sur laquelle le diagnostic du 2026-10-08 s'est arrêté. La
/// sonde `counts_what_each_tray_zone_contributes` y répond « 0 masquée » — mais
/// elle mesure **panneau fermé**, le seul cas où elle est muette par
/// construction : `TopLevelWindowForOverflowXamlIsland` **n'existe pas** au repos
/// (mesuré : `FindWindowW` échoue). Ce relevé compare donc les **deux chemins
/// dans le même processus, à la suite** :
///
///   - `enumerate()` — lecture seule, ne doit RIEN ouvrir ⇒ 0 masquée attendue ;
///   - `enumerate_avec_le_panneau()` — ouvre, lit, referme ⇒ ≫ 0 attendues.
///
/// ⚠️ **`visibles` peut valoir 0 ici sans que rien ne soit cassé** : notre dock
/// installe une fenêtre de classe `Shell_TrayWnd` (voir `tray_flux`), et
/// `vraie_barre` la saute par `NOTRE_HWND` — qui vaut **0 dans un processus de
/// sonde**. La barre VISIBLE mesurée ici peut donc être la nôtre. La zone
/// MASQUÉE, elle, n'est pas concernée : elle passe par `FindWindowW`, pas par
/// `vraie_barre`.
///
/// ⛔ **CE RELEVÉ OUVRE LE PANNEAU DE DÉBORDEMENT DE L'UTILISATEUR ~750 ms**, puis
/// le referme. C'est un geste sur le bureau RÉEL : d'où l'`#[ignore]`, qui le
/// réserve à une demande expresse.
#[test]
#[ignore]
fn ouvre_le_panneau_et_compte_les_masquees() {
    use crate::win32::tray_icons::{TrayIcon, TrayZone};

    // Le piège DPI, désarmé AVANT toute mesure — voir la sonde précédente.
    let _ = unsafe {
        windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )
    };

    let compter = |icones: &[TrayIcon]| -> (usize, usize) {
        (
            icones.iter().filter(|x| x.zone == TrayZone::Visible).count(),
            icones.iter().filter(|x| x.zone == TrayZone::Hidden).count(),
        )
    };

    println!("\n── LECTURE SEULE (enumerate) — ne doit RIEN ouvrir ──");
    let nues = crate::win32::tray_icons::enumerate();
    let (visibles, masquees) = compter(&nues);
    println!("  total {:>3}  =  visibles {visibles:>3}  +  masquées {masquees:>3}", nues.len());

    println!("\n── EN OUVRANT LE PANNEAU (enumerate_avec_le_panneau) ──");
    let debut = Instant::now();
    let ouvertes = crate::win32::tray_icons::enumerate_avec_le_panneau();
    let duree = debut.elapsed();
    let (visibles_ouverts, masquees_ouvertes) = compter(&ouvertes);
    println!(
        "  total {:>3}  =  visibles {visibles_ouverts:>3}  +  masquées {masquees_ouvertes:>3}   ({duree:.2?})",
        ouvertes.len()
    );
    let noms: Vec<&str> = ouvertes
        .iter()
        .filter(|x| x.zone == TrayZone::Hidden)
        .take(8)
        .map(|x| x.name.lines().next().unwrap_or("").trim())
        .collect();
    println!("  8 premières MASQUÉES : {noms:?}");
    println!();
}
