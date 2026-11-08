//! Lecture/écriture du presse-papiers — texte (`CF_UNICODETEXT`) ET image
//! (format registré `"PNG"` en priorité, sinon `CF_DIBV5`/`CF_DIB`) — façon
//! gestionnaire de presse-papiers de Brain_Shell (`cliphist`), au format
//! Windows natif plutôt qu'un utilitaire externe. L'historique lui-même
//! (voir `app::run`) est tenu en mémoire côté application, alimenté en
//! écoutant `WM_CLIPBOARDUPDATE` sur la fenêtre du dock (voir
//! `win32::window::Window::create_dock`) — et rechargé au lancement depuis
//! `%APPDATA%\NeuraDesk\clipboard_history.toml` (voir `config::
//! load_clipboard_history`/`save_clipboard_history`).

use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::Graphics::Gdi::{BITMAPINFOHEADER, BITMAPV5HEADER, BI_BITFIELDS};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{CF_DIB, CF_DIBV5, CF_UNICODETEXT, CLIPBOARD_FORMAT};

/// Grand côté maximal de la miniature conservée pour l'aperçu dans le menu
/// du presse-papiers (voir `win32::menu::show_clipboard_menu`,
/// `win32::window::draw_clipboard_image`) — calculée une seule fois à la
/// capture (voir `read`) ou au rechargement depuis le disque (voir
/// `ClipboardEntry::from_png_bytes`), jamais recalculée à chaque ouverture
/// du menu : l'image d'origine peut peser plusieurs Mo (une capture d'écran
/// 4K par exemple), la miniature reste petite en mémoire comme sur disque.
/// Volontairement assez grande (pas une simple icône) pour qu'une capture
/// d'écran contenant du texte reste lisible dans le menu (voir
/// `win32::menu::CLIPBOARD_MENU_WIDTH`, dimensionnée en conséquence).
pub const CLIPBOARD_THUMB_MAX_SIDE: u32 = 440;

/// Une entrée de l'historique du presse-papiers — texte ou image, voir doc
/// de module.
///
/// # ⛔ LE RGBA PLEINE RÉSOLUTION N'EST PLUS GARDÉ (2026-10-08)
///
/// Il l'était, et il était **le premier poste de mémoire de l'application**.
/// Mesuré à l'allocateur instrumenté (voir `crate::alloc_trace`) : `decode_png`
/// allouait **33 129 616 octets pour une seule capture d'écran 4K**, une fois
/// par entrée image de l'historique. L'historique étant plein (`app::
/// CLIPBOARD_HISTORY_LIMIT` = 500 entrées, dont 244 images), cela tenait
/// **~2 Go sur les 2,2 Go du processus** — pour un panneau qui n'affiche QUE
/// des miniatures (`thumb_rgba`, 440 px, voir `CLIPBOARD_THUMB_MAX_SIDE`).
///
/// **Le disque est la source de vérité** : le PNG y est déjà, en pleine
/// résolution. On ne garde donc ici que de quoi **retrouver** l'image
/// (`fichier`) et de quoi la **peindre** (`thumb_rgba`). `rgba` ne reste en
/// mémoire que tant que l'image n'a pas été écrite — c'est-à-dire pour les
/// quelques entrées créées pendant la session en cours, jamais pour les 244
/// rechargées au démarrage.
///
/// ⚠️ **CE QUE ÇA COÛTE, ET C'EST ASSUMÉ** : replacer une entrée dans le
/// presse-papiers (`write`) ou l'exporter (`png`) **relit et redécode** le PNG.
/// Ce sont des gestes de l'utilisateur, pas des chemins chauds — le prix est
/// payé au moment où on le demande, et jamais avant.
#[derive(Clone)]
pub enum ClipboardEntry {
    Text(String),
    Image {
        width: u32,
        height: u32,
        /// **LE RGBA PLEINE RÉSOLUTION — `Some` SEULEMENT TANT QUE L'IMAGE
        /// N'EST PAS PERSISTÉE.** Voir la doc de l'énuméré.
        rgba: Option<Vec<u8>>,
        /// **LE PNG QUI PORTE CETTE IMAGE SUR LE DISQUE**, une fois persistée.
        /// C'est par lui qu'on la relit (voir `rgba`/`png`).
        fichier: Option<std::path::PathBuf>,
        thumb_width: u32,
        thumb_height: u32,
        thumb_rgba: Vec<u8>,
    },
}

/// **L'ÉGALITÉ SE JUGE SUR LA MINIATURE, PLUS SUR LE RGBA.**
///
/// Le `rgba` n'est plus toujours là (voir la doc de l'énuméré), alors que la
/// miniature l'est TOUJOURS — et elle est dérivée de l'image entière par une
/// recette déterministe (`make_thumbnail`), donc deux images identiques ont la
/// même. Une égalité dérivée sur `rgba` serait de toute façon inutilisable :
/// elle comparerait 33 Mo octet par octet.
///
/// ⚠️ **CE QU'ON PERD** : deux images différentes dont la réduction à 440 px
/// tomberait sur les mêmes pixels se croiraient égales. Le seul appelant est le
/// **dédoublonnage** de l'historique (voir `app::run`) : au pire, une entrée
/// remonte en tête à la place d'une autre au lieu d'être ajoutée — jamais de
/// perte de donnée.
impl PartialEq for ClipboardEntry {
    fn eq(&self, autre: &Self) -> bool {
        match (self, autre) {
            (ClipboardEntry::Text(a), ClipboardEntry::Text(b)) => a == b,
            (
                ClipboardEntry::Image {
                    width: aw,
                    height: ah,
                    thumb_width: atl,
                    thumb_height: ath,
                    thumb_rgba: ami,
                    ..
                },
                ClipboardEntry::Image {
                    width: bw,
                    height: bh,
                    thumb_width: btl,
                    thumb_height: bth,
                    thumb_rgba: bmi,
                    ..
                },
            ) => aw == bw && ah == bh && atl == btl && ath == bth && ami == bmi,
            _ => false,
        }
    }
}

