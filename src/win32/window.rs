//! Fenêtre Win32 du dock : une forme en "⊓" toujours au premier plan —
//! une barre supérieure pleine largeur et deux barres latérales (gauche et
//! droite), toutes de l'épaisseur de la barre des tâches Windows, sans
//! jamais la recouvrir.
//!
//! (Tentative abandonnée : étendre les barres gauche/droite jusqu'au vrai
//! bord bas en comptant sur le système d'AppBars pour que la taskbar se
//! redimensionne et partage le bord avec elles — comportement historique
//! des AppBars tierces, mais vérifié entre-temps : la taskbar de Windows 11
//! ne bouge pas d'un pixel malgré une réservation AppBar qui, elle,
//! fonctionne bien côté zone de travail. Sans ce redimensionnement, étendre
//! nos barres aurait juste recouvert la taskbar — annulé.)
//!
//! Transparence : on n'utilise PAS `WS_EX_LAYERED` (incompatible avec le
//! modèle de présentation "flip" de DXGI). À la place on étend le glass DWM
//! sur toute la fenêtre via `DwmExtendFrameIntoClientArea`, et le swap chain
//! Direct3D (voir `gfx::swapchain`) est créé avec un canal alpha
//! prémultiplié : c'est DWM qui compose le résultat avec le bureau.
//!
//! Forme en "⊓" : la fenêtre elle-même reste rectangulaire, mais on ne rend
//! visible que l'union des trois bandes via `SetWindowRgn` — le reste laisse
//! voir le bureau/les fenêtres en dessous.
//!
//! Étendue : le rectangle couvre l'écran MOINS la barre des tâches Windows,
//! SAUF quand celle-ci est en auto-masquage — elle ne réserve alors plus rien et
//! la fenêtre descend jusqu'au bord bas (voir `primary_dock_bounds`).
//!
//! Réservation d'espace : les trois bandes sont aussi enregistrées comme
//! AppBars auprès du shell (voir `win32::appbar`), pour que les fenêtres
//! maximisées et les icônes du bureau ne les recouvrent pas non plus.

use std::cell::RefCell;
use std::collections::VecDeque;

use windows::core::{w, Error, Result, BOOL, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateEllipticRgn, CreateRectRgn, DeleteObject, GetMonitorInfoW, MonitorFromWindow, ScreenToClient,
    SetWindowRgn, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST, RGN_DIFF, RGN_OR,
};
use windows::Win32::System::DataExchange::AddClipboardFormatListener;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{MARGINS, WM_MOUSELEAVE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, SHQueryUserNotificationState, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETTASKBARPOS,
    APPBARDATA, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows, GetClassNameW, GetForegroundWindow,
    GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, IsZoomed, LoadCursorW, PeekMessageW,
    PostQuitMessage, RegisterClassExW,
    SetForegroundWindow, SetWindowPos, ShowWindow, SystemParametersInfoW, TranslateMessage,
    CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, GWL_STYLE, GW_HWNDPREV, GW_OWNER, HWND_TOPMOST, IDC_ARROW, MSG, PM_REMOVE, SM_CXSCREEN, SM_CYSCREEN,
    SPI_GETWORKAREA, SPI_SETWORKAREA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER,
    SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE, WS_CAPTION,
    WM_CLIPBOARDUPDATE, WM_DESTROY, WM_DISPLAYCHANGE, WM_ERASEBKGND,
    SendMessageW, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED, LoadImageW, SM_CXICON, SM_CYICON, SM_CXSMICON,
    SM_CYSMICON, WM_SETICON, SYSTEM_METRICS_INDEX,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_MBUTTONDOWN, WM_QUIT, WM_RBUTTONDOWN, WM_SETTINGCHANGE, WM_SIZE, WM_TIMER,
    WINDOW_EX_STYLE, WNDCLASSEXW,
    WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics;

use crate::win32::appbar::AppBar;
use crate::win32::screen_dock::BarThicknesses;
use crate::win32::taskbar_autohide;

/// **Dans quel état une capture d'écran met les barres** — voir
/// `Window::suspendre_pour_capture` et `SecondaryWindow::suspendre_pour_capture`.
///
/// ⭐ **TROIS ÉTATS, ET LES DEUX AXES SONT INDÉPENDANTS** : « la fenêtre est-elle
/// peinte ? » et « réserve-t-elle de la place ? ». Un booléen ne peut pas les
/// porter — les deux gestes de capture veulent des combinaisons différentes.
///
/// | état | fenêtre | réservations | qui |
/// |---|---|---|---|
/// | `Repos` | affichée | posées | hors capture, et la reprise |
/// | `Masquees` | cachée | **à zéro** | clic GAUCHE |
/// | `MasqueesSansLiberer` | cachée | **posées** | clic DROIT, APRÈS le gel |
///
/// ⛔ **`MasqueesSansLiberer` EST LE POINT SUBTIL, ET IL A ÉTÉ PAYÉ.** Le clic
/// droit libérait d'abord les réservations (pour que la fenêtre agrandie occupe
/// tout l'écran) — DÉCISION ANNULÉE PAR L'UTILISATEUR : « les fenêtres de
/// programme continuent de s'agrandir hors de la zone centrale délimitée par
/// l'app ». Une fenêtre agrandie qui déborde SOUS nos barres est donc un défaut,
/// pas un effet voulu : elle doit rester dans le cadre que les barres délimitent.
///
/// ⚠️ **LE MASQUAGE D'APRÈS LE GEL RESTE NÉCESSAIRE MALGRÉ TOUT** : la région du
/// dock EST sa zone de clic (`set_dock_region` l'écrit — son `wndproc` ne traite
/// aucun `WM_NCHITTEST`, et la fenêtre n'a pas `WS_EX_TRANSPARENT`). Une bande
/// laissée visible rend la sélection impossible dessous : « je ne peux pas
/// sélectionner hors de la zone centrale ». On cache donc la fenêtre APRÈS avoir
/// figé l'image, et on ne touche PAS aux réservations — sans quoi le bureau se
/// recomposerait sous les yeux de l'utilisateur pendant la sélection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EtatBarresPourCapture {
    /// État normal : réservations posées, fenêtre affichée selon le plein écran.
    Repos,
    /// Clic GAUCHE : fenêtre CACHÉE, réservations à zéro — dans l'image, le
    /// programme agrandi doit occuper tout l'écran.
    Masquees,
    /// Clic DROIT, APRÈS le gel : fenêtre CACHÉE, réservations CONSERVÉES.
    MasqueesSansLiberer,
}

/// Épaisseur de repli si la détection de la barre des tâches échoue
/// complètement (ne devrait jamais arriver en pratique sous Windows).
const FALLBACK_BAR_THICKNESS: i32 = 48;

/// Styles étendus des fenêtres de BARRES — dock primaire (`Window::create_dock`)
/// et docks secondaires (`SecondaryWindow::create`), les deux SEULS sites de
/// création d'une barre : ils doivent donner le même style, d'où cette
/// constante unique.
///
/// **`WS_EX_APPWINDOW` est ici, et `WS_EX_TOOLWINDOW` n'y est PLUS.** Une barre
/// doit se présenter au shell comme un programme NORMAL : présente dans Alt-Tab
/// et dans la barre des tâches. Or elle garde `WS_EX_NOACTIVATE` (indispensable :
/// une barre ne doit JAMAIS voler le focus, voir `create_dock`), et le MSDN est
/// explicite sur ce style — « *The window does not appear on the taskbar by
/// default. To force the window to appear on the taskbar, use the
/// `WS_EX_APPWINDOW` style.* » Quant à `WS_EX_TOOLWINDOW` : « *does not appear
/// in the taskbar or in the dialog that appears when the user presses ALT+TAB* ».
///
/// DÉFAUT RÉEL SIGNALÉ le 2026-09-20 (« je ne vois pas neuradesk dans le
/// alt-tab ») : les barres étaient créées AVEC `WS_EX_TOOLWINDOW`, et le réglage
/// « rendre l'interface visible sur les captures d'écran » se contentait de le
/// RETIRER (`gfx::capture_visibility::set_toolwindow`). C'est nécessaire mais
/// **insuffisant** pour une fenêtre `WS_EX_NOACTIVATE` : le seul levier
/// documenté, `WS_EX_APPWINDOW`, n'était jamais posé — la barre ne réapparaissait
/// donc nulle part, et le commentaire de `capture_visibility` qui annonçait cet
/// « effet de bord » l'affirmait sans l'avoir mesuré.
///
/// Ne PAS remettre `WS_EX_TOOLWINDOW` ici « pour ne pas polluer la barre des
/// tâches » : c'est exactement le style qui contredit les deux demandes — être
/// dans Alt-Tab, et être dans la liste des fenêtres d'OBS.
pub(crate) const DOCK_EX_STYLE: WINDOW_EX_STYLE =
    WINDOW_EX_STYLE(WS_EX_TOPMOST.0 | WS_EX_NOACTIVATE.0 | WS_EX_APPWINDOW.0);

/// Identifiant de la ressource d'icône de l'exécutable : `1 ICON` dans
/// `NeuraDeskIcon/NeuraDesk.rc`, compilé par `build.rs`. C'est l'icône du
/// FICHIER, distincte du logo dessiné À L'INTÉRIEUR de l'application (voir
/// `gfx::texture::LOGO_PNG`) — et c'est bien celle-ci que le shell doit montrer
/// pour la fenêtre, celle du `.exe` que l'utilisateur voit dans l'Explorateur.
const EXE_ICON_RESOURCE_ID: u16 = 1;

/// Les deux emplacements d'icône à poser sur une fenêtre, chacun avec la
/// métrique système qui donne sa dimension.
///
/// Séparé de l'appel Win32 pour être testable, et parce que le PAIRAGE compte
/// autant que l'ordre : `ICON_SMALL`/`SM_CXSMICON` (16 px) est ce que la BARRE
/// DES TÂCHES interroge (par `ICON_SMALL2`, qui retombe sur `ICON_SMALL` — voir
/// la doc de `WM_SETICON`), `ICON_BIG`/`SM_CXICON` (32 px) la grande icône du
/// sélecteur ALT+TAB. Les intervertir donnerait une icône minuscule étirée dans
/// Alt-Tab et un pavé illisible dans la barre des tâches.
fn exe_icon_slots() -> [(u32, SYSTEM_METRICS_INDEX, SYSTEM_METRICS_INDEX); 2] {
    [(ICON_SMALL, SM_CXSMICON, SM_CYSMICON), (ICON_BIG, SM_CXICON, SM_CYICON)]
}

/// Donne à `hwnd` l'icône de l'EXÉCUTABLE (`EXE_ICON_RESOURCE_ID`) comme icône
/// de FENÊTRE — un `WM_SETICON` aux deux emplacements que le shell interroge.
///
/// **POURQUOI EXPLICITEMENT, ALORS QUE LE SHELL SAIT SE DÉBROUILLER.** Les
/// fenêtres de barres ne déclaraient AUCUNE icône : ni `WM_SETICON` (absent de
/// tout le dépôt avant ce correctif), ni icône de classe (`WNDCLASSEXW` est
/// construit avec `..Default::default()`, donc `hIcon` et `hIconSm` nuls). Le
/// shell devait donc DEVINER — et c'est cette devinette qui affichait une icône
/// périmée dans Alt-Tab et le Gestionnaire des tâches pendant que l'Explorateur,
/// lui, montrait la bonne. DÉFAUT RÉEL SIGNALÉ le 2026-09-20 : « c'est encore
/// l'ancienne icône dans alt tab et le gestionnaire des tâches », alors que le
/// binaire portait la nouvelle icône — vérifié octet à octet contre
/// `NeuraDeskIcon/NeuraDeskIcon.ico` (calque 256×256 retrouvé à l'offset 2565456),
/// et que l'Explorateur affichait la bonne sur le FICHIER.
///
/// Poser l'icône ici rend la fenêtre explicite : le shell lit ce que nous
/// déclarons au lieu de replier sur une identité d'application qu'il garde en
/// cache. C'est la pratique Win32 normale pour une fenêtre qui veut apparaître
/// dans la barre des tâches — et elle est devenue nécessaire le 2026-09-20, quand
/// les barres ont commencé à y apparaître (`DOCK_EX_STYLE`).
///
/// `LR_SHARED` : la ressource est partagée et NE DOIT PAS être détruite — elle
/// vit aussi longtemps que le module, donc rien à libérer, et surtout pas un
/// `DestroyIcon` (le faire rendrait l'icône invalide pour toutes les fenêtres).
///
/// Un échec de chargement est IGNORÉ (`continue`) plutôt que fatal : une fenêtre
/// sans icône est un défaut cosmétique, pas une raison de refuser de démarrer le
/// dock. Voir `exe_icon_slots` pour les deux emplacements posés.
pub(crate) fn apply_exe_icon(hwnd: HWND) {
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let instance: HINSTANCE = module.into();
        for (slot, cx_metric, cy_metric) in exe_icon_slots() {
            // `PCWSTR(id)` sur un petit entier EST l'idiome `MAKEINTRESOURCE` :
            // la ressource est désignée par son ID numérique, jamais par un nom.
            let Ok(icon) = LoadImageW(
                Some(instance),
                windows::core::PCWSTR(EXE_ICON_RESOURCE_ID as usize as *const u16),
                IMAGE_ICON,
                GetSystemMetrics(cx_metric),
                GetSystemMetrics(cy_metric),
                LR_SHARED,
            ) else {
                continue;
            };
            SendMessageW(hwnd, WM_SETICON, Some(WPARAM(slot as usize)), Some(LPARAM(icon.0 as isize)));
        }
    }
}

/// Bord de l'écran sur lequel la barre des tâches Windows est ancrée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskbarEdge {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Debug, Clone)]
pub enum Event {
    MouseMove { x: f32, y: f32, hwnd: HWND },
    MouseLeave { hwnd: HWND },
    MouseDown { x: f32, y: f32, hwnd: HWND },
    MouseUp { x: f32, y: f32, hwnd: HWND },
    /// Clic droit : `app::run` fait le lien avec l'icône visée et affiche
    /// (le cas échéant) le menu contextuel via `win32::menu`.
    RightClick { x: f32, y: f32, hwnd: HWND },
    /// Clic MOLETTE : sur la barre du bas, il ouvre la sphère des fenêtres
    /// (voir `win32::cef_fenetres`). Coordonnées CLIENT, comme `RightClick`.
    MiddleClick { x: f32, y: f32, hwnd: HWND },
    /// Molette de la souris au-dessus d'une barre — sert au DÉFILEMENT des
    /// icônes quand une barre en contient plus que sa hauteur ne permet
    /// (voir `ui::dock::Dock::scroll_bar`, DEMANDE EXPLICITE). `delta` en
    /// multiples de `WHEEL_DELTA` (120), positif = vers l'avant (haut) ;
    /// `(x, y)` en coordonnées CLIENT (convertis depuis l'écran à
    /// l'émission, voir `wndproc`).
    MouseWheel { delta: f32, x: f32, y: f32, hwnd: HWND },
    Resized { width: u32, height: u32 },
    CloseRequested,
    /// Le presse-papiers vient de changer (voir `AddClipboardFormatListener`
    /// dans `Window::create_dock`) et contenait du texte OU une image (voir
    /// `win32::clipboard::read`) — `app::run` l'ajoute à son historique
    /// (voir `win32::clipboard`).
    ClipboardChanged(crate::win32::clipboard::ClipboardEntry),
    /// La topologie des écrans a (peut-être) changé : résolution/bpp de
    /// l'écran principal (`WM_DISPLAYCHANGE`) ou déplacement/redimensionnement
    /// de la VRAIE barre des tâches Windows sans changement de résolution
    /// (`WM_SETTINGCHANGE`/`SPI_SETWORKAREA`, voir `wndproc`). AUCUNE
    /// charge utile : les paramètres de `WM_DISPLAYCHANGE` ne décrivent que
    /// la résolution/profondeur de couleur du moniteur PRINCIPAL, inutiles
    /// pour connaître la topologie complète (écrans secondaires branchés/
    /// débranchés, disposition) — `app::run` réagit donc TOUJOURS en
    /// ré-énumérant l'état réel (voir `Window::resync_to_primary_monitor`,
    /// `rebuild_secondary_mirrors`) plutôt qu'en se fiant à un quelconque
    /// paramètre de message.
    DisplaysChanged,
    /// Le masquage automatique de la barre des tâches Windows vient d'être
    /// basculé DEPUIS le panneau de réglages de NeuraDesk (voir
    /// `win32::cef_settings`, case "Masquer automatiquement la barre des
    /// tâches"). `app::run` recale alors la fenêtre du dock sur l'espace
    /// réellement disponible (voir `primary_dock_bounds`) — SANS reconstruire
    /// les docks secondaires, contrairement à `DisplaysChanged` : ils sont hors
    /// périmètre, et c'est ce chemin-là que la doc accuse de boucles.
    ///
    /// **Poussée EN PLUS de la rediffusion que le shell émet de son côté**, et
    /// ce n'est pas un doublon inutile : le basculement est aussi possible depuis
    /// Windows, où nous n'avons aucune prise, donc le chemin
    /// `WM_SETTINGCHANGE(SPI_SETWORKAREA)` doit de toute façon exister — mais il
    /// dépend d'un comportement du shell que rien ne garantit, et `ABM_SETSTATE`
    /// ne renvoie rien qui permette de savoir s'il a réussi. Ce déclenchement
    /// direct rend le geste déterministe ; les deux resynchronisations qui en
    /// résultent sont idempotentes.
    PrimaryDockResync,
}

thread_local! {
    static EVENTS: RefCell<VecDeque<Event>> = RefCell::new(VecDeque::new());
    static TRACKING_LEAVE: RefCell<bool> = RefCell::new(false);
}

pub struct Window {
    pub hwnd: HWND,
    /// Largeur de la zone couverte (= largeur de l'écran, pour la barre
    /// supérieure pleine largeur).
    pub width: u32,
    /// Hauteur de la zone couverte : l'écran moins la barre des tâches — SAUF
    /// quand celle-ci est en auto-masquage, auquel cas elle ne réserve plus rien
    /// et la fenêtre descend jusqu'au bord bas (voir `primary_dock_bounds`, qui
    /// porte cette règle et dit pourquoi seul le bord bas est concerné).
    pub height: u32,
    /// Épaisseur/largeur ACTUELLES des trois barres NeuraDesk elles-mêmes
    /// (haut, gauche, droite — voir `set_dock_thicknesses`), réglables
    /// INDÉPENDAMMENT depuis le panneau de réglages (demande explicite) —
    /// DISTINCTES de l'épaisseur de la VRAIE barre des tâches Windows
    /// (`detect_taskbar`, qui ne sert qu'à positionner la fenêtre pour ne
    /// jamais la recouvrir, voir `create_dock` — jamais réglable par
    /// l'utilisateur, ce serait absurde). Valeur de départ = l'épaisseur de
    /// la vraie barre des tâches détectée (comportement d'origine, avant ce
    /// réglage), simplement le point de départ maintenant modifiable.
    pub top_thickness: u32,
    pub left_width: u32,
    pub right_width: u32,
    /// Épaisseur de la barre du BAS — **écran principal seulement**, et c'est la
    /// seule barre dont ce champ ne vient PAS de la barre des tâches Windows :
    /// elle n'a pas d'équivalent à imiter, donc son défaut est
    /// `ui::dock::DEFAULT_BOTTOM_THICKNESS` (voir `create_dock`).
    pub bottom_thickness: u32,
    /// Réservations AppBar des quatre bandes (voir `win32::appbar`) — leur
    /// rectangle réservé est mis à jour par `set_dock_thicknesses`, tenues
    /// en vie tant que `Window` existe, libérées à son `Drop` implicite.
    top_appbar: AppBar,
    left_appbar: AppBar,
    right_appbar: AppBar,
    bottom_appbar: AppBar,
    /// HWND assistants (invisibles, AUCUN n'est `hwnd` — la fenêtre
    /// principale/de rendu D3D11 — même la bande du haut, voir
    /// `create_appbar_helper`) portant les trois AppBars — voir
    /// `win32::appbar` (un HWND par bord, le shell n'en acceptant qu'un par
    /// fenêtre). Conservés ici (pas seulement des variables locales de
    /// `create_dock`) pour pouvoir les DÉTRUIRE et en recréer de TOUT NEUFS
    /// à chaque `reregister_appbars` — BUG RÉEL CONSTATÉ EN PRATIQUE :
    /// ré-enregistrer (`ABM_NEW`) sur les MÊMES HWND après un redémarrage
    /// d'explorer.exe rapportait un succès Windows sans jamais faire
    /// réellement changer la zone de travail réservée (`SPI_GETWORKAREA`),
    /// même après 10 tentatives échelonnées sur 4 secondes — probablement
    /// un état mis en cache CÔTÉ CLIENT par `shell32.dll` (dans NOTRE PROPRE
    /// processus) associant déjà ces HWND à un appbar "connu", empêchant le
    /// message d'atteindre réellement le nouveau shell.
    top_helper_hwnd: HWND,
    left_helper_hwnd: HWND,
    right_helper_hwnd: HWND,
    bottom_helper_hwnd: HWND,
    /// `true` si `sync_fullscreen_visibility` a masqué le dock parce qu'une
    /// application plein écran (jeu, lecteur vidéo...) est active — voir sa
    /// doc. Distinct de la visibilité "normale" de la fenêtre (toujours
    /// affichée sinon, pas de notion d'ouverture/fermeture comme
    /// Dash/Réglages/Presse-papiers) : sert uniquement à savoir s'il faut la
    /// RÉAFFICHER quand le plein écran se termine, sans réafficher à tort une
    /// fenêtre qui aurait été masquée pour une AUTRE raison entre-temps (il
    /// n'y en a pas aujourd'hui, mais évite d'avoir à le redémontrer si une
    /// s'ajoute plus tard).
    hidden_for_fullscreen: bool,
    /// HWND de l'application plein écran qui a déclenché `hidden_for_fullscreen`
    /// — voir `window_is_still_fullscreen`. Revérifié directement (plutôt que
    /// le focus courant) tant qu'on reste masqué, pour ne pas réapparaître à
    /// tort dès qu'un AUTRE écran/une de nos propres fenêtres reçoit le
    /// focus alors que cette application est toujours ouverte.
    fullscreen_hwnd: Option<HWND>,
    /// Boîte de DÉBORDEMENT composée à la région (voir `set_overflow`) — le
    /// rectangle (`gauche, haut, droite, bas`, pixels fenêtre) de l'icône
    /// actuellement magnifiée, ou `None`. Porté par la structure, et non passé
    /// à chaque appelant, parce que TROIS chemins posent la région
    /// (`create_dock`, `set_dock_thicknesses`, `set_visible_region`, cette
    /// dernière à chaque frame en masquage automatique) : aucun ne doit
    /// pouvoir écraser le débordement posé par un autre.
    overflow: Option<[i32; 4]>,
    /// Largeurs RÉGLÉES des colonnes gauche et droite (voir
    /// `set_bottom_band_columns`) — la bande du bas de la région s'arrête à
    /// elles, même quand une colonne est masquée.
    colonnes_bas: (u32, u32),
    /// Épaisseur RÉGLÉE de la barre du haut (voir `set_bottom_band_columns`) —
    /// les colonnes latérales de la région commencent SOUS elle, même masquée.
    haut_regle: u32,
    /// Rayon des CONGÉS de la zone libre (voir `ui::dock::conges`), 0 sans
    /// coins arrondis. Même contrat que `overflow` : `set_rayon_conge` le
    /// mémorise, le prochain découpage l'applique.
    rayon_conge: i32,
}

