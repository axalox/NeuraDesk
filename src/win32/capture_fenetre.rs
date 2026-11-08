//! ⭐ **LA CAPTURE D'UNE FENÊTRE, SANS `Windows.Graphics.Capture`.**
//!
//! DEMANDE EXPLICITE (2026-11-04) : « je voudrais capturer les fenêtres pour
//! éviter l'effet de clignotement quand on ajoute des effets ».
//!
//! **POURQUOI UNE SECONDE VOIE.** `win32::wgc` exige une fenêtre VISIBLE
//! (`E_INVALIDARG` sinon), met jusqu'à 600 ms à ouvrir sa session sur une
//! fenêtre Chromium, livre souvent une première image NOIRE, et **chaque
//! session fermée après avoir reçu une image laisse 15 à 70 Mo de mémoire GPU
//! « dédiée » au processus** (voir `wgc`) — c'est-à-dire que l'effet
//! d'ouverture paie ce prix à chaque fenêtre ouverte.
//!
//! Ici : `PrintWindow(hwnd, hdc, PW_RENDERFULLCONTENT)`. Un seul appel
//! synchrone, pas de périphérique à détenir, pas de session à oublier, et des
//! pixels qui arrivent ENTIERS dès le premier appel (aucune image fantôme).
//!
//! **CE QUI A ÉTÉ MESURÉ** (sonde `win32::capture_probe`, 2026-11-04, sur
//! Discord/Chromium, l'Explorateur et Notepad++ — trois moteurs de rendu
//! différents, mêmes conclusions) :
//!
//! | État de la fenêtre | Contenu rendu ? |
//! |---|---|
//! | telle que l'utilisateur la voit | ✅ |
//! | `WS_EX_LAYERED` + alpha 0 | ✅ |
//! | hors de l'écran | ✅ |
//! | **`WS_EX_LAYERED` + alpha 0 ET hors de l'écran** | ✅ ← l'état utile |
//! | **région vide (`SetWindowRgn`)** | ⛔ APLAT (la région découpe le rendu) |
//! | cachée (`SW_HIDE`) | ⚠️ GDI et l'Explorateur : ✅ · Chromium : ⛔ APLAT |
//! | `DWMWA_CLOAK` | ⛔ refusé depuis un autre processus (`E_ACCESSDENIED`) |
//!
//! Coût mesuré de l'appel : **9 à 38 ms** sur une fenêtre de 1500×1200, y
//! compris hors de l'écran et sous alpha nul. Une fenêtre DÉPLACÉE hors de
//! l'écran continue donc de se peindre, et sa capture est FRAÎCHE (vérifié sur
//! un témoin : repeint en vert après déplacement, capturé en vert).
//!
//! ⛔ **CE QUE ÇA NE RÉSOUT PAS.** Une fenêtre VIDÉE DE SA RÉGION n'est pas
//! capturable — c'est justement le moyen que `ouverture_fenetres` emploie pour
//! la cacher sans attendre une repeinte. Les deux mécanismes restent donc
//! complémentaires : la capture commence quand la région est rendue.
//!
//! ⚠️ **L'APPEL EST SYNCHRONE, CHEZ L'AUTRE PROCESSUS.** Il faut donc vérifier
//! que la fenêtre répond AVANT d'appeler (`repond`, 500 ms) : une application
//! figée bloquerait sinon le fil de l'animation, qui ne se relève pas.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, RECT, WPARAM, LPARAM};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    ID3D11Device, ID3D11ShaderResourceView, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, SendMessageTimeoutW, SMTO_ABORTIFHUNG, WM_NULL};

// `PrintWindow` est déclaré côté `windows-rs` dans `Win32::Storage::Xps` — une
// feature entière pour un seul appel. La liaison ci-dessous est celle que le
// crate générerait.
#[link(name = "user32")]
extern "system" {
    fn PrintWindow(hwnd: HWND, hdcblt: windows::Win32::Graphics::Gdi::HDC, nflags: u32) -> i32;
}

/// `PW_RENDERFULLCONTENT` — sans ce drapeau, `PrintWindow` demande à
/// l'application de se redessiner (`WM_PRINT`) et une fenêtre moderne rend un
/// rectangle noir.
const PW_RENDERFULLCONTENT: u32 = 0x0000_0002;

