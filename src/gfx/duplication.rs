//! Capture GPU native du bureau via la Desktop Duplication API
//! (`IDXGIOutputDuplication`) — remplace la capture GDI (`BitBlt`/
//! `StretchBlt`, voir l'ancienne implémentation encore utilisée en repli
//! dans `gfx::backdrop`) : la texture est fournie directement par DWM sur
//! le device D3D11 demandeur, sans passer par une copie CPU intermédiaire,
//! donc utilisable à pleine résolution écran à chaque frame sans le coût
//! qui forçait auparavant à sous-échantillonner dès la capture (voir
//! `gfx::backdrop::DOWNSCALE`, doc historique).
//!
//! Portée volontairement limitée au moniteur PRINCIPAL : le dock lui-même
//! ne s'ancre que sur `SM_CXSCREEN`/`SM_CYSCREEN` (voir
//! `win32::window::Window::create_dock`), jamais un moniteur secondaire —
//! inutile de dupliquer tous les moniteurs.
//!
//! Robustesse : `AcquireNextFrame` échoue normalement (`DXGI_ERROR_
//! WAIT_TIMEOUT`) dès que le bureau n'a pas changé depuis le dernier appel
//! — pas une erreur, on continue de servir la dernière image copiée. Une
//! vraie perte de session (`DXGI_ERROR_ACCESS_LOST` : verrouillage de
//! session, invite UAC sur le bureau sécurisé, changement de mode
//! graphique...) ou tout autre échec (RDP, GPU hybride où ce device ne
//! pilote pas le moniteur principal, composant absent) fait retomber sur
//! le chemin GDI de `gfx::backdrop` pour cette frame, avec un
//! réessai à froid plutôt qu'à chaque frame (voir `RETRY_COOLDOWN`) —
//! ne doit jamais faire planter ni geler l'appli (voir la mémoire
//! [[neuradesk-live-input-safety]] : cette appli tourne sur le bureau réel
//! de l'utilisateur).

use std::cell::RefCell;
use std::time::{Duration, Instant};

use windows::core::{Error, Interface, Result};
use windows::Win32::Foundation::{E_FAIL, RECT};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource, DXGI_ERROR_WAIT_TIMEOUT,
    DXGI_OUTDUPL_FRAME_INFO,
};

/// Si l'initialisation (ou une frame) échoue, on n'insiste pas à chaque
/// appel (jusqu'à 60×/s) — ce serait à la fois inutile (une session
/// verrouillée le reste souvent plusieurs secondes/minutes) et bruyant
/// (un `eprintln!` par frame). On retente au plus une fois par intervalle.
const RETRY_COOLDOWN: Duration = Duration::from_millis(1000);

/// Session de duplication active, plus l'état nécessaire pour continuer à
/// servir la dernière image connue entre deux frames réellement changées.
struct Duplication {
    output_dup: IDXGIOutputDuplication,
    /// Rectangle du moniteur dupliqué, en coordonnées bureau virtuel — les
    /// rects demandés par les appelants (voir `capture_rect_source` dans
    /// `gfx::blur`) sont dans ce même repère (coordonnées écran physiques
    /// absolues, voir `win32::window::Window::screen_origin`).
    desktop_rect: RECT,
    /// Copie persistante de la dernière image complète du bureau. La
    /// texture renvoyée par `AcquireNextFrame` ne survit PAS à
    /// `ReleaseFrame` (voir sa doc Win32) et n'a de toute façon pas
    /// `D3D11_BIND_SHADER_RESOURCE` — on la recopie donc ici à chaque
    /// nouvelle frame réellement reçue (`CopyResource`, coût GPU→GPU
    /// négligeable), et on continue de lire CETTE copie le reste du temps.
    full_frame: ID3D11Texture2D,
}