/// Fenêtre au premier plan couvrant EXACTEMENT le moniteur sur lequel elle
/// se trouve — voir la doc de `Window::sync_fullscreen_visibility`, qui
/// appelle cette fonction en complément de `SHQueryUserNotificationState`
/// (celle-ci ne détecte que le plein écran D3D exclusif/le mode
/// présentation, PAS le plein écran "sans bordure" d'une simple fenêtre
/// agrandie à la taille de l'écran — le cas de la plupart des lecteurs
/// vidéo/navigateurs). Même heuristique géométrique que la VRAIE barre des
/// tâches Windows et des outils comme TranslucentTB pour ce cas précis :
/// comparer le rectangle de la fenêtre au premier plan à celui de SON
/// moniteur — une correspondance EXACTE (pas seulement "grande fenêtre")
/// écarte les fenêtres maximisées normales, qui respectent la zone de
/// travail (donc plus petites que le moniteur entier) plutôt que de le
/// recouvrir totalement. `exclude` : notre PROPRE fenêtre (voir `create_dock`),
/// qui couvre elle aussi tout l'écran (la forme en "⊓" vient de
/// `SetWindowRgn`, PAS d'une fenêtre plus petite) — sans cette exclusion,
/// `sync_fullscreen_visibility` se masquerait purement et simplement
/// elle-même dès qu'elle a le focus.
/// **CETTE FENÊTRE EST-ELLE UNE SURFACE DU SHELL, ET NON UNE APPLICATION ?**
///
/// Une surface du shell couvre l'écran **en permanence** sans que l'utilisateur
/// « regarde » quoi que ce soit dedans. La prendre pour une application plein écran
/// masque le dock pour rien — et, pire, son HWND est **MÉMORISÉ** par
/// `sync_fullscreen_visibility` puis revérifié à chaque frame (voir
/// `window_is_still_fullscreen`) : l'état ne se décolle donc plus tout seul.
///
/// **LES DEUX CAS SONT MESURÉS, PAS SUPPOSÉS** :
/// - `Progman` / `WorkerW` : le bureau lui-même, que `GetForegroundWindow` rend quand
///   rien d'autre n'a le focus ;
/// - ⛔ **`Windows.UI.Core.CoreWindow` — MESURÉ le 2026-10-05.** Défaut signalé :
///   « au retour dans la session, je ne vois pas l'app mais j'ai l'horloge qui
///   correspond à une application en plein écran ». Sonde à l'appui, la fenêtre
///   mémorisée était celle de **`TextInputHost.exe`** (« Expérience d'entrée
///   Windows », l'hôte du clavier tactile) : classe `Windows.UI.Core.CoreWindow`,
///   rectangle `(0, 0) 2560x1440` — le moniteur ENTIER —, `exstyle` 0x200000
///   (`WS_EX_NOREDIRECTIONBITMAP`). **C'est l'écran de verrouillage qui le réveille**,
///   d'où le « au retour dans la session ».
///
/// ⚠️ **UNE VRAIE APPLICATION UWP N'EST PAS VISÉE, ET C'EST CE QUI REND L'EXCLUSION
/// SÛRE** : sa fenêtre de PREMIER NIVEAU est une `ApplicationFrameWindow`, et sa
/// CoreWindow est ENFANT. Un `Windows.UI.Core.CoreWindow` **de premier niveau** est
/// une surface du shell — le clavier tactile, le menu Démarrer, l'écran de
/// verrouillage. **C'est le point à revisiter** si un jeu UWP plein écran cessait un
/// jour de masquer le dock.
fn est_une_surface_du_shell(hwnd: HWND) -> bool {
    matches!(classe_de(hwnd).as_str(), "Progman" | "WorkerW" | "Windows.UI.Core.CoreWindow")
}

/// **Le nom de classe d'une fenêtre, pour le JOURNAL** — un `HWND` ne se lit pas, et
/// « quelle fenêtre nous a fait masquer » est exactement la question qui a coûté une
/// sonde entière le 2026-10-05 (voir `est_une_surface_du_shell`). `"?"` quand la
/// classe ne se lit pas — jamais de panique.
fn classe_de(hwnd: HWND) -> String {
    let mut classe = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut classe) };
    if n <= 0 {
        return "?".to_string();
    }
    String::from_utf16_lossy(&classe[..n as usize])
}

/// Renvoie le HWND au premier plan si celui-ci couvre EXACTEMENT le moniteur
/// sur lequel il se trouve, sinon `None` — `sync_fullscreen_visibility` le
/// mémorise pour pouvoir vérifier PLUS TARD, indépendamment du focus
/// courant, si CETTE fenêtre précise est encore active (voir
/// `window_is_still_fullscreen`).
fn foreground_fullscreen_window(exclude: HWND) -> Option<HWND> {
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() || foreground == exclude {
            return None;
        }
        // **AUCUNE fenêtre de NOTRE processus n'est « une application en plein
        // écran »** — DÉFAUT RÉEL SIGNALÉ : « l'interface de l'app est passée
        // en mode plein écran et ne rebascule pas en mode normal ».
        //
        // `exclude` ne couvrait QUE la fenêtre du dock. Nos autres fenêtres
        // (panneau d'aperçu, popups, horloge plein écran) sont topmost et
        // certaines couvrent l'écran : dès que l'une d'elles passait au premier
        // plan, elle satisfaisait l'heuristique géométrique juste en dessous, le
        // dock se croyait en plein écran et masquait ses barres. Pire, il
        // MÉMORISE le HWND fautif (voir `window_is_still_fullscreen`) : il ne
        // les réaffichait donc qu'une fois cette fenêtre fermée — d'où
        // l'impression de ne jamais rebasculer.
        //
        // Comparer le PROCESSUS plutôt qu'un HWND ferme la classe entière : il
        // n'y a plus de fenêtre à ajouter à la liste au fur et à mesure que
        // l'interface en gagne.
        let mut pid = 0u32;
        GetWindowThreadProcessId(foreground, Some(&mut pid));
        // ⭐ SAUF LE LECTEUR VIDÉO ET LA VISIONNEUSE EN PLEIN ÉCRAN — un vrai plein écran,
        // qui doit masquer le dock comme une autre (voir
        // `win32::lecteur_video::EN_PLEIN_ECRAN`).
        if pid == std::process::id() {
            let plein = crate::win32::lecteur_video::est_en_plein_ecran(foreground)
                || crate::win32::cef_visionneuse::est_en_plein_ecran(foreground);
            return plein.then_some(foreground);
        }
        // Exclut les SURFACES DU SHELL — voir `est_une_surface_du_shell`, qui porte
        // les deux cas mesurés et le pourquoi.
        if est_une_surface_du_shell(foreground) {
            return None;
        }

        if window_is_still_fullscreen(foreground) {
            Some(foreground)
        } else {
            None
        }
    }
}

/// Moniteur (`HMONITOR`) sur lequel se trouve un `HWND` donné.
fn hwnd_monitor(hwnd: HWND) -> HMONITOR {
    unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }
}

/// Renvoie le HWND d'une application plein écran, MAIS UNIQUEMENT si elle se
/// trouve sur `monitor` — sert à `sync_fullscreen_visibility` (`Window` et
/// `SecondaryWindow`) pour masquer chaque barre INDÉPENDAMMENT selon SON
/// PROPRE écran (déjà l'intention documentée dans `app::run`, "chacune vit
/// sur son PROPRE écran"), plutôt qu'une seule application plein écran
/// n'importe où masquant TOUTES les barres à la fois.
///
/// BUG RÉEL SIGNALÉ : "je voudrais que l'interface ne disparaisse pas de
/// l'écran où il n'y a pas d'application en plein écran". `foreground_fullscreen_window`
/// seule ne suffisait pas : elle dit SEULEMENT si le premier plan est plein
/// écran, jamais SUR QUEL écran — `Window`/`SecondaryWindow::sync_fullscreen_visibility`
/// se masquaient donc toutes ensemble dès qu'UNE seule application plein
/// écran existait où que ce soit, contrairement au commentaire déjà présent
/// dans `app::run` qui décrivait (sans jamais l'appliquer) le comportement
/// par écran attendu.
fn fullscreen_window_on_monitor(exclude: HWND, monitor: HMONITOR) -> Option<HWND> {
    if let Some(hwnd) = foreground_fullscreen_window(exclude) {
        // Plein écran détecté, mais sur un AUTRE écran que `monitor` : sans
        // rapport avec la barre qui pose la question.
        return if hwnd_monitor(hwnd) == monitor { Some(hwnd) } else { None };
    }
    // `SHQueryUserNotificationState` (plein écran D3D exclusif/mode
    // présentation) ne fournit aucun HWND — le foreground actuel reste le
    // seul candidat plausible pour savoir SUR QUEL écran ce signal
    // s'applique, retenu seulement s'il se trouve bien sur `monitor`.
    let exclusive = matches!(
        unsafe { SHQueryUserNotificationState() },
        Ok(QUNS_RUNNING_D3D_FULL_SCREEN) | Ok(QUNS_PRESENTATION_MODE)
    );
    if !exclusive {
        return None;
    }
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_invalid() || foreground == exclude || hwnd_monitor(foreground) != monitor {
        return None;
    }
    // Même exclusion par PROCESSUS que `foreground_fullscreen_window` : cette
    // branche de secours (signal D3D du système) rendait elle aussi une de nos
    // propres fenêtres quand elle avait le focus.
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(foreground, Some(&mut pid)) };
    if pid == std::process::id() {
        return None;
    }
    Some(foreground)
}

/// Vérifie qu'un HWND donné existe TOUJOURS, est TOUJOURS visible/non
/// réduit, et couvre TOUJOURS exactement le moniteur sur lequel il se trouve
/// — même heuristique géométrique que `foreground_window_is_fullscreen`
/// (voir sa doc), mais SANS regarder le focus courant : sert à vérifier
/// qu'une application plein écran DÉJÀ détectée est encore active, quand
/// bien même elle aurait depuis perdu le focus.
///
/// BUG RÉEL SIGNALÉ : "quand une fenêtre passe en plein écran l'interface
/// disparaît, quand je clique sur le second écran elle réapparaît mais
/// couvre alors le contenu qui est en plein écran alors qu'elle devrait
/// rester derrière". `sync_fullscreen_visibility` ne redécidait qu'à partir
/// du focus COURANT (`GetForegroundWindow`) : cliquer n'importe où sur un
/// autre écran (le bureau, une autre appli, une de nos propres fenêtres...)
/// change le foreground SANS que l'appli plein écran ait pour autant fermé
/// ou perdu sa taille — la lecture concluait alors à tort "plus de plein
/// écran" et réaffichait nos barres `WS_EX_TOPMOST`, au-dessus du contenu
/// plein écran toujours ouvert juste en dessous. En mémorisant le HWND
/// détecté et en revérifiant CE HWND précis (plutôt que le focus courant)
/// tant qu'on est masqué, on ne réaffiche plus qu'une fois l'appli plein
/// écran réellement fermée ou redimensionnée.
fn window_is_still_fullscreen(hwnd: HWND) -> bool {
    // ⛔ **LA SURFACE DU SHELL EST ÉCARTÉE ICI AUSSI, ET CE N'EST PAS REDONDANT** :
    // c'est CE test que `sync_fullscreen_visibility` rejoue à chaque frame sur le
    // HWND MÉMORISÉ. Sans cette ligne, un HWND déjà mémorisé avant le correctif
    // resterait collé jusqu'au redémarrage — le défaut qu'on vient de mesurer.
    if est_une_surface_du_shell(hwnd) {
        return false;
    }
    unsafe {
        if hwnd.is_invalid()
            || !IsWindow(Some(hwnd)).as_bool()
            || !IsWindowVisible(hwnd).as_bool()
            || IsIconic(hwnd).as_bool()
        {
            return false;
        }

        let mut window_rect = RECT::default();
        if GetWindowRect(hwnd, &mut window_rect).is_err() {
            return false;
        }

        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return false;
        }

        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        !agrandie_avec_son_titre(IsZoomed(hwnd).as_bool(), style) && couvre_l_ecran(window_rect, info.rcMonitor)
    }
}

/// Une fenêtre agrandie suit la zone de travail : sur un écran SANS barre, son
/// cadre déborde de quelques pixels — ce n'est pas un plein écran.
///
/// ⛔ **MAIS UN NAVIGATEUR EN PLEIN ÉCRAN RESTE « AGRANDI »** — DÉFAUT SIGNALÉ
/// (2026-10-18) : « quand je mets Twitch en plein écran, l'interface de l'app
/// reste ». MESURÉ sur Brave (`Chrome_WidgetWin_1`) : en plein écran, rect =
/// l'écran exact, `IsZoomed` VRAI, style `0x170B0000` — `WS_MAXIMIZE` gardé,
/// `WS_CAPTION` retiré. Exclure toute fenêtre agrandie le rendait invisible : ni
/// replacé dans la zone, ni dock masqué. C'est la barre de titre qui distingue
/// les deux.
fn agrandie_avec_son_titre(agrandie: bool, style: u32) -> bool {
    agrandie && style & WS_CAPTION.0 == WS_CAPTION.0
}

/// ⭐ **LA FENÊTRE COUVRE-T-ELLE L'ÉCRAN ?** — elle le CONTIENT, pas seulement
/// l'égale. DÉFAUT SIGNALÉ (2026-10-16) : « ça ne fonctionne pas pour Project
/// Zomboid même en mode fenêtré ». Fenêtré à la résolution de l'écran, un jeu
/// garde sa barre de titre et son cadre : sa fenêtre DÉBORDE de l'écran, et
/// l'égalité exacte ne la voyait pas — ni pour masquer le dock, ni pour la
/// replacer (voir `win32::plein_ecran_zone`).
pub(crate) fn couvre_l_ecran(fenetre: RECT, ecran: RECT) -> bool {
    fenetre.left <= ecran.left && fenetre.top <= ecran.top && fenetre.right >= ecran.right && fenetre.bottom >= ecran.bottom
}

#[cfg(test)]
mod couvre_tests {
    use super::*;

    #[test]
    fn une_fenetre_qui_deborde_de_l_ecran_le_couvre() {
        let ecran = RECT { left: -1920, top: 484, right: 0, bottom: 1564 };
        assert!(couvre_l_ecran(ecran, ecran), "sans bordure : égale");
        let fenetree = RECT { left: ecran.left - 8, top: ecran.top - 31, right: ecran.right + 8, bottom: ecran.bottom + 8 };
        assert!(couvre_l_ecran(fenetree, ecran), "fenêtrée : barre de titre et cadre dehors");
        let zone = RECT { left: ecran.left, top: ecran.top + 31, right: ecran.right, bottom: ecran.bottom };
        assert!(!couvre_l_ecran(zone, ecran), "dans la zone : non");
        let decalee = RECT { left: ecran.left + 1, ..ecran };
        assert!(!couvre_l_ecran(decalee, ecran), "un pixel de moins : non");
    }

    #[test]
    fn un_navigateur_en_plein_ecran_reste_agrandi_mais_sans_titre() {
        assert!(agrandie_avec_son_titre(true, 0x17CF_0000), "Brave agrandi : barre de titre");
        assert!(!agrandie_avec_son_titre(true, 0x170B_0000), "Brave en plein écran : agrandi, sans titre");
        assert!(!agrandie_avec_son_titre(false, 0x14CF_0000), "pas agrandie");
    }
}

/// Crée un HWND invisible (`WS_POPUP`, sans taille) dédié à porter UN
/// enregistrement AppBar (voir `win32::appbar`) — jamais affiché, jamais
/// utilisé pour autre chose. Partagé par `create_dock` (création initiale)
/// et `Window::reregister_appbars` (recréation après un redémarrage
/// d'explorer.exe, voir sa doc).
/// Procédure de fenêtre des HWND assistants d'AppBar (voir
/// `create_appbar_helper`) — un simple relais vers `DefWindowProcW`
/// (`DefWindowProcW` lui-même n'est pas un pointeur de fonction `extern
/// "system"` valide côté `windows-rs`, il faut ce mince relais). AUCUN
/// traitement spécial de `WM_DESTROY` ici, contrairement à `wndproc` (celui
/// de la fenêtre principale, qui appelle `PostQuitMessage`) — voir la doc
/// de `create_appbar_helper`.
unsafe extern "system" fn appbar_helper_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn create_appbar_helper(name: PCWSTR) -> Result<HWND> {
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None)?.into();
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("NeuraDeskAppBarHelperWindow"),
            name,
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )
    }
}

/// Coin supérieur gauche d'une fenêtre en coordonnées écran absolues, pour
/// n'importe quel `HWND` — partagé entre `Window::screen_origin` (barre
/// primaire) et `SecondaryWindow::screen_origin` (barres secondaires), afin
/// que `app::capture_bar_backdrops` (capture+flou du bureau sous les
/// barres) puisse s'appliquer identiquement aux deux : demande explicite
/// "même possibilités de réglages... flou" pour les écrans secondaires.
pub fn hwnd_screen_origin(hwnd: HWND) -> Result<(i32, i32)> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect)? };
    Ok((rect.left, rect.top))
}

