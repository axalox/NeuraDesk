//! **DEUX MOTEURS DE RECONNAISSANCE DE CARACTÈRES, UNE SEULE FORME.**
//!
//! DEMANDE (2026-10-09) : « la possibilité d'utiliser la reconnaissance de
//! caractères intégrée ou celle de Windows ». Les deux, donc — et **comparables**.
//!
//! ⭐ **ILS S'APPELLENT « NAPS2 » ET « COPILOT »** (demande 2026-11-04) : les
//! noms techniques — `tesseract`, `windows` — ne changent pas (c'est ce que les
//! pages envoient), seul le libellé montré les nomme ainsi.
//!
//! # Les deux moteurs, et d'où ils viennent
//!
//! ⭐ **« L'INTÉGRÉ » EST TESSERACT, CELUI QUE NAPS2 EMBARQUE.** Mesuré le
//! 2026-10-09 : `C:\Program Files\NAPS2\lib\_win64\tesseract.exe` est un
//! **Tesseract 5.5.1 autonome**, et les données de langue sont déjà téléchargées
//! dans `%APPDATA%\NAPS2\components\tesseract4\{fast,best}` (`eng`, **`fra`**,
//! `frm`). Rien à installer.
//!
//! ⛔ **IL NE TROUVE AUCUNE LANGUE SANS `TESSDATA_PREFIX`** — mesuré :
//! `List of available languages in "./" (0)`. Pointé sur `…\tesseract4\fast`, il
//! voit les trois. C'est donc **vérifié AVANT de lancer**, pas découvert après.
//!
//! ⭐ **« CELLE DE WINDOWS » EST `Windows.Media.Ocr`.** Le paquet `fr-fr` est
//! installé (`C:\Windows\OCR\fr-fr`).
//!
//! # ⛔ LES DEUX NE SE LANCENT PAS DE LA MÊME FAÇON, ET C'EST TOUT LE SUJET
//!
//! - **Tesseract est un SOUS-PROCESSUS** : on démarre, et on relève à chaque
//!   frame avec `try_wait` — **exactement** le motif de `win32::scanner::Scan`.
//!   Pas de fil, pas de canal : la boucle de rendu ne l'attend jamais.
//! - **Windows est une opération WinRT ASYNCHRONE**, et elle **gèle l'application
//!   entière** si on l'attend depuis le thread principal (STA). Le dépôt en porte
//!   le récit (`win32::taskbar_pins` : « l'app ENTIÈRE figée au démarrage »,
//!   toutes les fenêtres `IsHungAppWindow`). Elle tourne donc sur **un fil dédié
//!   MTA**, initialisé `CoInitializeEx(COINIT_MULTITHREADED)`, et rend son
//!   résultat par un canal — le motif de `win32::media`.
//!
//! # Le contrat, en une phrase
//!
//! `lancer` rend **tout de suite** (ou l'erreur qui empêche de commencer), et
//! `relever` rend `None` tant que ça travaille. **Un échec n'est jamais avalé** :
//! il sort en `Etat::Echec`, avec la phrase à montrer.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::win32::scanner::{self, OCR_LANGUE};

/// Pas de fenêtre de console pour un sous-processus — voir `win32::scanner`.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// **LA LANGUE DE WINDOWS.** ⚠️ **PAS LA MÊME VOCABULAIRE QUE TESSERACT** :
/// Tesseract veut `fra` (le code de son paquet de données), `Windows.Media.Ocr`
/// veut une étiquette BCP-47. Les confondre donnerait un moteur qui refuse de se
/// créer — d'où deux constantes distinctes, jamais une seule « langue ».
const LANGUE_WINDOWS: &str = "fr-FR";

/// Où Tesseract se trouve, dans l'ordre d'essai — même liste que `scanner`,
/// même raison (un raccourci mort de `Program Files (x86)` a déjà été relevé sur
/// cette machine alors que le programme vit dans `Program Files`).
const TESSERACT_CANDIDATS: [&str; 2] = [
    r"C:\Program Files\NAPS2\lib\_win64\tesseract.exe",
    r"C:\Program Files (x86)\NAPS2\lib\_win64\tesseract.exe",
];

