//! Swap chain DXGI dont l'ALPHA est composé par DWM — c'est ce qui permet au
//! rendu Direct3D du dock de laisser voir le bureau à travers ses pixels non
//! dessinés, sans passer par `WS_EX_LAYERED` (voir `win32::window`).
//!
//! **Cela ne marche QUE dans le modèle de présentation BitBlt** : un swap chain
//! en modèle *flip* ignore l'alpha, et ses pixels non dessinés sont NOIRS. Voir
//! `SwapChain::new`, dont le paramètre `transparent` choisit le modèle — et le
//! rappel du défaut réel que cette subtilité a causé.

use windows::core::Result;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11RenderTargetView, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_UNSPECIFIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN,
    DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory2, IDXGISwapChain1, DXGI_PRESENT, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_DISCARD, DXGI_SWAP_EFFECT_FLIP_DISCARD,
    DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

pub struct SwapChain {
    pub swapchain: IDXGISwapChain1,
    rtv: Option<ID3D11RenderTargetView>,
    pub width: u32,
    pub height: u32,
}

impl SwapChain {
    /// `transparent` : `true` pour une fenêtre dont DWM doit composer l'ALPHA —
    /// le dock (primaire ET secondaires), l'horloge plein écran, et depuis le
    /// 2026-11-12 les POPUPS CEF (Dash, Réglages, Presse-papiers, Notes, menus),
    /// l'INFOBULLE et les PANNEAUX D'APERÇUS. C'est le repli « sans fond »
    /// (`gfx::quad_renderer`, `Ombres.y`) qui le réclame : quand la LOUPE
    /// (`gfx::mag_capture`) ne fournit pas de fond — machine où elle est
    /// indisponible — le shader sort l'alpha de transparence de l'utilisateur,
    /// et DWM doit le composer. En modèle *flip* l'alpha est ignoré : la fenêtre
    /// resterait un aplat opaque assombri (rgb × alpha).
    ///
    /// Le paramètre ne change rien quand la capture est invisible (ni loupe ni
    /// repli : alpha 1, fenêtre opaque). Plus aucune fenêtre n'utilise `false` à
    /// ce jour ; le modèle *flip* reste disponible pour une fenêtre réellement
    /// opaque.
    ///
    /// **L'horloge plein écran a rejoint le premier groupe quand le texte de son
    /// horloge s'est mis à grossir au survol** : sa fenêtre doit désormais être
    /// plus GRANDE que ce qu'elle dessine (le texte magnifié déborde de la
    /// pilule), et tout cet écart serait noir en modèle *flip* — elle porte
    /// `DwmExtendFrameIntoClientArea` (voir `win32::fullscreen_clock`).
    ///
    /// **Le modèle de présentation DÉCIDE de la transparence par pixel**, et
    /// c'est un piège coûteux : un swap chain en modèle *flip*
    /// (`DXGI_SWAP_EFFECT_FLIP_DISCARD`) l'IGNORE, même avec
    /// `DwmExtendFrameIntoClientArea` posé — ses pixels jamais dessinés sont
    /// alors NOIRS et opaques. C'était le cas du dock, sans conséquence tant que
    /// sa région en « ⊓ » cachait tous ces pixels ; le débordement des icônes
    /// magnifiées les a rendus visibles (DÉFAUT RÉEL SIGNALÉ : « zone noire
    /// derrière les icônes grandifiées »). Le modèle BitBlt (`DISCARD`), lui,
    /// compose bien avec l'alpha — c'est l'intention d'origine de ce fichier,
    /// que le passage au modèle *flip* avait silencieusement cassée.
    pub fn new(device: &ID3D11Device, hwnd: HWND, width: u32, height: u32, transparent: bool) -> Result<Self> {
        let factory: IDXGIFactory2 = unsafe { CreateDXGIFactory1() }?;

        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: width,
            Height: height,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            // Le modèle BitBlt n'accepte qu'UN tampon (`SEQUENTIAL` en tolère
            // deux) ; le modèle flip en veut au moins deux.
            BufferCount: if transparent { 1 } else { 2 },
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: if transparent { DXGI_SWAP_EFFECT_DISCARD } else { DXGI_SWAP_EFFECT_FLIP_DISCARD },
            // PREMULTIPLIED n'est valide que pour CreateSwapChainForComposition ;
            // un swap chain lié directement à un HWND (notre cas) exige
            // UNSPECIFIED, sous peine de DXGI_ERROR_INVALID_CALL. La
            // translucidité vient donc de DwmExtendFrameIntoClientArea (voir
            // win32::window) — mais SEULEMENT en modèle BitBlt, voir la doc de
            // `new`.
            AlphaMode: DXGI_ALPHA_MODE_UNSPECIFIED,
            Flags: 0,
        };