/// Au plus ce délai pour qu'une fenêtre réponde avant qu'on la capture.
///
/// ⛔ Sans cette garde, `PrintWindow` sur une application figée bloque le fil
/// de l'animation AUSSI LONGTEMPS qu'elle est figée — c'est le défaut qui avait
/// déjà tué l'effet le 2026-10-19 (`cacher`).
const DELAI_REPONSE_MS: u32 = 500;

/// Une image de fenêtre lue côté process : BGRA, largeur d'une ligne = 4 × largeur.
pub struct Vue {
    pub largeur: u32,
    pub hauteur: u32,
    /// `largeur × hauteur × 4` octets, `B, G, R, A`, alpha FORCÉ à 255 (voir
    /// `capturer`).
    pub pixels: Vec<u8>,
    /// Signature grossière du contenu — deux appels qui la donnent identique
    /// ont rendu la même image (voir `Capture::poll`).
    pub empreinte: u64,
}

/// La fenêtre répond-elle ? Au plus `DELAI_REPONSE_MS`, sans attendre du tout
/// une fenêtre que Windows tient déjà pour figée.
pub fn repond(hwnd: HWND) -> bool {
    unsafe {
        SendMessageTimeoutW(hwnd, WM_NULL, WPARAM(0), LPARAM(0), SMTO_ABORTIFHUNG, DELAI_REPONSE_MS, None).0 != 0
    }
}

/// Une photo de la fenêtre — `None` si `PrintWindow` échoue ou si elle ne
/// répond pas.
///
/// ⚠️ **L'ALPHA DU DIB N'EST PAS CELUI DE LA FENÊTRE.** Un DIB 32 bits
/// `BI_RGB` laisse l'octet A à zéro : laissé tel quel, le quad s'afficherait
/// entièrement TRANSPARENT. On le force à 255 — la transparence de l'image
/// vient de l'effet (`Image::opacite`), pas de la fenêtre.
pub fn capturer(hwnd: HWND) -> Option<Vue> {
    if !repond(hwnd) {
        return None;
    }
    unsafe {
        let mut r = RECT::default();
        GetWindowRect(hwnd, &mut r).ok()?;
        let (largeur, hauteur) = (r.right - r.left, r.bottom - r.top);
        if largeur <= 0 || hauteur <= 0 {
            return None;
        }
        let ecran = GetDC(None);
        let dc = CreateCompatibleDC(Some(ecran));
        let mut info = BITMAPINFO::default();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = largeur;
        // Hauteur NÉGATIVE : DIB « top-down ». Sinon l'image arrive à l'envers.
        info.bmiHeader.biHeight = -hauteur;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB.0;
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let resultat = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0);
        let bitmap = match resultat {
            Ok(bitmap) => bitmap,
            Err(erreur) => {
                crate::diag!("[capture] DIB impossible ({largeur}×{hauteur}) : {erreur}");
                let _ = DeleteDC(dc);
                let _ = ReleaseDC(None, ecran);
                return None;
            }
        };
        let ancien = SelectObject(dc, bitmap.into());
        let dessine = PrintWindow(hwnd, dc, PW_RENDERFULLCONTENT);
        let vue = (dessine != 0 && !bits.is_null()).then(|| {
            let octets = std::slice::from_raw_parts(bits as *const u8, (largeur as usize) * (hauteur as usize) * 4);
            let mut pixels = octets.to_vec();
            for a in pixels.iter_mut().skip(3).step_by(4) {
                *a = 255;
            }
            let empreinte = empreinte(&pixels);
            Vue { largeur: largeur as u32, hauteur: hauteur as u32, pixels, empreinte }
        });
        SelectObject(dc, ancien);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        let _ = ReleaseDC(None, ecran);
        vue
    }
}