/// Implémentation manuelle plutôt qu'un `#[derive(Debug)]` : ce dernier
/// afficherait le contenu ENTIER de `rgba`/`thumb_rgba` (potentiellement
/// plusieurs millions d'octets pour une capture d'écran) au moindre
/// `{:?}` — requis par `#[derive(Debug)]` sur `win32::window::Event`, qui
/// n'a en pratique jamais besoin d'un affichage aussi détaillé.
impl std::fmt::Debug for ClipboardEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipboardEntry::Text(text) => f.debug_tuple("Text").field(text).finish(),
            ClipboardEntry::Image { width, height, .. } => {
                f.debug_struct("Image").field("width", width).field("height", height).finish_non_exhaustive()
            }
        }
    }
}

impl ClipboardEntry {
    /// `true` pour du texte entièrement blanc (espaces/retours à la ligne
    /// seuls) — une image n'est jamais "vide" au même sens, voir `app::run`
    /// (ne rien ajouter à l'historique pour une entrée vide évite de
    /// polluer la liste avec les changements de presse-papiers internes de
    /// certaines applications qui le vident temporairement).
    pub fn is_blank(&self) -> bool {
        match self {
            ClipboardEntry::Text(text) => text.trim().is_empty(),
            ClipboardEntry::Image { .. } => false,
        }
    }

    /// Reconstruit une entrée Image à partir d'octets PNG déjà lus sur le
    /// disque (voir `config::load_clipboard_history`) — recalcule la
    /// miniature (jamais persistée telle quelle, voir doc de
    /// `CLIPBOARD_THUMB_MAX_SIDE`). `None` si le fichier n'est pas un PNG
    /// valide (disque corrompu, fichier tronqué...) : l'entrée est alors
    /// simplement omise du rechargement plutôt que de faire échouer tout
    /// l'historique.
    pub fn from_png_bytes(bytes: &[u8], fichier: std::path::PathBuf) -> Option<Self> {
        let (width, height, rgba) = decode_png(bytes)?;
        let (thumb_width, thumb_height, thumb_rgba) =
            make_thumbnail(width, height, &rgba, CLIPBOARD_THUMB_MAX_SIDE);
        // ⛔ **`rgba` MEURT ICI, À LA FIN DE LA PORTÉE** — c'est tout l'objet du
        // correctif (voir la doc de `ClipboardEntry`) : il n'a servi qu'à
        // calculer la miniature, et le PNG reste sur le disque pour le reste.
        Some(ClipboardEntry::Image {
            width,
            height,
            rgba: None,
            fichier: Some(fichier),
            thumb_width,
            thumb_height,
            thumb_rgba,
        })
    }

    /// **LE PNG DE CETTE IMAGE, D'OÙ QU'IL VIENNE** — encodé depuis le `rgba`
    /// quand il est encore en mémoire, ou **relu tel quel** sur le disque (le
    /// fichier EST un PNG : il n'y a rien à réencoder). `None` pour du texte, ou
    /// si le fichier a disparu.
    pub fn to_png_bytes(&self) -> Option<Vec<u8>> {
        match self {
            ClipboardEntry::Text(_) => None,
            ClipboardEntry::Image { width, height, rgba: Some(rgba), .. } => {
                encode_png(*width, *height, rgba)
            }
            ClipboardEntry::Image { fichier: Some(chemin), .. } => std::fs::read(chemin).ok(),
            ClipboardEntry::Image { .. } => None,
        }
    }

    /// **LA SEULE CHOSE QUE L'ÉCRITURE SUR DISQUE A BESOIN** : le PNG à écrire,
    /// et UNIQUEMENT s'il y a quelque chose à écrire.
    ///
    /// ⚠️ **`None` VEUT DIRE « DÉJÀ PERSISTÉE »**, pas « erreur » : rendre le
    /// PNG du disque pour le réécrire à l'endroit d'où il vient serait un
    /// aller-retour pour rien, et c'est précisément ce que ce correctif supprime.
    pub fn png_a_ecrire(&self) -> Option<Vec<u8>> {
        match self {
            ClipboardEntry::Text(_) => None,
            ClipboardEntry::Image { rgba: None, .. } => None,
            ClipboardEntry::Image { width, height, rgba: Some(rgba), .. } => {
                encode_png(*width, *height, rgba)
            }
        }
    }

    /// **LE RGBA PLEINE RÉSOLUTION**, relu du disque s'il n'est plus en mémoire
    /// (voir la doc de `ClipboardEntry`). `None` si le fichier a disparu — une
    /// entrée évincée dont le PNG a été nettoyé, par exemple.
    pub fn rgba(&self) -> Option<Vec<u8>> {
        match self {
            ClipboardEntry::Text(_) => None,
            ClipboardEntry::Image { rgba: Some(rgba), .. } => Some(rgba.clone()),
            ClipboardEntry::Image { fichier: Some(chemin), .. } => {
                let octets = std::fs::read(chemin).ok()?;
                decode_png(&octets).map(|(_, _, rgba)| rgba)
            }
            ClipboardEntry::Image { .. } => None,
        }
    }