// ---------------------------------------------------------------------------
// LES MOTEURS — nommés, et chacun sait se relire
// ---------------------------------------------------------------------------

/// **LE MOTEUR DE RECONNAISSANCE.** Une énumération NOMMÉE, pas un booléen : le
/// dépôt paie assez souvent le défaut d'un état qui ne dit pas lequel il est.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Moteur {
    /// Tesseract, celui que NAPS2 embarque — « l'intégré ».
    Tesseract,
    /// `Windows.Media.Ocr` — « celle de Windows ».
    Windows,
}

impl Moteur {
    pub const TOUS: [(Moteur, &'static str, &'static str); 2] = [
        (Moteur::Tesseract, "tesseract", "NAPS2"),
        (Moteur::Windows, "windows", "Copilot"),
    ];

    /// Le nom que la page envoie — jamais le libellé, qui peut changer.
    pub fn nom(self) -> &'static str {
        match self {
            Moteur::Tesseract => "tesseract",
            Moteur::Windows => "windows",
        }
    }

    /// Le nom montré.
    ///
    /// ⭐ **« NAPS2 » ET « COPILOT »** — DEMANDE EXPLICITE (2026-11-04) : « le
    /// choix entre naps2 et copilot ». Ce sont les deux noms que l'utilisateur
    /// connaît : le premier est le Tesseract que NAPS2 embarque, le second est
    /// **le moteur de Windows** (`Windows.Media.Ocr`, nommé « Windows »
    /// jusqu'ici) — celui que l'utilisateur désigne par Copilot. Le nom
    /// technique reste `nom()`, que les pages envoient : renommer le libellé ne
    /// casse aucun verbe.
    pub fn libelle(self) -> &'static str {
        match self {
            Moteur::Tesseract => "NAPS2",
            Moteur::Windows => "Copilot",
        }
    }

    pub fn depuis_nom(texte: &str) -> Option<Self> {
        Self::TOUS.iter().find(|(_, connu, _)| connu.eq_ignore_ascii_case(texte)).map(|(moteur, _, _)| *moteur)
    }
}

/// **OÙ EN EST UNE RECONNAISSANCE.** Trois états NOMMÉS — voir la mémoire du
/// dépôt : un `Option<Enum>` est un `match` déguisé, et un booléen « en cours »
/// ne saurait pas porter la raison d'un échec.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Etat {
    EnCours,
    Pret(String),
    /// Le message est **destiné à l'écran** : un moteur qui échoue doit dire
    /// pourquoi, jamais rester muet.
    Echec(String),
}