/// FNV-1a sur un pixel sur 13 (premier) : assez pour distinguer deux images,
/// sans relire les 10 Mo d'une fenêtre plein écran à chaque appel.
fn empreinte(pixels: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in pixels.chunks_exact(4).step_by(13) {
        for &o in p {
            h ^= o as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
    }
    h
}

/// Une fenêtre suivie pour un effet : la dernière image, prête à dessiner.
///
/// Même surface que `wgc::CaptureSession` (`start`, `size`, `srv`, `poll`) :
/// l'effet d'ouverture s'en sert sans changer d'un pouce sa logique d'étapes.
pub struct Capture {
    hwnd: HWND,
    texture: Option<ID3D11Texture2D>,
    srv: Option<ID3D11ShaderResourceView>,
    size: (u32, u32),
    empreinte: u64,
    /// ⛔ `PrintWindow` coûte 10 à 40 ms : on ne le tente pas plus vite que ça,
    /// sinon l'animation d'un vol avancerait au rythme de la capture.
    derniere: Option<Instant>,
    intervalle: Duration,
}

/// Cadence des appels de `PrintWindow` pendant un effet.
const INTERVALLE: Duration = Duration::from_millis(50);

impl Capture {
    /// Prépare le suivi de `hwnd` ; sans effet si la fenêtre n'a pas de
    /// rectangle. Aucune ressource GPU n'est créée avant la première image : une
    /// session qui n'aboutit jamais ne coûte rien.
    pub fn start(hwnd: HWND) -> Option<Self> {
        let mut r = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut r) }.ok()?;
        if r.right - r.left <= 0 || r.bottom - r.top <= 0 {
            return None;
        }
        Some(Self {
            hwnd,
            texture: None,
            srv: None,
            size: (0, 0),
            empreinte: 0,
            derniere: None,
            intervalle: INTERVALLE,
        })
    }

    /// Taille (largeur, hauteur) du contenu capturé — `(0, 0)` avant la
    /// première image.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// La vue de la dernière image, s'il y en a une.
    pub fn srv(&self) -> Option<&ID3D11ShaderResourceView> {
        self.srv.as_ref()
    }

    /// Relit la fenêtre si sa dernière lecture date de plus de `intervalle` :
    /// `true` quand une image NEUVE a remplacé la précédente.
    ///
    /// **« Neuve » veut dire que l'empreinte a changé** — pas « un appel a eu
    /// lieu ». C'est ce que l'effet d'ouverture attend (`nouvelle`) pour savoir
    /// si la fenêtre a fini de se dessiner.
    pub fn poll(&mut self, device: &ID3D11Device) -> bool {
        if self.derniere.is_some_and(|d| d.elapsed() < self.intervalle) {
            return false;
        }
        self.derniere = Some(Instant::now());
        let Some(vue) = capturer(self.hwnd) else { return false };
        if self.srv.is_some() && vue.empreinte == self.empreinte && (vue.largeur, vue.hauteur) == self.size {
            return false;
        }
        match self.televerser(device, &vue) {
            Ok(()) => {
                self.size = (vue.largeur, vue.hauteur);
                self.empreinte = vue.empreinte;
                true
            }
            Err(erreur) => {
                crate::diag!("[capture] téléversement impossible ({}×{}) : {erreur}", vue.largeur, vue.hauteur);
                false
            }
        }
    }

    /// Crée (ou recrée, si la taille a changé) la texture et sa vue, puis y
    /// copie les pixels.
    fn televerser(&mut self, device: &ID3D11Device, vue: &Vue) -> windows::core::Result<()> {
        unsafe {
            let a_recreer = self.texture.is_none() || self.size != (vue.largeur, vue.hauteur);
            if a_recreer {
                // ⛔ L'ANCIENNE VUE AVANT L'ANCIENNE TEXTURE : une vue qui
                // survit à sa ressource est un usage après libération.
                self.srv = None;
                self.texture = None;
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: vue.largeur,
                    Height: vue.hauteur,
                    MipLevels: 1,
                    ArraySize: 1,
                    // Le DIB est du BGRA : c'est DÉJÀ le format que `wgc` livre,
                    // donc aucun échange de canaux.
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                };
                let mut texture: Option<ID3D11Texture2D> = None;
                device.CreateTexture2D(&desc, None, Some(&mut texture))?;
                self.texture = texture;
            }
            let texture = self.texture.as_ref().ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))?;
            let contexte = device.GetImmediateContext()?;
            contexte.UpdateSubresource(texture, 0, None, vue.pixels.as_ptr() as *const _, vue.largeur * 4, 0);
            if self.srv.is_none() {
                let mut srv: Option<ID3D11ShaderResourceView> = None;
                device.CreateShaderResourceView(texture, None, Some(&mut srv))?;
                self.srv = srv;
            }
            Ok(())
        }
    }

    /// Force la prochaine lecture, même si `intervalle` n'est pas écoulé —
    /// employé quand une étape change et qu'il faut une image TOUT DE SUITE.
    pub fn relire(&mut self) {
        self.derniere = None;
    }
}

