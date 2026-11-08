//! **SONDE — CAPTURER UNE FENÊTRE QUE PERSONNE NE VOIT.**
//!
//! DEMANDE EXPLICITE (2026-11-04) : « je voudrais capturer les fenêtres pour
//! éviter l'effet de clignotement quand on ajoute des effets ».
//!
//! **LE PROBLÈME, TEL QU'IL EST MESURÉ.** L'effet d'ouverture
//! (`win32::ouverture_fenetres`) doit montrer l'IMAGE d'une fenêtre avant que
//! la vraie ne paraisse. Or la capture employée, `Windows.Graphics.Capture`,
//! **REFUSE une fenêtre invisible** (`E_INVALIDARG`) : il faut donc lui rendre
//! sa région, la remettre à l'écran, puis la cacher par l'alpha — et c'est
//! exactement entre ces deux instants que la fenêtre se montre (10 à 20 ms
//! d'après son style, ~110 ms pour l'Explorateur, ~200 ms pour Edge).
//!
//! **LA QUESTION DE CETTE SONDE.** Existe-t-il une capture qui lit le CONTENU
//! d'une fenêtre **sans qu'elle soit visible** ? Si oui, on la cache d'un coup
//! (région vide, appliquée sans repeinte) et on la capture quand même : plus de
//! clignotement, et n'importe quel effet futur en profite.
//!
//! **LE CANDIDAT.** `PrintWindow(hwnd, hdc, PW_RENDERFULLCONTENT)` — la voie
//! classique des vignettes : elle lit la surface de redirection de DWM, pas
//! l'écran. Elle est SYNCHRONE (donc mesurable en temps), ne demande aucun
//! périphérique D3D, et rend des pixels dans un DIB côté process. On mesure
//! **le même appel dans six états de la même fenêtre**, en les restaurant tous.
//!
//! ⚠️ **ELLE TOUCHE À LA FENÊTRE D'UN AUTRE PROCESSUS** — c'est le prix de la
//! mesure, et c'est ce que fait déjà l'effet d'ouverture. Chaque état est
//! restauré par un `Drop` : même si une assertion échoue, la fenêtre retrouve
//! sa région, sa place, son style et sa visibilité. Aucune fenêtre n'est
//! fermée, aucun processus n'est tué.
//!
//! Mesure : `cargo test --release -- --ignored --nocapture
//! une_fenetre_que_personne_ne_voit_se_capture_quand_meme`
//! (choisir la fenêtre : `PROBE_CLASSE=CabinetWClass`, `PROBE_TITRE=…`) ;
//! sans cible, elle LISTE les fenêtres candidates et ne touche à rien.

use std::time::Instant;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmFlush, DwmGetWindowAttribute, DwmSetWindowAttribute, DWMWA_CLOAK, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateRectRgn, DeleteDC, DeleteObject, GetDC, GetWindowRgn, ReleaseDC,
    SelectObject, SetWindowRgn, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, RGN_ERROR,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowLongPtrW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsWindow,
    IsWindowVisible, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE, GWL_STYLE,
    LWA_ALPHA, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, WS_CAPTION, WS_CHILD, WS_EX_LAYERED,
    WS_EX_TOOLWINDOW,
};

// `PrintWindow` vit dans `Win32::Storage::Xps` côté `windows-rs` — une feature
// entière pour un seul appel. Ces quelques lignes l'évitent, et c'est la même
// liaison que celle que le crate générerait.
#[link(name = "user32")]
extern "system" {
    fn PrintWindow(hwnd: HWND, hdcblt: windows::Win32::Graphics::Gdi::HDC, nflags: u32) -> i32;
}

/// `PW_RENDERFULLCONTENT` — la valeur qui fait lire la surface DWM au lieu de
/// demander à l'application de se redessiner (`WM_PRINT`). Sans elle, une
/// fenêtre d'application moderne rend un rectangle noir.
const PW_RENDERFULLCONTENT: u32 = 0x0000_0002;