    /// Les dimensions de l'image — `None` pour une entrée texte.
    ///
    /// Rend des valeurs COPIÉES, à dessein : les appelants s'en servent pour
    /// appeler `rgba()`, qui emprunte `self` à son tour.
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        match self {
            ClipboardEntry::Text(_) => None,
            ClipboardEntry::Image { width, height, .. } => Some((*width, *height)),
        }
    }

    /// Le fichier qui porte cette image, s'il est connu.
    pub fn fichier(&self) -> Option<&std::path::Path> {
        match self {
            ClipboardEntry::Text(_) => None,
            ClipboardEntry::Image { fichier, .. } => fichier.as_deref(),
        }
    }

    /// Le RGBA **en mémoire seulement** — ce dont `config::
    /// save_clipboard_history` a besoin pour **hacher le contenu** et en tirer
    /// un nom de fichier (voir `clipboard_image_file_name`).
    pub fn rgba_en_memoire(&self) -> Option<&[u8]> {
        match self {
            ClipboardEntry::Text(_) => None,
            ClipboardEntry::Image { rgba, .. } => rgba.as_deref(),
        }
    }

    /// **ENREGISTRE QUE L'IMAGE EST PERSISTÉE — ET LIBÈRE LE `rgba` DANS LE MÊME
    /// MOUVEMENT.** C'est **ici, et nulle part ailleurs**, que la mémoire est
    /// rendue.
    pub fn marquer_persistee(&mut self, chemin: std::path::PathBuf) {
        if let ClipboardEntry::Image { rgba, fichier, .. } = self {
            *fichier = Some(chemin);
            *rgba = None;
        }
    }

    /// Encode la MINIATURE (`thumb_rgba`, voir `CLIPBOARD_THUMB_MAX_SIDE`)
    /// en PNG — pour `win32::cef_clipboard`, qui a besoin d'un `<img
    /// src="data:image/png;base64,...">` pour chaque ligne de la liste
    /// (l'aperçu HTML/CSS ne peut pas dessiner du RGBA brut comme
    /// `egui::ColorImage` le pouvait). `None` pour une entrée texte, comme
    /// `to_png_bytes`.
    pub fn thumb_png_bytes(&self) -> Option<Vec<u8>> {
        match self {
            ClipboardEntry::Text(_) => None,
            ClipboardEntry::Image { thumb_width, thumb_height, thumb_rgba, .. } => {
                encode_png(*thumb_width, *thumb_height, thumb_rgba)
            }
        }
    }
}

/// Relit le contenu actuel du presse-papiers — une image en priorité si le
/// presse-papiers en contient une (voir `read_image`), sinon du texte Unicode
/// (voir `read_text`). `None` si ni l'un ni l'autre (fichiers copiés, format
/// propriétaire...) ou en cas d'échec d'accès (presse-papiers verrouillé par
/// une autre application, situation transitoire courante et sans
/// conséquence : on retente simplement au prochain changement).
pub fn read() -> Option<ClipboardEntry> {
    if let Some((width, height, rgba)) = read_image() {
        let (thumb_width, thumb_height, thumb_rgba) = make_thumbnail(width, height, &rgba, CLIPBOARD_THUMB_MAX_SIDE);
        // ⚠️ **CAPTURE FRAÎCHE : le `rgba` est GARDÉ** (`Some`), parce que
        // l'image n'est pas encore sur le disque — c'est `png_a_ecrire` qui la
        // persistera, et `marquer_persistee` qui libérera ce tampon. Voir la doc
        // de `ClipboardEntry`.
        return Some(ClipboardEntry::Image {
            width,
            height,
            rgba: Some(rgba),
            fichier: None,
            thumb_width,
            thumb_height,
            thumb_rgba,
        });
    }
    read_text().map(ClipboardEntry::Text)
}

/// Place `entry` dans le presse-papiers — dispatché selon sa variante (voir
/// `write_text`/`write_image`), comme un vrai Ctrl+C : permet ensuite un
/// Ctrl+V normal dans n'importe quelle application.
pub fn write(entry: &ClipboardEntry) {
    match entry {
        ClipboardEntry::Text(text) => {
            // L'échec est ignoré ICI : `write` est appelé pour restaurer une
            // entrée d'historique, sans personne à qui dire l'échec. Les
            // appelants qui ont un écran pour le dire, eux, lisent le booléen.
            let _ = write_text(text);
        }
        ClipboardEntry::Image { width, height, .. } => {
            // ⚠️ **LE RGBA EST RELU DU DISQUE QUAND IL N'EST PLUS EN MÉMOIRE** —
            // c'est le prix du correctif, et il est payé ICI, sur un geste de
            // l'utilisateur (voir la doc de `ClipboardEntry`). L'échec est ignoré,
            // comme pour le texte : `write` restaure une entrée d'historique,
            // sans personne à qui dire l'échec.
            if let Some(rgba) = entry.rgba() {
                write_image(*width, *height, &rgba);
            }
        }
    }
}

/// Relit le contenu texte actuel du presse-papiers, si l'application qui
/// vient de le remplir y a placé du texte Unicode (`CF_UNICODETEXT`) —
/// `None` sinon (image, fichiers copiés, format propriétaire...) ou en cas
/// d'échec d'accès (presse-papiers verrouillé par une autre application,
/// situation transitoire courante et sans conséquence : on retente
/// simplement au prochain changement).
pub fn read_text() -> Option<String> {
    unsafe {
        OpenClipboard(None).ok()?;
        let result = (|| {
            // Le handle générique renvoyé par l'API presse-papiers est
            // typé `HANDLE`, mais représente en réalité un `HGLOBAL` — les
            // deux enveloppent le même pointeur brut, conversion directe.
            let handle = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
            let global = HGLOBAL(handle.0);
            let ptr = GlobalLock(global) as *const u16;
            if ptr.is_null() {
                return None;
            }
            // `CF_UNICODETEXT` est nul-terminé : on cherche la fin plutôt
            // que de se fier à `GlobalSize` (qui inclut souvent un peu de
            // rembourrage au-delà du nul).
            let mut len = 0usize;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
            let _ = GlobalUnlock(global);
            Some(text)
        })();
        let _ = CloseClipboard();
        result
    }
}

/// Place `text` dans le presse-papiers (format `CF_UNICODETEXT`), comme un
/// vrai Ctrl+C — permet ensuite un Ctrl+V normal dans n'importe quelle
/// application.
///
/// ⛔ **REND `true` SEULEMENT SI LE TEXTE EST RÉELLEMENT DANS LE PRESSE-PAPIERS.**
/// Elle ne rendait RIEN, et c'était un échec silencieux : `OpenClipboard` échoue
/// dès qu'une autre application tient le presse-papiers — un cas banal, pas
/// exotique. Un appelant qui annonçait « copié » mentait alors à l'utilisateur,
/// qui collait ensuite autre chose. Le dépôt refuse les échecs silencieux ; celui
/// -ci était le plus facile à croire.
///
/// ⚠️ Les appelants qui n'ont rien à dire de l'échec peuvent l'ignorer
/// (`let _ =`), mais ils le font EN CONNAISSANCE.
pub fn write_text(text: &str) -> bool {
    poser_texte(text, false)
}