impl Etat {
    /// Le nom que la page lit dans l'état JSON.
    pub fn nom(&self) -> &'static str {
        match self {
            Etat::EnCours => "encours",
            Etat::Pret(_) => "pret",
            Etat::Echec(_) => "echec",
        }
    }

    pub fn texte(&self) -> Option<&str> {
        match self {
            Etat::Pret(texte) => Some(texte),
            _ => None,
        }
    }

    pub fn message(&self) -> Option<&str> {
        match self {
            Etat::Echec(message) => Some(message),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// LA DISPONIBILITÉ — vérifiée, jamais devinée
// ---------------------------------------------------------------------------

/// Le chemin de `tesseract.exe`, s'il existe.
pub fn chemin_tesseract() -> Option<PathBuf> {
    TESSERACT_CANDIDATS.iter().map(PathBuf::from).find(|chemin| chemin.is_file())
}

/// **LE DOSSIER `tessdata` QUI CONTIENT LA LANGUE.** `None` si aucun ne l'a.
///
/// ⭐ **C'EST LA PIÈCE SANS LAQUELLE RIEN NE MARCHE** : sans `TESSDATA_PREFIX`,
/// Tesseract cherche dans `./` et ne voit **aucune** langue. On ne se contente
/// donc pas de trouver le dossier — on vérifie qu'il porte **le fichier de la
/// langue**, parce qu'un dossier `tessdata` vide donnerait exactement le même
/// silence.
///
/// ⚠️ **`fast` AVANT `best`** : les deux sont là, `best` est plus précis (13,9 Mo
/// contre 1,1) et plus lent. On prend le rapide, comme NAPS2 lui-même
/// (`OcrDefaultMode: Fast` dans son `appsettings.xml`).
pub fn tessdata() -> Option<PathBuf> {
    tessdata_pour(OCR_LANGUE)
}

/// La même chose, pour une langue donnée — séparée pour être **vérifiable**,
/// y compris avec une langue qui n'existe pas.
pub fn tessdata_pour(langue: &str) -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    let base = Path::new(&appdata).join("NAPS2").join("components").join("tesseract4");
    let fichier = format!("{langue}.traineddata");
    ["fast", "best"]
        .iter()
        .map(|mode| base.join(mode))
        .find(|dossier| dossier.join(&fichier).is_file())
}

// ---------------------------------------------------------------------------
// LA RECONNAISSANCE
// ---------------------------------------------------------------------------

/// **UNE RECONNAISSANCE LANCÉE, PAS ENCORE RELEVÉE.** Les deux moteurs n'ont pas
/// la même mécanique, et c'est ce `enum` qui les tient derrière une seule forme.
enum Travail {
    /// Un sous-processus : on relève avec `try_wait`, à chaque frame.
    Tesseract { enfant: Child, sortie: PathBuf },
    /// Un fil WinRT : on relève avec `try_recv`, qui ne bloque jamais.
    Windows { canal: Receiver<Result<String, String>> },
}

pub struct Reconnaissance {
    travail: Travail,
    /// Ce que le sous-processus a dit de travers — lu UNE fois, à la sortie.
    annule: bool,
}

impl Reconnaissance {
    /// **DÉMARRE.** Rend l'erreur qui empêche de commencer — outil absent, langue
    /// absente, fichier illisible — **tout de suite**, pour que le bouton puisse
    /// le dire sans attendre une frame.
    pub fn lancer(page: &Path, moteur: Moteur) -> Result<Self, String> {
        if !page.is_file() {
            return Err(format!("La page {} n'existe plus.", page.display()));
        }
        let travail = match moteur {
            Moteur::Tesseract => lancer_tesseract(page)?,
            Moteur::Windows => lancer_windows(page)?,
        };
        Ok(Self { travail, annule: false })
    }

    /// `None` tant que ça travaille ; sinon l'état FINAL, rendu **une seule fois**.
    pub fn relever(&mut self) -> Option<Etat> {
        match &mut self.travail {
            Travail::Tesseract { enfant, sortie } => relever_tesseract(enfant, sortie, self.annule),
            Travail::Windows { canal } => match canal.try_recv() {
                Ok(Ok(texte)) => Some(Etat::Pret(texte)),
                Ok(Err(message)) => Some(Etat::Echec(message)),
                Err(TryRecvError::Empty) => None,
                // ⚠️ **UN FIL MORT DOIT SE DIRE.** Sans ce bras, `try_recv`
                // rendrait `Disconnected` pour toujours et le bouton resterait
                // sur « en cours » sans fin — le pire des silences.
                Err(TryRecvError::Disconnected) => {
                    Some(Etat::Echec("La reconnaissance Windows s'est interrompue.".to_string()))
                }
            },
        }
    }

}

// --- Le bras Tesseract : un sous-processus, relevé à chaque frame ------------

fn lancer_tesseract(page: &Path) -> Result<Travail, String> {
    use std::os::windows::process::CommandExt;

    let exe = chemin_tesseract().ok_or_else(|| {
        "Tesseract est introuvable — il vient avec NAPS2, attendu dans « Program Files\\NAPS2\\lib\\_win64 ». Installez NAPS2.".to_string()
    })?;
    let data = tessdata().ok_or_else(|| {
        format!(
            "La langue « {OCR_LANGUE} » est absente des données de NAPS2 (attendu : %APPDATA%\\NAPS2\\components\\tesseract4). Ouvrez NAPS2 une fois et lancez-y un OCR pour la télécharger."
        )
    })?;

    // ⚠️ **UN ALIAS PAR LECTURE, ET ICI UN FICHIER PAR PAGE** : deux OCR lancés
    // en parallèle sur deux pages différentes ne doivent pas écrire au même
    // endroit. Le nom de la page fait donc partie du nom du texte.
    let base = scanner::dossier_session().join(format!("ocr-{}", nom_sur_de_la_page(page)));
    let _ = std::fs::create_dir_all(scanner::dossier_session());

    let enfant = Command::new(&exe)
        .args(arguments_tesseract(page, &base))
        // ⛔ **SANS `TESSDATA_PREFIX`, AUCUNE LANGUE N'EST TROUVÉE** (mesuré).
        .env("TESSDATA_PREFIX", &data)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|erreur| format!("Tesseract n'a pas pu démarrer : {erreur}"))?;

    Ok(Travail::Tesseract { enfant, sortie: base.with_extension("txt") })
}