/// Une photo de la fenêtre, telle que `PrintWindow` la rend.
struct Photo {
    image: image::RgbaImage,
    ms: u128,
    /// Nombre de couleurs distinctes sur une grille — un aplat (fenêtre pas
    /// dessinée, surface perdue) en donne 1 ou 2, un contenu en donne des
    /// dizaines.
    teintes: usize,
}

impl Photo {
    fn contenu(&self) -> bool {
        self.teintes > 8
    }
}

impl std::fmt::Display for Photo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:>5} ms  teintes {:>4}  {}", self.ms, self.teintes, if self.contenu() { "CONTENU" } else { "aplat/vide" })
    }
}

/// `PrintWindow` dans un DIB 32 bits de la taille du RECTANGLE de la fenêtre
/// (barre de titre comprise : `PW_RENDERFULLCONTENT` rend tout le cadre).
fn imprimer(hwnd: HWND) -> Option<Photo> {
    unsafe {
        let mut r = RECT::default();
        GetWindowRect(hwnd, &mut r).ok()?;
        let (l, h) = (r.right - r.left, r.bottom - r.top);
        if l <= 0 || h <= 0 {
            return None;
        }
        let ecran = GetDC(None);
        let dc = CreateCompatibleDC(Some(ecran));
        let mut info = BITMAPINFO::default();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = l;
        // Hauteur NÉGATIVE : DIB « top-down », sinon l'image arrive à l'envers.
        info.bmiHeader.biHeight = -h;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB.0;
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let bitmap = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bitmap) => bitmap,
            Err(erreur) => {
                let _ = DeleteDC(dc);
                let _ = ReleaseDC(None, ecran);
                eprintln!("CreateDIBSection : {erreur}");
                return None;
            }
        };
        let ancien = SelectObject(dc, bitmap.into());
        let debut = Instant::now();
        let dessine = PrintWindow(hwnd, dc, PW_RENDERFULLCONTENT);
        let ms = debut.elapsed().as_millis();
        let photo = (dessine != 0 && !bits.is_null()).then(|| {
            let octets = std::slice::from_raw_parts(bits as *const u8, (l * h * 4) as usize);
            let mut tampon = image::RgbaImage::new(l as u32, h as u32);
            for (i, p) in octets.chunks_exact(4).enumerate() {
                // DIB = BGRA. ⛔ L'octet A n'est PAS fiable (0 sur la plupart
                // des applications) : on le force à opaque.
                tampon.put_pixel(i as u32 % l as u32, i as u32 / l as u32, image::Rgba([p[2], p[1], p[0], 255]));
            }
            let mesure = teintes(&tampon);
            Photo { image: tampon, ms, teintes: mesure }
        });
        SelectObject(dc, ancien);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        let _ = ReleaseDC(None, ecran);
        if dessine == 0 {
            eprintln!("PrintWindow({:#x}) a rendu 0", hwnd.0 as isize);
        }
        photo
    }
}

/// Couleurs distinctes sur une grille de 64×64 — assez pour distinguer un aplat
/// d'un contenu, trop peu pour coûter quoi que ce soit.
fn teintes(image: &image::RgbaImage) -> usize {
    let mut vues = std::collections::HashSet::new();
    let (l, h) = (image.width().max(1), image.height().max(1));
    for y in 0..64u32 {
        for x in 0..64u32 {
            let p = image.get_pixel(x * l / 64, y * h / 64);
            vues.insert(u32::from_be_bytes([p[0], p[1], p[2], 0]));
        }
    }
    vues.len()
}

/// L'état d'origine d'une fenêtre, et de quoi le lui rendre **quoi qu'il
/// arrive** (voir le `Drop`).
struct Cible {
    hwnd: HWND,
    rect: RECT,
    ex: isize,
    /// Vraie si la fenêtre avait DÉJÀ sa propre région : on ne la vide pas, on
    /// ne saurait pas la lui rendre telle quelle (même garde que
    /// `ouverture_fenetres::vider_la_region`).
    region_propre: bool,
    region_videe: bool,
    ecartee: bool,
    cachee: bool,
    opaque_rendue: bool,
}