/// ⭐ **UN SECRET (mot de passe généré) : COPIÉ, MAIS HORS DE TOUT HISTORIQUE.**
/// DEMANDE EXPLICITE (2026-10-24) : le générateur de mots de passe du
/// Presse-papiers. Les trois formats sont la convention documentée de Windows
/// (« Clipboard Formats », section *Cloud Clipboard and Clipboard History
/// Formats*) que posent aussi KeePass et les gestionnaires de mots de passe :
/// l'historique de Windows (Win+V), la synchronisation entre appareils ET les
/// moniteurs de presse-papiers — dont le nôtre, voir `exclu_de_l_historique` —
/// laissent ce contenu de côté.
pub fn write_secret(text: &str) -> bool {
    poser_texte(text, true)
}

/// Le contenu actuel demande-t-il à rester HORS des historiques ? — voir
/// `write_secret` pour la convention. Vérifié AVANT `read` par le moniteur
/// (`window.rs`, `WM_CLIPBOARDUPDATE`) : un mot de passe copié depuis KeePass
/// n'entrait jusqu'ici dans notre historique, et sur le disque, comme le reste.
pub fn exclu_de_l_historique() -> bool {
    unsafe {
        let exclu = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        let ignore = RegisterClipboardFormatW(w!("Clipboard Viewer Ignore"));
        if (exclu != 0 && IsClipboardFormatAvailable(exclu).is_ok())
            || (ignore != 0 && IsClipboardFormatAvailable(ignore).is_ok())
        {
            return true;
        }
        let historique = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        if historique == 0 || IsClipboardFormatAvailable(historique).is_err() {
            return false;
        }
        // Présent : la VALEUR décide (un DWORD, 0 = « pas dans l'historique »).
        // Illisible (presse-papiers tenu par une autre application) : on
        // s'abstient — mieux vaut perdre une entrée que garder un secret.
        let Ok(()) = OpenClipboard(None) else { return true };
        let mut refuse = true;
        if let Ok(handle) = GetClipboardData(historique) {
            let global = HGLOBAL(handle.0);
            let ptr = GlobalLock(global) as *const u32;
            if !ptr.is_null() && GlobalSize(global) >= std::mem::size_of::<u32>() {
                refuse = std::ptr::read_unaligned(ptr) == 0;
            }
            if !ptr.is_null() {
                let _ = GlobalUnlock(global);
            }
        }
        let _ = CloseClipboard();
        refuse
    }
}

/// Un bloc mobile qui porte `octets` — `None` si l'allocation échoue.
unsafe fn bloc(octets: &[u8]) -> Option<HGLOBAL> {
    let global = GlobalAlloc(GMEM_MOVEABLE, octets.len().max(1)).ok()?;
    let ptr = GlobalLock(global) as *mut u8;
    if ptr.is_null() {
        return None;
    }
    std::ptr::copy_nonoverlapping(octets.as_ptr(), ptr, octets.len());
    let _ = GlobalUnlock(global);
    Some(global)
}

fn poser_texte(text: &str, secret: bool) -> bool {
    unsafe {
        let Ok(()) = OpenClipboard(None) else { return false };
        let _ = EmptyClipboard();
        if secret {
            // Posés AVANT le texte : un moniteur réveillé par le premier format
            // les trouve déjà là. Le presse-papiers prend possession des blocs.
            for (nom, valeur) in [
                (w!("ExcludeClipboardContentFromMonitorProcessing"), None),
                (w!("CanIncludeInClipboardHistory"), Some(0u32)),
                (w!("CanUploadToCloudClipboard"), Some(0u32)),
            ] {
                let format = RegisterClipboardFormatW(nom);
                let octets = valeur.map_or(vec![0u8], |v| v.to_le_bytes().to_vec());
                if let (true, Some(global)) = (format != 0, bloc(&octets)) {
                    let _ = SetClipboardData(format, Some(HANDLE(global.0)));
                }
            }
        }

        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let byte_len = wide.len() * std::mem::size_of::<u16>();

        let mut pose = false;
        if let Ok(global) = GlobalAlloc(GMEM_MOVEABLE, byte_len) {
            let ptr = GlobalLock(global) as *mut u16;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                let _ = GlobalUnlock(global);
                // Le presse-papiers prend possession du bloc en cas de
                // succès — ne pas le libérer nous-mêmes ensuite (pas de
                // `GlobalFree`). `SetClipboardData` attend un `HANDLE`
                // générique, même conversion directe que côté lecture.
                pose = SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(global.0))).is_ok();
            }
        }

        let _ = CloseClipboard();
        pose
    }
}

/// Relit l'image actuelle du presse-papiers en RGBA8, si l'application qui
/// vient de le remplir y en a placé une. Trois formats essayés dans l'ordre
/// (le premier disponible gagne) :
/// 1. Le format registré `"PNG"` (voir `RegisterClipboardFormatW`) — celui
///    que les captures d'écran modernes de Windows (Outil Capture d'écran,
///    Win+Maj+S) et la plupart des navigateurs placent en plus des formats
///    classiques ci-dessous ; décodage via `image`, gère nativement la
///    transparence.
/// 2. `CF_DIBV5` — le format bitmap "moderne" de Windows, avec un vrai
///    canal alpha (voir `decode_dib_bytes`).
/// 3. `CF_DIB` — le format bitmap classique, sans alpha explicite (l'image
///    est alors considérée entièrement opaque).
///
/// `None` si aucun des trois n'est présent (texte, fichiers copiés...) ou
/// en cas d'échec d'accès (voir `read_text`).
pub fn read_image() -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        OpenClipboard(None).ok()?;
        let result = (|| {
            if let Some(bytes) = read_png_format() {
                if let Some(decoded) = decode_png(&bytes) {
                    return Some(decoded);
                }
            }
            read_dib(CF_DIBV5).or_else(|| read_dib(CF_DIB))
        })();
        let _ = CloseClipboard();
        result
    }
}