/// **LES ARGUMENTS DE TESSERACT — PURS, donc vérifiables sans rien lancer.**
///
/// ⭐ **MÊME DISCIPLINE QUE `scanner::arguments`, ET POUR LA MÊME RAISON** : c'est
/// la seule partie de ce bras qui décide quelque chose, et un sous-processus se
/// lance mal sans que rien ne le dise. Une fois mesuré :
///
/// ```text
/// tesseract.exe <page> <base> -l fra
/// ```
///
/// ⚠️ **LA SORTIE EST UNE BASE, PAS UN FICHIER** : Tesseract ajoute lui-même
/// `.txt` — d'où `relever_tesseract`, qui lit `base.with_extension("txt")`. Lui
/// passer un chemin complet donnerait `page-1.jpg.txt`, et on chercherait un
/// fichier qui n'existe pas.
pub fn arguments_tesseract(page: &Path, base: &Path) -> Vec<std::ffi::OsString> {
    vec![page.into(), base.into(), "-l".into(), OCR_LANGUE.into()]
}

/// **LE VERDICT D'UN OCR TESSERACT.**
///
/// ⛔ **LE FICHIER EST LA SEULE PREUVE QUI VAUT** — même doctrine que
/// `scanner::Scan::relever` : Tesseract peut sortir en 0 sans rien écrire, et
/// annoncer une reconnaissance sur un code de sortie serait annoncer ce qu'on n'a
/// pas vérifié. La sortie du processus n'est lue que pour dire **pourquoi** ça a
/// échoué.
fn relever_tesseract(enfant: &mut Child, sortie: &Path, annule: bool) -> Option<Etat> {
    use std::io::Read;

    let statut = match enfant.try_wait() {
        Ok(None) => return None,
        Ok(Some(statut)) => statut,
        Err(erreur) => return Some(Etat::Echec(format!("La reconnaissance est perdue : {erreur}"))),
    };
    // ⚠️ **ABANDONNÉ : ON NE LIT RIEN.** Le fichier peut être à moitié écrit, et
    // rendre un texte tronqué serait pire que ne rien rendre — le nom de ce
    // défaut est « une réponse qui a l'air complète ».
    if annule {
        let _ = std::fs::remove_file(sortie);
        return Some(Etat::Echec("Reconnaissance abandonnée.".to_string()));
    }

    // ⚠️ **ON NETTOIE APRÈS LECTURE, JAMAIS AVANT** : le `.txt` est la SOURCE du
    // texte, pas un résidu qu'on jette à l'aveugle.
    if let Some(texte) = std::fs::read_to_string(sortie).ok() {
        let _ = std::fs::remove_file(sortie);
        let propre = texte.trim();
        // ⭐ **« RIEN TROUVÉ » N'EST PAS « ÉCHEC »** : une page blanche donne zéro
        // caractère, et c'est une réponse honnête.
        return Some(Etat::Pret(propre.to_string()));
    }

    // Pas de fichier : Tesseract a échoué. On lit ce qu'il a dit.
    let mut detail = String::new();
    if let Some(mut sortie_std) = enfant.stdout.take() {
        let _ = sortie_std.read_to_string(&mut detail);
    }
    if let Some(mut erreurs) = enfant.stderr.take() {
        let mut suite = String::new();
        let _ = erreurs.read_to_string(&mut suite);
        if !suite.trim().is_empty() {
            detail.push_str(&suite);
        }
    }
    let derniere = detail.lines().map(str::trim).filter(|l| !l.is_empty()).next_back().unwrap_or("");
    Some(Etat::Echec(match (statut.success(), derniere.is_empty()) {
        (_, false) => derniere.to_string(),
        (false, true) => format!(
            "Tesseract a échoué (code {}).",
            statut.code().map_or("?".to_string(), |c| c.to_string())
        ),
        (true, true) => "Tesseract s'est terminé sans rien écrire — la page est peut-être vide ou illisible.".to_string(),
    }))
}

