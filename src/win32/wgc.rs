//! **Aperçus vivants du contenu d'une fenêtre** — Windows.Graphics.Capture.
//!
//! Une session par fenêtre visée, qui rend sa dernière frame sous forme de
//! `ID3D11ShaderResourceView` directement dessinable par le `QuadRenderer`
//! (`QuadInstance::new_textured`) — c'est tout l'intérêt par rapport aux
//! vignettes DWM, qui ne sont pas des textures et que DWM peint par-dessus la
//! fenêtre de destination.
//!
//! **TOUT CE QUI SUIT A ÉTÉ MESURÉ** sur la machine de l'utilisateur, avec une
//! sonde jetable, avant d'écrire ce module — c'est ce qui a évité de concevoir
//! sur des suppositions :
//!
//! | Point | Mesure |
//! |---|---|
//! | `IsSupported()` | `true` |
//! | `CreateForWindow` sur un `HWND` | OK, `item.Size()` = taille de la fenêtre |
//! | partir du `ID3D11Device` DE L'APPLICATION | OK (`CreateDirect3D11DeviceFromDXGIDevice`) |
//! | `CreateFreeThreaded` sur thread STA | OK |
//! | **`SetIsBorderRequired(false)`** | **OK — pas de bordure jaune** |
//! | frames SANS handler `FrameArrived` | oui, en sondant `TryGetNextFrame` |
//! | frame -> texture -> SRV | `S_OK`, format **87 = `B8G8R8A8_UNORM`** |
//! | fenêtre RÉDUITE | item valide, taille minuscule, **0 frame** |
//!
//! **AUCUNE conversion de canal n'est nécessaire** : `B8G8R8A8_UNORM` décrit la
//! disposition EN MÉMOIRE ; l'échange vers `(r, g, b, a)` est fait par le
//! matériel à l'échantillonnage. Le piège à ne pas commettre serait de forcer le
//! format du SRV à `R8G8B8A8_UNORM` « pour être cohérent » : les deux formats
//! ne sont PAS dans la même famille de compatibilité DXGI et
//! `CreateShaderResourceView` échouerait. On passe donc `None` en descripteur.
//!
//! **WGC ne réduit PAS à la taille du pool** : un pool créé à moitié taille rend
//! quand même les frames à la taille de la SOURCE. C'est le sampler qui réduit
//! au dessin — donc, à terme, un aliasing de minification possible sur une
//! grande fenêtre affichée en petite vignette.

use windows::core::{HSTRING, Interface, Result, HRESULT};
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11ShaderResourceView, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::RoGetActivationFactory;

/// Tampons du pool. Deux suffisent et c'est la valeur des exemples Microsoft ;
/// au-delà, on ne fait que retarder l'image affichée.
const FRAME_BUFFER_COUNT: i32 = 2;

/// Vrai si Windows sait capturer une fenêtre sur cette machine — testé UNE fois
/// (`GraphicsCaptureSession::IsSupported`) plutôt qu'à chaque session : sur une
/// édition sans le pack média, la réponse est `false` et toute la
/// fonctionnalité doit se désactiver sans le moindre échec par frame.
pub fn is_supported() -> bool {
    GraphicsCaptureSession::IsSupported().unwrap_or(false)
}

/// Une session de capture pour UNE fenêtre, avec sa dernière frame prête à
/// dessiner.
pub struct CaptureSession {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    /// La frame NON FERMÉE. Le pool n'a que deux tampons : une frame qu'on
    /// garde immobilise le sien, et ne jamais la fermer finit par étrangler le
    /// pool. On n'en détient donc qu'UNE, fermée au début du sondage suivant.
    frame: Option<Direct3D11CaptureFrame>,
    /// La texture et la vue de la dernière frame. **Détruites AVANT la frame
    /// dont elles sont issues** (voir `poll`) : une vue qui survit à sa
    /// ressource est un usage après libération.
    texture: Option<ID3D11Texture2D>,
    srv: Option<ID3D11ShaderResourceView>,
    size: (u32, u32),
}