impl Window {
    /// Crée et affiche la fenêtre du dock : une forme en "⊓" (barre
    /// supérieure + barres latérales gauche et droite) couvrant les bords de
    /// l'écran principal, sans recouvrir la barre des tâches — sauf quand
    /// celle-ci est en auto-masquage, auquel cas elle ne réserve plus rien et la
    /// fenêtre occupe l'écran entier (voir `primary_dock_bounds`).
    pub fn create_dock() -> Result<Self> {
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None)?.into();
            let class_name = w!("NeuraDeskDockWindow");

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
                lpfnWndProc: Some(wndproc),
                hInstance: instance,
                lpszClassName: class_name,
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                ..Default::default()
            };
            let atom = RegisterClassExW(&wc);
            if atom == 0 {
                return Err(Error::from_thread());
            }

            // Classe DÉDIÉE aux HWND assistants d'AppBar (voir
            // `create_appbar_helper`) — `lpfnWndProc: DefWindowProcW`, PAS
            // `wndproc` (celui de la fenêtre principale) : BUG RÉEL CONSTATÉ
            // EN PRATIQUE — les trois assistants partageaient jusqu'ici la
            // MÊME classe/procédure de fenêtre que la fenêtre principale,
            // dont le `WM_DESTROY` appelle SANS CONDITION `PostQuitMessage`
            // (voir `wndproc`, pensé pour la fermeture de LA fenêtre
            // principale) — détruire un simple assistant (voir
            // `Window::reregister_appbars`, qui en a besoin pour écarter un
            // état mis en cache par `shell32.dll`) suffisait donc à fermer
            // TOUTE l'application. `DefWindowProcW` ne fait rien de spécial
            // sur `WM_DESTROY`, exactement ce qu'il faut pour des fenêtres
            // qui ne servent jamais qu'à porter un enregistrement AppBar.
            let helper_wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(appbar_helper_wndproc),
                hInstance: instance,
                lpszClassName: w!("NeuraDeskAppBarHelperWindow"),
                ..Default::default()
            };
            if RegisterClassExW(&helper_wc) == 0 {
                return Err(Error::from_thread());
            }

            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let screen_h = GetSystemMetrics(SM_CYSCREEN);
            let (edge, taskbar_thickness) = detect_taskbar();

            // La fenêtre est ancrée au coin de la zone de travail (l'écran
            // moins la barre des tâches, quel que soit son bord) le plus
            // proche du coin supérieur gauche : sa barre du haut occupe
            // toute la largeur disponible, sa barre de gauche toute la
            // hauteur disponible. Elle ne recouvre donc jamais la barre des
            // tâches, quelle que soit sa position (voir la doc de module
            // pour la tentative d'extension jusqu'au bord bas, abandonnée).
            // `taskbar_thickness` (la VRAIE barre des tâches Windows) sert
            // UNIQUEMENT ici, à positionner la fenêtre — DISTINCT de
            // `dock_thickness` juste en dessous (l'épaisseur des barres DE
            // NEURADESK lui-même, réglable indépendamment).
            //
            // C'est `primary_dock_bounds` — et non `dock_window_bounds` — qui
            // décide : une barre des tâches en auto-masquage ne réserve plus
            // rien, donc la fenêtre va jusqu'au bord dans ce cas (voir sa doc,
            // et notamment pourquoi seul le bord bas est concerné).
            // `taskbar_thickness`, lui, reste l'épaisseur PHYSIQUE : c'est ce
            // que `dock_thickness` ci-dessous attend.
            let (origin_x, origin_y, width, height) = primary_dock_bounds(
                screen_w,
                screen_h,
                edge,
                taskbar_thickness,
                taskbar_autohide::is_enabled(),
            );

            // Épaisseur de DÉPART des trois barres NeuraDesk elles-mêmes
            // (voir la doc du champ `Window::top_thickness`) — calquée sur
            // la vraie barre des tâches par défaut (comportement d'origine),
            // mais les trois curseurs du panneau de réglages la font ensuite
            // diverger indépendamment via `set_dock_thicknesses`.
            let dock_thickness = taskbar_thickness;
            // ⚠️ **DÉCLARÉ ICI, ET PAS PLUS BAS** : la région du dock (juste après
            // la création de la fenêtre) a besoin de la 4ᵉ épaisseur, donc elle
            // doit exister avant `set_dock_region` — alors que les AppBars, eux,
            // ne sont enregistrés que plus loin.
            //
            // La barre du BAS est la SEULE dont l'épaisseur par défaut ne vient
            // pas de la barre des tâches Windows : elle n'a rien à imiter, donc
            // c'est `ui::dock::DEFAULT_BOTTOM_THICKNESS` (voir sa doc).
            let bottom_thickness = crate::ui::dock::DEFAULT_BOTTOM_THICKNESS.round() as i32;

            let style = WS_POPUP;

            let hwnd = CreateWindowExW(
                DOCK_EX_STYLE,
                class_name,
                w!("NeuraDesk"),
                style,
                origin_x,
                origin_y,
                width,
                height,
                None,
                None,
                Some(instance),
                None,
            )?;

            // Transparence réelle composée par DWM (voir doc en tête de fichier).
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

            set_dock_region(hwnd, width, height, dock_thickness, dock_thickness, dock_thickness, bottom_thickness, None, (dock_thickness, dock_thickness, dock_thickness), 0);

            // Exclut la fenêtre du dock de toute capture d'écran (la
            // nôtre y compris, voir `gfx::backdrop`) sans la masquer pour
            // autant : elle reste normalement affichée à l'utilisateur,
            // seules les captures (BitBlt, PrintWindow, Duplication API...)
            // la traitent comme transparente. Disponible depuis Windows 10
            // 2004 ; ignoré silencieusement sur les versions antérieures
            // (le flou d'arrière-plan capturerait alors aussi le dock).
            // Pose l'affinité d'affichage ET l'exclusion de la Desktop Duplication
        // ensemble — voir `capture_visibility::apply_to_window`, doc.
        crate::gfx::capture_visibility::apply_to_window(hwnd);
        // Icône de FENÊTRE (barre des tâches + Alt-Tab) — voir `apply_exe_icon`,
        // et le défaut réel qu'elle corrige.
        apply_exe_icon(hwnd);

            // S'abonne aux changements du presse-papiers (voir
            // `WM_CLIPBOARDUPDATE` plus bas, `win32::clipboard`) — méthode
            // moderne (depuis Vista), remplace l'ancienne chaîne de
            // visualisateurs (`SetClipboardViewer`) dépréciée et plus
            // fragile (un maillon qui plante casse toute la chaîne).
            let _ = AddClipboardFormatListener(hwnd);

            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);

            // Réserve l'espace des trois bandes auprès du shell (fenêtres
            // maximisées + icônes du bureau), voir `win32::appbar`. La
            // barre du haut est toujours enregistrée côté ABE_TOP, la barre
            // de gauche côté ABE_LEFT et la barre de droite côté ABE_RIGHT :
            // c'est la fenêtre elle-même (via `origin_x`/`origin_y`/
            // `width`) qui s'est déjà décalée pour éviter la vraie barre
            // des tâches, quel que soit son bord. Un AppBar par HWND, donc
            // un HWND assistant (invisible) par bande — y COMPRIS pour la
            // barre du haut (`top_helper_hwnd`, PAS `hwnd`, la fenêtre
            // principale/de rendu D3D11) : voir `create_appbar_helper` et
            // `reregister_appbars`, doc détaillée, pour pourquoi ce choix
            // matters au-delà de la simple limite "un AppBar par HWND".
            let top_helper_hwnd = create_appbar_helper(w!("NeuraDeskAppBarHelperTop"))?;
            let left_helper_hwnd = create_appbar_helper(w!("NeuraDeskAppBarHelperLeft"))?;
            let right_helper_hwnd = create_appbar_helper(w!("NeuraDeskAppBarHelperRight"))?;
            // Quatrième bande, celle du BAS (`ABE_BOTTOM`) — **écran principal
            // seulement** : rien n'enregistre d'AppBar pour un écran secondaire
            // (voir `SecondaryWindow`), et sa géométrie y vaut 0 (voir
            // `BarThicknesses::derive_for_screen`).
            let bottom_helper_hwnd = create_appbar_helper(w!("NeuraDeskAppBarHelperBottom"))?;

            let top_appbar = AppBar::register(
                top_helper_hwnd,
                ABE_TOP,
                RECT {
                    left: origin_x,
                    top: origin_y,
                    right: origin_x + width,
                    bottom: origin_y + dock_thickness,
                },
            );
            let left_appbar = AppBar::register(
                left_helper_hwnd,
                ABE_LEFT,
                RECT {
                    left: origin_x,
                    top: origin_y + dock_thickness,
                    right: origin_x + dock_thickness,
                    bottom: origin_y + height,
                },
            );
            let right_appbar = AppBar::register(
                right_helper_hwnd,
                ABE_RIGHT,
                RECT {
                    left: origin_x + width - dock_thickness,
                    top: origin_y + dock_thickness,
                    right: origin_x + width,
                    bottom: origin_y + height,
                },
            );
            // La barre du bas réserve le MILIEU, entre les deux colonnes (voir
            // `poser_reservations`, qui porte la règle et le pourquoi).
            let bottom_appbar = AppBar::register(
                bottom_helper_hwnd,
                ABE_BOTTOM,
                RECT {
                    left: origin_x + dock_thickness,
                    top: origin_y + height - bottom_thickness,
                    right: origin_x + width - dock_thickness,
                    bottom: origin_y + height,
                },
            );

            Ok(Self {
                hwnd,
                width: width as u32,
                height: height as u32,
                top_thickness: dock_thickness as u32,
                left_width: dock_thickness as u32,
                right_width: dock_thickness as u32,
                bottom_thickness: bottom_thickness as u32,
                top_appbar,
                left_appbar,
                right_appbar,
                bottom_appbar,
                top_helper_hwnd,
                left_helper_hwnd,
                right_helper_hwnd,
                bottom_helper_hwnd,
                hidden_for_fullscreen: false,
                fullscreen_hwnd: None,
                overflow: None,
                colonnes_bas: (dock_thickness as u32, dock_thickness as u32),
                haut_regle: dock_thickness as u32,
                rayon_conge: 0,
            })
        }
    }

    /// Coin supérieur gauche de la fenêtre en coordonnées écran absolues.
    /// Utilisé pour convertir les rectangles des barres (coordonnées
    /// fenêtre) en rectangles de capture d'écran (voir `gfx::backdrop`,
    /// appelé depuis `app::run`).
    pub fn screen_origin(&self) -> Result<(i32, i32)> {
        hwnd_screen_origin(self.hwnd)
    }

    /// Change l'épaisseur/largeur des trois barres NeuraDesk (curseurs
    /// "Épaisseur barre du haut"/"Largeur barre gauche"/"Largeur barre
    /// droite" du panneau de réglages, demande explicite) — re-découpe la
    /// région visible de la fenêtre (voir `set_dock_region`) ET met à jour
    /// les trois réservations AppBar (voir `win32::appbar::AppBar::update`)
    /// EN PLACE, sans recréer la fenêtre ni ses AppBars : la fenêtre elle-
    /// même reste dimensionnée pour éviter la VRAIE barre des tâches (voir
    /// `create_dock`), inchangée par ce réglage — seule la région visible
    /// À L'INTÉRIEUR change. Ne touche PAS `self.width`/`self.height` (la
    /// taille du RECTANGLE de la fenêtre), seulement la géométrie découpée
    /// dedans.
    pub fn set_dock_thicknesses(&mut self, top: u32, left: u32, right: u32, bottom: u32) {
        let (width, height) = (self.width as i32, self.height as i32);
        unsafe {
            set_dock_region(
                self.hwnd,
                width,
                height,
                top as i32,
                left as i32,
                right as i32,
                bottom as i32,
                self.overflow,
                (self.colonnes_bas.0 as i32, self.colonnes_bas.1 as i32, self.haut_regle as i32),
                self.rayon_conge,
            );
        }
        self.poser_reservations(top, left, right, bottom);
        self.top_thickness = top;
        self.left_width = left;
        self.right_width = right;
        self.bottom_thickness = bottom;
        // Voir `nudge_maximized_windows`, doc détaillée — sans cet appel,
        // une fenêtre déjà maximisée sur l'écran principal AVANT ce
        // changement de curseur resterait recouverte par le dock élargi
        // jusqu'à son prochain agrandissement manuel.
        nudge_maximized_windows_retrying(unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST) });
    }

    /// Met le dock dans l'état demandé le temps d'une capture d'écran — voir
    /// `EtatBarresPourCapture`, qui porte le tableau des trois états et ce qui
    /// les distingue. DEMANDE EXPLICITE pour le clic GAUCHE : « l'app se cache et
    /// l'espace du bureau redevient standard avant de pouvoir prendre la
    /// capture ». Les épaisseurs mémorisées (`top_thickness`...) ne bougent PAS :
    /// ce sont elles que la reprise repose.
    pub fn suspendre_pour_capture(&mut self, etat: EtatBarresPourCapture) {
        match etat {
            EtatBarresPourCapture::Repos => {
                self.poser_reservations(self.top_thickness, self.left_width, self.right_width, self.bottom_thickness);
                // Un plein écran survenu entre-temps garde la main : c'est
                // `sync_fullscreen_visibility` qui réaffichera.
                if !self.hidden_for_fullscreen {
                    unsafe {
                        let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                    }
                }
            }
            EtatBarresPourCapture::Masquees => {
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                }
                self.poser_reservations(0, 0, 0, 0);
            }
            // ⛔ **AUCUNE RÉSERVATION N'EST TOUCHÉE — c'est tout l'objet de cet
            // état**, et c'est ce qui garde la fenêtre agrandie DANS le cadre des
            // barres au lieu de la laisser déborder dessous.
            //
            // ⛔ **ET ON SORT AVANT LE `nudge`, POUR LA MÊME RAISON** : rien n'a
            // changé pour le shell, donc redimensionner les fenêtres agrandies les
            // « bousculerait pour rien » — la règle est déjà écrite dans la doc de
            // `PhaseCapture::Demandee`.
            EtatBarresPourCapture::MasqueesSansLiberer => {
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                }
                return;
            }
        }
        nudge_maximized_windows_retrying(unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST) });
    }

    /// **L'écran de ce dock, et la zone de travail que le shell publie
    /// MAINTENANT pour lui** (`MONITORINFO::rcWork`) — donc la zone **réservée**
    /// tant qu'une réservation est posée.
    ///
    /// ⭐ **C'EST LE RELEVÉ DE RÉFÉRENCE DE LA CAPTURE** : pris juste AVANT
    /// `suspendre_pour_capture(true)`, il dit ce que le shell devra rendre (voir
    /// `ecrans_ont_rendu_la_place`).
    ///
    /// ⚠️ **NE PAS RECONSTRUIRE CETTE GÉOMÉTRIE À LA MAIN** (« l'écran moins les
    /// épaisseurs moins la barre des tâches ») : ce serait un SECOND
    /// `poser_reservations`, et les deux divergeraient au premier changement. Le
    /// shell est la seule autorité sur ce qu'il réserve.
    ///
    /// `None` si le shell ne répond pas : l'appelant n'a alors aucune référence
    /// fiable, et renoncer à attendre vaut mieux que comparer à un rectangle
    /// bidon — qui rendrait la condition vraie au tout premier relevé.
    pub fn ecran_et_zone_de_travail(&self) -> Option<(HMONITOR, RECT)> {
        let ecran = unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        unsafe { GetMonitorInfoW(ecran, &mut info) }.as_bool().then_some((ecran, info.rcWork))
    }

    /// **Ce dock réserve-t-il réellement de l'espace ?** — autrement dit : sa
    /// suspension va-t-elle DÉPLACER une zone de travail ?
    ///
    /// ⚠️ **LIT LES ÉPAISSEURS RÉSERVÉES, PAS LES CONFIGURÉES.** Ces quatre
    /// champs sont ceux que `set_dock_thicknesses` a posés, donc ceux de
    /// `reserved_primary_thicknesses` — qui met à **ZÉRO** toute barre en
    /// masquage automatique. Lire les épaisseurs configurées répondrait « oui »
    /// pour une barre qui ne réserve rien, et chaque capture attendrait alors
    /// l'échéance dure : le piège d'un garde-fou appliqué à la mauvaise
    /// grandeur, à un nouvel endroit.
    pub fn reserve_de_l_espace(&self) -> bool {
        self.top_thickness > 0 || self.left_width > 0 || self.right_width > 0 || self.bottom_thickness > 0
    }

    /// Pose les quatre réservations AppBar aux épaisseurs données.
    fn poser_reservations(&self, top: u32, left: u32, right: u32, bottom: u32) {
        let (origin_x, origin_y) = self.screen_origin().unwrap_or((0, 0));
        let (width, height) = (self.width as i32, self.height as i32);
        self.top_appbar.update(RECT {
            left: origin_x,
            top: origin_y,
            right: origin_x + width,
            bottom: origin_y + top as i32,
        });
        self.left_appbar.update(RECT {
            left: origin_x,
            top: origin_y + top as i32,
            right: origin_x + left as i32,
            bottom: origin_y + height,
        });
        self.right_appbar.update(RECT {
            left: origin_x + width - right as i32,
            top: origin_y + top as i32,
            right: origin_x + width,
            bottom: origin_y + height,
        });
        // ⚠️ **LA BARRE DU BAS RÉSERVE LE MILIEU**, pas toute la largeur : les
        // colonnes latérales descendent jusqu'en bas (voir `dock_region_rects` et
        // `ui::dock::bar_rects`, qui portent la décision).
        self.bottom_appbar.update(RECT {
            left: origin_x + left as i32,
            top: origin_y + height - bottom as i32,
            right: origin_x + width - right as i32,
            bottom: origin_y + height,
        });
    }

    /// Change UNIQUEMENT la région visible/découpée de la fenêtre (voir
    /// `set_dock_region`) — SANS toucher aux réservations AppBar ni appeler
    /// `nudge_maximized_windows_retrying`, contrairement à
    /// `set_dock_thicknesses`. Appelée à CHAQUE frame où l'état de
    /// révélation ponctuel change en mode masquage automatique (voir
    /// `app::run`, `primary_bars_revealed`) : BUG RÉEL SIGNALÉ (voir
    /// `app::reserved_primary_thicknesses`, doc) — faire varier la
    /// réservation AppBar à cette fréquence causait un redimensionnement
    /// visible des fenêtres déjà maximisées à chaque survol/départ de
    /// souris, au lieu de simplement les recouvrir. Ne met PAS à jour
    /// `self.top_thickness`/`left_width`/`right_width` (qui restent la
    /// valeur RÉSERVÉE, voir `reregister_appbars`/
    /// `resync_to_primary_monitor`), seulement l'apparence.
    pub fn set_visible_region(&mut self, top: u32, left: u32, right: u32, bottom: u32) {
        let (width, height) = (self.width as i32, self.height as i32);
        unsafe {
            // Le débordement fait partie des entrées : cette fonction est
            // appelée à CHAQUE frame en masquage automatique, elle écraserait
            // donc la boîte posée par `set_overflow` si elle l'ignorait.
            set_dock_region(
                self.hwnd,
                width,
                height,
                top as i32,
                left as i32,
                right as i32,
                bottom as i32,
                self.overflow,
                (self.colonnes_bas.0 as i32, self.colonnes_bas.1 as i32, self.haut_regle as i32),
                self.rayon_conge,
            );
        }
    }

    /// Enregistre la boîte de DÉBORDEMENT de l'icône magnifiée, composée à la
    /// région du dock (voir `set_dock_region` et `dock_region_rects`) —
    /// (`gauche, haut, droite, bas`) en pixels fenêtre, ou `None`.
    ///
    /// DEMANDE EXPLICITE : « qu'elles puissent déborder de la barre qui les
    /// contient ». C'est la seule façon d'y arriver dans cette fenêtre, la
    /// région étant ce qui rogne tout ce qui sort du « ⊓ » (voir
    /// `set_dock_region`, doc, pour la contrepartie sur les clics).
    ///
    /// **N'applique PAS la région lui-même**, et c'est délibéré : les deux
    /// fonctions qui la posent (`set_dock_thicknesses`, `set_visible_region`)
    /// lisent `self.overflow`, de sorte qu'AUCUNE application de région ne peut
    /// oublier le débordement. Les faire toutes deux rappeler `SetWindowRgn`
    /// ici doublerait la recomposition de forme à chaque survol, alors que la
    /// région couvre la quasi-totalité de la zone de travail et que la fenêtre
    /// a un cadre DWM étendu (voir `create_dock`) — c'est précisément ce que
    /// l'appelant évite en passant l'échelle VISÉE plutôt que l'échelle animée
    /// (voir `app::run`). L'appelant doit donc veiller à ce qu'une application
    /// de région suive : `app::run` l'inclut dans la condition qui déclenche
    /// `set_visible_region`.
    pub fn set_overflow(&mut self, overflow: Option<[i32; 4]>) {
        self.overflow = overflow;
    }

    /// Enregistre les largeurs RÉGLÉES des colonnes gauche et droite, auxquelles
    /// la bande du bas de la région s'arrête (voir `dock_region_rects_entre`) —
    /// même quand une colonne est masquée et que la région la retire. Comme
    /// `set_overflow`, **n'applique pas la région** : `app::run` l'appelle à
    /// chaque frame AVANT le bloc qui la pose, et un changement de ces largeurs
    /// passe toujours par `set_dock_thicknesses`, qui la repose.
    /// Le rayon des congés de la zone libre (voir `ui::dock::conges`). Comme
    /// `set_overflow`, **n'applique pas la région** : `app::run` compare et
    /// déclenche `set_visible_region` quand il change.
    pub fn set_rayon_conge(&mut self, rayon: i32) {
        self.rayon_conge = rayon;
    }

    pub fn set_bottom_band_columns(&mut self, left: u32, right: u32) {
        self.colonnes_bas = (left, right);
    }

    /// L'épaisseur RÉGLÉE de la barre du haut, sous laquelle les colonnes
    /// latérales de la région commencent (voir `dock_region_rects_entre`). Même
    /// contrat que `set_bottom_band_columns` : n'applique pas la région.
    pub fn set_top_band_reglee(&mut self, top: u32) {
        self.haut_regle = top;
    }

    /// Repasse la fenêtre du dock au PREMIER RANG des fenêtres « toujours
    /// au-dessus », sans la déplacer, la redimensionner ni l'activer.
    ///
    /// **Conditionnée par `app::run` au débordement** — c'est-à-dire à la
    /// présence d'un élément AGRANDI — DEMANDE EXPLICITE : « je voudrais que les
    /// icônes agrandies soient toujours visibles, que rien ne puisse les
    /// recouvrir ». Entre fenêtres toutes « topmost » (le dock, les popups CEF,
    /// l'infobulle, le panneau d'aperçus), l'ordre réel dépend de QUI a été
    /// positionné en dernier : sans ce rappel, une icône agrandie passait sous
    /// la dernière fenêtre reposée — le Dash, les Réglages, l'aperçu.
    ///
    /// **L'appelant la rappelle à chaque frame** tant qu'un élément est agrandi,
    /// et non une seule fois à l'apparition du débordement : le panneau
    /// d'aperçus se repose à chaque frame (voir `window_previews::
    /// layout_and_render`), donc un rappel unique serait aussitôt annulé.
    ///
    /// Contrepartie assumée, à connaître : pendant un survol, les éléments
    /// agrandis passent devant les AUTRES fenêtres de NeuraDesk — y compris le
    /// menu contextuel, le Dash et les Réglages, s'ils se trouvent sous eux.
    pub fn raise_to_top(&self) {
        // ⛔ **NE PAS SE REMONTER QUAND RIEN NE PASSE DEVANT** — DÉFAUT RÉEL
        // SIGNALÉ (2026-11-13) : « quand je survole une icône les coins arrondis
        // au bord de l'écran clignotent ». MESURÉ à la sonde d'ordre Z : pendant
        // un survol, `app::run` appelle cette fonction toutes les 250 ms (le
        // rappel de précaution de la magnification), et chaque `SetWindowPos`
        // sur le dock réinsère AUSSI ses fenêtres POSSÉDÉES — les coins noirs
        // des écrans — que DWM recompose alors, d'où le clignotement. Rien ne
        // passait devant le dock dans ce cas : les seules fenêtres au-dessus de
        // lui sont ses propres coins (voir `un_voisin_visible_devant`, qui les
        // ignore) et le dock n'avait donc rien à faire.
        if un_voisin_visible_devant(self.hwnd) {
            unsafe {
                let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
        // Les coins noirs des écrans restent AU-DESSUS du dock : ils sont posés
        // sur ses angles extérieurs (voir `win32::coins_ecran`).
        //
        // ⛔ **LES SECONDAIRES D'ABORD, LES PRIMAIRES ENSUITE** — DÉFAUT RÉEL
        // SIGNALÉ (2026-11-13) : « quand je survole une icône les coins arrondis
        // au bord de l'écran clignotent ». MESURÉ : l'ordre inverse laissait les
        // deux coins des écrans secondaires repasser DEVANT ceux du primaire à
        // chaque remontée (4 fois par seconde pendant une magnification), et
        // `coins_ecran::synchroniser` (1 Hz) les remettait derrière — les coins
        // primaires changeaient de rang deux fois par seconde (`14..21` ↔
        // `16..23`), et DWM les recomposait à chaque inversion. L'ordre RELATIF
        // des deux familles n'a aucune importance (elles sont sur des écrans
        // différents) ; ce qui compte est qu'il soit TOUJOURS LE MÊME. C'est
        // `coins_ecran::cible_de_remontee` qui empêche la plupart de ces
        // remontées d'avoir lieu ; cet ordre est la seconde barrière, pour le
        // cas où une même fenêtre en recouvre un de chaque famille.
        crate::win32::coins_secondaires::remonter();
        crate::win32::coins_ecran::remonter();
        // L'ombre sous la barre du haut aussi : elle tombe sur le haut des barres
        // latérales (voir `win32::ombre_barre`).
        crate::win32::ombre_barre::remonter();
    }

    /// Recale la fenêtre du dock sur l'état ACTUEL de l'écran principal —
    /// appelée par `app::run` sur `Event::DisplaysChanged` (débattue, voir
    /// `app::DISPLAY_CHANGE_DEBOUNCE`), jamais depuis `wndproc` lui-même
    /// (voir la doc de `Event::DisplaysChanged` : `SetWindowPos` pendant le
    /// traitement d'un message a déjà causé une réentrance `RefCell` par le
    /// passé dans ce projet, voir les commentaires de `launcher`/
    /// `cef_popup`). Ré-exécute exactement le calcul de `create_dock`
    /// (`GetSystemMetrics` + `detect_taskbar` + auto-masquage, voir
    /// `primary_dock_bounds`, factorisée entre les deux) : l'écran principal a
    /// pu changer de résolution, ou la barre des tâches Windows changer de bord/
    /// épaisseur — ou basculer en auto-masquage, ce qui libère sa bande — pendant
    /// que l'application tournait (moniteur débranché/rebranché, changement
    /// d'affichage...). Ne touche PAS aux AppBars elles-mêmes ni à leurs HWND
    /// assistants (voir `reregister_appbars` pour ce cas différent, un état shell
    /// perdu) : `set_dock_thicknesses` suffit ici, la fenêtre garde ses mêmes
    /// AppBars, seule leur géométrie est réappliquée à la nouvelle origine/taille.
    ///
    /// **Ce que l'appelant DOIT faire ensuite** : `set_dock_thicknesses` (en fin
    /// de fonction) redécoupe la région aux épaisseurs RÉSERVÉES et remet
    /// `self.overflow` à zéro. Une barre NeuraDesk en auto-masquage et
    /// actuellement RÉVÉLÉE se retrouverait donc rognée à son épaisseur
    /// réservée — invisible — jusqu'à la prochaine bascule, et l'icône magnifiée
    /// perdrait son débordement. `app::run` doit donc forcer une réapplication de
    /// la région (voir `force_region_reapply`) : le démarrage le fait déjà
    /// (`set_visible_region` juste après `set_dock_thicknesses`), ce chemin-là
    /// l'avait oublié.
    pub fn resync_to_primary_monitor(&mut self) {
        let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        let (edge, taskbar_thickness) = detect_taskbar();
        let (origin_x, origin_y, width, height) = primary_dock_bounds(
            screen_w,
            screen_h,
            edge,
            taskbar_thickness,
            taskbar_autohide::is_enabled(),
        );

        unsafe {
            let _ = SetWindowPos(self.hwnd, None, origin_x, origin_y, width, height, SWP_NOZORDER | SWP_NOACTIVATE);
        }
        self.width = width as u32;
        self.height = height as u32;
        // La boîte de débordement est en coordonnées FENÊTRE, et la fenêtre
        // vient de changer d'origine et de taille : la boîte mémorisée est
        // périmée. On l'efface plutôt que de la composer — `app::run` la
        // recalcule à la prochaine frame, et l'effacer évite qu'elle survive si
        // le nouveau calcul tombait par hasard sur la même valeur, ce qui la
        // laisserait appliquée à une géométrie qui n'est plus la sienne.
        self.overflow = None;
        self.set_dock_thicknesses(self.top_thickness, self.left_width, self.right_width, self.bottom_thickness);
    }

    /// RE-enregistre les trois AppBars (voir `win32::appbar`) auprès du
    /// shell EN COURS, avec la géométrie ACTUELLE — nécessaire après un
    /// redémarrage d'`explorer.exe` (quelle qu'en soit la cause — c'est le
    /// cas que `win32::cef_settings::take_native_tint_restart_pending`
    /// surveille pour les redémarrages que NOUS déclenchons) : BUG RÉEL SIGNALÉ
    /// ("les fenêtres maximisées et les icônes du bureau ne s'adaptent pas
    /// pour occuper la zone centrale" quand ce mode s'active) — l'API
    /// AppBar (`ABM_NEW`) est un état tenu EN MÉMOIRE par le processus
    /// `explorer.exe` ; le tuer/le relancer efface cet état sans que notre
    /// `Window` en soit informée.
    ///
    /// Ré-enregistrer sur les MÊMES HWND que la création initiale — essayé
    /// d'abord — rapportait un succès Windows (`ABM_NEW`/`ABM_SETPOS`
    /// renvoient tous deux `1`) SANS jamais faire changer `SPI_GETWORKAREA`,
    /// même après 10 tentatives échelonnées sur 4 secondes (donc PAS
    /// seulement un problème de délai — le shell pas encore "prêt" — voir
    /// l'historique) : probablement un état mis en cache CÔTÉ CLIENT par
    /// `shell32.dll` (DANS NOTRE PROPRE processus, pas celui du shell)
    /// associant déjà ces HWND à un appbar "connu", empêchant le message
    /// d'atteindre réellement le nouveau shell. Fix : détruire les anciens
    /// HWND assistants et en créer de TOUT NEUFS À CHAQUE TENTATIVE (voir
    /// `create_appbar_helper`), pour écarter cette hypothèse — combiné à la
    /// vérification/nouvelle tentative via `SPI_GETWORKAREA` (voir
    /// `current_work_area`) au cas où un délai de stabilisation du shell
    /// s'ajoute malgré tout par-dessus.
    pub fn reregister_appbars(&mut self) {
        let (origin_x, origin_y) = self.screen_origin().unwrap_or((0, 0));
        let (width, height) = (self.width as i32, self.height as i32);
        let (top, left, right) = (self.top_thickness as i32, self.left_width as i32, self.right_width as i32);
        let bottom = self.bottom_thickness as i32;

        let expected_work_area = RECT {
            left: origin_x + left,
            top: origin_y + top,
            right: origin_x + width - right,
            // ⚠️ **LA BARRE DU BAS RÉTRÉCIT LA ZONE DE TRAVAIL PAR LE BAS** :
            // c'est elle qu'on vient vérifier après un redémarrage d'explorer,
            // donc l'oublier ici ferait conclure à tort que la réservation a
            // réussi (ou échoué) — et la boucle de réessai tournerait 10 fois
            // pour rien, ou s'arrêterait sur un état faux.
            bottom: origin_y + height - bottom,
        };
        const MAX_ATTEMPTS: u32 = 10;
        const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(400);
        for _ in 0..MAX_ATTEMPTS {
            unsafe {
                let _ = DestroyWindow(self.top_helper_hwnd);
                let _ = DestroyWindow(self.left_helper_hwnd);
                let _ = DestroyWindow(self.right_helper_hwnd);
                let _ = DestroyWindow(self.bottom_helper_hwnd);
            }
            let (Ok(top_helper_hwnd), Ok(left_helper_hwnd), Ok(right_helper_hwnd), Ok(bottom_helper_hwnd)) = (
                create_appbar_helper(w!("NeuraDeskAppBarHelperTop")),
                create_appbar_helper(w!("NeuraDeskAppBarHelperLeft")),
                create_appbar_helper(w!("NeuraDeskAppBarHelperRight")),
                create_appbar_helper(w!("NeuraDeskAppBarHelperBottom")),
            ) else {
                std::thread::sleep(RETRY_DELAY);
                continue;
            };
            self.top_helper_hwnd = top_helper_hwnd;
            self.left_helper_hwnd = left_helper_hwnd;
            self.right_helper_hwnd = right_helper_hwnd;
            self.bottom_helper_hwnd = bottom_helper_hwnd;

            self.top_appbar = AppBar::register(
                top_helper_hwnd,
                ABE_TOP,
                RECT { left: origin_x, top: origin_y, right: origin_x + width, bottom: origin_y + top },
            );
            self.left_appbar = AppBar::register(
                left_helper_hwnd,
                ABE_LEFT,
                RECT {
                    left: origin_x,
                    top: origin_y + top,
                    right: origin_x + left,
                    bottom: origin_y + height,
                },
            );
            self.right_appbar = AppBar::register(
                right_helper_hwnd,
                ABE_RIGHT,
                RECT {
                    left: origin_x + width - right,
                    top: origin_y + top,
                    right: origin_x + width,
                    bottom: origin_y + height,
                },
            );
            // Le bas réserve le MILIEU (voir `poser_reservations`).
            self.bottom_appbar = AppBar::register(
                bottom_helper_hwnd,
                ABE_BOTTOM,
                RECT {
                    left: origin_x + left,
                    top: origin_y + height - bottom,
                    right: origin_x + width - right,
                    bottom: origin_y + height,
                },
            );
            if current_work_area() == expected_work_area {
                nudge_maximized_windows_retrying(unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST) });
                break;
            }
            std::thread::sleep(RETRY_DELAY);
        }
    }

    /// Masque le dock quand une application PLEIN ÉCRAN (jeu, lecteur vidéo
    /// en plein écran...) est active, le réaffiche sinon — demande explicite
    /// : "quand je lance une vidéo en mode plein écran l'interface de l'app
    /// reste visible par dessus la vidéo". Nos trois barres sont
    /// `WS_EX_TOPMOST` (voir `create_dock`) : rien ne les empêchait de
    /// rester au-dessus d'une vidéo/un jeu en plein écran, contrairement à la
    /// VRAIE barre des tâches Windows, qui s'efface dans ce cas précis —
    /// jamais reproduit jusqu'ici faute de détecter cette situation.
    ///
    /// `SHQueryUserNotificationState` — API PUBLIQUE et DOCUMENTÉE (PAS une
    /// astuce de rétro-ingénierie) — sert normalement à décider si on peut
    /// déranger l'utilisateur avec une notification ; `QUNS_RUNNING_D3D_FULL_SCREEN`
    /// (jeu en plein écran exclusif Direct3D) et `QUNS_PRESENTATION_MODE`
    /// (mode présentation) couvrent une PARTIE du signal que la VRAIE barre
    /// des tâches Windows utilise pour s'auto-masquer.
    ///
    /// INSUFFISANT SEUL — BUG RÉEL SIGNALÉ EN PRATIQUE ("l'app est toujours
    /// visible quand je lance une vidéo en plein écran") : la plupart des
    /// lecteurs vidéo/navigateurs (VLC, YouTube en plein écran, lecteur
    /// intégré de Windows...) utilisent un plein écran "sans bordure" — une
    /// fenêtre normale simplement agrandie à la taille exacte de l'écran,
    /// PAS le plein écran D3D exclusif que détecte `SHQueryUserNotificationState`
    /// — combiné ici à `foreground_fullscreen_window` (voir sa doc), la
    /// même heuristique géométrique que la VRAIE barre des tâches et des
    /// outils comme TranslucentTB utilisent pour CE cas précis. Voir aussi
    /// `window_is_still_fullscreen`, qui permet de rester masqué sans
    /// dépendre du focus courant.
    ///
    /// Simple `ShowWindow(SW_HIDE/SW_SHOW)` — PAS une nouvelle
    /// réservation/libération d'AppBar (voir `reregister_appbars`, bien plus
    /// coûteux et fragile) : la réservation d'espace écran reste enregistrée
    /// pendant le masquage, exactement comme la VRAIE barre des tâches en
    /// mode auto-masqué, aucune raison de la toucher ici.
    pub fn sync_fullscreen_visibility(&mut self) {
        // Voir `fullscreen_window_on_monitor` : le plein écran se juge
        // ÉCRAN PAR ÉCRAN — demande explicite "je voudrais que l'interface
        // ne disparaisse pas de l'écran où il n'y a pas d'application en
        // plein écran".
        let own_monitor = hwnd_monitor(self.hwnd);
        // Voir `window_is_still_fullscreen` : tant qu'on est déjà masqué, on
        // revérifie CE HWND précis plutôt que le focus courant — cliquer
        // ailleurs (un autre écran, une de nos propres fenêtres...) ne doit
        // pas suffire à conclure que l'appli plein écran a fermé. On
        // revérifie aussi son écran : si elle a depuis changé de moniteur
        // (rare), elle ne concerne plus cette barre-ci.
        if self.hidden_for_fullscreen {
            if let Some(hwnd) = self.fullscreen_hwnd {
                if window_is_still_fullscreen(hwnd) && hwnd_monitor(hwnd) == own_monitor {
                    return;
                }
            }
        }
        let fullscreen_here = fullscreen_window_on_monitor(self.hwnd, own_monitor);
        // ⭐ Replacée dans la zone centrale (voir `win32::plein_ecran_zone`) : elle
        // ne couvre plus l'écran, le dock reste.
        if let Some(fenetre) = fullscreen_here.filter(|_| !self.hidden_for_fullscreen) {
            if crate::win32::plein_ecran_zone::rediriger(fenetre) {
                return;
            }
        }
        if fullscreen_here.is_some() && !self.hidden_for_fullscreen {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            // **LE JOURNAL NE NOMMAIT RIEN, ET C'EST CE QUI A COÛTÉ LE PLUS CHER.**
            // Le 2026-10-05, « au retour dans la session je ne vois plus l'app » a
            // demandé une sonde entière, faute de pouvoir lire QUELLE fenêtre nous
            // avait fait masquer. Un HWND ne se lit pas ; le nom de classe, si.
            if let Some(fenetre) = fullscreen_here {
                crate::diag!("dock masque : plein ecran au premier plan, classe {}", classe_de(fenetre));
            }
            self.hidden_for_fullscreen = true;
            self.fullscreen_hwnd = fullscreen_here;
        } else if fullscreen_here.is_none() && self.hidden_for_fullscreen {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOW);
            }
            crate::diag!("dock reaffiche : plus d'application plein ecran");
            self.hidden_for_fullscreen = false;
            self.fullscreen_hwnd = None;
        }
    }

    /// Dock principal actuellement masqué pour cause de plein écran (voir
    /// `sync_fullscreen_visibility`) — consommé par `app::run` pour piloter
    /// `win32::fullscreen_clock::FullscreenClockWindow` : la mini-barre
    /// d'horloge ne doit apparaître QUE lorsque le dock est masqué pour
    /// cette raison précise (pas, par exemple, pendant un simple
    /// redémarrage d'explorer.exe ou un autre masquage éventuel à venir).
    pub fn is_hidden_for_fullscreen(&self) -> bool {
        self.hidden_for_fullscreen
    }

    /// Traite tous les messages Windows en attente, sans bloquer.
    ///
    /// Rend `(continuer, messages)` : `continuer` vaut `false` quand la fenêtre
    /// doit se fermer (WM_QUIT), et `messages` compte ceux qui ont été traités.
    ///
    /// **`WM_TIMER` N'EST PAS COMPTÉ** : la seule minuterie de ce fil est celle
    /// de `win32::tray_flux` (réaffirmation du rang du shell, toutes les 100 ms),
    /// qui ne change RIEN à l'image. La compter garderait le dock en train de
    /// présenter dix fois par seconde pour rien (voir `app::run`, « présenter
    /// seulement les changements »).
    pub fn pump_messages() -> (bool, u32) {
        let mut traites = 0u32;
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    return (false, traites);
                }
                if msg.message != WM_TIMER {
                    traites += 1;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        (true, traites)
    }

    /// Récupère et vide la file d'évènements accumulés par le wndproc
    /// depuis le dernier appel.
    pub fn drain_events() -> Vec<Event> {
        EVENTS.with(|q| q.borrow_mut().drain(..).collect())
    }
}