/// Le nom d'une page, sans extension ni dossier — pour nommer son texte.
fn nom_sur_de_la_page(page: &Path) -> String {
    page.file_stem().map(|nom| nom.to_string_lossy().into_owned()).unwrap_or_else(|| "page".to_string())
}

// --- Le bras Windows : un fil MTA dédié, jamais le fil principal -------------

/// **⛔ CE FIL EXISTE POUR UNE RAISON PRÉCISE, ET LE DÉFAUT EST DÉJÀ ARRIVÉ.**
///
/// `IAsyncOperation::join()` depuis le thread PRINCIPAL — qui est STA — **ne rend
/// jamais la main** : en appartement à thread unique, l'achèvement d'une opération
/// asynchrone WinRT a besoin que ce même thread pompe des messages, ce que
/// `join()` l'empêche justement de faire. Le dépôt en porte le récit dans
/// `win32::taskbar_pins` : **l'application ENTIÈRE figée au démarrage**, toutes
/// les fenêtres `IsHungAppWindow`.
///
/// On démarre donc un fil **MTA**, et on ne communique que par un canal.
fn lancer_windows(page: &Path) -> Result<Travail, String> {
    let (tx, canal) = mpsc::channel();
    let chemin = page.to_path_buf();
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_MULTITHREADED,
            );
        }
        let resultat = reconnaitre_avec_windows(&chemin);
        unsafe { windows::Win32::System::Com::CoUninitialize() };
        let _ = tx.send(resultat);
    });
    Ok(Travail::Windows { canal })
}