impl Cible {
    fn prendre(hwnd: HWND) -> Option<Self> {
        let mut rect = RECT::default();
        let mut tampon = [0u16; 128];
        unsafe {
            GetWindowRect(hwnd, &mut rect).ok()?;
            let n = GetWindowTextW(hwnd, &mut tampon).max(0) as usize;
            let _ = n;
            let sonde = CreateRectRgn(0, 0, 0, 0);
            let region_propre = GetWindowRgn(hwnd, sonde) != RGN_ERROR;
            let _ = DeleteObject(sonde.into());
            Some(Self {
                hwnd,
                rect,
                ex: GetWindowLongPtrW(hwnd, GWL_EXSTYLE),
                region_propre,
                region_videe: false,
                ecartee: false,
                cachee: false,
                opaque_rendue: false,
            })
        }
    }

    /// Une pause qui laisse parler DWM et la fenêtre — sans toucher au fil de
    /// message du processus visité, qui est le sien.
    fn laisser_faire(ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        unsafe {
            let _ = DwmFlush();
        }
    }

    fn rendre_alpha(&mut self) {
        if self.opaque_rendue {
            return;
        }
        unsafe {
            let _ = SetLayeredWindowAttributes(self.hwnd, COLORREF(0), 255, LWA_ALPHA);
        }
        self.opaque_rendue = true;
        Self::laisser_faire(120);
    }