/// Copie du dock affichée sur un écran secondaire — TOUJOURS réduite à la
/// seule bande du haut, pleine largeur de l'écran (DEMANDE EXPLICITE : "on
/// ne va laisser que la barre supérieure dans les interfaces secondaires").
/// Existait auparavant aussi en mode "application complète" (barres
/// gauche/droite en plus, forme en "⊓" comme la barre primaire, via un
/// `DockMode` à deux variantes) — retiré : ce second mode était la source de
/// deux bugs réels signalés coup sur coup dès qu'on le testait sérieusement
/// (menus ancrés qui recouvraient la barre au lieu de s'ouvrir dessous —
/// `active_layout.total_height` valait la hauteur de LA FENÊTRE, qui en
/// mode "barre du haut seule" n'est QUE l'épaisseur de cette barre, pas
/// celle de l'écran — et icônes des barres gauche/droite invisibles, aucune
/// zone physique où les dessiner une fois la fenêtre réduite à la bande du
/// haut). Une seule forme possible élimine les deux à la racine plutôt que
/// de les corriger au cas par cas.
///
/// RÉSERVE aussi une AppBar (voir `win32::appbar`) exactement comme `Window`
/// (barre primaire), sur le bord du haut uniquement — PAS un simple miroir
/// décoratif. (Doc historique périmée : une v1 sans AppBar existait
/// effectivement, mais a depuis été remplacée par ce qui suit `create`
/// ci-dessous, y compris le "nudge" des fenêtres déjà maximisées, voir
/// `nudge_maximized_windows`.)
///
/// PAS interactive (`WS_EX_TRANSPARENT`, voir `create`) : aucun clic/survol
/// n'est géré ici (pas de wndproc partagé avec `wndproc`, voir
/// `secondary_wndproc`, qui ne fait rien d'autre que `DefWindowProcW`) —
/// rendre les copies interactives (glisser une icône, popups ancrés sur le
/// bon écran...) est un chantier bien plus large, différé après cette
/// première version.
pub struct SecondaryWindow {
    pub hwnd: HWND,
    /// Identifiant stable de l'écran (voir `win32::monitors::MonitorInfo`) —
    /// permet à `app::run` de faire correspondre cette fenêtre à son réglage
    /// persistant même si l'ordre des écrans change au branchement/
    /// débranchement.
    pub device_name: String,
    pub width: u32,
    pub height: u32,
    /// Épaisseur de la bande dessinée ici — DÉRIVÉE indépendamment des
    /// propriétés de CET écran par `win32::screen_dock::
    /// derive_secondary_thickness`, PAS copiée de la barre primaire (voir sa
    /// doc pour la règle exacte : taskbar Windows propre à cet écran si
    /// présente, sinon repli mis à l'échelle du DPI de cet écran). PAS
    /// re-synchronisée en continu si l'utilisateur bouge les curseurs du
    /// panneau de réglages ensuite (limite acceptée de cette première
    /// version — `app::run` recrée la fenêtre au lieu de la redimensionner
    /// en place quand ces réglages changent — voir `rebuild_secondary_mirrors`).
    /// `width`/`height` ci-dessus valent déjà `(largeur de l'écran,
    /// top_thickness)` — cette fenêtre EST la bande du haut, voir
    /// `resolve_secondary_geometry`.
    pub top_thickness: u32,
    pub top_helper_hwnd: Option<HWND>,
    pub top_appbar: Option<AppBar>,
    hidden_for_fullscreen: bool,
    /// Voir `Window::fullscreen_hwnd` — même rôle, dupliqué comme le reste
    /// de la logique plein écran (voir `sync_fullscreen_visibility`).
    fullscreen_hwnd: Option<HWND>,
    /// État courant du masquage automatique (voir `set_band_reveal`,
    /// DEMANDE EXPLICITE : "rendre les barres de l'interface secondaire
    /// masquable automatiquement") — `true` = bande affichée/réservée
    /// normalement, INDÉPENDANT de `hidden_for_fullscreen` ci-dessus (une
    /// application plein écran a toujours priorité, voir `app::run` qui ne
    /// touche PAS à ce champ tant que `is_hidden_for_fullscreen()` est
    /// vrai). Toujours `true` à la création : la bande démarre visible.
    band_revealed: bool,
    /// Masquage automatique actif au moment du `create` (voir `app::
    /// rebuild_secondary_docks`, DEMANDE EXPLICITE : "les programmes n'ont
    /// pas tout l'espace disponible pour s'agrandir" quand la barre se
    /// masque) — décide si `top_appbar` réserve `top_thickness` (mode
    /// normal, comme avant) ou RIEN (mode auto-masquage : la bande
    /// n'apparaît qu'en survol du bord et RECOUVRE le contenu au lieu de
    /// lui retirer de l'espace en permanence, comme la vraie barre des
    /// tâches Windows en "masquage automatique"). Comparé dans le filtre
    /// de réutilisation de `rebuild_secondary_docks` : un changement de ce
    /// réglage force une recréation (la réservation ne peut pas être
    /// changée en place sans retomber dans le bug de boucle documenté sur
    /// `set_band_reveal`).
    pub auto_hide: bool,
    /// Écran de cette copie, tel que fourni à `create` — conservé pour que
    /// `reassert_geometry` puisse RECALCULER la géométrie attendue sans
    /// dépendre de l'appelant (voir sa doc : le shell peut déplacer la
    /// fenêtre après coup, et le seul remède fiable est de recomparer).
    monitor_handle: HMONITOR,
    monitor_rect: RECT,
    /// Barre des tâches de CET écran, telle que détectée à la création —
    /// `None` = cet écran n'en a pas (voir `detect_secondary_taskbar`).
    monitor_taskbar_rect: Option<RECT>,
}