/// La chaîne WinRT complète. **Elle s'écrit comme du Rust synchrone parce qu'on
/// est sur un fil MTA** — les `.join()` y sont légitimes (voir `lancer_windows`).
fn reconnaitre_avec_windows(page: &Path) -> Result<String, String> {
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
    use windows::core::HSTRING;

    let octets = std::fs::read(page).map_err(|erreur| format!("{} est illisible : {erreur}", page.display()))?;

    // ⚠️ **ON PASSE PAR UN FLUX EN MÉMOIRE, PAS PAR UN FICHIER** : la page est
    // déjà lue, et `BitmapDecoder` sait lire un flux aussi bien qu'un fichier —
    // sans avoir à ouvrir un `StorageFile`, donc sans permission supplémentaire.
    let flux = InMemoryRandomAccessStream::new().map_err(|e| format!("flux en mémoire impossible : {e}"))?;
    let ecrivain = DataWriter::CreateDataWriter(&flux).map_err(|e| format!("écrivain impossible : {e}"))?;
    ecrivain.WriteBytes(&octets).map_err(|e| format!("écriture impossible : {e}"))?;
    ecrivain.StoreAsync().and_then(|op| op.join()).map_err(|e| format!("écriture impossible : {e}"))?;
    ecrivain.FlushAsync().and_then(|op| op.join()).map_err(|e| format!("écriture impossible : {e}"))?;
    let _ = ecrivain.DetachStream();
    flux.Seek(0).map_err(|e| format!("flux non rembobinable : {e}"))?;

    let decodeur = BitmapDecoder::CreateAsync(&flux)
        .and_then(|op| op.join())
        .map_err(|e| format!("image non décodable : {e}"))?;
    let bitmap = decodeur
        .GetSoftwareBitmapAsync()
        .and_then(|op| op.join())
        .map_err(|e| format!("bitmap non lisible : {e}"))?;

    // ⭐ **LA LANGUE, ET LE REPLI.** `fr-FR` est le paquet mesuré sur cette
    // machine ; s'il manque (autre machine), on retombe sur la langue de
    // l'utilisateur — et si ça échoue AUSSI, on le DIT, avec quoi faire.
    let langue = Language::CreateLanguage(&HSTRING::from(LANGUE_WINDOWS)).ok();
    let moteur = langue
        .and_then(|langue| OcrEngine::TryCreateFromLanguage(&langue).ok())
        .or_else(|| OcrEngine::TryCreateFromUserProfileLanguages().ok())
        .ok_or_else(|| {
            format!(
                "Aucun moteur OCR n'est disponible pour « {LANGUE_WINDOWS} ». Ajoutez la langue dans Paramètres › Heure et langue › Langue et région."
            )
        })?;

    let resultat = moteur
        .RecognizeAsync(&bitmap)
        .and_then(|op| op.join())
        .map_err(|e| format!("reconnaissance impossible : {e}"))?;
    // ⭐ **ON RECOLLE LES LIGNES, ON NE PREND PAS `Text()` TEL QUEL.**
    // Mesuré sur une facture de test (2026-10-09) : `Text()` rend **tout sur une
    // seule ligne** — 183 caractères d'un bloc, « FACTURE 2026-0912 Client :
    // Societe Exemple Date : … ». Tesseract, lui, en rendait 13 lignes fidèles.
    // Comme le but est de POUVOIR COMPARER les deux, les faire diverger sur la
    // mise en page ferait conclure à une différence de qualité qui n'existe pas.
    let lignes = resultat.Lines().map_err(|e| format!("lignes illisibles : {e}"))?;
    let mut morceaux = Vec::new();
    for i in 0..lignes.Size().unwrap_or(0) {
        if let Ok(ligne) = lignes.GetAt(i) {
            if let Ok(texte) = ligne.Text() {
                morceaux.push(texte.to_string());
            }
        }
    }
    if morceaux.is_empty() {
        // Repli : pas de lignes, mais peut-être un texte.
        let texte = resultat.Text().map_err(|e| format!("texte illisible : {e}"))?;
        return Ok(texte.to_string().trim().to_string());
    }
    Ok(morceaux.join("\n").trim().to_string())
}

// ---------------------------------------------------------------------------
// LES SORTIES
// ---------------------------------------------------------------------------

/// **COPIE LE TEXTE DANS LE PRESSE-PAPIERS** — et rend `false` si un autre
/// programme le tient, ce que l'appelant doit DIRE (voir `clipboard::write_text`).
pub fn copier_le_texte(texte: &str) -> bool {
    crate::win32::clipboard::write_text(texte)
}