#[cfg(test)]
mod tests {
    //! ⭐ **LE TÉMOIN.** Les mesures sur les fenêtres d'AUTRES applications (voir
    //! la doc du module) disent ce que `PrintWindow` accepte ; ce témoin, lui,
    //! vérifie ce qui compte pour un effet : une fenêtre DÉPLACÉE HORS DE
    //! L'ÉCRAN rend son contenu, et ce contenu est FRAIS (pas la première image
    //! gardée en mémoire).
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows::core::w;
    use windows::Win32::Foundation::{COLORREF, HINSTANCE, LRESULT};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, CreateRectRgn, CreateSolidBrush, EndPaint, FillRect, InvalidateRect,
        SetWindowRgn, UpdateWindow, PAINTSTRUCT,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetSystemMetrics, PeekMessageW,
        RegisterClassExW, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, MSG, PM_REMOVE,
        SM_CXVIRTUALSCREEN, SM_XVIRTUALSCREEN, SW_SHOWNOACTIVATE,
        WM_ERASEBKGND, WM_PAINT, WNDCLASSEXW, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
    };

    /// La couleur que le témoin peint à sa prochaine repeinte (COLORREF).
    static TEINTE: AtomicU32 = AtomicU32::new(0);
    const ROUGE: u32 = 0x0000_00FF;
    const VERT: u32 = 0x0000_FF00;

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

    fn pomper(ms: u64) {
        let fin = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < fin {
            let mut msg = MSG::default();
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            unsafe {
                let _ = windows::Win32::Graphics::Dwm::DwmFlush();
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// La couleur du pixel du milieu, telle que la capture l'a rendue.
    fn milieu(vue: &Vue) -> (u8, u8, u8) {
        let i = (((vue.hauteur / 2) * vue.largeur + vue.largeur / 2) * 4) as usize;
        (vue.pixels[i], vue.pixels[i + 1], vue.pixels[i + 2])
    }

    fn teintes(vue: &Vue) -> usize {
        let pas = (vue.largeur * vue.hauteur / 256).max(1);
        let mut vues = std::collections::HashSet::new();
        for i in (0..vue.pixels.len() / 4).step_by(pas as usize) {
            vues.insert([vue.pixels[i * 4], vue.pixels[i * 4 + 1], vue.pixels[i * 4 + 2], vue.pixels[i * 4 + 3]]);
        }
        vues.len()
    }

    #[test]
    fn une_fenetre_hors_ecran_livre_son_contenu_et_se_rafraichit() {
        let module = unsafe { GetModuleHandleW(None) }.expect("module");
        let hinstance = HINSTANCE(module.0);
        let classe = w!("NeuraDeskTemoinCapture");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(peint),
            hInstance: hinstance,
            lpszClassName: classe,
            ..Default::default()
        };
        assert_ne!(unsafe { RegisterClassExW(&wc) }, 0, "classe du témoin");

        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                classe,
                w!("témoin de capture"),
                WS_OVERLAPPEDWINDOW,
                100,
                100,
                320,
                240,
                None,
                None,
                Some(hinstance),
                None,
            )
        }
        .expect("fenêtre du témoin");
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        pomper(200);

        // Arrange : un aplat rouge, visible.
        TEINTE.store(ROUGE, Ordering::SeqCst);
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, true);
            let _ = UpdateWindow(hwnd);
        }
        pomper(150);
        let vue = capturer(hwnd).expect("capture de la fenêtre visible");
        assert_eq!(milieu(&vue), (0, 0, 255), "BGRA : le rouge y est (0, 0, 255)");
        assert!(teintes(&vue) > 4, "une fenêtre dessinée n'est pas un aplat : {} teintes", teintes(&vue));

        // Act : hors de TOUS les écrans, puis repeinte en vert.
        let hors_ecran = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) + GetSystemMetrics(SM_CXVIRTUALSCREEN) + 400 };
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                hwnd,
                None,
                hors_ecran,
                100,
                0,
                0,
                windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
            );
        }
        pomper(200);
        TEINTE.store(VERT, Ordering::SeqCst);
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, true);
            let _ = UpdateWindow(hwnd);
        }
        pomper(300);

        // Assert : hors de l'écran, la fenêtre se peint encore — et la capture
        // rend la NOUVELLE image, pas la première.
        let vue = capturer(hwnd).expect("capture de la fenêtre hors de l'écran");
        assert_eq!(milieu(&vue), (0, 255, 0), "BGRA : le vert y est (0, 255, 0) — l'image est fraîche");

        // Et la limite MESURÉE : une région vide découpe le rendu, donc la
        // capture ne rend plus le contenu. C'est pour cela que l'effet rend sa
        // région à la fenêtre avant de la capturer.
        unsafe {
            SetWindowRgn(hwnd, Some(CreateRectRgn(0, 0, 0, 0)), false);
        }
        pomper(150);
        let vide = capturer(hwnd);
        assert!(
            vide.as_ref().is_none_or(|v| teintes(v) <= 2),
            "une région vide ne rend qu'un aplat (mesuré) : {:?}",
            vide.as_ref().map(teintes)
        );

        unsafe {
            let _ = SetWindowRgn(hwnd, None, true);
            let _ = DestroyWindow(hwnd);
        }
    }

    /// Le témoin du voile peint TOUJOURS en bleu : `TEINTE` appartient à
    /// l'autre témoin, et les tests tournent en parallèle.
    unsafe extern "system" fn peint_en_bleu(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if message == WM_PAINT {
            unsafe {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let brosse = CreateSolidBrush(COLORREF(0x00FF_0000));
                let mut r = RECT::default();
                let _ = GetClientRect(hwnd, &mut r);
                let _ = FillRect(dc, &r, brosse);
                let _ = DeleteObject(brosse.into());
                let _ = EndPaint(hwnd, &ps);
            }
            return LRESULT(0);
        }
        if message == WM_ERASEBKGND {
            return LRESULT(1);
        }
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    /// ⭐ **LE VOILE DU MODULE INJECTÉ** (`window-hook`) : il pose `DWMWA_CLOAK`
    /// sur la fenêtre de SON processus AVANT qu'elle ne se montre, puis
    /// NeuraDesk la capture pour le vol. Ce témoin vérifie ce dont dépend tout
    /// le chemin : une fenêtre voilée dès avant son affichage se peint quand
    /// même, et `PrintWindow` rend ce contenu — et non un aplat.
    #[test]
    fn une_fenetre_voilee_avant_son_affichage_livre_son_contenu() {
        use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK};
        let module = unsafe { GetModuleHandleW(None) }.expect("module");
        let hinstance = HINSTANCE(module.0);
        let classe = w!("NeuraDeskTemoinVoile");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(peint_en_bleu),
            hInstance: hinstance,
            lpszClassName: classe,
            ..Default::default()
        };
        assert_ne!(unsafe { RegisterClassExW(&wc) }, 0, "classe du témoin");
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                classe,
                w!("témoin du voile"),
                WS_OVERLAPPEDWINDOW,
                120,
                120,
                320,
                240,
                None,
                None,
                Some(hinstance),
                None,
            )
        }
        .expect("fenêtre du témoin");

        // Arrange : voilée AVANT d'être montrée — l'ordre du module injecté.
        let voile: i32 = 1;
        let pose = unsafe {
            DwmSetWindowAttribute(hwnd, DWMWA_CLOAK, &voile as *const _ as _, std::mem::size_of::<i32>() as u32)
        };
        assert!(pose.is_ok(), "le voile est permis dans son propre processus : {pose:?}");
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = UpdateWindow(hwnd);
        }
        pomper(250);

        // Act
        let vue = capturer(hwnd).expect("capture de la fenêtre voilée");

        // Assert
        assert_eq!(milieu(&vue), (255, 0, 0), "BGRA : le bleu peint sous le voile y est");
        assert!(teintes(&vue) > 4, "le cadre et le contenu, pas un aplat : {} teintes", teintes(&vue));

        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}