/// Place l'image RGBA8 `width`×`height` dans le presse-papiers, sous DEUX
/// formats à la fois (comportement standard d'une application qui copie une
/// image — offrir plusieurs formats simultanés, laisser le colleur choisir
/// celui qu'il comprend) : `CF_DIBV5` (voir `write_dibv5`, le format bitmap
/// que la quasi-totalité des applications Windows savent coller) et le
/// format registré `"PNG"` (voir `write_png_format`, préservation exacte de
/// la transparence pour les applications qui le préfèrent, ex. navigateurs).
/// Best-effort silencieux en cas d'échec, comme `write_text`.
pub fn write_image(width: u32, height: u32, rgba: &[u8]) {
    unsafe {
        let Ok(()) = OpenClipboard(None) else { return };
        let _ = EmptyClipboard();
        write_dibv5(width, height, rgba);
        write_png_format(width, height, rgba);
        let _ = CloseClipboard();
    }
}

/// Relit les octets bruts du format registré `"PNG"` (voir `read_image`) —
/// à appeler avec le presse-papiers déjà ouvert (voir `OpenClipboard`).
/// `RegisterClipboardFormatW` est idempotent : si une autre application a
/// déjà enregistré ce nom (cas courant, ex. un navigateur qui vient de
/// placer une image), on récupère le même identifiant qu'elle plutôt que
/// d'en créer un nouveau — c'est ce qui permet à deux applications de se
/// comprendre sur un format "maison" comme celui-ci.
unsafe fn read_png_format() -> Option<Vec<u8>> {
    let format = RegisterClipboardFormatW(w!("PNG"));
    if format == 0 {
        return None;
    }
    let handle = GetClipboardData(format).ok()?;
    let global = HGLOBAL(handle.0);
    let ptr = GlobalLock(global) as *const u8;
    if ptr.is_null() {
        return None;
    }
    let size = GlobalSize(global);
    let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
    let _ = GlobalUnlock(global);
    Some(bytes)
}

/// Place les octets PNG de `width`×`height`/`rgba` sous le format registré
/// `"PNG"` (voir `write_image`) — à appeler avec le presse-papiers déjà
/// ouvert et déjà vidé (voir `EmptyClipboard`).
unsafe fn write_png_format(width: u32, height: u32, rgba: &[u8]) {
    let Some(png) = encode_png(width, height, rgba) else { return };
    let format = RegisterClipboardFormatW(w!("PNG"));
    if format == 0 {
        return;
    }
    if let Ok(global) = GlobalAlloc(GMEM_MOVEABLE, png.len()) {
        let ptr = GlobalLock(global) as *mut u8;
        if !ptr.is_null() {
            std::ptr::copy_nonoverlapping(png.as_ptr(), ptr, png.len());
            let _ = GlobalUnlock(global);
            let _ = SetClipboardData(format, Some(HANDLE(global.0)));
        }
    }
}

/// Relit une image `CF_DIB`/`CF_DIBV5` — à appeler avec le presse-papiers
/// déjà ouvert. Les deux partagent le même préfixe de 40 octets que
/// `BITMAPINFOHEADER` (voir `decode_dib_bytes`), d'où un seul chemin de
/// décodage pour les deux formats.
unsafe fn read_dib(format: CLIPBOARD_FORMAT) -> Option<(u32, u32, Vec<u8>)> {
    let handle = GetClipboardData(format.0 as u32).ok()?;
    let global = HGLOBAL(handle.0);
    let ptr = GlobalLock(global) as *const u8;
    if ptr.is_null() {
        return None;
    }
    let size = GlobalSize(global);
    let result = decode_dib_bytes(std::slice::from_raw_parts(ptr, size));
    let _ = GlobalUnlock(global);
    result
}

/// Décode un buffer `CF_DIB`/`CF_DIBV5` brut (en-tête suivi immédiatement
/// des pixels, sans en-tête de fichier `.bmp`) en RGBA8. Gère le 24 et le
/// 32 bits par pixel non compressés (`BI_RGB`/`BI_BITFIELDS`) — largement
/// suffisant pour tout ce qu'une vraie action "copier une image" produit en
/// pratique (captures d'écran, images collées depuis un navigateur/éditeur
/// d'image) ; une palette indexée (<24bpp, très rare pour ce cas d'usage)
/// n'est volontairement pas prise en charge. Le canal alpha n'est lu que si
/// l'en-tête est un `BITMAPV5HEADER` (`biSize` d'au moins sa taille) ET que
/// `bV5AlphaMask` n'est pas nul — sinon l'image est considérée entièrement
/// opaque, comme le veut la convention `CF_DIB` classique.
fn decode_dib_bytes(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if data.len() < std::mem::size_of::<BITMAPINFOHEADER>() {
        return None;
    }
    // Les deux formats partagent le même préfixe de 40 octets (voir doc de
    // fonction) : la lecture des champs communs est valide quel que soit
    // celui réellement présent.
    let header = unsafe { &*(data.as_ptr() as *const BITMAPINFOHEADER) };
    let width = header.biWidth.unsigned_abs();
    let height = header.biHeight.unsigned_abs();
    let bit_count = header.biBitCount;
    let bytes_per_pixel = (bit_count / 8) as usize;
    if bytes_per_pixel < 3 || width == 0 || height == 0 {
        return None;
    }

    let is_v5 = header.biSize as usize >= std::mem::size_of::<BITMAPV5HEADER>();
    let has_alpha = is_v5 && {
        let v5 = unsafe { &*(data.as_ptr() as *const BITMAPV5HEADER) };
        v5.bV5AlphaMask != 0
    };
    let pixel_offset = if is_v5 {
        header.biSize as usize
    } else if header.biCompression == BI_BITFIELDS.0 {
        // En-tête V1 classique + table de 3 DWORD (masques R/G/B, pas
        // d'alpha) juste avant les pixels.
        std::mem::size_of::<BITMAPINFOHEADER>() + 12
    } else {
        std::mem::size_of::<BITMAPINFOHEADER>()
    };

    let top_down = header.biHeight < 0;
    let row_bytes = (width as usize * bit_count as usize).div_ceil(32) * 4; // lignes alignées sur 4 octets, convention DIB
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        let src_row = if top_down { y } else { height - 1 - y };
        let row_start = pixel_offset + src_row as usize * row_bytes;
        if row_start + width as usize * bytes_per_pixel > data.len() {
            return None;
        }
        let row = &data[row_start..];
        for x in 0..width {
            let src = &row[x as usize * bytes_per_pixel..];
            let dst_index = ((y * width + x) * 4) as usize;
            rgba[dst_index] = src[2]; // R
            rgba[dst_index + 1] = src[1]; // G
            rgba[dst_index + 2] = src[0]; // B
            rgba[dst_index + 3] = if has_alpha { src[3] } else { 255 };
        }
    }
    Some((width, height, rgba))
}