/// Procédure de fenêtre des copies secondaires — un simple relais vers
/// `DefWindowProcW`, à l'exception de `WM_ERASEBKGND` (même flash blanc/noir
/// à éviter que sur `wndproc`, voir sa doc). AUCUN traitement de
/// `WM_DESTROY` : contrairement à `wndproc` (fenêtre principale), fermer une
/// copie secondaire ne doit JAMAIS terminer toute l'application — même
/// raison que `appbar_helper_wndproc`, voir sa doc.
unsafe extern "system" fn secondary_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => {
            TRACKING_LEAVE.with(|t| {
                if !*t.borrow() {
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut tme);
                    *t.borrow_mut() = true;
                }
            });
            push_event(Event::MouseMove {
                x: loword(lparam) as f32,
                y: hiword(lparam) as f32,
                hwnd,
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            TRACKING_LEAVE.with(|t| *t.borrow_mut() = false);
            push_event(Event::MouseLeave { hwnd });
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            // Défilement des icônes d'une barre (voir `Dock::scroll_bar`).
            //
            // DEUX particularités de ce message par rapport à tous les autres
            // messages souris traités ici :
            // - `lparam` porte des coordonnées ÉCRAN, pas client — d'où la
            //   conversion, les autres donnant déjà du client ;
            // - le delta est dans `wparam` (mot de POIDS FORT), en multiples
            //   de `WHEEL_DELTA` (120), positif vers l'avant (le haut).
            //
            // Reçu SANS que la fenêtre ait le focus : le dock est
            // `WS_EX_NOACTIVATE` et ne le prend jamais (voir `create_dock`),
            // c'est le réglage Windows « faire défiler les fenêtres inactives
            // au survol » qui route le message vers la fenêtre SOUS LE
            // CURSEUR.
            let mut point = POINT { x: loword(lparam) as i32, y: hiword(lparam) as i32 };
            unsafe {
                let _ = ScreenToClient(hwnd, &mut point);
            }
            let delta = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16 as f32;
            push_event(Event::MouseWheel { delta, x: point.x as f32, y: point.y as f32, hwnd });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            // `SetCapture` (relâchée sur `WM_LBUTTONUP` ci-dessous) — voir
            // `ui::dock::Dock::begin_potential_drag`/`end_drag` : sans elle,
            // glisser une icône puis relâcher le bouton APRÈS avoir quitté
            // les limites de la fenêtre ne délivrerait plus jamais le
            // `WM_LBUTTONUP` correspondant à CETTE fenêtre, laissant le
            // glisser bloqué indéfiniment (icône restée "flottante", ordre
            // des barres jamais figé). Standard Win32 pour tout
            // glisser-déposer, sans effet de bord sur un simple clic
            // (relâché immédiatement, capture libérée aussitôt).
            unsafe {
                SetCapture(hwnd);
            }
            push_event(Event::MouseDown {
                x: loword(lparam) as f32,
                y: hiword(lparam) as f32,
                hwnd,
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            unsafe {
                let _ = ReleaseCapture();
            }
            push_event(Event::MouseUp {
                x: loword(lparam) as f32,
                y: hiword(lparam) as f32,
                hwnd,
            });
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            push_event(Event::RightClick {
                x: loword(lparam) as f32,
                y: hiword(lparam) as f32,
                hwnd,
            });
            LRESULT(0)
        }
        WM_MBUTTONDOWN => {
            push_event(Event::MiddleClick {
                x: loword(lparam) as f32,
                y: hiword(lparam) as f32,
                hwnd,
            });
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

impl SecondaryWindow {
    /// Crée et affiche la copie sur `monitor` (voir `win32::monitors`),
    /// TOUJOURS réduite à la bande du haut (voir la doc de `SecondaryWindow`
    /// pour pourquoi le mode "application complète" a été retiré).
    /// L'épaisseur de la bande (`top_thickness`, calculée plus bas) est
    /// DÉRIVÉE des propriétés propres à CET écran par `win32::screen_dock::
    /// derive_secondary_thickness` — `primary_dpi`/`primary_thickness_fallback`
    /// ne servent QUE de repli mis à l'échelle pour un écran sans taskbar
    /// Windows propre (voir la doc de `derive_secondary_thickness` et de
    /// `win32::screen_dock::BarThicknesses`) : la barre primaire n'est
    /// jamais copiée telle quelle.
    pub fn create(
        monitor: &crate::win32::monitors::MonitorInfo,
        primary_dpi: u32,
        primary_thickness_fallback: BarThicknesses,
        thickness_override: Option<BarThicknesses>,
        // Voir la doc du champ `auto_hide` — décide si `top_appbar` réserve
        // `top_thickness` ou rien du tout.
        auto_hide: bool,
    ) -> Result<Self> {
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None)?.into();
            let class_name = w!("NeuraDeskSecondaryDockWindow");

            // `create` est appelée une fois PAR écran secondaire actif : la
            // classe ne doit être enregistrée qu'une seule fois pour tout le
            // processus (`RegisterClassExW` échoue sinon au deuxième appel).
            static CLASS_REGISTERED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
            CLASS_REGISTERED.get_or_init(|| {
                let wc = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(secondary_wndproc),
                    hInstance: instance,
                    lpszClassName: class_name,
                    hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                    ..Default::default()
                };
                RegisterClassExW(&wc);
            });

            let (edge, taskbar_thickness) = detect_secondary_taskbar(monitor.rect, monitor.taskbar_rect);

            // `left`/`right` de `thicknesses` restent calculés (source
            // partagée avec la dérivation de la barre PRIMAIRE, voir
            // `BarThicknesses`) mais ne servent plus à rien ici — aucune
            // bande latérale n'existe plus sur un écran secondaire.
            let thicknesses =
                resolve_secondary_thicknesses(monitor, taskbar_thickness, primary_dpi, primary_thickness_fallback, thickness_override);
            let top_thickness = thicknesses.top;

            let (origin_x, origin_y, width, height) = resolve_secondary_geometry(monitor, edge, taskbar_thickness, top_thickness);

            let hwnd = CreateWindowExW(
                DOCK_EX_STYLE,
                class_name,
                w!("NeuraDesk Dock"),
                WS_POPUP,
                origin_x,
                origin_y,
                width,
                height,
                None,
                None,
                Some(instance),
                None,
            )?;

            // Transparence réelle composée par DWM — même technique que
            // `Window::create_dock`, voir la doc en tête de fichier.
            let margins = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

            // La fenêtre EST déjà exactement la bande du haut (voir
            // `resolve_secondary_geometry`) — la région couvre tout le
            // client, pas de découpe en "⊓" à faire ici (contrairement à
            // `Window::create_dock`, barre PRIMAIRE, toujours pleine forme).
            // `None` en dernier : un dock SECONDAIRE ne magnifie jamais (voir
            // `Dock::update`, appelé pour le seul dock primaire), donc il n'a
            // jamais de boîte de débordement à composer.
            set_dock_region(hwnd, width, height, top_thickness as i32, 0, 0, 0, None, (0, 0, top_thickness as i32), 0);

            // Pose l'affinité d'affichage ET l'exclusion de la Desktop Duplication
        // ensemble — voir `capture_visibility::apply_to_window`, doc.
        crate::gfx::capture_visibility::apply_to_window(hwnd);
        // Icône de FENÊTRE (barre des tâches + Alt-Tab) — voir `apply_exe_icon`.
        apply_exe_icon(hwnd);
            // ⛔ **`SW_SHOWNOACTIVATE`, JAMAIS `SW_SHOWNORMAL`** — DÉFAUT SIGNALÉ
            // (2026-11-09) : « le dash réglage se ferme tout seul à la fin d'un
            // réglage ». `WS_EX_NOACTIVATE` n'empêche que l'activation par CLIC :
            // `SW_SHOWNORMAL` active la fenêtre qu'il montre. Or chaque réglage qui
            // change la géométrie d'un écran secondaire (épaisseur…) RECRÉE cette
            // fenêtre (`app::rebuild_secondary_docks`) : elle prenait le premier
            // plan, et `cef_popup::hide_if_focus_elsewhere` masquait les Réglages
            // (journal : « popup … masque » 0,85 s après l'action).
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

            let helper = create_appbar_helper(w!("NeuraDeskAppBarHelperTop"))?;
            // Voir la doc du champ `auto_hide` : en mode auto-masquage, on
            // réserve un rectangle de hauteur NULLE — la bande devient un
            // simple overlay qui recouvre le contenu au lieu de lui retirer
            // l'espace en permanence, exactement comme la vraie barre des
            // tâches Windows en "masquage automatique" (qui, elle non plus,
            // ne rétrécit pas la zone de travail des fenêtres maximisées).
            let reserved_bottom = if auto_hide { origin_y } else { origin_y + top_thickness as i32 };
            let top_appbar = AppBar::register(
                helper,
                ABE_TOP,
                RECT { left: origin_x, top: origin_y, right: origin_x + width, bottom: reserved_bottom },
            );

            // Voir `nudge_maximized_windows`, doc détaillée — BUG RÉEL SIGNALÉ
            // ("l'activation des barres secondaires ne fait pas se
            // redimensionner les fenêtres dans l'espace libre") : l'AppBar
            // ci-dessus vient de réduire la zone de travail de CET écran,
            // mais une fenêtre déjà maximisée dessus AVANT cette activation
            // ne le sait pas encore tant qu'elle n'est pas nudgée
            // explicitement.
            nudge_maximized_windows_retrying(monitor.handle);

            Ok(Self {
                hwnd,
                device_name: monitor.device_name.clone(),
                width: width as u32,
                height: height as u32,
                top_thickness,
                top_helper_hwnd: Some(helper),
                top_appbar: Some(top_appbar),
                hidden_for_fullscreen: false,
                fullscreen_hwnd: None,
                band_revealed: true,
                auto_hide,
                monitor_handle: monitor.handle,
                monitor_rect: monitor.rect,
                monitor_taskbar_rect: monitor.taskbar_rect,
            })
        }
    }

    /// Bascule cette bande entre sa réservation normale (`top_thickness`) et
    /// une réservation NULLE, SANS recréer la fenêtre/l'AppBar (voir la doc
    /// de module de `rebuild_secondary_docks`, bien trop coûteux — flash
    /// visible garanti — pour être rappelé à chaque frame pendant un survol
    /// de bord d'écran) — DEMANDE EXPLICITE ("rendre les barres de
    /// l'interface secondaire masquable automatiquement"). `self.width`/
    /// `self.height`/`self.top_thickness` restent INCHANGÉS (cette fenêtre
    /// EST la bande, voir la doc de struct : la redimensionner en place
    /// nécessiterait aussi de redimensionner la swapchain D3D associée,
    /// hors de portée de cette bascule volontairement légère) — seuls
    /// `ShowWindow` (visibilité + hit-test : une fenêtre masquée ne reçoit
    /// plus aucun message souris) et la réservation AppBar (espace de
    /// travail rendu aux autres fenêtres) changent. Sans effet si déjà dans
    /// l'état demandé (appelée à CHAQUE FRAME par `app::run` tant que le
    /// masquage automatique est actif, voir son appelant).
    pub fn set_band_reveal(&mut self, revealed: bool) {
        if revealed == self.band_revealed {
            return;
        }
        self.band_revealed = revealed;
        unsafe {
            let _ = ShowWindow(self.hwnd, if revealed { SW_SHOW } else { SW_HIDE });
        }
        // PAS de `appbar.update(...)` ici — volontaire, voir l'historique de
        // cette ligne (BUG RÉEL VÉCU : "la barre apparait et disparait en
        // boucle"). Faire varier la réservation `SHAppBarMessage` à chaque
        // bascule fait rediffuser `WM_SETTINGCHANGE(SPI_SETWORKAREA)` par le
        // shell (même mécanisme que `win32::window::is_workarea_setting_
        // change`/`app::Event::DisplaysChanged`) : `app::run` planifie alors
        // un `rebuild_secondary_docks` débattu (`DISPLAY_CHANGE_DEBOUNCE`),
        // qui referme la boucle. La VRAIE barre des tâches Windows en mode
        // "masquer automatiquement" évite ce même écueil en gardant sa
        // réservation FIXE en permanence (juste une fine bande de
        // déclenchement) plutôt qu'en l'ajustant à chaque apparition — on
        // fait pareil : la réservation posée par `top_appbar` au moment du
        // `create` reste inchangée pendant tout le cycle de vie de la
        // fenêtre, seule la visibilité (`ShowWindow`) bascule.
        //
        // Conséquence acceptée : une fenêtre déjà maximisée sur cet écran ne
        // récupère PAS l'espace de la barre pendant qu'elle est masquée —
        // comme pour la vraie barre des tâches en auto-masquage, qui ne
        // rend pas non plus cet espace aux fenêtres maximisées.
        nudge_maximized_windows_retrying(unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST) });
    }

    /// Repose la fenêtre sur la géométrie CALCULÉE pour elle (voir
    /// `resolve_secondary_geometry`) si le shell l'en a déplacée — no-op si
    /// elle est déjà à sa place, donc sûr à appeler à répétition.
    ///
    /// BUG RÉEL SIGNALÉ ("la barre supérieure de l'interface secondaire s'est
    /// déplacée, elle n'est plus en haut de l'écran secondaire") — cause
    /// établie par traçage des messages Win32, pas par déduction :
    /// `SecondaryWindow::create` positionne la fenêtre CORRECTEMENT (rectangle
    /// demandé = rectangle obtenu, vérifié après `CreateWindowExW`), l'AppBar
    /// réserve le bon rectangle (484→524 sur l'écran de ce poste), et
    /// pourtant la fenêtre se retrouve 40 px plus bas — exactement l'épaisseur
    /// de la bande — dès le PREMIER `Window::pump_messages`. Le coupable est
    /// le SHELL lui-même : un `WM_WINDOWPOSCHANGING` (`msg=0x0046`,
    /// `DEMANDE=(-1920,524) 1920x40`, `flags=0x80014` = `SWP_NOACTIVATE |
    /// SWP_NOZORDER | SWP_NOOWNERZORDER`) arrive de façon ASYNCHRONE en
    /// réaction à l'enregistrement AppBar, et repose la fenêtre AppBar en la
    /// poussant hors de sa propre réservation. Rien dans notre code ne
    /// produit ce déplacement (tous nos sites `SetWindowPos` sont sans effet
    /// ici, et le `wndproc` secondaire ne fait que relayer à
    /// `DefWindowProcW`).
    ///
    /// Comme ce repositionnement est décidé par le shell APRÈS coup et hors
    /// de notre contrôle, le seul remède fiable est de constater puis de
    /// corriger : `app::run` appelle ceci une fois la première salve de
    /// messages pompée (voir `geometry_reasserted` là-bas), plutôt qu'à
    /// chaque frame — la position ne bouge plus ensuite (vérifié : elle reste
    /// stable sur toutes les frames suivantes).
    pub fn reassert_geometry(&self) {
        let (want_x, want_y, want_w, want_h) = self.geometrie_voulue();
        let mut current = RECT::default();
        unsafe {
            if GetWindowRect(self.hwnd, &mut current).is_err() {
                return;
            }
        }
        if (current.left, current.top, current.right - current.left, current.bottom - current.top)
            == (want_x, want_y, want_w, want_h)
        {
            return;
        }
        // Trace (voir `diag`, inerte sans `NEURADESK_DIAG`) : ce cas est RARE et
        // c'est exactement ce qu'on veut savoir d'un défaut de position — OÙ le
        // shell avait posé la fenêtre, et où on la remet. Sans elle, un écart
        // constaté à l'écran ne laisse que le symptôme.
        crate::diag!(
            "[{}] geometrie reposee : voulu ({want_x},{want_y}) {want_w}x{want_h}, le shell avait mis ({},{}) {}x{}",
            self.device_name,
            current.left,
            current.top,
            current.right - current.left,
            current.bottom - current.top
        );
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                None,
                want_x,
                want_y,
                want_w,
                want_h,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
    }

    /// Géométrie `(x, y, largeur, hauteur)` CALCULÉE pour cette bande —
    /// recalculée à partir des données mémorisées à la création (voir les
    /// champs `monitor_*`) : même source de vérité que `create`, donc aucune
    /// divergence possible entre la géométrie appliquée au départ et celle
    /// re-posée ensuite (voir `reassert_geometry`).
    fn geometrie_voulue(&self) -> (i32, i32, i32, i32) {
        let (edge, taskbar_thickness) = detect_secondary_taskbar(self.monitor_rect, self.monitor_taskbar_rect);
        // ⚠️ **`work_rect` N'EST PAS LUE PAR `resolve_secondary_geometry`** (elle ne
        // travaille que sur `rect` et `taskbar_rect`) — mais on interroge quand même
        // le shell au lieu d'y mettre une valeur inventée : ce champ est documenté
        // comme « ce que le shell publie », et un `MonitorInfo` qui mentirait dessus
        // serait un piège pour le prochain lecteur. Repli sur le rectangle de
        // l'écran si le shell ne répond pas, ce qui ne peut que SUR-estimer la
        // place disponible.
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let work_rect = if unsafe { GetMonitorInfoW(self.monitor_handle, &mut info) }.as_bool() {
            info.rcWork
        } else {
            self.monitor_rect
        };
        let monitor = crate::win32::monitors::MonitorInfo {
            handle: self.monitor_handle,
            device_name: self.device_name.clone(),
            rect: self.monitor_rect,
            work_rect,
            is_primary: false,
            taskbar_rect: self.monitor_taskbar_rect,
        };
        resolve_secondary_geometry(&monitor, edge, taskbar_thickness, self.top_thickness)
    }

    /// Voir `Window::suspendre_pour_capture` et `EtatBarresPourCapture` — même
    /// rôle pour cette bande. La reprise repose la réservation de `create`
    /// (hauteur NULLE en masquage automatique, voir le champ `auto_hide`) et ne
    /// réaffiche que si la bande était révélée et qu'aucun plein écran ne la
    /// masque.
    pub fn suspendre_pour_capture(&mut self, etat: EtatBarresPourCapture) {
        // ⛔ **`MasqueesSansLiberer` NE TOUCHE NI À LA RÉSERVATION NI AU `nudge`** —
        // seule la visibilité change, à la fin. C'est ce qui garde la fenêtre
        // agrandie DANS le cadre de la bande au lieu de la laisser déborder
        // dessous, et ce qui évite de « bousculer les fenêtres agrandies pour
        // rien » (voir `EtatBarresPourCapture`).
        let sans_liberer = etat == EtatBarresPourCapture::MasqueesSansLiberer;
        if !sans_liberer {
            let (x, y, largeur, _) = self.geometrie_voulue();
            let reserve = etat == EtatBarresPourCapture::Repos && !self.auto_hide;
            let bas = if reserve { y + self.top_thickness as i32 } else { y };
            if let Some(appbar) = &self.top_appbar {
                appbar.update(RECT { left: x, top: y, right: x + largeur, bottom: bas });
            }
        }
        let montrer = etat == EtatBarresPourCapture::Repos && self.band_revealed && !self.hidden_for_fullscreen;
        unsafe {
            let _ = ShowWindow(self.hwnd, if montrer { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
        if sans_liberer {
            return;
        }
        nudge_maximized_windows_retrying(self.monitor_handle);
    }

    /// Voir `Window::ecran_et_zone_de_travail` — même rôle et mêmes réserves
    /// pour cette bande d'écran secondaire.
    pub fn ecran_et_zone_de_travail(&self) -> Option<(HMONITOR, RECT)> {
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        unsafe { GetMonitorInfoW(self.monitor_handle, &mut info) }
            .as_bool()
            .then_some((self.monitor_handle, info.rcWork))
    }

    /// Voir `Window::reserve_de_l_espace`. La règle est celle de
    /// `suspendre_pour_capture` juste au-dessus (`let reserve = !suspendre &&
    /// !self.auto_hide`) : une bande en masquage automatique ne réserve RIEN,
    /// donc sa suspension ne déplace aucune zone de travail.
    pub fn reserve_de_l_espace(&self) -> bool {
        !self.auto_hide && self.top_thickness > 0
    }

    /// Même logique que `Window::screen_origin` (voir sa doc détaillée) —
    /// permet à `app::capture_bar_backdrops` (capture+flou du bureau sous
    /// les barres) de s'appliquer aussi aux écrans secondaires : demande
    /// explicite "même possibilités de réglages... flou".
    pub fn screen_origin(&self) -> Result<(i32, i32)> {
        hwnd_screen_origin(self.hwnd)
    }

    /// Même logique que `Window::sync_fullscreen_visibility` (voir sa doc
    /// détaillée), dupliquée : pas d'état partagé entre les deux types, et
    /// une application plein écran sur N'IMPORTE QUEL écran doit aussi
    /// masquer les copies (comme la vraie barre des tâches Windows le fait
    /// pour ses propres fenêtres secondaires).
    pub fn sync_fullscreen_visibility(&mut self) {
        // Voir `fullscreen_window_on_monitor` : le plein écran se juge
        // ÉCRAN PAR ÉCRAN — demande explicite "je voudrais que l'interface
        // ne disparaisse pas de l'écran où il n'y a pas d'application en
        // plein écran".
        let own_monitor = hwnd_monitor(self.hwnd);
        // Voir `window_is_still_fullscreen` : tant qu'on est déjà masqué, on
        // revérifie CE HWND précis plutôt que le focus courant — cliquer
        // ailleurs (un autre écran, une de nos propres fenêtres...) ne doit
        // pas suffire à conclure que l'appli plein écran a fermé. On
        // revérifie aussi son écran : si elle a depuis changé de moniteur
        // (rare), elle ne concerne plus cette barre-ci.
        if self.hidden_for_fullscreen {
            if let Some(hwnd) = self.fullscreen_hwnd {
                if window_is_still_fullscreen(hwnd) && hwnd_monitor(hwnd) == own_monitor {
                    return;
                }
            }
        }
        let fullscreen_here = fullscreen_window_on_monitor(self.hwnd, own_monitor);
        // ⭐ Replacée dans la zone centrale (voir `win32::plein_ecran_zone`) : elle
        // ne couvre plus l'écran, le dock reste.
        if let Some(fenetre) = fullscreen_here.filter(|_| !self.hidden_for_fullscreen) {
            if crate::win32::plein_ecran_zone::rediriger(fenetre) {
                return;
            }
        }
        if fullscreen_here.is_some() && !self.hidden_for_fullscreen {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            // **LE JOURNAL NE NOMMAIT RIEN, ET C'EST CE QUI A COÛTÉ LE PLUS CHER.**
            // Le 2026-10-05, « au retour dans la session je ne vois plus l'app » a
            // demandé une sonde entière, faute de pouvoir lire QUELLE fenêtre nous
            // avait fait masquer. Un HWND ne se lit pas ; le nom de classe, si.
            if let Some(fenetre) = fullscreen_here {
                crate::diag!("dock masque : plein ecran au premier plan, classe {}", classe_de(fenetre));
            }
            self.hidden_for_fullscreen = true;
            self.fullscreen_hwnd = fullscreen_here;
        } else if fullscreen_here.is_none() && self.hidden_for_fullscreen {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOW);
            }
            crate::diag!("dock reaffiche : plus d'application plein ecran");
            self.hidden_for_fullscreen = false;
            self.fullscreen_hwnd = None;
        }
    }

    /// Voir `Window::is_hidden_for_fullscreen` (même rôle, pour l'écran
    /// secondaire porté par ce `SecondaryWindow`) — consommé par `app::run`
    /// pour piloter la `win32::fullscreen_clock::FullscreenClockWindow`
    /// propre à CET écran (voir `SecondaryDock::fullscreen_clock`).
    pub fn is_hidden_for_fullscreen(&self) -> bool {
        self.hidden_for_fullscreen
    }

    /// L'écran de cette bande (voir `app::bandes_secondaires`, qui y lit le DPI).
    pub fn monitor_handle(&self) -> HMONITOR {
        self.monitor_handle
    }
}

impl Drop for SecondaryWindow {
    fn drop(&mut self) {
        // `AppBar::drop` (voir sa doc) a besoin du HWND assistant ENCORE
        // VALIDE pour envoyer `ABM_REMOVE` — les prendre explicitement ICI
        // (avant de détruire les HWND juste en dessous) plutôt que de
        // laisser la glue de destruction automatique des champs s'en
        // charger : celle-ci ne s'exécute QU'APRÈS ce corps de `drop`, donc
        // APRÈS les `DestroyWindow` ci-dessous, ce qui aurait envoyé
        // `ABM_REMOVE` sur un HWND déjà détruit — réservation orpheline
        // côté shell.
        drop(self.top_appbar.take());
        unsafe {
            // Destroy the helper window if it exists
            if let Some(hwnd) = self.top_helper_hwnd {
                let _ = DestroyWindow(hwnd);
            }
            // Destroy the secondary window itself
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// Bord d'ancrage et épaisseur de la barre des tâches Windows trouvée sur UN
/// écran donné (voir `win32::monitors::MonitorInfo::taskbar_rect`) —
/// géométrique plutôt que basé sur `ABM_GETTASKBARPOS` (voir `detect_taskbar`) :
/// cette API ne renvoie que la position de la barre des tâches PRINCIPALE,
/// jamais celle d'un `Shell_SecondaryTrayWnd`. Repli sur `Bottom`/épaisseur 0
/// si l'écran n'a pas de barre des tâches détectée (utilisateur ayant
/// désactivé "Afficher la barre des tâches sur tous les écrans").
pub fn detect_secondary_taskbar(monitor_rect: RECT, taskbar_rect: Option<RECT>) -> (TaskbarEdge, i32) {
    let Some(tb) = taskbar_rect else { return (TaskbarEdge::Bottom, 0) };
    let mw = monitor_rect.right - monitor_rect.left;
    let mh = monitor_rect.bottom - monitor_rect.top;
    let tw = tb.right - tb.left;
    let th = tb.bottom - tb.top;
    // Tolérance en pixels pour absorber un léger débord/arrondi — sans ça,
    // une comparaison stricte pourrait rater une correspondance évidente.
    const SLACK: i32 = 4;
    if tw >= mw - SLACK {
        if tb.top <= monitor_rect.top + SLACK {
            (TaskbarEdge::Top, th)
        } else {
            (TaskbarEdge::Bottom, th)
        }
    } else if th >= mh - SLACK {
        if tb.left <= monitor_rect.left + SLACK {
            (TaskbarEdge::Left, tw)
        } else {
            (TaskbarEdge::Right, tw)
        }
    } else {
        (TaskbarEdge::Bottom, th)
    }
}

/// Calcule les épaisseurs EFFECTIVES des bandes d'un écran secondaire —
/// SOURCE UNIQUE partagée par `SecondaryWindow::create` (calcul réel) et
/// `app::rebuild_secondary_docks` (comparaison "cette fenêtre précédente
/// est-elle encore à jour ?", voir sa doc détaillée : BUG RÉEL SIGNALÉ
/// "fenêtres déjà maximisées qui ne se redimensionnent pas, les nouvelles
/// non plus") — sans ce partage, `rebuild_secondary_docks` aurait dû
/// dupliquer cette dérivation pour sa comparaison, avec le risque classique
/// de divergence silencieuse entre les deux copies.
pub(crate) fn resolve_secondary_thicknesses(
    monitor: &crate::win32::monitors::MonitorInfo,
    taskbar_thickness: i32,
    primary_dpi: u32,
    primary_thickness_fallback: BarThicknesses,
    thickness_override: Option<BarThicknesses>,
) -> BarThicknesses {
    let mw = monitor.rect.right - monitor.rect.left;
    let mh = monitor.rect.bottom - monitor.rect.top;
    // Indépendante par bande (`top`/`left`/`right` peuvent différer sur la
    // barre primaire, voir `Window::set_dock_thicknesses`) mais PARTAGE le
    // même statut "taskbar propre présente ou non" pour les trois : soit CET
    // écran a sa propre taskbar (une seule, un seul bord), soit aucune.
    let monitor_dpi = crate::win32::dpi::get_dpi_for_monitor(monitor.handle);
    let monitor_shorter_dimension_px = mw.min(mh).max(0) as u32;
    // `.max(0)` : `taskbar_thickness` vient d'une soustraction de RECT (voir
    // `detect_secondary_taskbar`) qui pourrait être négative sur un rect
    // transitoire/malformé pendant un changement de topologie — même
    // précaution que pour `monitor_shorter_dimension_px` ci-dessus, pour ne
    // pas laisser un cast `as u32` retourner une valeur énorme par
    // wrap-around.
    //
    // Note : `taskbar_rect.is_some()` seul (sans vérifier `taskbar_thickness
    // > 0`) suffit en pratique — un `0` ici signifierait un rect détecté
    // mais dégénéré, ce qui ferait sauter le repli DPI et clamper
    // directement au plancher (`MIN_BAR_THICKNESS_PX`) au lieu de mettre à
    // l'échelle la barre primaire ; résultat toujours visible/sain, juste
    // une source différente de celle documentée pour le cas normal.
    let own_taskbar_thickness = monitor.taskbar_rect.is_some().then_some(taskbar_thickness.max(0) as u32);
    // Réglage manuel de CET écran (voir `app::SecondaryScreenOverrides`,
    // demande explicite : "pouvoir régler les barres secondaires
    // individuellement") — prime sur la dérivation automatique ci-dessus
    // quand présent.
    thickness_override
        .unwrap_or_else(|| primary_thickness_fallback.derive_for_screen(own_taskbar_thickness, monitor_dpi, primary_dpi, monitor_shorter_dimension_px))
}

/// Calcule l'origine/taille de la fenêtre d'un dock secondaire — SOURCE
/// UNIQUE partagée par `SecondaryWindow::create` (calcul réel) et
/// `app::rebuild_secondary_docks` (comparaison de réutilisation, voir sa
/// doc). BUG RÉEL VÉCU ("l'app sur l'écran secondaire se relance en
/// boucle") : `rebuild_secondary_docks` calculait d'abord sa propre
/// attente simplifiée (`height == hauteur totale du moniteur`), qui
/// oubliait que `DockMode::Full` retranche l'épaisseur d'une VRAIE
/// taskbar Windows présente sur CET écran (voir `detect_secondary_
/// taskbar`) — la comparaison échouait donc TOUJOURS sur un écran avec sa
/// propre taskbar, recréant la fenêtre (donc ses AppBars) en boucle
/// (auto-entretenue via `WM_SETTINGCHANGE`/`Event::DisplaysChanged`,
/// voir `app::rebuild_secondary_docks`).
pub(crate) fn resolve_secondary_geometry(
    monitor: &crate::win32::monitors::MonitorInfo,
    edge: TaskbarEdge,
    taskbar_thickness: i32,
    top_thickness: u32,
) -> (i32, i32, i32, i32) {
    let mw = monitor.rect.right - monitor.rect.left;
    // La fenêtre EST la bande du haut : pas de forme en "⊓" à découper
    // (voir `set_dock_region`, toujours appelé avec left=right=0 désormais).
    match edge {
        TaskbarEdge::Top => (monitor.rect.left, monitor.rect.top + taskbar_thickness, mw, top_thickness as i32),
        TaskbarEdge::Bottom => (monitor.rect.left, monitor.rect.top, mw, top_thickness as i32),
        TaskbarEdge::Left => (monitor.rect.left + taskbar_thickness, monitor.rect.top, mw - taskbar_thickness, top_thickness as i32),
        TaskbarEdge::Right => (monitor.rect.left, monitor.rect.top, mw - taskbar_thickness, top_thickness as i32),
    }
}

/// Zone de travail ACTUELLE (`SPI_GETWORKAREA`) — voir `reregister_appbars`,
/// qui vérifie que sa réservation a bien été prise en compte plutôt que de
/// faire confiance au succès rapporté par `SHAppBarMessage` seul.
fn current_work_area() -> RECT {
    let mut work_area = RECT::default();
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut work_area as *mut RECT as *mut _),
            Default::default(),
        );
    }
    work_area
}

/// Force les fenêtres DÉJÀ maximisées sur `monitor` à se redimensionner
/// IMMÉDIATEMENT contre la zone de travail actuelle de ce moniteur — BUG RÉEL
/// SIGNALÉ ("l'activation des barres secondaires ne fait pas se redimensionner
/// les fenêtres dans l'espace libre") : `SHAppBarMessage(ABM_SETPOS)` (voir
/// `win32::appbar::AppBar::register`/`update`) met bien à jour
/// `SPI_GETWORKAREA` — les FUTURS agrandissements respectent donc déjà le
/// dock — mais ce n'est qu'un comportement câblé en dur dans `explorer.exe`
/// pour la VRAIE barre des tâches qui force en plus les fenêtres DÉJÀ
/// maximisées à se recalculer instantanément ; une AppBar tierce (la nôtre)
/// n'obtient jamais ce traitement spécial, documenté comme une limite connue
/// de l'API AppBar. Appelée juste après chaque enregistrement/mise à jour
/// d'AppBar qui RÉDUIT la zone de travail (activation d'un dock secondaire,
/// curseurs d'épaisseur de la barre primaire...) — inutile côté retrait
/// (`ABM_REMOVE`/`Drop`), agrandir une fenêtre déjà maximisée vers l'espace
/// libéré n'est pas gênant visuellement et n'a jamais été signalé.
///
/// `SetWindowPos` direct sur la zone de travail plutôt qu'un cycle
/// restaurer/re-maximiser (`WM_SYSCOMMAND`/`ShowWindow`) : évite le clignotement
/// d'une VRAIE animation de fenêtre sur des applications qui n'y sont pour
/// rien, tout en obtenant le même résultat géométrique final (une fenêtre
/// maximisée EST, par définition, dimensionnée exactement sur la zone de
/// travail de son moniteur).
unsafe fn nudge_maximized_windows(monitor: HMONITOR) {
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return;
    }
    struct NudgeState {
        monitor: HMONITOR,
        work: RECT,
    }
    let mut state = NudgeState { monitor, work: info.rcWork };

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = unsafe { &*(lparam.0 as *const NudgeState) };
        if !unsafe { IsWindowVisible(hwnd) }.as_bool() || !unsafe { IsZoomed(hwnd) }.as_bool() {
            return true.into();
        }
        if unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) } != state.monitor {
            return true.into();
        }
        let w = state.work;
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                w.left,
                w.top,
                w.right - w.left,
                w.bottom - w.top,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
        true.into()
    }

    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut state as *mut NudgeState as isize));
    }
}