/// Le nom proposé pour un `.txt` — **PURE**, donc vérifiable sans rien écrire.
pub fn nom_de_fichier_texte(page: &Path, moteur: Moteur) -> String {
    format!("{}_{}.txt", nom_sur_de_la_page(page), moteur.nom())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les noms des moteurs font l'aller-retour — c'est ce que la page envoie.
    #[test]
    fn les_noms_de_moteurs_font_l_aller_retour() {
        for (moteur, nom, _libelle) in Moteur::TOUS {
            assert_eq!(Moteur::depuis_nom(nom), Some(moteur));
            assert_eq!(Moteur::depuis_nom(&nom.to_uppercase()), Some(moteur));
        }
        assert_eq!(Moteur::depuis_nom("truc"), None);
        // ⚠️ **LES DEUX NOMS SONT DISTINCTS** : deux moteurs qui porteraient le
        // même nom rendraient le choix impossible, et la comparaison aussi.
        assert_ne!(Moteur::Tesseract.nom(), Moteur::Windows.nom());
    }

    /// ⭐ **PUR, DONC VÉRIFIABLE SANS RIEN LANCER** — même discipline que
    /// `scanner::arguments`.
    #[test]
    fn le_nom_du_fichier_texte_suit_la_page_et_le_moteur() {
        let page = Path::new(r"C:\Temp\neuradesk-scanner\page-3.jpg");
        assert_eq!(nom_de_fichier_texte(page, Moteur::Tesseract), "page-3_tesseract.txt");
        assert_eq!(nom_de_fichier_texte(page, Moteur::Windows), "page-3_windows.txt");
    }

    /// ⭐ **LA LIGNE DE COMMANDE ENTIÈRE, ET DANS L'ORDRE.** Comparer le vecteur
    /// complet plutôt que des présences éparses : c'est ce qui attrape un argument
    /// ajouté au mauvais endroit, ou une base d'écriture devenue un chemin
    /// complet (Tesseract écrirait alors `page-1.jpg.txt`).
    #[test]
    fn la_ligne_de_commande_de_tesseract() {
        let page = Path::new(r"C:\Temp\neuradesk-scanner\page-1.jpg");
        let base = Path::new(r"C:\Temp\neuradesk-scanner\ocr-page-1");
        assert_eq!(
            arguments_tesseract(page, base),
            vec![
                std::ffi::OsString::from(r"C:\Temp\neuradesk-scanner\page-1.jpg"),
                std::ffi::OsString::from(r"C:\Temp\neuradesk-scanner\ocr-page-1"),
                std::ffi::OsString::from("-l"),
                std::ffi::OsString::from(OCR_LANGUE),
            ]
        );
        // ⚠️ **LA SORTIE N'A PAS D'EXTENSION** : c'est Tesseract qui pose `.txt`.
        let sortie = base.with_extension("txt");
        assert!(sortie.ends_with("ocr-page-1.txt"), "{sortie:?}");
        assert_eq!(sortie, Path::new(r"C:\Temp\neuradesk-scanner\ocr-page-1.txt"));
    }

    /// ⛔ **UNE LANGUE ABSENTE SE DIT, ELLE NE LÈVE PAS.** `tessdata_pour` fait
    /// une lecture de disque : elle doit rendre `None` proprement, pour que
    /// `lancer` en fasse un `Err` NOMMÉ (« la langue fra est absente de … »)
    /// plutôt qu'un plantage — le binaire est compilé avec `panic = "abort"`.
    ///
    /// ⚠️ **ET LA LANGUE QUI EXISTE DOIT SE TROUVER**, sinon ce test passerait
    /// aussi sur une machine sans NAPS2 et ne garderait rien.
    #[test]
    fn une_langue_absente_rend_none_sans_lever() {
        assert_eq!(tessdata_pour("zzz-cette-langue-n-existe-pas"), None);
        if chemin_tesseract().is_some() {
            let dossier = tessdata_pour(OCR_LANGUE)
                .unwrap_or_else(|| panic!("Tesseract est la mais {OCR_LANGUE} est introuvable"));
            assert!(dossier.join(format!("{OCR_LANGUE}.traineddata")).is_file());
        }
    }

    /// Un état nomme ce qu'il porte — et **un échec porte TOUJOURS sa phrase** :
    /// c'est la règle du dépôt, un moteur qui échoue ne reste pas muet.
    #[test]
    fn les_etats_nomment_ce_qu_ils_portent() {
        assert_eq!(Etat::EnCours.nom(), "encours");
        assert_eq!(Etat::EnCours.texte(), None);
        assert_eq!(Etat::Pret("bonjour".to_string()).nom(), "pret");
        assert_eq!(Etat::Pret("bonjour".to_string()).texte(), Some("bonjour"));
        let echec = Etat::Echec("Tesseract est introuvable".to_string());
        assert_eq!(echec.nom(), "echec");
        assert_eq!(echec.texte(), None);
        assert!(echec.message().is_some_and(|m| !m.is_empty()), "un echec muet serait le defaut refuse");
    }

    /// Un lancement sur un fichier absent rend une phrase, tout de suite — pas un
    /// « en cours » qui ne finira jamais.
    #[test]
    fn une_page_absente_est_refusee_immediatement() {
        let resultat = Reconnaissance::lancer(Path::new(r"C:\nulle-part\page-99.jpg"), Moteur::Tesseract);
        let message = resultat.err().expect("un fichier absent doit etre refuse");
        assert!(message.contains("n'existe plus"), "{message}");
    }

    // -----------------------------------------------------------------------
    // LES SONDES — `cargo test --bins -- --ignored --nocapture sonde_ocr`
    //
    // ⛔ **CE QU'UNE CAPTURE NE PEUT PAS PROUVER, ET POURQUOI CES SONDES
    // EXISTENT.** Le volet d'OCR s'ouvre au CLIC, et le harnais de capture a un
    // clic AVEUGLE (voir la mémoire du dépôt : un clic a déjà réécrit une
    // configuration). Personne ne peut donc photographier un résultat de
    // reconnaissance. Ces deux sondes lancent les VRAIS moteurs et impriment ce
    // qu'ils rendent — c'est la seule vérification qui vaille pour cette partie.
    //
    // Elles ne tournent que sur demande (`--ignored`) et ne font rien si
    // `NEURADESK_SONDE_OCR` n'est pas posée : aucun test automatique ne doit
    // dépendre d'un NAPS2 installé ni d'une image qui traîne.
    // -----------------------------------------------------------------------

    fn image_de_sonde() -> Option<PathBuf> {
        let chemin = std::env::var_os("NEURADESK_SONDE_OCR").map(PathBuf::from)?;
        if chemin.is_file() {
            Some(chemin)
        } else {
            eprintln!("NEURADESK_SONDE_OCR pointe sur {} — absent.", chemin.display());
            None
        }
    }

    /// **LA SONDE COMMUNE** : lance un moteur sur l'image de sonde et imprime son
    /// verdict. Partagée par les deux sondes pour qu'elles ne puissent pas
    /// diverger dans leur façon d'attendre.
    fn sonder(moteur: Moteur) {
        let Some(image) = image_de_sonde() else {
            eprintln!("Posez NEURADESK_SONDE_OCR=<chemin d'une image> pour sonder.");
            return;
        };
        let mut reconnaissance = match Reconnaissance::lancer(&image, moteur) {
            Ok(reconnaissance) => reconnaissance,
            Err(message) => {
                eprintln!("[{}] LANCEMENT REFUSE : {message}", moteur.nom());
                return;
            }
        };
        // On attend « à la main » : c'est la même boucle que la frame, en serré.
        for _ in 0..600 {
            match reconnaissance.relever() {
                Some(etat) => {
                    match etat {
                        Etat::Pret(texte) => {
                            let lignes = texte.lines().count();
                            eprintln!(
                                "[{}] {} caracteres, {lignes} lignes :\n---\n{texte}\n---",
                                moteur.nom(),
                                texte.chars().count()
                            );
                        }
                        Etat::Echec(message) => eprintln!("[{}] ECHEC : {message}", moteur.nom()),
                        Etat::EnCours => unreachable!("relever ne rend EnCours que par erreur"),
                    }
                    return;
                }
                None => std::thread::sleep(std::time::Duration::from_millis(25)),
            }
        }
        eprintln!("[{}] RIEN APRES 15 s", moteur.nom());
    }

    /// `cargo test --bins -- --ignored --nocapture sonde_ocr_tesseract`
    #[test]
    #[ignore = "sonde : lance le vrai Tesseract, NEURADESK_SONDE_OCR=<image>"]
    fn sonde_ocr_tesseract() {
        sonder(Moteur::Tesseract);
    }

    /// `cargo test --bins -- --ignored --nocapture sonde_ocr_windows`
    ///
    /// ⚠️ **C'EST LA SEULE FOIS QUE LE BRAS WINRT EST EXERCÉ.** Il ne peut pas
    /// l'être par les tests ordinaires : il démarre un fil MTA et attend une
    /// opération asynchrone du système. Si quelque chose ne va pas de ce côté
    /// (format de pixel refusé, paquet de langue absent), c'est ICI que ça se
    /// voit — et nulle part ailleurs.
    #[test]
    #[ignore = "sonde : lance le vrai Windows.Media.Ocr, NEURADESK_SONDE_OCR=<image>"]
    fn sonde_ocr_windows() {
        sonder(Moteur::Windows);
    }
}