/// Place l'image RGBA8 sous `CF_DIBV5` (voir `write_image`) — à appeler
/// avec le presse-papiers déjà ouvert et déjà vidé. `BI_BITFIELDS` avec des
/// masques RGBA explicites (plutôt que `BI_RGB` seul) : convention suivie
/// par les applications qui écrivent elles-mêmes du `CF_DIBV5` avec canal
/// alpha (ex. les navigateurs), la façon la plus sûre de faire comprendre
/// aux applications qui lisent ce format qu'un canal alpha existe bel et
/// bien. Top-down (`bV5Height` négatif), comme le reste du code de ce
/// projet qui construit des DIB (voir `gfx::text::create_dib`,
/// `gfx::icon`) : pas de retournement de lignes à faire ici, `rgba` est
/// déjà dans cet ordre.
unsafe fn write_dibv5(width: u32, height: u32, rgba: &[u8]) {
    let header = BITMAPV5HEADER {
        bV5Size: std::mem::size_of::<BITMAPV5HEADER>() as u32,
        bV5Width: width as i32,
        bV5Height: -(height as i32),
        bV5Planes: 1,
        bV5BitCount: 32,
        bV5Compression: BI_BITFIELDS,
        bV5SizeImage: width * height * 4,
        bV5RedMask: 0x00FF0000,
        bV5GreenMask: 0x0000FF00,
        bV5BlueMask: 0x000000FF,
        bV5AlphaMask: 0xFF000000,
        // `LCS_sRGB` ('sRGB' en ASCII little-endian) : pas exposée comme
        // constante par le crate `windows`, valeur documée par Microsoft.
        // Sans elle, `bV5CSType` vaudrait 0 (espace colorimétrique calibré
        // par `bV5Endpoints`/`bV5Gamma*`, tous nuls ici) et certains
        // lecteurs afficheraient l'image avec des couleurs faussées.
        bV5CSType: 0x7352_4742,
        bV5Intent: 4, // LCS_GM_IMAGES (perceptuel, adapté à une photo/capture)
        ..Default::default()
    };
    let header_bytes =
        std::slice::from_raw_parts(&header as *const BITMAPV5HEADER as *const u8, std::mem::size_of::<BITMAPV5HEADER>());
    let pixel_len = (width * height * 4) as usize;
    let Ok(global) = GlobalAlloc(GMEM_MOVEABLE, header_bytes.len() + pixel_len) else { return };
    let ptr = GlobalLock(global) as *mut u8;
    if ptr.is_null() {
        return;
    }
    std::ptr::copy_nonoverlapping(header_bytes.as_ptr(), ptr, header_bytes.len());
    let pixels = std::slice::from_raw_parts_mut(ptr.add(header_bytes.len()), pixel_len);
    for (dst, src) in pixels.chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
        dst[0] = src[2]; // B
        dst[1] = src[1]; // G
        dst[2] = src[0]; // R
        dst[3] = src[3]; // A
    }
    let _ = GlobalUnlock(global);
    let _ = SetClipboardData(CF_DIBV5.0 as u32, Some(HANDLE(global.0)));
}

/// Réduit `rgba` à une miniature d'au plus `max_side` de grand côté (jamais
/// agrandie si l'image d'origine est déjà plus petite, pour ne pas flouter
/// une petite image en l'étirant) — voir doc de `ClipboardEntry::Image`.
/// `max_side` est un paramètre explicite (plutôt que `CLIPBOARD_THUMB_MAX_
/// SIDE` codé en dur ici) : `win32::notes` réutilise cette fonction avec sa
/// propre borne, plus petite (voir `notes::NOTE_THUMB_MAX_SIDE`).
pub(crate) fn make_thumbnail(width: u32, height: u32, rgba: &[u8], max_side: u32) -> (u32, u32, Vec<u8>) {
    let Some(image) = image::RgbaImage::from_raw(width, height, rgba.to_vec()) else {
        return (0, 0, Vec::new());
    };
    let longest = width.max(height).max(1);
    let scale = (max_side as f64 / longest as f64).min(1.0);
    let thumb_width = ((width as f64 * scale).round() as u32).max(1);
    let thumb_height = ((height as f64 * scale).round() as u32).max(1);
    let thumb = image::imageops::resize(&image, thumb_width, thumb_height, image::imageops::FilterType::Triangle);
    (thumb_width, thumb_height, thumb.into_raw())
}