/// Retente `nudge_maximized_windows` plusieurs fois, à quelques centaines de
/// millisecondes d'intervalle — BUG RÉEL SIGNALÉ ("ça marche un bref
/// instant seulement après le lancement de l'app") : `SHAppBarMessage(
/// ABM_SETPOS)` retourne un succès immédiat, mais le shell ne met sa zone
/// de travail (`GetMonitorInfoW`/`rcWork`, relue par un SEUL appel de
/// `nudge_maximized_windows`) à jour que de façon ASYNCHRONE — quasi
/// instantanément juste après le lancement de l'app (shell encore inactif,
/// d'où l'effet observé "seulement un bref instant"), avec un délai
/// perceptible en cours de session (shell plus occupé) quand une barre
/// secondaire est activée depuis le panneau de réglages. Un seul appel
/// relisait alors l'ANCIENNE zone de travail (encore non réduite) et
/// redimensionnait les fenêtres déjà maximisées dessus — sans effet visible
/// — sans jamais retenter une fois le shell réellement à jour. Retenter
/// (chaque appel relit `GetMonitorInfoW` À NEUF) finit par appliquer la
/// BONNE géométrie ; même constat empirique que `reregister_appbars`
/// (relecture différée du shell), dont le cas — état perdu après un
/// redémarrage d'`explorer.exe` — reste distinct (ne duplique donc pas son
/// calcul de zone de travail "attendue", volontairement plus simple ici).
///
/// Tourne sur un thread À PART (`std::thread::spawn`) plutôt qu'en bloquant
/// l'appelant : ce dernier est TOUJOURS le thread UI (`SecondaryWindow::
/// create`/`set_dock_thicknesses`/`reregister_appbars` sont tous appelés
/// depuis la boucle de rendu principale, voir `app::run`) — le geler jusqu'à
/// 1,6s (`NUDGE_ATTEMPTS` × `NUDGE_DELAI`) aurait arrêté la pompe de messages de
/// CE process (donc CEF, le rendu D3D11...) pendant la retentative, et
/// `SetWindowPos` sur une fenêtre CIBLE dont le thread propriétaire est lui
/// aussi occupé se bloque déjà en `SendMessage` implicite (documentation
/// Win32 de `WM_WINDOWPOSCHANGING`) — geler l'UI de NeuraDesk en plus n'aide
/// en rien et prolongeait inutilement le blocage visible pendant le test.
/// `HMONITOR` est un simple handle opaque (`isize`) — `Send` sans souci.
///
/// ⛔ **LE BUDGET DE CETTE FONCTION VIT AU NIVEAU DU MODULE** — voir
/// `NUDGE_ATTEMPTS` et `budget_nudge`, définis juste après
/// `nudge_maximized_windows_retrying_blocking`, et pourquoi.
fn nudge_maximized_windows_retrying(monitor: HMONITOR) {
    // `HMONITOR` embarque un `*mut c_void` (non-`Send` par défaut, même si
    // c'est juste un handle opaque, jamais déréférencé comme pointeur réel)
    // — le fait transiter en `isize` le temps de traverser vers le thread,
    // puis reconstruit via `HMONITOR(ptr)` juste en dessous.
    let monitor_raw = monitor.0 as isize;
    std::thread::spawn(move || {
        let monitor = HMONITOR(monitor_raw as *mut std::ffi::c_void);
        nudge_maximized_windows_retrying_blocking(monitor, NUDGE_ATTEMPTS, NUDGE_DELAI);
    });
}

fn nudge_maximized_windows_retrying_blocking(monitor: HMONITOR, attempts: u32, retry_delay: std::time::Duration) {
    for attempt in 0..attempts {
        unsafe {
            nudge_maximized_windows(monitor);
        }
        if attempt + 1 < attempts {
            std::thread::sleep(retry_delay);
        }
    }
}

/// Combien de fois `nudge_maximized_windows_retrying` retente, et à quel
/// intervalle.
///
/// ⛔ **AU NIVEAU DU MODULE, ET PAS LOCALES À LA FONCTION** : ce budget est ce
/// que la capture d'écran doit attendre avant de figer l'écran — `budget_nudge`
/// en dérive la durée, et `app::CAPTURE_REFLUX_MAX` dérive de LUI. Deux
/// constantes retapées ailleurs dériveraient en silence l'une de l'autre :
/// c'est la dérive « deux jumeaux » que ce dépôt a déjà payée deux fois.
pub(crate) const NUDGE_ATTEMPTS: u32 = 8;
pub(crate) const NUDGE_DELAI: std::time::Duration = std::time::Duration::from_millis(200);

/// Dernier instant où un redimensionnement peut encore arriver, à partir du
/// lancement de `nudge_maximized_windows_retrying` : la dernière tentative part
/// APRÈS le dernier sommeil, d'où `(N - 1) × délai` — 1400 ms avec les valeurs
/// courantes, et non 1600.
pub(crate) fn budget_nudge() -> std::time::Duration {
    NUDGE_DELAI * (NUDGE_ATTEMPTS - 1)
}

/// `exterieur` contient-il `interieur` ? Bornes INCLUSES — un bord partagé
/// compte comme contenu, c'est le cas normal d'une fenêtre agrandie posée sur
/// la zone de travail.
fn contient(exterieur: RECT, interieur: RECT) -> bool {
    exterieur.left <= interieur.left
        && exterieur.top <= interieur.top
        && exterieur.right >= interieur.right
        && exterieur.bottom >= interieur.bottom
}

/// Le cœur PUR de `ecrans_ont_rendu_la_place` — aucun appel Win32, donc
/// éprouvable en `cargo test` sans bureau.
///
/// Un écran a rendu la place quand sa zone de travail a **bougé** depuis la
/// réservation, ET qu'aucune des fenêtres agrandies qui s'y trouvent ne la
/// déborde — au sens où aucune ne la CONTIENT PAS.
///
/// ⚠️ **CONTENANCE, JAMAIS ÉGALITÉ.** `GetWindowRect` d'une fenêtre agrandie
/// est gonflé par le bord de redimensionnement invisible de DWM (~8 px de
/// chaque côté), donc `rect == rcWork` est **faux en permanence** : la
/// condition ne se satisferait jamais et chaque capture irait jusqu'à
/// l'échéance dure. Un `SetWindowPos(hwnd, rcWork)` posé par
/// `nudge_maximized_windows` produit, lui, un rectangle qui CONTIENT `rcWork`.
///
/// ⚠️ **LES DEUX MOITIÉS SONT NÉCESSAIRES, ET VOICI POURQUOI.**
/// - La contenance SEULE est fausse : tant que le shell n'a pas appliqué la
///   libération, sa zone de travail est encore l'ancienne — et la fenêtre,
///   elle aussi encore à l'ancienne, la contient parfaitement. Le test
///   rendrait vrai sans qu'un pixel ait bougé.
/// - Le déplacement SEUL est faux : le shell publie la nouvelle zone de
///   travail AVANT que le fil de `nudge_maximized_windows_retrying` n'ait
///   redimensionné quoi que ce soit (les deux sont indépendants, et le second
///   retente toutes les `NUDGE_DELAI`).
fn ecran_a_rendu(avant: RECT, maintenant: RECT, fenetres: &[RECT]) -> bool {
    maintenant != avant && fenetres.iter().all(|f| contient(*f, maintenant))
}

/// Rectangle de chaque fenêtre **agrandie et visible**, avec son écran.
///
/// ⚠️ **LE FILTRE EST PLUS STRICT QUE CELUI DE `nudge_maximized_windows`**, et
/// il doit l'être : `IsIconic` et `est_une_surface_du_shell` sont exactement ce
/// que `window_is_still_fullscreen` écarte déjà (voir sa doc). Une fenêtre
/// réduite qui se dirait agrandie, ou une surface du shell, rendrait la
/// condition de `ecrans_ont_rendu_la_place` **insatisfiable** — donc chaque
/// capture attendrait l'échéance dure, à chaque fois.
fn fenetres_agrandies() -> Vec<(HMONITOR, RECT)> {
    struct Etat {
        trouvees: Vec<(HMONITOR, RECT)>,
    }
    let mut etat = Etat { trouvees: Vec::new() };

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let etat = unsafe { &mut *(lparam.0 as *mut Etat) };
        if !unsafe { IsWindowVisible(hwnd) }.as_bool()
            || !unsafe { IsZoomed(hwnd) }.as_bool()
            || unsafe { IsIconic(hwnd) }.as_bool()
            || est_une_surface_du_shell(hwnd)
        {
            return true.into();
        }
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
            return true.into();
        }
        etat.trouvees.push((unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }, rect));
        true.into()
    }

    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut etat as *mut Etat as isize));
    }
    etat.trouvees
}

/// **Le bureau a-t-il fini de rendre aux fenêtres agrandies la place que nos
/// barres réservaient ?** — voir `app::PhaseCapture::Attente`, qui gèle l'écran
/// dès que c'est vrai.
///
/// `avant` porte, par écran dont on vient de suspendre la barre, la zone de
/// travail telle que le shell la publiait **avant** la suspension — donc
/// RÉSERVÉE. Un écran ABSENT de `avant` n'est pas regardé : c'est ce qui
/// permet de n'attendre que là où quelque chose a réellement été libéré.
///
/// ⚠️ **CE N'EST PAS UNE PREUVE QUE TOUT EST FINI.** `poser_reservations` émet
/// quatre `ABM_SETPOS` que le shell peut appliquer par ÉTAPES, et un relevé
/// peut être vrai au milieu d'une telle transition. Ce qui garantit qu'on ne
/// bloque jamais, c'est l'échéance dure (`app::CAPTURE_REFLUX_MAX`) ; cette
/// fonction ne fait que rendre le gel PLUS TÔT dans le cas courant.
pub fn ecrans_ont_rendu_la_place(avant: &[(HMONITOR, RECT)]) -> bool {
    let fenetres = fenetres_agrandies();
    avant.iter().all(|(ecran, avant_work)| {
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !unsafe { GetMonitorInfoW(*ecran, &mut info) }.as_bool() {
            // Écran débranché/reconfiguré pendant l'attente : il n'y a plus
            // rien à réparer là. Exiger sa zone de travail ferait attendre
            // l'échéance dure pour un écran qui n'existe plus.
            return true;
        }
        let ici: Vec<RECT> = fenetres.iter().filter(|(m, _)| m == ecran).map(|(_, r)| *r).collect();
        ecran_a_rendu(*avant_work, info.rcWork, &ici)
    })
}

/// Calcule l'origine (coin supérieur gauche) et la taille de la fenêtre du
/// dock à partir des dimensions de l'écran principal et du bord d'ancrage/
/// de l'épaisseur de la VRAIE barre des tâches Windows (voir
/// `detect_taskbar`) — la fenêtre occupe toute la zone de travail restante,
/// ancrée au coin le plus proche du coin supérieur gauche de l'écran (voir
/// la doc de `Window::create_dock`, qui explique pourquoi). Fonction PURE
/// (aucun appel Win32) volontairement extraite de `create_dock` : réutilisée
/// telle quelle par `Window::resync_to_primary_monitor` (revalidée après un
/// changement de topologie d'écran, voir `Event::DisplaysChanged`), et
/// testable unitairement sans écran/fenêtre réels.
///
/// Largeur/hauteur résultantes bornées à 1px minimum (même garde-fou que
/// l'ancien terminal déroulant) : `screen_h/screen_w - taskbar_thickness` peut
/// devenir négatif ou nul en pleine transition de topologie d'écran (valeurs
/// transitoires rapportées par Windows pendant un changement de résolution/
/// débranchement, voir `app::DISPLAY_CHANGE_DEBOUNCE`) — un `CreateWindowExW`/
/// `SetWindowPos` avec une taille négative échouerait ou produirait un
/// comportement indéfini.
// APPELANTS : passer par `primary_dock_bounds` ci-dessous, jamais ici
// directement — c'est elle qui porte la politique d'auto-masquage, et l'unique
// appel direct qui resterait la contournerait en silence.
fn dock_window_bounds(screen_w: i32, screen_h: i32, edge: TaskbarEdge, taskbar_thickness: i32) -> (i32, i32, i32, i32) {
    let (origin_x, origin_y, width, height) = match edge {
        TaskbarEdge::Bottom => (0, 0, screen_w, screen_h - taskbar_thickness),
        TaskbarEdge::Top => (0, taskbar_thickness, screen_w, screen_h - taskbar_thickness),
        TaskbarEdge::Left => (taskbar_thickness, 0, screen_w - taskbar_thickness, screen_h),
        TaskbarEdge::Right => (0, 0, screen_w - taskbar_thickness, screen_h),
    };
    (origin_x, origin_y, width.max(1), height.max(1))
}

/// Bornes de la fenêtre du dock, en tenant compte de l'état d'AUTO-MASQUAGE de
/// la vraie barre des tâches Windows — DEMANDE EXPLICITE : « quand la barre des
/// tâches est masquée, l'interface principale s'adapte à l'espace supplémentaire
/// disponible ».
///
/// **Le fait Windows qui rend la chose nécessaire** : `ABM_GETTASKBARPOS` (voir
/// `detect_taskbar`) ne tient PAS compte de l'auto-masquage et renvoie toujours
/// le rectangle de la barre DÉPLOYÉE, alors que la zone de travail, elle, va
/// bien jusqu'au bord de l'écran (`SPI_GETWORKAREA` « accounts for AUTOHIDE »).
/// Sans cette fonction, le dock s'arrête donc sur une bande que Windows ne
/// réserve plus — et que les fenêtres maximisées comme les icônes du bureau
/// utilisent déjà.
///
/// **Le bord BAS uniquement, et c'est une décision, pas un oubli.** La fenêtre du
/// dock occupe le bord où se trouve la barre des tâches (c'est tout l'objet de
/// `dock_window_bounds`). Sur un bord HAUT, la barre du haut de NeuraDesk couvre
/// toute la largeur — donc la TOTALITÉ de la bande de déclenchement, et une
/// barre des tâches masquée deviendrait inatteignable à la souris (seul
/// `Win`/`Win+T` la ramènerait). Même raisonnement pour les barres latérales sur
/// les bords gauche et droite, qui courent sur toute la hauteur. Le bord bas est
/// le seul où la bande survit : la région du dock est en « ⊓ », donc le bas
/// CENTRE reste libre — seuls les deux coins bas sont réclamés. Les tests
/// `primary_dock_bounds_ignores_a_hidden_{top,left,right}_taskbar` figent cette
/// exclusion : l'élargir un jour doit faire tomber un test, pas surprendre.
///
/// **Défaut de repli assumé** : si `taskbar_autohide::is_enabled` ne peut pas
/// lire l'état (`FindWindowW("Shell_TrayWnd")` échoue), il rend `false` et
/// l'épaisseur pleine est conservée — comportement d'avant. On ne sur-étend
/// jamais par erreur.
///
/// PURE (l'appelant fournit l'état d'auto-masquage, seul lui fait du Win32) :
/// testable sans écran ni fenêtre.
fn primary_dock_bounds(
    screen_w: i32,
    screen_h: i32,
    edge: TaskbarEdge,
    taskbar_thickness: i32,
    taskbar_auto_hide: bool,
) -> (i32, i32, i32, i32) {
    let reserved = if taskbar_auto_hide && edge == TaskbarEdge::Bottom { 0 } else { taskbar_thickness };
    dock_window_bounds(screen_w, screen_h, edge, reserved)
}

/// Détecte le bord d'ancrage et l'épaisseur (en pixels physiques) de la
/// barre des tâches Windows via `SHAppBarMessage`, quel que soit son bord.
/// Se replie sur la différence entre l'écran et la zone de travail
/// (`SPI_GETWORKAREA`, en supposant alors une barre en bas — cas par
/// défaut), puis sur une constante raisonnable si même cela échoue.
///
/// **Ignore DÉLIBÉRÉMENT l'auto-masquage** (contrairement à la zone de travail,
/// `ABM_GETTASKBARPOS` ne l'entre pas en compte) : c'est voulu, parce que la
/// valeur rendue ici a DEUX usages qui ne veulent pas la même chose. La décision
/// « cette barre réserve-t-elle de la place ? » appartient à
/// `primary_dock_bounds`, seul endroit qui consulte l'auto-masquage ; l'épaisseur
/// rendue ici reste, elle, l'épaisseur PHYSIQUE de la barre — c'est elle qui
/// donne l'épaisseur de départ des trois barres NeuraDesk à la création
/// (`create_dock`) et la référence d'épaisseur des écrans secondaires
/// (`app::rebuild_secondary_docks`). La zéroter ici donnerait des barres de 0 px
/// à toute installation neuve.
fn detect_taskbar() -> (TaskbarEdge, i32) {
    unsafe {
        let mut data = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            ..Default::default()
        };
        if SHAppBarMessage(ABM_GETTASKBARPOS, &mut data) != 0 {
            let edge = match data.uEdge {
                e if e == ABE_LEFT => TaskbarEdge::Left,
                e if e == ABE_RIGHT => TaskbarEdge::Right,
                e if e == ABE_TOP => TaskbarEdge::Top,
                _ => TaskbarEdge::Bottom,
            };
            let thickness = match edge {
                TaskbarEdge::Left | TaskbarEdge::Right => data.rc.right - data.rc.left,
                TaskbarEdge::Top | TaskbarEdge::Bottom => data.rc.bottom - data.rc.top,
            };
            if thickness > 0 {
                return (edge, thickness);
            }
        }

        let mut work_area = RECT::default();
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        if SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut work_area as *mut RECT as *mut _),
            Default::default(),
        )
        .is_ok()
        {
            let thickness = screen_h - work_area.bottom;
            if thickness > 0 {
                return (TaskbarEdge::Bottom, thickness);
            }
        }
    }

    (TaskbarEdge::Bottom, FALLBACK_BAR_THICKNESS)
}

/// Rectangles composant la région du dock : les quatre bandes en CADRE (« ⊓ »
/// refermé par la barre du BAS), PUIS la boîte de débordement de l'icône
/// magnifiée s'il y en a une.
///
/// ⚠️ **La barre du bas referme le « ⊓ » en cadre**, et elle prend les COINS,
/// comme celle du haut : les bandes latérales s'arrêtent donc au-dessus d'elle
/// (voir `ui::dock::bar_rects`, qui applique la même règle au dessin). À
/// `bottom == 0` — écran secondaire, ou barre du bas désactivée — les deux
/// rectangles latéraux valent `height`, soit exactement la géométrie d'avant.
///
/// Fonction PURE — aucun appel Windows, donc testable directement (voir les
/// tests `dock_region_rects_*` plus bas dans ce fichier).
///
/// **Le débordement est IGNORÉ quand toutes les barres sont masquées** (mode
/// masquage automatique, où les épaisseurs valent zéro). Sans cette
/// garde, la région deviendrait le débordement SEUL — une icône grossie
/// flottant sur le bureau, sans aucune barre. Le cas est atteignable : la
/// disposition des icônes suit les épaisseurs RÉSERVÉES et ne bouge donc pas
/// quand les barres se retirent, si bien que l'échelle de survol peut encore
/// être supérieure à 1 pendant l'animation de sortie du pointeur.
///
/// Le débordement est INTERSECTÉ avec la fenêtre : une boîte entièrement
/// dehors est écartée plutôt que posée vide (`CreateRectRgn` refuse un
/// rectangle dégénéré).
#[cfg(test)]
fn dock_region_rects(
    width: i32,
    height: i32,
    top: i32,
    left: i32,
    right: i32,
    bottom: i32,
    overflow: Option<[i32; 4]>,
) -> Vec<[i32; 4]> {
    dock_region_rects_entre(width, height, top, left, right, bottom, overflow, (left, right, top))
}

/// `dock_region_rects`, mais avec les épaisseurs RÉGLÉES `reglees` (gauche,
/// droite, haut) au lieu des seules épaisseurs VISIBLES : la bande du bas
/// s'arrête aux colonnes réglées, et les colonnes commencent sous la barre du
/// haut réglée — que ces barres soient masquées ou non.
///
/// ⛔ **SECOND DÉFAUT SIGNALÉ, LE MÊME EN HAUT (2026-10-03)** : « les barres de
/// droite et gauche affichent le contenu de la barre du haut quand la barre du
/// haut est cachée ». Les colonnes étaient découpées depuis `y = 0` : le coin
/// supérieur, où la barre du haut est toujours DESSINÉE, restait donc visible.
///
/// ⛔ **DÉFAUT SIGNALÉ (2026-10-03)** : « quand les barres de droite et de gauche
/// sont masquées je vois leur contenu dans la barre du bas ». Masquer une barre
/// latérale ne retire que sa colonne de la RÉGION (`left`/`right` à 0) : le dock,
/// lui, la dessine toujours à sa largeur réglée, sur toute la hauteur. Une bande
/// du bas calculée entre colonnes NULLES s'étendait donc d'un bord à l'autre de
/// l'écran, et laissait voir le pied des colonnes masquées, icônes comprises.
#[allow(clippy::too_many_arguments)]
fn dock_region_rects_entre(
    width: i32,
    height: i32,
    top: i32,
    left: i32,
    right: i32,
    bottom: i32,
    overflow: Option<[i32; 4]>,
    reglees: (i32, i32, i32),
) -> Vec<[i32; 4]> {
    let (gauche_reglee, droite_reglee, haut_regle) = reglees;
    // ⚠️ **LA BARRE DU BAS OCCUPE LE MILIEU, PAS LES COINS** (voir
    // `ui::dock::bar_rects`, qui porte la décision et le pourquoi) : les bandes
    // latérales gardent donc TOUTE la hauteur, et la bande du bas se pose entre
    // elles. À `bottom == 0` le quatrième rectangle est dégénéré et les trois
    // autres valent exactement la géométrie d'avant.
    let mut rects = vec![
        [0, 0, width, top],
        [0, haut_regle.min(height), left, height],
        [width - right, haut_regle.min(height), width, height],
        // ⭐ **SOUS UNE COLONNE MASQUÉE, LA BANDE DU BAS VA JUSQU'AU BORD** —
        // DEMANDE EXPLICITE (2026-10-14) : « que l'espace créé soit remplacé par un
        // morceau de barre ». Le pied de la colonne (toujours dessinée, icônes
        // comprises) n'est plus montré tel quel : `app::run` peint par-dessus un
        // BOUT de barre du bas (voir `ui::dock::bouts_de_barre_du_bas`).
        [
            if left == 0 { 0 } else { left.max(gauche_reglee) },
            height - bottom,
            if right == 0 { width } else { width - right.max(droite_reglee) },
            height,
        ],
    ];
    if top > 0 || left > 0 || right > 0 || bottom > 0 {
        if let Some([x0, y0, x1, y1]) = overflow {
            let (x0, y0) = (x0.max(0), y0.max(0));
            let (x1, y1) = (x1.min(width), y1.min(height));
            if x1 > x0 && y1 > y0 {
                rects.push([x0, y0, x1, y1]);
            }
        }
    }
    rects
}