/// État du thread courant — un seul flux de duplication partagé par
/// toutes les fenêtres (dock, Dash, Réglages, presse-papiers, popups),
/// plutôt qu'une session par fenêtre : Windows n'autorise qu'un nombre
/// limité de sessions de duplication simultanées par sortie, et une seule
/// suffit puisque tout le rendu tourne sur le même thread (voir la boucle
/// de messages partagée, `app::run`).
enum Slot {
    Uninit,
    Active(Duplication),
    /// Échec récent (init ou frame) — voir `RETRY_COOLDOWN`.
    Failed { since: Instant },
}

thread_local! {
    static STATE: RefCell<Slot> = RefCell::new(Slot::Uninit);
}

/// Libère la session de duplication ET sa texture — à appeler AVANT que le
/// device D3D11 ne soit détruit (voir `app::run`, qui s'en charge par un garde
/// déclaré juste après lui).
///
/// **Sans cet appel, l'application MEURT à chaque fermeture.** `STATE` est un
/// `thread_local!`, donc détruit à la fin du THREAD — c'est-à-dire APRÈS
/// `app::run`, donc après le device lui-même : `full_frame` était encore
/// vivante quand le device mourait. D3D11/DXGI signale les objets encore
/// vivants d'un device qu'on détruit en levant une exception `0x87A` (facility
/// DXGI) que rien ne rattrape — processus tué net, sans le moindre message.
/// DÉFAUT RÉEL CONSTATÉ : un vidage dans `%LOCALAPPDATA%\CrashDumps` à CHAQUE
/// sortie, y compris propres, depuis toujours.
pub fn release() {
    STATE.with(|state| *state.borrow_mut() = Slot::Uninit);
}

/// Assure qu'une image récente du bureau principal est disponible et la
/// renvoie (texture GPU persistante + rectangle du moniteur en coordonnées
/// bureau virtuel), ou `None` si la Desktop Duplication est indisponible
/// pour cette frame (session verrouillée, RDP, composant absent, device
/// sur le mauvais adaptateur GPU...) — l'appelant doit alors retomber sur
/// la capture GDI (voir `gfx::backdrop`).
pub(crate) fn ensure_frame(device: &ID3D11Device, ctx: &ID3D11DeviceContext) -> Option<(ID3D11Texture2D, RECT)> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        match &mut *state {
            Slot::Uninit => {}
            Slot::Failed { since } => {
                if since.elapsed() < RETRY_COOLDOWN {
                    return None;
                }
            }
            Slot::Active(dup) => match dup.acquire_and_copy(ctx) {
                Ok(()) => return Some((dup.full_frame.clone(), dup.desktop_rect)),
                Err(err) => {
                    eprintln!("[neuradesk] Desktop Duplication perdue, repli GDI temporaire ({err})");
                    *state = Slot::Failed { since: Instant::now() };
                    return None;
                }
            },
        }

        // Ici : Uninit, ou Failed dont le délai de réessai est passé.
        match Duplication::new(device) {
            Ok(mut dup) => {
                // Première image tout de suite (sinon `full_frame` reste
                // vide/noire jusqu'au prochain changement réel du bureau).
                let _ = dup.acquire_and_copy(ctx);
                let result = (dup.full_frame.clone(), dup.desktop_rect);
                *state = Slot::Active(dup);
                Some(result)
            }
            Err(err) => {
                eprintln!("[neuradesk] Desktop Duplication indisponible, repli GDI ({err})");
                *state = Slot::Failed { since: Instant::now() };
                None
            }
        }
    })
}

impl Duplication {
    fn new(device: &ID3D11Device) -> Result<Self> {
        let output1 = find_primary_output(device)?;
        let dxgi_device: IDXGIDevice = device.cast()?;
        let output_dup = unsafe { output1.DuplicateOutput(&dxgi_device)? };
        let desc = unsafe { output1.GetDesc()? };
        let desktop_rect = desc.DesktopCoordinates;
        let width = (desktop_rect.right - desktop_rect.left).max(1) as u32;
        let height = (desktop_rect.bottom - desktop_rect.top).max(1) as u32;
        let full_frame = create_full_frame_texture(device, width, height)?;
        Ok(Self { output_dup, desktop_rect, full_frame })
    }