/// Encode un buffer RGBA8 en PNG (voir `win32::dash::encode_png`, la même
/// recette en double ici : les deux fonctions ont des appelants aux besoins
/// de possession différents — celle-ci reçoit `&[u8]` car ses appelants
/// gardent `rgba` après l'appel, contrairement à ceux de `dash.rs`).
/// `pub(crate)` : `config::store_note_image` réutilise cette fonction pour
/// persister une image collée dans une note, plutôt que de dupliquer la même
/// recette `image::DynamicImage::write_to`.
pub(crate) fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let buffer = image::RgbaImage::from_raw(width, height, rgba.to_vec())?;
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(buffer)
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .ok()?;
    Some(bytes)
}

/// Décode des octets PNG en RGBA8 (voir `read_image`,
/// `ClipboardEntry::from_png_bytes`). `None` si `bytes` n'est pas un PNG
/// valide. `pub(crate)` : `config::regenerate_note_thumbnail` réutilise cette
/// fonction pour redécoder la pleine résolution d'une image de note quand sa
/// miniature a été perdue/corrompue indépendamment (voir sa doc).
pub(crate) fn decode_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    Some((rgba.width(), rgba.height(), rgba.into_raw()))
}

/// **UN QUART DE TOUR D'UNE IMAGE RGBA** — `horaire` à droite, sinon à gauche.
/// Rend `(largeur, hauteur, pixels)` de l'image tournée (les côtés s'échangent).
/// Sans perte : chaque pixel est déplacé, aucun n'est recalculé.
///
/// DEMANDE (2026-11-08) : « la possibilité de faire des rotations d'image dans
/// l'éditeur, le presse-papier et les notes ». Partagée par le presse-papiers
/// ([`ClipboardEntry::pivotee`]) et les images de note (`cef_notes`).
///
/// `None` si `rgba` n'a pas la taille `largeur × hauteur × 4`.
pub(crate) fn pivoter_rgba(width: u32, height: u32, rgba: &[u8], horaire: bool) -> Option<(u32, u32, Vec<u8>)> {
    let (l, h) = (width as usize, height as usize);
    if rgba.len() != l.checked_mul(h)?.checked_mul(4)? {
        return None;
    }
    let mut sortie = vec![0u8; rgba.len()];
    // Image tournée : `h` colonnes, `l` lignes.
    for y in 0..h {
        for x in 0..l {
            let (nx, ny) = if horaire { (h - 1 - y, x) } else { (y, l - 1 - x) };
            let source = (y * l + x) * 4;
            let cible = (ny * h + nx) * 4;
            sortie[cible..cible + 4].copy_from_slice(&rgba[source..source + 4]);
        }
    }
    Some((height, width, sortie))
}

impl ClipboardEntry {
    /// **L'ENTRÉE TOURNÉE D'UN QUART DE TOUR** — une NOUVELLE entrée, RGBA en
    /// mémoire et sans fichier : la prochaine `config::save_clipboard_history`
    /// écrit son PNG (le nom se déduit du contenu) et efface l'ancien, que plus
    /// rien ne désigne. `None` pour du texte, ou si l'image n'est plus lisible.
    pub fn pivotee(&self, horaire: bool) -> Option<Self> {
        let (width, height) = self.dimensions()?;
        let rgba = self.rgba()?;
        let (width, height, rgba) = pivoter_rgba(width, height, &rgba, horaire)?;
        let (thumb_width, thumb_height, thumb_rgba) = make_thumbnail(width, height, &rgba, CLIPBOARD_THUMB_MAX_SIDE);
        Some(ClipboardEntry::Image { width, height, rgba: Some(rgba), fichier: None, thumb_width, thumb_height, thumb_rgba })
    }
}

#[cfg(test)]
mod tests_pivoter {
    use super::*;

    /// 3 × 2, chaque pixel marqué par son numéro :
    /// ```text
    /// 0 1 2
    /// 3 4 5
    /// ```
    fn numerotee() -> Vec<u8> {
        (0u8..6).flat_map(|n| [n, 0, 0, 255]).collect()
    }

    fn numeros(rgba: &[u8]) -> Vec<u8> {
        rgba.chunks(4).map(|p| p[0]).collect()
    }

    #[test]
    fn un_quart_de_tour_horaire_met_la_colonne_de_gauche_en_haut() {
        let (l, h, sortie) = pivoter_rgba(3, 2, &numerotee(), true).expect("taille juste");
        assert_eq!((l, h), (2, 3));
        // 3 0
        // 4 1
        // 5 2
        assert_eq!(numeros(&sortie), vec![3, 0, 4, 1, 5, 2]);
    }

    #[test]
    fn un_quart_de_tour_anti_horaire_met_la_colonne_de_droite_en_haut() {
        let (l, h, sortie) = pivoter_rgba(3, 2, &numerotee(), false).expect("taille juste");
        assert_eq!((l, h), (2, 3));
        // 2 5
        // 1 4
        // 0 3
        assert_eq!(numeros(&sortie), vec![2, 5, 1, 4, 0, 3]);
    }

    #[test]
    fn quatre_quarts_de_tour_rendent_l_image() {
        let mut image = (3, 2, numerotee());
        for _ in 0..4 {
            image = pivoter_rgba(image.0, image.1, &image.2, true).expect("taille juste");
        }
        assert_eq!(image, (3, 2, numerotee()));
    }

    #[test]
    fn une_taille_fausse_est_refusee() {
        assert!(pivoter_rgba(3, 3, &numerotee(), true).is_none());
    }