/// Découpe la fenêtre en "⊓" : union d'une bande horizontale pleine largeur
/// (le haut, épaisseur `top`) et de deux bandes verticales (gauche `left`,
/// droite `right` — INDÉPENDANTES depuis le panneau de réglages, demande
/// explicite), PLUS la boîte de débordement de l'icône magnifiée (voir
/// `dock_region_rects`). Le reste de la fenêtre devient invisible et laisse
/// passer les clics/le rendu du bureau. Rappelée par `Window::
/// set_dock_thicknesses` à chaque changement, pas seulement à la création.
///
/// **Élargir la région est la SEULE façon de laisser une icône déborder de sa
/// barre.** Le dépôt a mesuré le mur : tout ce qui est dessiné dans la zone
/// libre y est rogné par Windows « quel que soit le contenu du back buffer »
/// (voir `win32::tooltip`, doc de module — c'est pour cette raison exacte que
/// l'infobulle est une fenêtre à part). Contrepartie assumée, à connaître : la
/// région **EST** la zone de clic du dock (son `wndproc` ne traite aucun
/// `WM_NCHITTEST`, et la fenêtre n'a pas `WS_EX_TRANSPARENT`), donc pendant le
/// survol, un clic dans la boîte de débordement est délivré au dock au lieu de
/// l'application derrière.
unsafe fn set_dock_region(
    hwnd: HWND,
    width: i32,
    height: i32,
    top: i32,
    left: i32,
    right: i32,
    bottom: i32,
    overflow: Option<[i32; 4]>,
    reglees: (i32, i32, i32),
    rayon_conge: i32,
) {
    let combined = CreateRectRgn(0, 0, 0, 0);
    for rect in dock_region_rects_entre(width, height, top, left, right, bottom, overflow, reglees) {
        let piece = CreateRectRgn(rect[0], rect[1], rect[2], rect[3]);
        CombineRgn(Some(combined), Some(combined), Some(piece), RGN_OR);
        // `Take`-like : `CombineRgn` ne consomme pas la source, elle reste
        // nôtre et doit être libérée (même règle que les barres ci-dessus).
        let _ = DeleteObject(piece.into());
    }
    // LES CONGÉS de la zone libre (voir `ui::dock::conges`), aux épaisseurs
    // VISIBLES : une barre masquée emporte les siens. Chacun est son carré MOINS
    // le disque centré sur son coin intérieur. ⚠️ La région est CRÉNELÉE (GDI
    // n'a pas d'anticrénelage) : c'est le dessin, anticrénelé, qui lisse le bord.
    let epaisseurs = [top, left, right, bottom].map(|e| e as f32);
    for conge in crate::ui::dock::conges(width as f32, height as f32, epaisseurs, rayon_conge as f32) {
        let [x, y, cote, _] = conge.rect.map(|v| v.round() as i32);
        let (cx, cy) = (x + if conge.vers[0] > 0.0 { cote } else { 0 }, y + if conge.vers[1] > 0.0 { cote } else { 0 });
        let carre = CreateRectRgn(x, y, x + cote, y + cote);
        let disque = CreateEllipticRgn(cx - cote, cy - cote, cx + cote, cy + cote);
        CombineRgn(Some(carre), Some(carre), Some(disque), RGN_DIFF);
        CombineRgn(Some(combined), Some(combined), Some(carre), RGN_OR);
        let _ = DeleteObject(carre.into());
        let _ = DeleteObject(disque.into());
    }
    // SetWindowRgn prend possession de `combined` ; les régions sources
    // restent nôtres et doivent être libérées explicitement.
    let _ = SetWindowRgn(hwnd, Some(combined), true);
}

/// Une fenêtre **visible, devant `hwnd`, qui le recouvre** existe-t-elle — sans
/// lui appartenir ?
///
/// Sert à ne PAS remonter une fenêtre déjà en place (voir
/// `Window::raise_to_top` et `ombre_barre::rangement_magnifie`) : remonter est
/// un vrai `SetWindowPos`, qui réinsère aussi les fenêtres POSSÉDÉES et fait
/// recomposer leur contenu — les coins noirs des écrans clignotaient pour cette
/// raison, toutes les 250 ms, pendant un survol.
///
/// On IGNORE donc les fenêtres possédées par `hwnd` (elles sont au-dessus de lui
/// par construction, sans le recouvrir « par erreur »), celles qui FLOTTENT
/// au-dessus de la bande (voir `flotte_au_dessus` : coins, infobulles, IME) et
/// celles dont le rectangle ne le recouvre pas (un dock d'un autre écran, une
/// fenêtre ailleurs). Géométrie illisible → `true` : on préfère l'ancien
/// comportement — remonter pour rien — à une fenêtre qui resterait sous une
/// autre.
pub(crate) fn un_voisin_visible_devant(hwnd: HWND) -> bool {
    let mut mien = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut mien) }.is_err() {
        return true;
    }
    let mut courante = unsafe { GetWindow(hwnd, GW_HWNDPREV) }.ok();
    while let Some(voisine) = courante {
        let possedee = unsafe { GetWindow(voisine, GW_OWNER) }.ok() == Some(hwnd);
        if !possedee && !flotte_au_dessus(voisine) && unsafe { IsWindowVisible(voisine) }.as_bool() {
            let mut sien = RECT::default();
            if unsafe { GetWindowRect(voisine, &mut sien) }.is_ok()
                && sien.left < mien.right
                && sien.right > mien.left
                && sien.top < mien.bottom
                && sien.bottom > mien.top
            {
                return true;
            }
        }
        courante = unsafe { GetWindow(voisine, GW_HWNDPREV) }.ok();
    }
    false
}

/// Une fenêtre qui FLOTTE au-dessus de la bande et ne justifie jamais de la
/// remonter : les coins noirs des écrans (posés sur les angles des barres, et
/// tous possédés par le dock du haut — un dock SECONDAIRE les compte donc
/// « étrangers »), les infobulles (elles se réaffirment elles-mêmes — voir
/// `TooltipWindow::show`), notre horloge plein écran (une instance par écran,
/// qui se pose elle-même), la fenêtre **IME** du fil, que Windows garde au
/// sommet de son propre chef, le **`ThumbnailDeviceHelperWnd`** de
/// l'explorateur — un carré de 1 px en (0, 0) qui recouvre le dock du haut — et
/// `ForegroundStaging`, la fenêtre d'attente de Windows pendant un changement
/// de premier plan. Aucune ne montre jamais de contenu à recouvrir.
///
/// ⛔ **SANS CETTE LISTE, LE DOCK SE REMONTE SANS ARRÊT** — MESURÉ le
/// 2026-11-13 à la sonde d'ordre Z, pendant un survol maintenu : 169 remontées
/// du dock secondaire, 172 fois parce que le voisin du dessus était un
/// `NdCoinEcran`, puis 330 remontées du dock principal pour le carré de
/// l'explorateur. Chaque remontée réinsère les coins POSSÉDÉS du dock, et DWM
/// les recompose : le clignotement signalé.
pub(crate) fn flotte_au_dessus(hwnd: HWND) -> bool {
    classe_flottante(&classe_de(hwnd), true)
}

/// Comme `flotte_au_dessus`, mais les COINS d'écran comptent comme des fenêtres
/// ordinaires. Sert à `coins_ecran::cible_de_remontee` : deux coins ne se
/// recouvrent jamais en vrai (chacun est sur un angle), et les compter comme
/// « flottants » là-bas rendrait le recouvrement indétectable.
pub(crate) fn flotte_au_dessus_hors_coins(hwnd: HWND) -> bool {
    classe_flottante(&classe_de(hwnd), false)
}

fn classe_flottante(classe: &str, coins_compris: bool) -> bool {
    match classe {
        "NdCoinEcran" => coins_compris,
        "NeuraDeskTooltipWindow" | "NeuraDeskFullscreenClockWindow" | "IME" | "MSCTFIME UI" | "ThumbnailDeviceHelperWnd" | "ForegroundStaging" => true,
        _ => false,
    }
}

#[inline]
fn loword(l: LPARAM) -> u16 {
    (l.0 & 0xFFFF) as u16
}

#[inline]
fn hiword(l: LPARAM) -> u16 {
    ((l.0 >> 16) & 0xFFFF) as u16
}

fn push_event(event: Event) {
    EVENTS.with(|q| q.borrow_mut().push_back(event));
}

/// Demande un recalage de la fenêtre du dock sur l'espace réellement disponible
/// (voir `Event::PrimaryDockResync`, qui porte le pourquoi) — appelée par le
/// panneau de réglages quand la case « Masquer automatiquement la barre des
/// tâches » est basculée, puisqu'aucune autre information ne dit si
/// `ABM_SETSTATE` a bien pris.
///
/// Réutilise la file `EVENTS` que `app::run` draine à chaque frame : pas de
/// nouvel état global à tenir, et rien à faire hors du fil d'interface.
pub fn request_primary_dock_resync() {
    push_event(Event::PrimaryDockResync);
}

/// Filtre `WM_SETTINGCHANGE` (voir `wndproc`) sur SEULEMENT le cas où la
/// VRAIE barre des tâches Windows vient de bouger/changer d'épaisseur SANS
/// changement de résolution (`wParam == SPI_SETWORKAREA`) — ce message
/// générique arrive en continu pour des dizaines de réglages système sans
/// aucun rapport (thème, police, DPI...) ; le pousser sans filtrer noierait
/// `app::run` sous des recalculs de topologie d'écran inutiles. Extraite en
/// fonction PURE (plutôt que testée en ligne dans le bras `match` de
/// `wndproc`, qui ne peut pas s'invoquer hors d'un vrai message Windows)
/// pour rester unitairement testable.
fn is_workarea_setting_change(wparam: u32) -> bool {
    wparam == SPI_SETWORKAREA.0
}