    fn poser_alpha_nul(&mut self) {
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, self.ex | WS_EX_LAYERED.0 as isize);
            let _ = SetLayeredWindowAttributes(self.hwnd, COLORREF(0), 0, LWA_ALPHA);
        }
        self.opaque_rendue = false;
        Self::laisser_faire(120);
    }

    fn ecarter(&mut self) {
        unsafe {
            let _ = SetWindowPos(self.hwnd, None, self.rect.left, self.rect.bottom + 64, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
        self.ecartee = true;
        Self::laisser_faire(120);
    }

    fn ramener(&mut self) {
        if !self.ecartee {
            return;
        }
        unsafe {
            let _ = SetWindowPos(self.hwnd, None, self.rect.left, self.rect.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
        self.ecartee = false;
        Self::laisser_faire(80);
    }

    fn vider_region(&mut self) -> bool {
        if self.region_propre {
            return false;
        }
        let vide = unsafe { SetWindowRgn(self.hwnd, Some(CreateRectRgn(0, 0, 0, 0)), false) };
        self.region_videe = vide != 0;
        Self::laisser_faire(80);
        self.region_videe
    }

    fn rendre_region(&mut self) {
        if !self.region_videe {
            return;
        }
        unsafe {
            let _ = SetWindowRgn(self.hwnd, None, true);
        }
        self.region_videe = false;
        Self::laisser_faire(120);
    }

    fn cacher(&mut self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        self.cachee = true;
        Self::laisser_faire(80);
    }

    fn montrer(&mut self) {
        if !self.cachee {
            return;
        }
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        self.cachee = false;
        Self::laisser_faire(120);
    }
}

impl Drop for Cible {
    /// ⛔ **LA FENÊTRE EST TOUJOURS RENDUE** — région, place, alpha, visibilité.
    /// Une sonde qui laisse une fenêtre vide ou hors de l'écran est un défaut
    /// pire que celui qu'elle mesure.
    fn drop(&mut self) {
        if unsafe { !IsWindow(Some(self.hwnd)) }.as_bool() {
            return;
        }
        unsafe {
            if self.cachee {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
            if self.ecartee {
                let _ = SetWindowPos(self.hwnd, None, self.rect.left, self.rect.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
            if self.region_videe {
                let _ = SetWindowRgn(self.hwnd, None, true);
            }
            if !self.opaque_rendue {
                let _ = SetLayeredWindowAttributes(self.hwnd, COLORREF(0), 255, LWA_ALPHA);
            }
            // Le style étendu est reposé TEL QUEL, en dernier : c'est lui qui
            // décide si la fenêtre est `LAYERED` à nouveau.
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, self.ex);
        }
        Self::laisser_faire(100);
    }
}

/// Les fenêtres qui peuvent servir de témoin : visibles, titrées, d'un autre
/// processus, assez grandes, et pas une surcouche.
fn candidates() -> Vec<(HWND, String, String)> {
    unsafe extern "system" fn une(hwnd: HWND, l: LPARAM) -> windows::core::BOOL {
        let liste = &mut *(l.0 as *mut Vec<(HWND, String, String)>);
        unsafe {
            let mut r = RECT::default();
            if !IsWindowVisible(hwnd).as_bool() || GetWindowRect(hwnd, &mut r).is_err() {
                return true.into();
            }
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            if ex & WS_EX_TOOLWINDOW.0 != 0
                || style & WS_CHILD.0 != 0
                || style & WS_CAPTION.0 != WS_CAPTION.0
                || r.right - r.left < 320
                || r.bottom - r.top < 240
            {
                return true.into();
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == std::process::id() {
                return true.into();
            }
            let mut titre = [0u16; 128];
            let nt = GetWindowTextW(hwnd, &mut titre).max(0) as usize;
            if nt == 0 {
                return true.into();
            }
            let mut classe = [0u16; 128];
            let nc = GetClassNameW(hwnd, &mut classe).max(0) as usize;
            liste.push((
                hwnd,
                String::from_utf16_lossy(&classe[..nc]),
                String::from_utf16_lossy(&titre[..nt]),
            ));
        }
        true.into()
    }
    let mut liste: Vec<(HWND, String, String)> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(une), LPARAM(&mut liste as *mut _ as isize));
    }
    liste
}

#[test]
#[ignore]
fn une_fenetre_que_personne_ne_voit_se_capture_quand_meme() {
    let classe = std::env::var("PROBE_CLASSE").ok();
    let titre = std::env::var("PROBE_TITRE").ok();
    let liste = candidates();
    let cible = liste.iter().find(|(_, c, t)| {
        classe.as_ref().map(|f| c.contains(f.as_str())).unwrap_or(true)
            && titre.as_ref().map(|f| t.contains(f.as_str())).unwrap_or(true)
    });
    let Some((hwnd, classe, titre)) = cible.cloned() else {
        println!("aucune fenêtre ne correspond — candidates ({}):", liste.len());
        for (_, c, t) in &liste {
            println!("  {c:<28} {t}");
        }
        return;
    };
    println!("cible : {classe} « {titre} » {:#x}\n", hwnd.0 as isize);

    let mut c = match Cible::prendre(hwnd) {
        Some(c) => c,
        None => {
            println!("fenêtre illisible");
            return;
        }
    };
    let dossier = r"C:\ndtarget\capture-sonde";
    let _ = std::fs::create_dir_all(dossier);

    let ligne = |etat: &str, photo: Option<&Photo>| match photo {
        Some(p) => {
            let _ = p.image.save(format!(r"{dossier}\{etat}.png"));
            println!("{etat:<20} {p}");
        }
        None => println!("{etat:<20} ⛔ PrintWindow a ÉCHOUÉ"),
    };

    // 1. TÉMOIN : la fenêtre telle que l'utilisateur la voit.
    let visible = imprimer(hwnd);
    ligne("00-vue", visible.as_ref());

    // 2. `WS_EX_LAYERED` + alpha 0 — ce que fait l'effet aujourd'hui.
    c.poser_alpha_nul();
    let layered = imprimer(hwnd);
    ligne("01-layered-0", layered.as_ref());
    c.rendre_alpha();

    // 2 bis. LES DEUX À LA FOIS — l'état qu'emploierait un effet qui ne montre
    //    JAMAIS la fenêtre : alpha nul (elle ne se voit pas) et hors de l'écran
    //    (même composée, elle ne touche aucun écran). La capture y lit-elle ?
    c.poser_alpha_nul();
    c.ecarter();
    let les_deux = imprimer(hwnd);
    ligne("02b-layered+ecartee", les_deux.as_ref());
    c.ramener();
    c.rendre_alpha();

    // 3. HORS DE L'ÉCRAN — l'état où l'effet attend la première image WGC.
    c.ecarter();
    let ecartee = imprimer(hwnd);
    ligne("02-hors-ecran", ecartee.as_ref());
    c.ramener();

    // 4. RÉGION VIDE — le seul masquage qui agit SANS repeinte, donc celui qui
    //    supprimerait le clignotement. La question : la capture le supporte-t-elle ?
    if c.vider_region() {
        let region = imprimer(hwnd);
        ligne("03-region-vide", region.as_ref());
        c.rendre_region();
    } else {
        println!("03-region-vide      ⛔ écartée : la fenêtre a déjà SA région");
    }

    // 5. CACHÉE pour de bon.
    c.cacher();
    let cachee = imprimer(hwnd);
    ligne("04-cachee", cachee.as_ref());
    c.montrer();

    // 6. `DWMWA_CLOAK` — attendu REFUSÉ depuis un autre processus.
    let un: i32 = 1;
    let cloak = unsafe { DwmSetWindowAttribute(hwnd, DWMWA_CLOAK, &un as *const _ as _, 4) };
    println!("05-cloak            {:?}", cloak);
    if cloak.is_ok() {
        let cloakee = imprimer(hwnd);
        ligne("05-cloak", cloakee.as_ref());
        let zero: i32 = 0;
        let _ = unsafe { DwmSetWindowAttribute(hwnd, DWMWA_CLOAK, &zero as *const _ as _, 4) };
    }

    // 7. La fenêtre a-t-elle bougé ? (la capture de référence doit être la même)
    let retour = imprimer(hwnd);
    ligne("06-retour", retour.as_ref());

    let mut r = RECT::default();
    let _ = unsafe { GetWindowRect(hwnd, &mut r) };
    let mut cadre = RECT::default();
    let _ = unsafe { DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut cadre as *mut _ as _, 8) };
    println!(
        "\nrectangle {:?} (cadre visible {:?}), visible {} — fenêtre RESTAURÉE",
        c.rect,
        cadre,
        unsafe { IsWindowVisible(hwnd) }.as_bool()
    );
    println!("images écrites dans {dossier}");
}

// ═══════════════════════════════════════════════════════════════════════════
// DEUXIÈME QUESTION : une fenêtre ÉCARTÉE (ou cachée) SE PEINT-ELLE, et sa
// capture est-elle FRAÎCHE ? `PrintWindow` lisant une surface de redirection,
// il faut savoir si cette surface reçoit encore les repeintes — sinon on
// capturerait toujours la première image, et un effet poserait une vignette
// périmée.
// ═══════════════════════════════════════════════════════════════════════════

use std::sync::atomic::{AtomicU32, Ordering};
use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, EndPaint, FillRect, InvalidateRect, UpdateWindow, PAINTSTRUCT,
    RDW_ALLCHILDREN, RDW_INVALIDATE, RDW_UPDATENOW, RedrawWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetSystemMetrics, MsgWaitForMultipleObjectsEx,
    PeekMessageW, RegisterClassExW, TranslateMessage, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, SM_CXVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, WM_ERASEBKGND, WM_PAINT, WNDCLASSEXW, WS_OVERLAPPEDWINDOW, CS_HREDRAW, CS_VREDRAW,
    WINDOW_EX_STYLE, GetClientRect,
};

/// La couleur que le témoin peint à sa prochaine repeinte (COLORREF : 0x00BBGGRR).
static TEINTE: AtomicU32 = AtomicU32::new(0);

const ROUGE: u32 = 0x0000_00FF;
const VERT: u32 = 0x0000_FF00;
const BLEU: u32 = 0x00FF_0000;

unsafe extern "system" fn peint(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_PAINT => {
            unsafe {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let brosse = CreateSolidBrush(COLORREF(TEINTE.load(Ordering::SeqCst)));
                let mut r = RECT::default();
                let _ = GetClientRect(hwnd, &mut r);
                let _ = FillRect(dc, &r, brosse);
                let _ = DeleteObject(brosse.into());
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// Le pixel du MILIEU de la zone cliente de la photo — celui qui dit quelle
/// teinte y est réellement peinte.
fn teinte(photo: &Photo) -> u32 {
    let (l, h) = (photo.image.width(), photo.image.height());
    // Le haut de l'image est la barre de titre : on vise franchement le centre.
    let p = photo.image.get_pixel(l / 2, h / 2);
    u32::from_be_bytes([p[0], p[1], p[2], 0])
}

fn nommer(teinte: u32) -> &'static str {
    match teinte {
        0x0000_00FF => "ROUGE",
        0x0000_FF00 => "VERT",
        0x00FF_0000 => "BLEU",
        _ => "autre",
    }
}

#[test]
#[ignore]
fn une_fenetre_ecartee_se_peint_et_se_capture() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    /// Traite les messages de NOS fenêtres pendant `ms`.
    fn pomper(ms: u64) {
        let fin = Instant::now() + std::time::Duration::from_millis(ms);
        while Instant::now() < fin {
            let mut msg = MSG::default();
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            unsafe {
                let _ = DwmFlush();
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    let (largeur, hauteur) = (500, 400);
    let module = unsafe { GetModuleHandleW(None) }.expect("module");
    let classe = w!("NeuraDeskSondeCapture");
    let hinstance = HINSTANCE(module.0);
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(peint),
        hInstance: hinstance,
        lpszClassName: classe,
        ..Default::default()
    };
    assert_ne!(unsafe { RegisterClassExW(&wc) }, 0, "classe");

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            classe,
            w!("témoin de capture"),
            WS_OVERLAPPEDWINDOW,
            300,
            300,
            largeur,
            hauteur,
            None,
            None,
            Some(hinstance),
            None,
        )
    }
    .expect("fenêtre");
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    pomper(250);

    // Hors de TOUS les écrans, à droite du bureau virtuel.
    let hors_ecran = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) + GetSystemMetrics(SM_CXVIRTUALSCREEN) + 400 };

    let mut etape = |nom: &str| {
        match imprimer(hwnd) {
            Some(ref p) => println!(
                "{nom:<34} {} | pixel du milieu : {:#08x} {}",
                p,
                teinte(p),
                nommer(teinte(p))
            ),
            None => println!("{nom:<34} ⛔ PrintWindow a ÉCHOUÉ"),
        }
    };

    TEINTE.store(ROUGE, Ordering::SeqCst);
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, true);
        let _ = UpdateWindow(hwnd);
    }
    pomper(150);
    etape("1. à l'écran, peint en ROUGE");

    unsafe {
        let _ = SetWindowPos(hwnd, None, hors_ecran, 300, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
    pomper(200);
    println!("   (déplacée en x={hors_ecran}, visible={})", unsafe { IsWindowVisible(hwnd) }.as_bool());

    TEINTE.store(VERT, Ordering::SeqCst);
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, true);
        let _ = UpdateWindow(hwnd);
    }
    pomper(300);
    etape("2. HORS ÉCRAN, repeinte en VERT");

    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    pomper(120);
    TEINTE.store(BLEU, Ordering::SeqCst);
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, true);
        let _ = UpdateWindow(hwnd);
    }
    pomper(250);
    etape("3. CACHÉE, repeinte en BLEU");
    println!("   (visible={} après SW_HIDE)", unsafe { IsWindowVisible(hwnd) }.as_bool());

    unsafe {
        let _ = RedrawWindow(Some(hwnd), None, None, RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN);
    }
    pomper(250);
    etape("4. CACHÉE, RedrawWindow UPDATENOW");

    // Et si on la remonte HORS ÉCRAN, garde-t-elle la repeinte faite cachée ?
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    pomper(300);
    etape("5. remontée HORS ÉCRAN");

    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}