    #[test]
    fn l_entree_pivotee_echange_ses_cotes_et_garde_ses_pixels_en_memoire() {
        let entree = ClipboardEntry::Image {
            width: 3,
            height: 2,
            rgba: Some(numerotee()),
            fichier: None,
            thumb_width: 3,
            thumb_height: 2,
            thumb_rgba: numerotee(),
        };
        let tournee = entree.pivotee(true).expect("une image");
        assert_eq!(tournee.dimensions(), Some((2, 3)));
        assert_eq!(tournee.rgba().map(|r| numeros(&r)), Some(vec![3, 0, 4, 1, 5, 2]));
        assert!(matches!(tournee, ClipboardEntry::Image { fichier: None, thumb_width: 2, thumb_height: 3, .. }));
        assert!(ClipboardEntry::Text("a".into()).pivotee(true).is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une image de `cote` × `cote` dont les pixels VARIENT — une image unie
    /// pourrait passer un test de miniature par accident.
    fn image(cote: u32) -> (u32, u32, Vec<u8>) {
        let pixels = (cote * cote) as usize;
        let mut rgba = Vec::with_capacity(pixels * 4);
        for i in 0..pixels {
            rgba.extend_from_slice(&[(i % 251) as u8, (i % 97) as u8, 200, 255]);
        }
        (cote, cote, rgba)
    }

    /// Le PNG encodé, et une entrée FRAÎCHE telle que `read` la produit.
    fn fraiche(cote: u32) -> (Vec<u8>, Vec<u8>, ClipboardEntry) {
        let (largeur, hauteur, rgba) = image(cote);
        let (tl, th, trgba) = make_thumbnail(largeur, hauteur, &rgba, CLIPBOARD_THUMB_MAX_SIDE);
        let png = encode_png(largeur, hauteur, &rgba).expect("encodage PNG");
        let entree = ClipboardEntry::Image {
            width: largeur,
            height: hauteur,
            rgba: Some(rgba.clone()),
            fichier: None,
            thumb_width: tl,
            thumb_height: th,
            thumb_rgba: trgba,
        };
        (png, rgba, entree)
    }

    /// ⛔ **LE CŒUR DU CORRECTIF DU 2026-10-08.** Une image RELUE DU DISQUE ne
    /// garde PAS son rgba pleine résolution — c'est ce tampon qui tenait ~2 Go
    /// sur les 2,2 du processus (voir la doc de `ClipboardEntry`).
    #[test]
    fn une_image_relue_du_disque_ne_garde_pas_son_rgba() {
        let (png, _, _) = fraiche(16);
        let relue = ClipboardEntry::from_png_bytes(&png, std::path::PathBuf::from("x.png"))
            .expect("relecture");

        assert!(
            relue.rgba_en_memoire().is_none(),
            "le rgba pleine resolution doit etre LIBERE a la relecture"
        );
        assert!(relue.fichier().is_some(), "le fichier doit rester : c'est lui, la source");
        assert!(relue.thumb_png_bytes().is_some(), "la miniature, elle, doit etre la");
        assert_eq!(relue.dimensions(), Some((16, 16)));
    }

    /// **ET LE RGBA RESTE ACCESSIBLE** — relu du disque à la demande. Sans ce
    /// test, on aurait « gagné » la mémoire en cassant le repli dans le
    /// presse-papiers (`write`) et l'export (`to_png_bytes`).
    #[test]
    fn le_rgba_se_relit_du_disque_a_la_demande() {
        let (png, rgba, _) = fraiche(16);
        let dossier = std::env::temp_dir().join(format!("nd-presse-papiers-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).expect("dossier temporaire");
        let chemin = dossier.join("image.png");
        std::fs::write(&chemin, &png).expect("ecriture du PNG");

        let relue = ClipboardEntry::from_png_bytes(&png, chemin.clone()).expect("relecture");
        assert_eq!(
            relue.rgba().as_deref(),
            Some(rgba.as_slice()),
            "le rgba relu doit etre IDENTIQUE a celui d'origine"
        );
        assert_eq!(
            relue.to_png_bytes().as_deref(),
            Some(png.as_slice()),
            "l'export doit rendre le PNG du disque, sans le reencoder"
        );

        let _ = std::fs::remove_dir_all(&dossier);
    }

    /// ⭐ **L'INVARIANT DU DÉDOUBLONNAGE** — l'entrée FRAÎCHE (rgba en mémoire,
    /// pas de fichier) et la même RELUE du disque (l'inverse) doivent être
    /// ÉGALES, sinon `clipboard_history.retain(...)` cesse de reconnaître un
    /// doublon et l'historique en accumule.
    #[test]
    fn une_image_fraiche_egale_la_meme_relue_du_disque() {
        let (png, _, entree_fraiche) = fraiche(16);
        let relue = ClipboardEntry::from_png_bytes(&png, std::path::PathBuf::from("x.png"))
            .expect("relecture");

        assert_eq!(
            entree_fraiche, relue,
            "la meme image doit se reconnaitre, d'ou qu'elle vienne"
        );
    }

    /// **LA PERSISTANCE LIBÈRE LE TAMPON** — c'est le seul endroit où la mémoire
    /// est rendue, donc il mérite son test.
    #[test]
    fn marquer_persistee_libere_le_rgba() {
        let (_, _, mut entree) = fraiche(16);
        assert!(entree.png_a_ecrire().is_some(), "avant persistance, il y a a ecrire");

        entree.marquer_persistee(std::path::PathBuf::from("y.png"));

        assert!(entree.rgba_en_memoire().is_none(), "le tampon doit etre libere");
        assert!(entree.png_a_ecrire().is_none(), "plus rien a ecrire : l'image est sur le disque");
        assert_eq!(entree.fichier(), Some(std::path::Path::new("y.png")));
    }
}

#[cfg(test)]
mod tests_secret {
    /// SONDE sur le VRAI presse-papiers : un secret est marqué « hors
    /// historique », un texte ordinaire non. Remet le texte d'avant.
    /// `cargo test --bins sonde_secret_hors_historique -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn sonde_secret_hors_historique() {
        let avant = super::read_text();
        assert!(super::write_secret("sonde-secret"));
        let secret_exclu = super::exclu_de_l_historique();
        let relu = super::read_text();
        assert!(super::write_text("sonde-ordinaire"));
        let ordinaire_exclu = super::exclu_de_l_historique();
        if let Some(avant) = avant {
            let _ = super::write_text(&avant);
        }
        println!("secret exclu : {secret_exclu}, relu : {relu:?}, ordinaire exclu : {ordinaire_exclu}");
        assert!(secret_exclu && !ordinaire_exclu);
        assert_eq!(relu.as_deref(), Some("sonde-secret"));
    }
}