impl CaptureSession {
    /// Ouvre une session sur `hwnd`. `None` si la fenêtre refuse la capture —
    /// cas normal et non signalé d'une fenêtre réduite, protégée, ou en cours
    /// de fermeture, qu'on traite en n'affichant pas de vignette.
    pub fn start(device: &ID3D11Device, hwnd: HWND) -> Option<Self> {
        // Une seule fois pour toute la vie du processus : sur une édition de
        // Windows sans le pack média, `IsSupported` est faux et AUCUNE session
        // ne s'ouvrira — le panneau se contentera de cellules vides, plutôt que
        // de retenter (et échouer) à chaque survol.
        if !is_supported() {
            return None;
        }
        unsafe {
            let winrt_device = winrt_device(device)?;

            let item: GraphicsCaptureItem = RoGetActivationFactory::<IGraphicsCaptureItemInterop>(
                &HSTRING::from("Windows.Graphics.Capture.GraphicsCaptureItem"),
            )
            .ok()?
            .CreateForWindow(hwnd)
            .ok()?;

            let item_size = item.Size().ok()?;
            if item_size.Width <= 0 || item_size.Height <= 0 {
                return None;
            }

            let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
                &winrt_device,
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                FRAME_BUFFER_COUNT,
                item_size,
            )
            .ok()?;
            let session = pool.CreateCaptureSession(&item).ok()?;

            // Pas de curseur dans la vignette (ce n'est pas une capture
            // d'écran), et surtout PAS de bordure de capture : sans cet appel,
            // Windows dessine un cadre jaune autour de la fenêtre visée, ce qui
            // serait visible par l'utilisateur à chaque survol. Mesuré : la
            // désactivation passe sur cette machine (Windows 11).
            //
            // Les deux réglages DOIVENT précéder `StartCapture` : après, c'est
            // `E_ILLEGAL_METHOD_CALL`.
            let _ = session.SetIsCursorCaptureEnabled(false);
            match session.SetIsBorderRequired(false) {
                Ok(()) => {}
                Err(error) => {
                    // Sur une build antérieure à Windows 11, l'interface
                    // `IGraphicsCaptureSession3` n'existe pas. On le dit UNE
                    // fois, plutôt que d'afficher une bordure jaune sans
                    // explication.
                    eprintln!(
                        "[neuradesk][wgc] bordure de capture impossible à désactiver ({error:?}) — \
                         une bordure jaune apparaîtra autour des fenêtres prévisualisées"
                    );
                }
            }
            session.StartCapture().ok()?;

            Some(Self {
                pool,
                session,
                frame: None,
                texture: None,
                srv: None,
                size: (item_size.Width as u32, item_size.Height as u32),
            })
        }
    }

    /// Taille (largeur, hauteur) du contenu capturé.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// La vue de la dernière frame, s'il y en a une — à dessiner en
    /// `QuadInstance::new_textured`.
    pub fn srv(&self) -> Option<&ID3D11ShaderResourceView> {
        self.srv.as_ref()
    }

    /// Prend la frame suivante si elle est arrivée : `true` si `srv` a changé.
    ///
    /// **L'ORDRE DES LIBÉRATIONS EST LE CŒUR DE CETTE FONCTION** : on ferme la
    /// frame précédente, on en prend une nouvelle, on en tire texture et vue,
    /// puis on lâche l'ANCIENNE texture et l'ANCIENNE vue — jamais une vue qui
    /// survit à sa texture.
    ///
    /// Sonder plutôt que s'abonner à `FrameArrived` : aucun `TypedEventHandler`
    /// à implémenter, donc aucune marshalling d'appartement et aucun rappel
    /// possible pendant qu'on tient un emprunt sur l'état — et la frame est
    /// consommée exactement quand on la dessine. Mesuré : le pool se remplit
    /// quand même.
    pub fn poll(&mut self, device: &ID3D11Device) -> bool {
        if let Some(frame) = self.frame.take() {
            let _ = frame.Close();
        }

        let frame = match self.pool.TryGetNextFrame() {
            Ok(frame) => frame,
            // `TryGetNextFrame` rend `Err(Error::empty())` quand il n'y a RIEN
            // de neuf : cas NORMAL, pas un échec. Le confondre avec une erreur
            // remplirait la console d'un message par frame et par fenêtre.
            Err(error) if error.code() == HRESULT(0) => return false,
            Err(_) => return false,
        };

        let captured: Result<ID3D11Texture2D> = unsafe {
            frame
                .Surface()
                .and_then(|surface| surface.cast::<IDirect3DDxgiInterfaceAccess>())
                .and_then(|access| access.GetInterface::<ID3D11Texture2D>())
        };
        let Ok(texture) = captured else {
            let _ = frame.Close();
            return false;
        };
        if let Ok(content) = frame.ContentSize() {
            if content.Width > 0 && content.Height > 0 {
                self.size = (content.Width as u32, content.Height as u32);
            }
        }

        let mut srv: Option<ID3D11ShaderResourceView> = None;
        // `None` en descripteur = vue AU FORMAT DE LA RESSOURCE : c'est ce qui
        // rend le `B8G8R8A8_UNORM` de WGC directement échantillonnable.
        if unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut srv)) }.is_err() {
            let _ = frame.Close();
            return false;
        }

        // Les anciennes AVANT les nouvelles : voir la doc de la fonction.
        self.texture = None;
        self.srv = None;
        self.texture = Some(texture);
        self.srv = srv;
        self.frame = Some(frame);
        true
    }
}

impl Drop for CaptureSession {
    /// Libère dans l'ordre inverse de la création — vue, texture, frame, pool,
    /// session. Laisser tomber au `Drop` de chaque champ marcherait aussi, mais
    /// l'ordre explicite évite de dépendre de l'ordre de déclaration.
    fn drop(&mut self) {
        self.srv = None;
        self.texture = None;
        if let Some(frame) = self.frame.take() {
            let _ = frame.Close();
        }
        let _ = self.pool.Close();
        let _ = self.session.Close();
    }
}

/// Le périphérique WinRT que réclame WGC, fabriqué depuis CELUI DE
/// L'APPLICATION — pas un second périphérique : les textures produites arrivent
/// donc directement dans notre pipeline, sans partage inter-périphériques.
///
/// Exige `D3D11_CREATE_DEVICE_BGRA_SUPPORT` sur le périphérique (voir
/// `gfx::device`) : c'est la seule exigence de WGC, et elle est déjà remplie.
fn winrt_device(device: &ID3D11Device) -> Option<IDirect3DDevice> {
    unsafe {
        device
            .cast::<IDXGIDevice>()
            .and_then(|dxgi| CreateDirect3D11DeviceFromDXGIDevice(&dxgi))
            .and_then(|inspectable| inspectable.cast::<IDirect3DDevice>())
            .ok()
    }
}