/// ⭐ **LE MESSAGE PAR LEQUEL `window-hook` ANNONCE UN GESTE RETENU.**
///
/// Enregistré des DEUX côtés par le même NOM, jamais par un numéro : un
/// `WM_APP + n` en dur entrerait en collision avec celui d'une autre
/// application, alors que les messages enregistrés sont uniques pour la session.
///
/// Disposition des mots :
/// - `wParam` : la fenêtre de l'application (HWND) ;
/// - `lParam` : `geste | (retenue_ms << 8)`. Gestes : `1` fermer, `2` réduire,
///   `3` agrandir, `4` restaurer (voir `ouverture_fenetres::Geste`).
///
/// C'est ce qui évite tout `WM_COPYDATA` (donc tout envoi bloquant depuis le
/// fil de l'application, qui attendrait notre réponse).
pub fn message_de_geste() -> u32 {
    static MESSAGE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe {
        windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW(windows::core::w!(
            "NeuraDesk.GesteFenetre"
        ))
    })
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => {
            TRACKING_LEAVE.with(|t| {
                if !*t.borrow() {
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut tme);
                    *t.borrow_mut() = true;
                }
            });
            push_event(Event::MouseMove {
                x: loword(lparam) as f32,
                y: hiword(lparam) as f32,
                hwnd,
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            TRACKING_LEAVE.with(|t| *t.borrow_mut() = false);
            push_event(Event::MouseLeave { hwnd });
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            // Défilement des icônes d'une barre (voir `Dock::scroll_bar`).
            //
            // DEUX particularités de ce message par rapport à tous les autres
            // messages souris traités ici :
            // - `lparam` porte des coordonnées ÉCRAN, pas client — d'où la
            //   conversion, les autres donnant déjà du client ;
            // - le delta est dans `wparam` (mot de POIDS FORT), en multiples
            //   de `WHEEL_DELTA` (120), positif vers l'avant (le haut).
            //
            // Reçu SANS que la fenêtre ait le focus : le dock est
            // `WS_EX_NOACTIVATE` et ne le prend jamais (voir `create_dock`),
            // c'est le réglage Windows « faire défiler les fenêtres inactives
            // au survol » qui route le message vers la fenêtre SOUS LE
            // CURSEUR.
            let mut point = POINT { x: loword(lparam) as i32, y: hiword(lparam) as i32 };
            unsafe {
                let _ = ScreenToClient(hwnd, &mut point);
            }
            let delta = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16 as f32;
            push_event(Event::MouseWheel { delta, x: point.x as f32, y: point.y as f32, hwnd });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            // `SetCapture` (relâchée sur `WM_LBUTTONUP` ci-dessous) — voir
            // `ui::dock::Dock::begin_potential_drag`/`end_drag` : sans elle,
            // glisser une icône puis relâcher le bouton APRÈS avoir quitté
            // les limites de la fenêtre ne délivrerait plus jamais le
            // `WM_LBUTTONUP` correspondant à CETTE fenêtre, laissant le
            // glisser bloqué indéfiniment (icône restée "flottante", ordre
            // des barres jamais figé). Standard Win32 pour tout
            // glisser-déposer, sans effet de bord sur un simple clic
            // (relâché immediatamente, cattura liberata immediatamente).
            unsafe {
                SetCapture(hwnd);
            }
            push_event(Event::MouseDown {
                x: loword(lparam) as f32,
                y: hiword(lparam) as f32,
                hwnd,
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            unsafe {
                let _ = ReleaseCapture();
            }
            push_event(Event::MouseUp {
                x: loword(lparam) as f32,
                y: hiword(lparam) as f32,
                hwnd,
            });
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            push_event(Event::RightClick {
                x: loword(lparam) as f32,
                y: hiword(lparam) as f32,
                hwnd,
            });
            LRESULT(0)
        }
        WM_MBUTTONDOWN => {
            push_event(Event::MiddleClick {
                x: loword(lparam) as f32,
                y: hiword(lparam) as f32,
                hwnd,
            });
            LRESULT(0)
        }
        WM_SIZE => {
            push_event(Event::Resized {
                width: loword(lparam) as u32,
                height: hiword(lparam) as u32,
            });
            LRESULT(0)
        }
        // On dessine tout nous-mêmes en Direct3D : on court-circuite l'effacement
        // GDI du fond pour éviter un flash blanc/noir à chaque frame.
        WM_ERASEBKGND => LRESULT(1),
        // Voir `AddClipboardFormatListener` dans `create_dock` — lit le
        // contenu (texte OU image, voir `win32::clipboard::read`) tout de
        // suite (l'API presse-papiers ne redéclenche pas de message vers
        // nous-mêmes, sans risque de ré-entrance) plutôt que de le laisser
        // à `app::run`, pour rester cohérent avec le reste de `push_event`.
        WM_CLIPBOARDUPDATE => {
            // ⭐ Un contenu marqué « hors historique » (mot de passe de KeePass,
            // de notre générateur) n'y entre pas — voir `clipboard::write_secret`.
            if crate::win32::clipboard::exclu_de_l_historique() {
                return LRESULT(0);
            }
            if let Some(entry) = crate::win32::clipboard::read() {
                push_event(Event::ClipboardChanged(entry));
            }
            LRESULT(0)
        }
        // Topologie/résolution de l'écran principal changée (voir la doc de
        // `Event::DisplaysChanged`) — se contente de POUSSER l'événement,
        // AUCUNE manipulation de fenêtre/swap chain ici : `SetWindowPos`
        // pendant le traitement d'un message a déjà causé une réentrance
        // `RefCell` par le passé dans ce projet (voir les commentaires de
        // `win32::cef_popup`, l'autre module ayant déjà rencontré ce bug —
        // l'ancien module `win32::launcher` retiré depuis). Le vrai recalcul de
        // géométrie (voir `Window::resync_to_primary_monitor`) est fait
        // plus tard par `app::run`, en dehors de `wndproc`, une fois la
        // rafale de messages retombée (voir `app::DISPLAY_CHANGE_DEBOUNCE`).
        WM_DISPLAYCHANGE => {
            push_event(Event::DisplaysChanged);
            LRESULT(0)
        }
        // Voir `is_workarea_setting_change` : seul `SPI_SETWORKAREA` (la
        // vraie barre des tâches a bougé/changé d'épaisseur) déclenche un
        // recalcul de topologie ici — les autres réglages système (`WM_
        // SETTINGCHANGE` couvre des dizaines de cas sans rapport) sont
        // ignorés.
        WM_SETTINGCHANGE => {
            if is_workarea_setting_change(wparam.0 as u32) {
                push_event(Event::DisplaysChanged);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            push_event(Event::CloseRequested);
            PostQuitMessage(0);
            LRESULT(0)
        }
        // ── LE MODULE INJECTÉ (`window-hook`) NOUS ANNONCE UN GESTE RETENU ──
        // Il retient `WM_CLOSE` / `SC_MINIMIZE` dans l'application, et nous
        // prévient ICI pour qu'on joue l'effet PENDANT la retenue — sinon la
        // fenêtre ne disparaîtrait qu'à la libération, l'effet arrivant après
        // coup. Voir `message_de_geste` pour la disposition des deux mots.
        msg if msg == message_de_geste() => {
            let application = wparam.0 as isize;
            let geste = (lparam.0 & 0xFF) as u32;
            let retenue = (lparam.0 >> 8) as u64;
            crate::diag!("[geste] application {application:#x}, geste {geste}, retenue {retenue} ms");
            crate::win32::ouverture_fenetres::geste_retenu(
                windows::Win32::Foundation::HWND(application as *mut std::ffi::c_void),
                geste,
                retenue,
            );
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les barres doivent se présenter au shell comme un programme NORMAL :
    /// c'est ce que `WS_EX_APPWINDOW` demande et ce que `WS_EX_TOOLWINDOW`
    /// interdit. Les quatre bits sont verrouillés ENSEMBLE — les trois autres
    /// portent aussi du sens, et `NOACTIVATE` est justement la RAISON d'être
    /// d'`APPWINDOW` ici (voir la doc de la constante : sans lui, une fenêtre
    /// `NOACTIVATE` n'apparaît ni dans Alt-Tab ni dans la barre des tâches, quel
    /// que soit le reste).
    #[test]
    fn the_dock_is_a_normal_shell_window() {
        let style = DOCK_EX_STYLE.0;
        assert_ne!(
            style & WS_EX_APPWINDOW.0, 0,
            "sans APPWINDOW, une barre NOACTIVATE reste absente d'Alt-Tab et de la barre des tâches"
        );
        assert_eq!(
            style & WS_EX_TOOLWINDOW.0, 0,
            "TOOLWINDOW exclut explicitement de la barre des tâches ET du sélecteur Alt-Tab"
        );
        assert_ne!(style & WS_EX_NOACTIVATE.0, 0, "une barre ne doit jamais voler le focus");
        assert_ne!(style & WS_EX_TOPMOST.0, 0, "une barre reste au-dessus de tout");
    }

    /// `NOACTIVATE` ne protège que du CLIC : montrer une barre par
    /// `SW_SHOWNORMAL` l'active quand même — et une barre secondaire recréée par
    /// un réglage refermait ainsi le panneau Réglages (2026-11-09).
    #[test]
    fn aucune_barre_n_est_montree_en_l_activant() {
        let source = include_str!("window.rs");
        let interdit = ["ShowWindow(hwnd, SW_SHOW", "NORMAL)"].concat();
        assert!(!source.contains(&interdit), "une barre montrée par SW_SHOWNORMAL vole le premier plan");
    }

    /// `apply_exe_icon` pose l'icône aux DEUX emplacements que le shell
    /// interroge, chacun avec SA dimension système. Les intervertir donnerait une
    /// icône minuscule étirée dans Alt-Tab et un pavé illisible dans la barre des
    /// tâches : c'est ce que ce test empêche.
    #[test]
    fn the_two_window_icon_slots_keep_their_own_metric() {
        let slots = exe_icon_slots();
        assert_eq!(slots[0].0, ICON_SMALL, "la barre des taches interroge ICON_SMALL");
        assert_eq!(slots[0].1, SM_CXSMICON);
        assert_eq!(slots[0].2, SM_CYSMICON);
        assert_eq!(slots[1].0, ICON_BIG, "Alt-Tab affiche ICON_BIG");
        assert_eq!(slots[1].1, SM_CXICON);
        assert_eq!(slots[1].2, SM_CYICON);
        assert_ne!(
            SM_CXSMICON, SM_CXICON,
            "les deux emplacements ne peuvent pas partager une dimension"
        );
    }

    // `dock_window_bounds` — voir sa doc : fonction pure, testable sans
    // écran/fenêtre réels. Écran factice 1920x1080, barre des tâches de
    // 48px (comme `FALLBACK_BAR_THICKNESS`) sur chacun des 4 bords.
    const SCREEN_W: i32 = 1920;
    const SCREEN_H: i32 = 1080;
    const TASKBAR_THICKNESS: i32 = 48;

    #[test]
    fn dock_window_bounds_avoids_a_bottom_taskbar() {
        let bounds = dock_window_bounds(SCREEN_W, SCREEN_H, TaskbarEdge::Bottom, TASKBAR_THICKNESS);
        assert_eq!(bounds, (0, 0, SCREEN_W, SCREEN_H - TASKBAR_THICKNESS));
    }

    #[test]
    fn dock_window_bounds_avoids_a_top_taskbar() {
        let bounds = dock_window_bounds(SCREEN_W, SCREEN_H, TaskbarEdge::Top, TASKBAR_THICKNESS);
        assert_eq!(bounds, (0, TASKBAR_THICKNESS, SCREEN_W, SCREEN_H - TASKBAR_THICKNESS));
    }

    #[test]
    fn dock_window_bounds_avoids_a_left_taskbar() {
        let bounds = dock_window_bounds(SCREEN_W, SCREEN_H, TaskbarEdge::Left, TASKBAR_THICKNESS);
        assert_eq!(bounds, (TASKBAR_THICKNESS, 0, SCREEN_W - TASKBAR_THICKNESS, SCREEN_H));
    }

    #[test]
    fn dock_window_bounds_avoids_a_right_taskbar() {
        let bounds = dock_window_bounds(SCREEN_W, SCREEN_H, TaskbarEdge::Right, TASKBAR_THICKNESS);
        assert_eq!(bounds, (0, 0, SCREEN_W - TASKBAR_THICKNESS, SCREEN_H));
    }

    // Valeurs transitoires possibles EN PLEINE transition de topologie
    // d'écran (voir la doc de `dock_window_bounds`) : la taskbar rapportée
    // est plus épaisse que l'écran lui-même n'est haut/large. La largeur/
    // hauteur résultante ne doit jamais tomber à zéro ou en dessous.
    #[test]
    fn dock_window_bounds_clamps_a_taskbar_thicker_than_the_screen() {
        let bounds = dock_window_bounds(SCREEN_W, SCREEN_H, TaskbarEdge::Bottom, SCREEN_H + 100);
        assert_eq!(bounds, (0, 0, SCREEN_W, 1));

        let bounds = dock_window_bounds(SCREEN_W, SCREEN_H, TaskbarEdge::Left, SCREEN_W + 100);
        assert_eq!(bounds, (SCREEN_W + 100, 0, 1, SCREEN_H));
    }

    // `primary_dock_bounds` — la POLITIQUE d'auto-masquage, par-dessus
    // `dock_window_bounds` ci-dessus (dont les cinq tests restent valides tels
    // quels : c'est tout l'intérêt d'avoir séparé les deux). Même écran factice.

    /// Une barre en bas en auto-masquage ne réserve plus RIEN : la fenêtre
    /// descend jusqu'au bord. C'est la demande explicite.
    #[test]
    fn primary_dock_bounds_ignores_a_hidden_bottom_taskbar() {
        let bounds = primary_dock_bounds(SCREEN_W, SCREEN_H, TaskbarEdge::Bottom, TASKBAR_THICKNESS, true);
        assert_eq!(bounds, (0, 0, SCREEN_W, SCREEN_H));
    }

    /// Les trois autres bords sont DÉLIBÉRÉMENT exclus (voir la doc de
    /// `primary_dock_bounds` : la barre du dock y couvrirait toute la bande de
    /// déclenchement de la barre des tâches, qui deviendrait inatteignable à la
    /// souris). Ce test fige l'exclusion : l'élargir un jour doit le faire
    /// tomber, pas passer inaperçu.
    #[test]
    fn primary_dock_bounds_ignores_a_hidden_top_taskbar() {
        let bounds = primary_dock_bounds(SCREEN_W, SCREEN_H, TaskbarEdge::Top, TASKBAR_THICKNESS, true);
        assert_eq!(bounds, dock_window_bounds(SCREEN_W, SCREEN_H, TaskbarEdge::Top, TASKBAR_THICKNESS));
    }

    #[test]
    fn primary_dock_bounds_ignores_a_hidden_left_taskbar() {
        let bounds = primary_dock_bounds(SCREEN_W, SCREEN_H, TaskbarEdge::Left, TASKBAR_THICKNESS, true);
        assert_eq!(bounds, dock_window_bounds(SCREEN_W, SCREEN_H, TaskbarEdge::Left, TASKBAR_THICKNESS));
    }

    #[test]
    fn primary_dock_bounds_ignores_a_hidden_right_taskbar() {
        let bounds = primary_dock_bounds(SCREEN_W, SCREEN_H, TaskbarEdge::Right, TASKBAR_THICKNESS, true);
        assert_eq!(bounds, dock_window_bounds(SCREEN_W, SCREEN_H, TaskbarEdge::Right, TASKBAR_THICKNESS));
    }

    /// **Sans auto-masquage, rien ne change** — la forme exécutable de « les
    /// cinq tests de `dock_window_bounds` tiennent toujours », et elle survit à
    /// une modification de l'une ou l'autre des deux fonctions, contrairement au
    /// fait de les laisser simplement en place.
    #[test]
    fn primary_dock_bounds_without_auto_hide_delegates_to_dock_window_bounds() {
        for edge in [TaskbarEdge::Top, TaskbarEdge::Bottom, TaskbarEdge::Left, TaskbarEdge::Right] {
            assert_eq!(
                primary_dock_bounds(SCREEN_W, SCREEN_H, edge, TASKBAR_THICKNESS, false),
                dock_window_bounds(SCREEN_W, SCREEN_H, edge, TASKBAR_THICKNESS),
                "bord {edge:?} : l'auto-masquage désactivé doit rendre exactement l'ancien calcul"
            );
        }
    }

    /// Le garde-fou de taille (voir la doc de `dock_window_bounds`) doit valoir
    /// dans les DEUX états — l'auto-masquage court-circuite la soustraction, mais
    /// pas la borne, qui protège d'une valeur transitoire de changement de
    /// topologie d'écran.
    #[test]
    fn primary_dock_bounds_still_clamps_a_taskbar_thicker_than_the_screen() {
        // Sans auto-masquage : exactement le garde-fou de `dock_window_bounds`.
        let bounds = primary_dock_bounds(SCREEN_W, SCREEN_H, TaskbarEdge::Bottom, SCREEN_H + 100, false);
        assert_eq!(bounds, (0, 0, SCREEN_W, 1));

        // Avec l'auto-masquage sur le bord bas, l'épaisseur n'est plus soustraite
        // du tout : il n'y a donc plus rien à borner, et la fenêtre fait l'écran.
        let bounds = primary_dock_bounds(SCREEN_W, SCREEN_H, TaskbarEdge::Bottom, SCREEN_H + 100, true);
        assert_eq!(bounds, (0, 0, SCREEN_W, SCREEN_H));

        // Sur un bord EXCLU, en revanche, le garde-fou reste seul rempart.
        let bounds = primary_dock_bounds(SCREEN_W, SCREEN_H, TaskbarEdge::Left, SCREEN_W + 100, true);
        assert_eq!(bounds, (SCREEN_W + 100, 0, 1, SCREEN_H));
    }

    /// Épaisseur 0 (aucune barre des tâches détectée) : l'auto-masquage ne
    /// change rien, et surtout ne doit pas produire de taille nulle ou négative.
    #[test]
    fn primary_dock_bounds_with_a_zero_thickness_is_a_no_op() {
        for auto_hide in [true, false] {
            assert_eq!(
                primary_dock_bounds(SCREEN_W, SCREEN_H, TaskbarEdge::Bottom, 0, auto_hide),
                (0, 0, SCREEN_W, SCREEN_H),
                "auto-masquage {auto_hide} : épaisseur nulle = écran entier"
            );
        }
    }

    // `dock_region_rects` — voir sa doc : fonction pure, testable sans
    // écran/fenêtre réels. Elle compose la forme en « ⊓ » ET le débordement de
    // l'icône magnifiée.
    const REGION_W: i32 = 3840;
    const REGION_H: i32 = 2088;
    const REGION_TOP: i32 = 72;
    const REGION_SIDE: i32 = 140;
    /// Épaisseur de la barre du BAS dans ces tests — non nulle, sinon le cas
    /// intéressant (le cadre refermé) ne serait jamais éprouvé.
    const REGION_BOTTOM: i32 = 104;

    /// Sans rien de magnifié, la région est EXACTEMENT les quatre barres — un
    /// CADRE, et non plus un « ⊓ » : non-régression, le débordement ne doit rien
    /// changer au cas courant.
    #[test]
    fn dock_region_rects_without_overflow_is_the_four_bars() {
        let rects = dock_region_rects(
            REGION_W,
            REGION_H,
            REGION_TOP,
            REGION_SIDE,
            REGION_SIDE,
            REGION_BOTTOM,
            None,
        );
        assert_eq!(
            rects,
            vec![
                [0, 0, REGION_W, REGION_TOP],
                // Les colonnes commencent SOUS la barre du haut : l'union est
                // la même tant qu'elle est visible, et elles ne montrent pas son
                // coin quand elle est masquée.
                [0, REGION_TOP, REGION_SIDE, REGION_H],
                [REGION_W - REGION_SIDE, REGION_TOP, REGION_W, REGION_H],
                [REGION_SIDE, REGION_H - REGION_BOTTOM, REGION_W - REGION_SIDE, REGION_H],
            ]
        );
    }

    /// **LA BARRE DU BAS OCCUPE LE MILIEU, PAS LES COINS.**
    ///
    /// ⛔ **CE TEST FIGE UNE DÉCISION QUE J'AVAIS PRISE À L'ENVERS.** J'avais fait
    /// prendre les coins au bas, par symétrie avec le haut. L'usage a tranché
    /// contre : l'utilisateur a vu le premier raccourci « décalé à gauche sur la
    /// barre de gauche ». La symétrie était un faux raisonnement — la barre du
    /// haut prend les coins parce que RIEN d'autre n'y vit, alors que les colonnes
    /// latérales descendent jusqu'en bas de l'écran.
    #[test]
    fn la_barre_du_bas_occupe_le_milieu() {
        let rects = dock_region_rects(
            REGION_W,
            REGION_H,
            REGION_TOP,
            REGION_SIDE,
            REGION_SIDE,
            REGION_BOTTOM,
            None,
        );
        // Les colonnes latérales gardent TOUTE la hauteur...
        assert_eq!(rects[1][3], REGION_H, "bande de gauche");
        assert_eq!(rects[2][3], REGION_H, "bande de droite");
        // ...et le bas vit ENTRE elles, sans toucher aux coins.
        assert_eq!(rects[3][0], REGION_SIDE, "le bas commence après la colonne gauche");
        assert_eq!(
            rects[3][2],
            REGION_W - REGION_SIDE,
            "et s'arrête avant la colonne droite"
        );
        assert_eq!(rects[3][1], REGION_H - REGION_BOTTOM);
        assert_eq!(rects[3][3], REGION_H);
    }

    /// Barres latérales MASQUÉES (colonnes visibles à 0) : la bande du bas va
    /// jusqu'aux bords de l'écran, où un BOUT de barre est peint par-dessus le pied
    /// des colonnes (voir `ui::dock::bouts_de_barre_du_bas`). DEMANDE EXPLICITE du
    /// 2026-10-14 ; avant, elle s'arrêtait aux colonnes RÉGLÉES et laissait un trou.
    #[test]
    fn la_barre_du_bas_s_etend_sous_les_colonnes_masquees() {
        let rects = dock_region_rects_entre(
            REGION_W,
            REGION_H,
            REGION_TOP,
            0,
            0,
            REGION_BOTTOM,
            None,
            (REGION_SIDE, REGION_SIDE, REGION_TOP),
        );
        assert_eq!((rects[1][2], rects[2][0]), (0, REGION_W), "les deux colonnes sont retirées");
        assert_eq!(rects[3][0], 0, "le bas part du bord gauche");
        assert_eq!(rects[3][2], REGION_W, "et va jusqu'au bord droit");
    }

    /// Le même défaut en haut : barre du haut MASQUÉE (épaisseur visible 0), les
    /// colonnes commencent sous sa hauteur RÉGLÉE — elles ne montrent pas son coin.
    #[test]
    fn les_colonnes_ne_montrent_pas_la_barre_du_haut_masquee() {
        let rects =
            dock_region_rects_entre(REGION_W, REGION_H, 0, REGION_SIDE, REGION_SIDE, 0, None, (REGION_SIDE, REGION_SIDE, REGION_TOP));
        assert_eq!(rects[0][3], 0, "la bande du haut est retirée");
        assert_eq!((rects[1][1], rects[2][1]), (REGION_TOP, REGION_TOP), "les colonnes commencent sous elle");
    }

    /// **LA GARANTIE DE RÉTROCOMPATIBILITÉ** : à `bottom == 0` — un écran
    /// SECONDAIRE, ou la barre du bas désactivée — la géométrie est EXACTEMENT
    /// celle d'avant qu'elle existe. C'est ce qui permet d'ajouter une barre sans
    /// toucher à ce qui marchait, et c'est la première chose qu'un changement de
    /// formule casserait sans le dire.
    #[test]
    fn une_barre_du_bas_nulle_redonne_la_geometrie_d_avant() {
        let avec = dock_region_rects(REGION_W, REGION_H, REGION_TOP, REGION_SIDE, REGION_SIDE, 0, None);
        assert_eq!(
            avec,
            vec![
                [0, 0, REGION_W, REGION_TOP],
                // Les colonnes commencent SOUS la barre du haut : l'union est
                // la même tant qu'elle est visible, et elles ne montrent pas son
                // coin quand elle est masquée.
                [0, REGION_TOP, REGION_SIDE, REGION_H],
                [REGION_W - REGION_SIDE, REGION_TOP, REGION_W, REGION_H],
                // Le quatrième rectangle est DÉGÉNÉRÉ (hauteur nulle, et étroit
                // d'autant) : il ne peint rien et `CreateRectRgn` l'accepte, mais
                // il est bien là — quatre bandes, dont une vide, plutôt qu'une
                // arité variable.
                [REGION_SIDE, REGION_H, REGION_W - REGION_SIDE, REGION_H],
            ]
        );
    }

    /// Une icône magnifiée débordant de sa barre ajoute SON rectangle : c'est
    /// ce qui la rend visible hors du cadre — le seul moyen, Windows rognant
    /// tout ce qui est dessiné dans la zone libre (voir `set_dock_region`).
    #[test]
    fn dock_region_rects_unions_the_overflow() {
        let overflow = [0, 40, 200, 320];
        let rects = dock_region_rects(
            REGION_W,
            REGION_H,
            REGION_TOP,
            REGION_SIDE,
            REGION_SIDE,
            REGION_BOTTOM,
            Some(overflow),
        );
        // Les quatre bandes, PUIS le débordement.
        assert_eq!(rects.len(), 5);
        assert_eq!(rects[4], overflow);
    }

    /// **Le piège du masquage automatique.** Quand les barres sont
    /// masquées, leurs épaisseurs valent zéro : sans garde, la région ne
    /// contiendrait plus QUE le débordement, soit une icône grossie flottant sur
    /// le bureau sans aucune barre. Le cas est atteignable — la disposition des
    /// icônes suit les épaisseurs RÉSERVÉES et ne bouge donc pas quand les
    /// barres se retirent, si bien que l'échelle peut encore être supérieure à 1
    /// pendant l'animation de sortie du pointeur.
    #[test]
    fn dock_region_rects_ignores_the_overflow_when_all_bars_are_hidden() {
        let overflow = [0, 40, 200, 320];
        let rects = dock_region_rects(REGION_W, REGION_H, 0, 0, 0, 0, Some(overflow));
        assert_eq!(rects.len(), 4, "aucun rectangle de débordement ajouté");
        assert!(!rects.contains(&overflow));
    }

    /// Le débordement est intersecté avec la fenêtre : une boîte qui en sort
    /// partiellement est ramenée, une boîte entièrement dehors est ÉCARTÉE
    /// (`CreateRectRgn` refuse un rectangle dégénéré).
    #[test]
    fn dock_region_rects_clips_the_overflow_to_the_window() {
        let clipped = dock_region_rects(
            REGION_W,
            REGION_H,
            REGION_TOP,
            REGION_SIDE,
            REGION_SIDE,
            REGION_BOTTOM,
            Some([-50, -30, 200, 320]),
        );
        assert_eq!(clipped[4], [0, 0, 200, 320]);

        let outside = dock_region_rects(
            REGION_W,
            REGION_H,
            REGION_TOP,
            REGION_SIDE,
            REGION_SIDE,
            REGION_BOTTOM,
            Some([REGION_W + 10, 10, REGION_W + 200, 300]),
        );
        assert_eq!(outside.len(), 4, "boîte entièrement dehors : écartée");

        let empty = dock_region_rects(
            REGION_W,
            REGION_H,
            REGION_TOP,
            REGION_SIDE,
            REGION_SIDE,
            REGION_BOTTOM,
            Some([10, 10, 10, 300]),
        );
        assert_eq!(empty.len(), 4, "boîte de largeur nulle : écartée");
    }

    #[test]
    fn workarea_setting_change_is_recognized() {
        assert!(is_workarea_setting_change(SPI_SETWORKAREA.0));
    }

    #[test]
    fn unrelated_setting_changes_are_ignored() {
        // Valeurs prises au hasard parmi d'autres actions `SPI_*` réelles
        // (respectivement `SPI_SETDESKWALLPAPER` et `SPI_GETNONCLIENTMETRICS`)
        // — n'importe quelle valeur différente de `SPI_SETWORKAREA` convient,
        // le but est juste de vérifier que le filtre n'est pas un simple
        // "toujours vrai" déguisé.
        assert!(!is_workarea_setting_change(20));
        assert!(!is_workarea_setting_change(41));
        assert!(!is_workarea_setting_change(0));
    }

    /// **SONDE — CE QUE LA DÉTECTION DE PLEIN ÉCRAN VOIT, MAINTENANT, EN CHIFFRES.**
    ///
    /// SIGNALÉ : « au retour dans la session, je ne vois pas l'app mais j'ai
    /// l'horloge qui correspond à une application en plein écran » — autrement dit
    /// `hidden_for_fullscreen` est resté VRAI après un déverrouillage, et la
    /// mini-barre d'horloge se prend pour le mode plein écran.
    ///
    /// ⛔ **CE CHEMIN N'AVAIT AUCUNE TRACE**, et il n'y a pas moyen de conclure sans
    /// nombres : la décision tient à `SHQueryUserNotificationState`, au premier plan
    /// COURANT, et à une heuristique géométrique — trois choses qu'un raisonnement
    /// ne devine pas. **Elle se lance pendant que l'état fautif est ENCORE là**, ce
    /// qui évite de redemander à l'utilisateur de verrouiller sa session pour rien :
    /// l'état du système est global, un processus de test le lit aussi bien que
    /// l'application.
    ///
    /// `cargo test --bins -- --ignored --nocapture la_sonde_du_plein_ecran`
    #[test]
    #[ignore]
    fn la_sonde_du_plein_ecran() {
        unsafe {
            println!("SHQueryUserNotificationState = {:?}", SHQueryUserNotificationState());

            let premier_plan = GetForegroundWindow();
            let mut pid = 0u32;
            GetWindowThreadProcessId(premier_plan, Some(&mut pid));
            let mut classe = [0u16; 256];
            let n = GetClassNameW(premier_plan, &mut classe);
            let classe = String::from_utf16_lossy(&classe[..n.max(0) as usize]);
            println!("premier plan = {premier_plan:?}  pid={pid} (nous={})  classe={classe}", std::process::id());

            let mut rect = RECT::default();
            let _ = GetWindowRect(premier_plan, &mut rect);
            let ecran = MonitorFromWindow(premier_plan, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(ecran, &mut info);
            println!(
                "  fenetre = ({}, {}) {}x{}   ecran = ({}, {}) {}x{}",
                rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top,
                info.rcMonitor.left, info.rcMonitor.top,
                info.rcMonitor.right - info.rcMonitor.left,
                info.rcMonitor.bottom - info.rcMonitor.top,
            );
            println!(
                "  visible = {}  reduite = {}  sur le meme ecran que le premier plan = {}",
                IsWindowVisible(premier_plan).as_bool(),
                IsIconic(premier_plan).as_bool(),
                hwnd_monitor(premier_plan) == ecran,
            );

            println!("window_is_still_fullscreen(premier plan)   = {}", window_is_still_fullscreen(premier_plan));
            println!("foreground_fullscreen_window(null)         = {:?}", foreground_fullscreen_window(HWND::default()));
            println!("fullscreen_window_on_monitor(null, ecran)  = {:?}", fullscreen_window_on_monitor(HWND::default(), ecran));

            // ⛔ **LE COUPABLE EST ICI, S'IL EXISTE.** `sync_fullscreen_visibility`
            // MÉMORISE le HWND détecté et, tant qu'il est encore « plein écran »,
            // **ne réévalue rien** (voir `window_is_still_fullscreen`, dont la doc
            // porte le pourquoi). Un HWND qui reste collé après un déverrouillage
            // bloquerait donc le dock dans l'état masqué — et la seule façon de le
            // savoir est de LISTER les fenêtres qui passent encore le test.
            use windows::Win32::UI::WindowsAndMessaging::{
                EnumWindows, GetWindow, GetWindowLongPtrW, InternalGetWindowText, GWL_EXSTYLE, GW_OWNER,
            };
            unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
                if window_is_still_fullscreen(hwnd) {
                    let liste = &mut *(lparam.0 as *mut Vec<String>);
                    let mut pid = 0u32;
                    GetWindowThreadProcessId(hwnd, Some(&mut pid));
                    let mut c = [0u16; 256];
                    let n = GetClassNameW(hwnd, &mut c);
                    let mut r = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut r);
                    let mut titre = [0u16; 256];
                    let nt = InternalGetWindowText(hwnd, &mut titre);
                    let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
                    let proprio = GetWindow(hwnd, GW_OWNER);
                    liste.push(format!(
                        "{hwnd:?} pid={pid} classe={} rect=({}, {}) {}x{} exstyle=0x{ex:X} proprietaire={proprio:?} titre={:?}",
                        String::from_utf16_lossy(&c[..n.max(0) as usize]),
                        r.left, r.top, r.right - r.left, r.bottom - r.top,
                        String::from_utf16_lossy(&titre[..nt.max(0) as usize]),
                    ));
                }
                BOOL(1)
            }
            let mut trouvees: Vec<String> = Vec::new();
            let _ = EnumWindows(Some(cb), LPARAM(&mut trouvees as *mut _ as isize));
            println!("\nFENETRES PASSANT ENCORE LE TEST DE PLEIN ECRAN ({}):", trouvees.len());
            for t in &trouvees {
                println!("  {t}");
            }
        }
    }

    /// **FERMER L'APPLICATION PROPREMENT — LE LEVIER QUI MANQUAIT QUAND ELLE EST INVISIBLE.**
    ///
    /// MESURÉ le 2026-10-05 : le dock masqué par un faux plein écran (voir
    /// `est_une_surface_du_shell`), l'utilisateur n'avait plus **aucune interface**
    /// pour la fermer — il ne restait que le gestionnaire de tâches. Et ce dépôt
    /// interdit `Stop-Process`, qui laisse des fenêtres fantômes (voir la fiche
    /// `neuradesk-ghost-windows-and-forced-kill`).
    ///
    /// `WM_CLOSE` sur la fenêtre principale est **exactement ce que fait son bouton
    /// Quitter** : `wndproc` ne le traite pas spécialement, `DefWindowProcW` détruit
    /// la fenêtre, et son `WM_DESTROY` appelle `PostQuitMessage` — sans condition, et
    /// c'est voulu (voir le commentaire d'`appbar_helper_wndproc`, qui explique
    /// pourquoi les fenêtres AUXILIAIRES ne doivent PAS faire pareil).
    ///
    /// ⚠️ **CE N'EST PAS UNE ENTRÉE SIMULÉE** : c'est un message de fenêtre, la voie
    /// que le shell lui-même emprunte quand on ferme une application. Rien n'est
    /// injecté dans la file d'entrées de l'utilisateur.
    ///
    /// `cargo test --bins -- --ignored --nocapture fermer_l_application_masquee`
    #[test]
    #[ignore]
    fn fermer_l_application_masquee() {
        use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_CLOSE};
        unsafe {
            let principale = FindWindowW(w!("NeuraDeskDockWindow"), None)
                .ok()
                .filter(|hwnd| !hwnd.is_invalid())
                .expect("aucune fenetre `NeuraDeskDockWindow` : l'application ne tourne pas");
            println!("fenetre principale = {principale:?} — envoi de WM_CLOSE");
            PostMessageW(Some(principale), WM_CLOSE, WPARAM(0), LPARAM(0)).expect("envoi de WM_CLOSE");
            println!("envoye — l'application doit se fermer d'elle-meme en quelques frames.");
        }
    }
}

#[cfg(test)]
mod reflux_de_capture_tests {
    use super::{contient, ecran_a_rendu};
    use windows::Win32::Foundation::RECT;

    const RESERVE: RECT = RECT { left: 0, top: 40, right: 3840, bottom: 2160 };
    const RENDUE: RECT = RECT { left: 0, top: 0, right: 3840, bottom: 2160 };

    /// Une fenêtre agrandie qui a suivi le mouvement contient la nouvelle zone
    /// de travail ; une fenêtre restée à l'ancienne ne la contient pas.
    #[test]
    fn une_fenetre_agrandie_doit_contenir_la_zone_de_travail() {
        assert!(ecran_a_rendu(RESERVE, RENDUE, &[RENDUE]), "la fenêtre a suivi");
        assert!(!ecran_a_rendu(RESERVE, RENDUE, &[RESERVE]), "la fenêtre est restée à l'ancienne taille");
    }

    /// ⚠️ **LE BORD INVISIBLE DE DWM** : `GetWindowRect` d'une fenêtre agrandie
    /// est gonflé d'environ 8 px par côté, donc `rect == rcWork` est **faux en
    /// permanence**. Une égalité stricte ferait attendre CHAQUE capture jusqu'à
    /// la borne dure — c'est précisément ce que ce test écarte.
    #[test]
    fn le_bord_invisible_de_dwm_est_accepte() {
        let gonflee = RECT { left: -8, top: -8, right: 3848, bottom: 2168 };
        assert_ne!(gonflee, RENDUE, "prémisse : ce n'est PAS l'égalité stricte");
        assert!(ecran_a_rendu(RESERVE, RENDUE, &[gonflee]));
    }

    /// ⛔ **LA CONTENANCE SEULE NE PROUVE RIEN** : tant que le shell n'a pas
    /// appliqué la libération, la zone de travail est encore l'ancienne — et la
    /// fenêtre, elle aussi encore à l'ancienne, la contient parfaitement. Le
    /// test rendrait vrai sans qu'un pixel ait bougé.
    #[test]
    fn une_zone_de_travail_inchangee_ne_prouve_rien() {
        assert!(!ecran_a_rendu(RESERVE, RESERVE, &[RESERVE]));
    }

    /// Sans aucune fenêtre agrandie sur l'écran, seul le déplacement de la zone
    /// de travail décide.
    #[test]
    fn sans_fenetre_agrandie_seule_la_zone_de_travail_compte() {
        assert!(ecran_a_rendu(RESERVE, RENDUE, &[]));
        assert!(!ecran_a_rendu(RESERVE, RESERVE, &[]));
    }

    /// Bornes INCLUSES : un bord partagé compte comme contenu — c'est le cas
    /// normal d'une fenêtre posée exactement sur la zone de travail.
    #[test]
    fn les_bornes_partagees_comptent_comme_contenues() {
        let r = RECT { left: 0, top: 0, right: 100, bottom: 100 };
        assert!(contient(r, r));
        assert!(contient(r, RECT { left: 10, top: 10, right: 90, bottom: 90 }));
        assert!(!contient(r, RECT { left: 0, top: 0, right: 101, bottom: 100 }), "1 px de trop à droite");
    }
}