        let swapchain = unsafe { factory.CreateSwapChainForHwnd(device, hwnd, &desc, None, None)? };
        let rtv = Some(create_rtv(device, &swapchain)?);

        Ok(Self {
            swapchain,
            rtv,
            width,
            height,
        })
    }

    pub fn rtv(&self) -> Option<&ID3D11RenderTargetView> {
        self.rtv.as_ref()
    }

    /// Le back buffer courant (index 0) — voir `gfx::capture::dump_backbuffer`,
    /// seul moyen de vérifier visuellement le rendu de cette application
    /// (ses fenêtres sont exclues de toute capture d'écran externe). À
    /// appeler AVANT `present` : `DXGI_SWAP_EFFECT_FLIP_DISCARD` (voir plus
    /// bas) ne garantit le contenu du buffer 0 qu'entre le dernier rendu et
    /// le `Present` qui le fait défiler.
    pub fn back_buffer(&self) -> Result<ID3D11Texture2D> {
        unsafe { self.swapchain.GetBuffer(0) }
    }

    pub fn resize(&mut self, device: &ID3D11Device, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 || (width == self.width && height == self.height) {
            return Ok(());
        }
        // Il faut relâcher toute référence sur les back buffers avant de
        // redimensionner, sinon ResizeBuffers échoue.
        self.rtv = None;
        let redimensionne = unsafe { self.swapchain.ResizeBuffers(0, width, height, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0)) };
        if let Err(erreur) = redimensionne {
            // Les buffers n'ont pas changé : on leur rend leur cible, sinon le
            // prochain rendu à l'ancienne taille n'en aurait plus (plantage du
            // 2026-11-01, `ouverture_fenetres`).
            self.rtv = create_rtv(device, &self.swapchain).ok();
            return Err(erreur);
        }
        self.rtv = Some(create_rtv(device, &self.swapchain)?);
        self.width = width;
        self.height = height;
        Ok(())
    }

    pub fn present(&self) -> Result<()> {
        unsafe { self.swapchain.Present(1, DXGI_PRESENT(0)).ok() }
    }

    /// Présente SANS ATTENDRE le prochain rafraîchissement (`Present(0)`).
    ///
    /// Pour les petites fenêtres dont le contenu n'a aucune raison de suivre
    /// le rythme de l'écran — l'INFOBULLE, et elle seule pour l'instant (voir
    /// `win32::tooltip::TooltipWindow::show`).
    ///
    /// ⚠️ **CE N'EST PAS ELLE QUI A RÉGLÉ LA SACCADE.** Mesuré le 2026-11-11 :
    /// présenter la fenêtre d'infobulle coûte ~6 ms À CHAQUE APPEL, même en
    /// `Present(0)` — la désynchronisation n'y change donc presque rien. Ce
    /// qui a rendu les 143 images/s, c'est de NE PLUS PRÉSENTER à chaque
    /// frame (voir `TooltipWindow::show`, qui ne redessine que si son image
    /// change). Cet appel-ci évite seulement d'ajouter une attente de vsync
    /// aux rares redessins qui restent.
    pub fn present_sans_attente(&self) -> Result<()> {
        unsafe { self.swapchain.Present(0, DXGI_PRESENT(0)).ok() }
    }
}

fn create_rtv(device: &ID3D11Device, swapchain: &IDXGISwapChain1) -> Result<ID3D11RenderTargetView> {
    unsafe {
        let backbuffer: ID3D11Texture2D = swapchain.GetBuffer(0)?;
        let mut rtv = None;
        device.CreateRenderTargetView(&backbuffer, None, Some(&mut rtv))?;
        Ok(rtv.unwrap())
    }
}
