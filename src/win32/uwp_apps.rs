//! Découverte des applications UWP/Store (WhatsApp, Xbox, apps installées
//! depuis le Microsoft Store...) — complète `win32::apps`, qui ne voit que
//! les raccourcis `.lnk` classiques du menu Démarrer. Une application UWP
//! n'a PAS de raccourci `.lnk` : le menu Démarrer la fait apparaître via un
//! mécanisme entièrement différent (le "namespace" virtuel `shell:AppsFolder`,
//! peuplé à partir des paquets installés), invisible à un simple scan de
//! dossiers — d'où la nécessité de cette API séparée
//! (`Windows.Management.Deployment.PackageManager`, WinRT) plutôt qu'une
//! simple extension du scan existant.
//!
//! Lancement : PAS `ShellExecuteW` sur un chemin de fichier (une appli UWP
//! n'a pas d'exécutable classique à ce niveau) mais `win32::appicon::open`
//! sur le chemin virtuel `shell:AppsFolder\<AppUserModelId>` — la même
//! syntaxe que la barre d'adresse de l'Explorateur accepte pour ouvrir une
//! appli UWP par son identifiant.
//!
//! Icône : le "logo" de l'appli (voir `AppDisplayInfo::GetLogo`) est un flux
//! d'image (PNG dans l'immense majorité des cas) directement exposé par
//! WinRT — pas besoin de `SHGetFileInfoW`/GDI comme pour les raccourcis
//! classiques (voir `win32::dash::icon_data_uri`), juste décoder le flux.

use windows::core::HSTRING;
use windows::Foundation::Size;
use windows::Management::Deployment::PackageManager;
use windows::Storage::Streams::DataReader;
use windows_future::IAsyncOperation;

/// Une application UWP/Store installée pour l'utilisateur courant.
pub struct UwpApp {
    pub name: String,
    /// `PackageFamilyName!AppId` — identifiant à passer tel quel dans
    /// `shell:AppsFolder\<...>` pour lancer l'application (voir doc de
    /// module) et comme clé de cache d'icône (voir `win32::dash::
    /// ICON_CACHE`, qui accepte aussi bien un chemin de fichier qu'un
    /// identifiant UWP, les deux étant de simples chaînes opaques pour lui).
    pub app_user_model_id: String,
    /// Logo déjà décodé en RGBA8 (largeur, hauteur, pixels) — contrairement
    /// aux raccourcis classiques, extrait ici pendant l'énumération plutôt
    /// que paresseusement à la demande : on a déjà l'objet WinRT sous la
    /// main à ce moment précis, le ré-obtenir plus tard nécessiterait de
    /// ré-énumérer tous les paquets installés pour retomber dessus.
    pub icon_rgba: Option<(u32, u32, Vec<u8>)>,
}

/// Énumère les applications UWP/Store installées pour l'utilisateur courant
/// (chaîne vide passée à `FindPackagesByUserSecurityId` : convention WinRT
/// documentée pour cibler l'utilisateur courant sans avoir à résoudre son
/// SID nous-mêmes). Best-effort : renvoie une liste vide si l'API échoue
/// plutôt que de faire échouer tout le scan des applications (voir
/// `apps::enumerate_apps`, qui fusionne ce résultat avec les raccourcis
/// classiques).
pub fn enumerate() -> Vec<UwpApp> {
    let Ok(manager) = PackageManager::new() else { return Vec::new() };
    let Ok(packages) = manager.FindPackagesByUserSecurityId(&HSTRING::new()) else { return Vec::new() };

    let mut apps = Vec::new();
    for package in packages {
        // Ni les paquets "framework" (bibliothèques partagées, jamais
        // lancés directement) ni les paquets de ressources (langues,
        // scaling...) n'ont d'entrée utilisable dans le menu Démarrer.
        if package.IsFramework().unwrap_or(true) || package.IsResourcePackage().unwrap_or(true) {
            continue;
        }
        let Ok(entries) = package.GetAppListEntries() else { continue };
        for entry in entries {
            let Ok(app_user_model_id) = entry.AppUserModelId() else { continue };
            let Ok(display_info) = entry.DisplayInfo() else { continue };
            let Ok(name) = display_info.DisplayName() else { continue };
            let name = name.to_string();
            if name.trim().is_empty() {
                continue;
            }
            let icon_rgba = logo_rgba(&display_info);
            apps.push(UwpApp { name, app_user_model_id: app_user_model_id.to_string(), icon_rgba });
        }
    }
    apps
}

/// Lit le flux du logo (voir `AppDisplayInfo::GetLogo`) et le décode en
/// RGBA8 — `None` si l'appli n'a pas de logo exploitable ou si une étape
/// quelconque échoue (best-effort, voir doc de module).
///
/// Rogné via `gfx::icon::trim_transparent_padding` (voir sa doc) : la
/// plupart des logos UWP/MSIX modernes suivent un gabarit Fluent avec une
/// bonne marge transparente intégrée tout autour du glyphe (pensée pour le
/// masque de forme/l'ombre que Windows applique lui-même sur une tuile
/// Démarrer) — affichée brute dans une tuile de taille fixe (voir
/// `win32::dash`, `.tile img`), cette marge fait paraître l'icône "toute
/// petite" au milieu d'un grand espace vide, quelle que soit la résolution
/// demandée à `GetLogo`.
fn logo_rgba(display_info: &windows::ApplicationModel::AppDisplayInfo) -> Option<(u32, u32, Vec<u8>)> {
    let logo_ref = display_info.GetLogo(Size { Width: 64.0, Height: 64.0 }).ok()?;
    let stream = logo_ref.OpenReadAsync().get_blocking()?;
    let size = stream.Size().ok()? as u32;
    if size == 0 || size > 8 * 1024 * 1024 {
        return None; // taille aberrante — mieux vaut renoncer que tenter d'allouer n'importe quoi
    }

    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.join().ok()?;
    let mut bytes = vec![0u8; size as usize];
    reader.ReadBytes(&mut bytes).ok()?;

    let decoded = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (width, height) = decoded.dimensions();
    let (width, height, rgba) = crate::gfx::icon::trim_transparent_padding(width, height, &decoded.into_raw());
    Some((width, height, rgba))
}

/// Petit raccourci pour `IAsyncOperation::get()` qui avale l'erreur en
/// `None` — voir `win32::media::GetBlocking`, même rôle, copié ici plutôt
/// que partagé : un simple trait à deux lignes, pas de valeur à le coupler
/// entre deux modules qui n'ont rien d'autre en commun.
trait GetBlocking<T> {
    fn get_blocking(self) -> Option<T>;
}

impl<T: windows::core::RuntimeType + 'static> GetBlocking<T> for windows::core::Result<IAsyncOperation<T>> {
    fn get_blocking(self) -> Option<T> {
        self.ok()?.join().ok()
    }
}