    /// Récupère la prochaine image si le bureau a changé (`CopyResource`
    /// dans `full_frame`), ou ne fait rien si `DXGI_ERROR_WAIT_TIMEOUT`
    /// (aucun changement depuis le dernier appel — le cas NORMAL en
    /// pratique, un bureau statique ne pousse pas de nouvelle image à
    /// chaque interrogation à 60+ Hz). Toute autre erreur (perte de
    /// session...) remonte à l'appelant.
    fn acquire_and_copy(&mut self, ctx: &ID3D11DeviceContext) -> Result<()> {
        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        let acquired = unsafe { self.output_dup.AcquireNextFrame(0, &mut frame_info, &mut resource) };
        match acquired {
            Ok(()) => {
                // `ReleaseFrame` DOIT être appelée avant le prochain
                // `AcquireNextFrame` (sinon DXGI_ERROR_INVALID_CALL) — on
                // la fait tout de suite après avoir copié le contenu utile,
                // que la copie réussisse ou non (`?` renverrait sinon sans
                // relâcher la frame, verrouillant tous les appels suivants).
                let copy_result = match &resource {
                    Some(resource) => resource.cast::<ID3D11Texture2D>().map(|tex| unsafe {
                        ctx.CopyResource(&self.full_frame, &tex);
                    }),
                    None => Ok(()),
                };
                unsafe { self.output_dup.ReleaseFrame()? };
                copy_result
            }
            Err(err) if err.code() == DXGI_ERROR_WAIT_TIMEOUT => Ok(()),
            Err(err) => Err(err),
        }
    }
}

/// Trouve, parmi les sorties de l'adaptateur GPU qui possède `device`
/// (`IDXGIDevice::GetAdapter` — garantit que `DuplicateOutput(device)` ne
/// peut pas échouer pour cause d'adaptateur incompatible), celle dont les
/// coordonnées bureau commencent à (0, 0) : c'est TOUJOURS le moniteur
/// principal en coordonnées bureau virtuel Windows, quelle que soit sa
/// position physique — voir `win32::window::Window::create_dock`, qui
/// n'ancre le dock que sur ce moniteur-là. Si `device` tourne sur un
/// adaptateur qui ne pilote pas le moniteur principal (GPU hybride/
/// discret sur certains portables), aucune sortie ne correspond ici : on
/// remonte une erreur, l'appelant retombe sur GDI (qui lit toujours le
/// bureau réel, indépendamment du GPU).
fn find_primary_output(device: &ID3D11Device) -> Result<IDXGIOutput1> {
    let dxgi_device: IDXGIDevice = device.cast()?;
    let adapter = unsafe { dxgi_device.GetAdapter()? };

    let mut index = 0;
    loop {
        let output = match unsafe { adapter.EnumOutputs(index) } {
            Ok(output) => output,
            Err(_) => break, // DXGI_ERROR_NOT_FOUND : fin de l'énumération.
        };
        if let Ok(desc) = unsafe { output.GetDesc() } {
            if desc.DesktopCoordinates.left == 0 && desc.DesktopCoordinates.top == 0 {
                if let Ok(output1) = output.cast::<IDXGIOutput1>() {
                    return Ok(output1);
                }
            }
        }
        index += 1;
    }

    Err(Error::new(E_FAIL, "aucune sortie DXGI principale trouvée pour la Desktop Duplication"))
}

/// Texture de copie persistante du bureau — jamais échantillonnée
/// directement par un shader (voir `Duplication::full_frame`, doc du
/// champ) : uniquement source de `CopySubresourceRegion` dans
/// `gfx::blur::render_backdrop`, donc `BindFlags: 0` suffit.
fn create_full_frame_texture(device: &ID3D11Device, width: u32, height: u32) -> Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: 0,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture))? };
    Ok(texture.expect("CreateTexture2D a réussi sans remplir `texture`"))
}
