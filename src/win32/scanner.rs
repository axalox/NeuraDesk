//! **L'APPLET SCANNER** — elle pilote NAPS2 depuis le dock (voir
//! `win32::cef_scanner` pour le panneau, et `assets/popups/scanner.html`).
//!
//! # Pourquoi NAPS2, et par quelle porte
//!
//! NAPS2 est un logiciel de numérisation GPL, installé par l'utilisateur — il
//! n'est PAS dans ce dépôt. Il expose deux interfaces : un SDK .NET
//! (`NAPS2.Sdk`), inutilisable d'ici (ni COM, ni interface C), et un
//! **exécutable console**, `NAPS2.Console.exe`, qui EST la porte de ce module.
//!
//! ⛔ **APPELE COMME UN PROCESSUS SÉPARÉ, JAMAIS LIÉ** : c'est ce qui tient la
//! GPL à distance, et c'est aussi la seule forme possible depuis Rust.
//!
//! # ⭐ LE PROFIL PORTE L'APPAREIL, LES OPTIONS PORTENT LE RESTE
//!
//! `-p "<nom du profil>"` : c'est le profil NAPS2 qui nomme le scanner. Sans lui
//! il faudrait `--driver` **et** `--device`, c'est-à-dire deviner — et
//! `--listdevices` exige de toute façon `--driver` (mesuré : sans lui, NAPS2 sort
//! en erreur). Les réglages du panneau écrasent ensuite le profil par des options
//! explicites.
//!
//! # Ce qui a été MESURÉ sur cette machine (2026-10-09)
//!
//! - NAPS2 **8.4.1**, `C:\Program Files\NAPS2\NAPS2.Console.exe` ;
//! - scanner **Brother MFC-J480DW LAN**, **deux profils** dans
//!   `%APPDATA%\NAPS2\profiles.xml` : `TW-Brother MFC-J480DW LAN` (TWAIN,
//!   **défaut**) et `Brother MFC-J480DW LAN` (WIA) — A4, 300 dpi, couleur, vitre ;
//! - OCR **`fra`** installé (Tesseract) : l'OCR français marche.
//!
//! ⚠️ **LA SOURCE EST LA VITRE** (`PaperSource: Glass`) et l'appareil est **sur
//! le réseau** : un scan demande qu'on pose la feuille, et il échoue si
//! l'imprimante dort. Ce module ne peut rien y faire — il le DIT dans son
//! message d'échec au lieu de rester muet.
//!
//! # ⭐ LE MULTIPAGE NE DEMANDE RIEN DE NEUF
//!
//! **`-i/--import` accepte PLUSIEURS fichiers séparés par des points-virgules, et
//! se combine à `-n 0`** — relevé dans l'aide de NAPS2 : « The `-i/--import`
//! option can be combined with `-n 0` to combine files or change file type
//! without scanning ». C'est EXACTEMENT un assembleur, et **il n'a pas besoin du
//! scanner** pour ça.
//!
//! Un document multipage n'est donc rien de plus qu'une **`Session`** : chaque
//! clic numérise **UNE** page dans un fichier temporaire dont on connaît le
//! chemin, la liste s'allonge, et « Terminer » assemble (`arguments_assemblage`).
//!
//! ⭐ **UNE PAGE EST TOUJOURS UNE IMAGE** (`EXTENSION_PAGE`) — voir la constante,
//! qui porte la raison MESURÉE : le dépôt n'a aucun rastériseur PDF, donc une page
//! PDF ne pourrait être ni montrée, ni vignettée, ni lue par un moteur d'OCR.
//! **Le format du DOCUMENT reste `Reglages::extension`**, et il se choisit
//! maintenant **à « Terminer »**, une fois les pages vues — il n'y a plus de
//! format « figé par la première page ».
//!
//! ⭐ **C'EST L'ASSEMBLAGE QUI PORTE L'OCR, PAS LA PASSE** : NAPS2 **ignore
//! `--ocrlang` en silence** quand la sortie est une image (mesuré), et produit une
//! **vraie couche texte** (`/ToUnicode`, police `Type0`) quand la sortie est un
//! PDF. Voir `arguments` et `arguments_assemblage`.
//!
//! ⛔ **NOUS NOMMONS NOUS-MÊMES TOUTES LES SORTIES** (`nom_de_document`,
//! `Session::chemin_a_venir`) : NAPS2 sait écrire `$(YYYY)` dans un `-o`, mais un
//! chemin à jetons obligerait ensuite à RETROUVER le fichier en cherchant le plus
//! récent du dossier. Un nom littéral se connaît d'avance, et deviner n'est pas
//! vérifier.
//!
//! # Le lancement ne bloque jamais la boucle de rendu
//!
//! ⭐ **`try_wait()` + relevé à chaque frame**, exactement le motif de
//! `win32::decodeur_document` : on démarre le sous-processus, et `Scan::relever`
//! rend `None` tant qu'il travaille. Pas de fil, pas de canal — un scan dure
//! quelques secondes, et la boucle de rendu ne l'attend pas.
//!
//! ⛔ **`CREATE_NO_WINDOW`**, comme le décodeur : `NAPS2.Console.exe` est un
//! exécutable CONSOLE, et le lancer depuis une application sans console lui en
//! ferait allouer une — donc une fenêtre noire qui clignote par-dessus le dock.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// Où NAPS2 s'installe, dans l'ordre d'essai.
///
/// ⚠️ **LES DEUX, ET C'EST MESURÉ** : un raccourci mort de `Program Files (x86)`
/// a déjà été relevé dans le menu Démarrer de cette machine alors que le
/// programme vit dans `Program Files` (voir `win32::apps`, qui écarte ce
/// raccourci). Un chemin unique ferait dépendre l'applet d'une installation.
const EXE_CANDIDATS: [&str; 2] = [
    r"C:\Program Files\NAPS2\NAPS2.Console.exe",
    r"C:\Program Files (x86)\NAPS2\NAPS2.Console.exe",
];

/// La langue de l'OCR — DEMANDE : le dépôt est francophone, et `fra` est
/// installé (mesuré).
///
/// ⚠️ **`pub` PARCE QU'ELLE SERT À DEUX ENDROITS, ET DOIT RESTER UNE SEULE** :
/// NAPS2 la reçoit en `--ocrlang` (l'assemblage), et `win32::ocr` la donne à
/// Tesseract en `-l` puis cherche `<langue>.traineddata`. Deux constantes
/// divergeraient au premier changement de langue, et l'une des deux chercherait
/// un fichier qui n'existe pas.
pub const OCR_LANGUE: &str = "fra";

/// Pas de fenêtre de console pour le sous-processus — voir la doc de module.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// **UNE PASSE NUMÉRISE UNE PAGE.** Depuis le multipage, le nombre de pages n'est
/// plus un réglage : chaque clic ajoute UNE page à la session, et le document se
/// compose dans `arguments_assemblage`. Le laisser libre dans le panneau donnerait
/// un réglage sans effet — le défaut que ce dépôt refuse.
const PAGES_PAR_PASSE: u32 = 1;

/// Le nom du dossier des pages d'une session, sous `%TEMP%`.
const DOSSIER_SESSION: &str = "neuradesk-scanner";

// ---------------------------------------------------------------------------
// LES RÉGLAGES — et chacun sait se nommer, parce que la page les écrit
// ---------------------------------------------------------------------------

/// **UNE LISTE ÉCRITE DEUX FOIS SE VÉRIFIE.** Chaque famille de réglages porte
/// ses noms ET l'aller-retour, exactement comme `bandeau::Position` : la page n'a
/// pas le droit d'inventer un nom que Rust ne connaît pas, et un test compare les
/// deux côtés.
///
/// ⚠️ **ÉCRITES À LA MAIN, PAS ENGENDRÉES PAR UNE MACRO** — c'est l'idiome du
/// dépôt (`Position::TOUTES`), et trois `match` de quatre lignes se relisent mieux
/// qu'une macro qui les cache.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Vitre,
    Chargeur,
    RectoVerso,
}

impl Source {
    pub const TOUTES: [(Source, &'static str, &'static str); 3] = [
        (Source::Vitre, "glass", "Vitre"),
        (Source::Chargeur, "feeder", "Chargeur"),
        (Source::RectoVerso, "duplex", "Recto-verso"),
    ];

    /// Le nom que NAPS2 attend.
    pub fn nom(self) -> &'static str {
        match self {
            Source::Vitre => "glass",
            Source::Chargeur => "feeder",
            Source::RectoVerso => "duplex",
        }
    }

    pub fn depuis_nom(texte: &str) -> Option<Self> {
        Self::TOUTES.iter().find(|(_, connu, _)| connu.eq_ignore_ascii_case(texte)).map(|(v, _, _)| *v)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Couleur {
    Couleur,
    Gris,
    NoirBlanc,
}

impl Couleur {
    pub const TOUTES: [(Couleur, &'static str, &'static str); 3] = [
        (Couleur::Couleur, "color", "Couleur"),
        (Couleur::Gris, "gray", "Gris"),
        (Couleur::NoirBlanc, "bw", "Noir et blanc"),
    ];

    pub fn nom(self) -> &'static str {
        match self {
            Couleur::Couleur => "color",
            Couleur::Gris => "gray",
            Couleur::NoirBlanc => "bw",
        }
    }

    pub fn depuis_nom(texte: &str) -> Option<Self> {
        Self::TOUTES.iter().find(|(_, connu, _)| connu.eq_ignore_ascii_case(texte)).map(|(v, _, _)| *v)
    }
}

/// **LE FORMAT DU FICHIER ÉCRIT** — c'est l'EXTENSION du chemin qui le décide,
/// pour NAPS2 aussi (relevé dans son aide).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Extension {
    Pdf,
    Jpeg,
}

impl Extension {
    pub const TOUTES: [(Extension, &'static str, &'static str); 2] =
        [(Extension::Pdf, "pdf", "PDF"), (Extension::Jpeg, "jpg", "JPEG")];

    pub fn nom(self) -> &'static str {
        match self {
            Extension::Pdf => "pdf",
            Extension::Jpeg => "jpg",
        }
    }

    pub fn depuis_nom(texte: &str) -> Option<Self> {
        Self::TOUTES.iter().find(|(_, connu, _)| connu.eq_ignore_ascii_case(texte)).map(|(v, _, _)| *v)
    }
}

/// **LES FORMATS DE PAGE PROPOSÉS** — une liste COURTE et fermée, parce que
/// NAPS2 en accepte aussi des dimensions libres (« 8.5x11in ») et que la page
/// n'a pas à en faire un champ de saisie : qui veut un format exotique le règle
/// dans son profil NAPS2, que ce module ne remplace pas.
pub const FORMATS: [&str; 4] = ["A4", "A5", "Letter", "Legal"];
/// Les résolutions proposées.
pub const RESOLUTIONS: [u32; 3] = [200, 300, 600];

/// **CE QUE LE PANNEAU CHOISIT.** Chaque champ a un sens pour un scan, et aucun
/// n'est décoratif : `Default` porte les valeurs MESURÉES du profil de cette
/// machine, pour qu'un premier scan parte sans rien régler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reglages {
    /// Le `DisplayName` d'un profil NAPS2. **Vide = le profil le plus récent**,
    /// et c'est le repli quand `profiles.xml` est illisible ou absent.
    pub profil: String,
    pub source: Source,
    pub dpi: u32,
    pub couleur: Couleur,
    pub format: String,
    pub deskew: bool,
    pub ocr: bool,
    pub destination: PathBuf,
    pub extension: Extension,
}

impl Default for Reglages {
    fn default() -> Self {
        Self {
            profil: String::new(),
            // La vitre : c'est ce que déclarent les deux profils mesurés.
            source: Source::Vitre,
            dpi: 300,
            couleur: Couleur::Couleur,
            format: "A4".to_string(),
            deskew: false,
            ocr: false,
            destination: PathBuf::new(),
            extension: Extension::Pdf,
        }
    }
}

/// **LE DOSSIER QUE LE SYSTÈME PROPOSE** — `Documents\Scans`, où « Documents »
/// suit `FOLDERID_Documents` (donc une redirection du shell, ici
/// `OneDrive\Documents`).
pub fn dossier_par_defaut() -> PathBuf {
    crate::win32::ssh::local::dossier_documents().join("Scans")
}

impl Reglages {
    /// Les réglages d'un panneau qui n'a jamais rien enregistré : les valeurs
    /// mesurées du profil de cette machine, et le dossier proposé par le système.
    pub fn neufs() -> Self {
        Self { destination: dossier_par_defaut(), ..Self::default() }
    }

    /// **LE TEXTE ÉCRIT DANS `scanner_reglages.txt`** — `clé=valeur`, une par
    /// ligne, dans un ordre FIXE pour qu'un diff soit lisible.
    pub fn vers_texte(&self) -> String {
        let mut lignes = vec![
            format!("profil={}", self.profil),
            format!("source={}", self.source.nom()),
            format!("dpi={}", self.dpi),
            format!("couleur={}", self.couleur.nom()),
            format!("format={}", self.format),
            format!("deskew={}", if self.deskew { 1 } else { 0 }),
            format!("ocr={}", if self.ocr { 1 } else { 0 }),
            format!("extension={}", self.extension.nom()),
            format!("destination={}", self.destination.display()),
        ];
        lignes.push(String::new());
        lignes.join("\n")
    }

    /// **L'ANALYSE — PURE, ET TOLÉRANTE À DESSEIN.** Une clé inconnue, une valeur
    /// illisible ou une ligne sans `=` sont IGNORÉES : on garde le défaut. Un
    /// réglage qu'on ne sait pas relire ne doit pas empêcher le panneau de
    /// s'ouvrir — le dépôt a déjà payé le contraire (`TailleZone::depuis_cote`,
    /// où un flottant là où un entier était attendu effaçait une zone).
    pub fn depuis_texte(texte: &str, defaut: &Reglages) -> Reglages {
        let mut reglages = defaut.clone();
        for ligne in texte.lines() {
            let Some((cle, valeur)) = ligne.split_once('=') else { continue };
            let (cle, valeur) = (cle.trim(), valeur.trim());
            match cle {
                "profil" => reglages.profil = valeur.to_string(),
                "source" => reglages.source = Source::depuis_nom(valeur).unwrap_or(defaut.source),
                "dpi" => reglages.dpi = valeur.parse().unwrap_or(defaut.dpi),
                "couleur" => reglages.couleur = Couleur::depuis_nom(valeur).unwrap_or(defaut.couleur),
                "format" => {
                    if FORMATS.iter().any(|f| f.eq_ignore_ascii_case(valeur)) {
                        reglages.format = valeur.to_string();
                    }
                }
                "deskew" => reglages.deskew = valeur == "1",
                "ocr" => reglages.ocr = valeur == "1",
                // ⚠️ **`pages` A DISPARU, ET C'EST VOLONTAIRE** : c'était un
                // réglage, c'est devenu la liste de la `Session`. Un fichier
                // écrit avant le multipage en porte encore un — il tombe dans le
                // `_ => {}` ci-dessous, donc il est IGNORÉ au lieu de faire
                // échouer l'ouverture du panneau.
                "extension" => reglages.extension = Extension::depuis_nom(valeur).unwrap_or(defaut.extension),
                // ⚠️ **LE CHEMIN DOIT ÊTRE ABSOLU**, même exigence que
                // `config::chemin_retenu` : un chemin relatif serait lu depuis le
                // répertoire de travail du processus, qui n'est pas celui que
                // l'utilisateur croit. La présence du dossier, elle, n'est PAS
                // exigée — c'est nous qui le créons au premier scan.
                "destination" => {
                    let chemin = PathBuf::from(valeur);
                    if chemin.is_absolute() {
                        reglages.destination = chemin;
                    }
                }
                _ => {}
            }
        }
        reglages
    }
}

// ---------------------------------------------------------------------------
// LA LIGNE DE COMMANDE — PURE, et c'est la seule partie qui a de la logique
// ---------------------------------------------------------------------------

/// **LA LIGNE DE COMMANDE D'UNE PASSE DE SCAN, CONSTRUITE SANS RIEN LANCER.**
///
/// ⭐ **PURE À DESSEIN** : c'est la seule partie de ce module qui décide quelque
/// chose, et une fonction qui construit des arguments se teste sans scanner, sans
/// sous-processus et sans réseau — ce qui compte quand le seul appareil est sur
/// le réseau et qu'une numérisation ne se simule pas.
///
/// ⚠️ **LE CHEMIN DE SORTIE EST UN PARAMÈTRE, PAS UN RÉGLAGE** : depuis le
/// multipage, c'est l'APPELANT qui nomme chaque page (`Session::chemin_a_venir`)
/// puis le document (`nom_de_document`). `Reglages::destination` dit seulement
/// *où*, et « Terminer » seul s'en sert.
///
/// ⚠️ **L'ORDRE COMPTE PEU POUR NAPS2**, mais il est FIXE pour que le test puisse
/// comparer un vecteur entier plutôt que des présences éparses.
pub fn arguments(reglages: &Reglages, sortie: &Path) -> Vec<String> {
    let mut args = Vec::new();
    if !reglages.profil.trim().is_empty() {
        args.push("-p".to_string());
        args.push(reglages.profil.trim().to_string());
    }
    args.push("--source".to_string());
    args.push(reglages.source.nom().to_string());
    args.push("--dpi".to_string());
    args.push(reglages.dpi.to_string());
    args.push("--bitdepth".to_string());
    args.push(reglages.couleur.nom().to_string());
    args.push("--pagesize".to_string());
    args.push(reglages.format.clone());
    // ⚠️ **LE REDRESSEMENT NE S'ÉTEINT PAS** : NAPS2 n'a pas de `--nodeskew`, donc
    // une case décochée se contente de NE PAS l'activer — et c'est le profil qui
    // décide. Les deux profils mesurés portent `AutoDeskew: false`, donc le
    // résultat est celui qu'on attend. Écrit ici pour que personne ne cherche le
    // drapeau manquant.
    if reglages.deskew {
        args.push("--deskew".to_string());
    }
    // ⛔ **AUCUN ARGUMENT D'OCR ICI, ET C'EST MESURÉ.** Une passe écrit une
    // **IMAGE** (`EXTENSION_PAGE`), et NAPS2 **ignore `--ocrlang` en silence**
    // quand la sortie est une image : le 2026-10-09, `-n 0 -i page.jpg -o out.jpg
    // --ocrlang fra` est sorti en **code 0** sans rien reconnaître et sans un mot.
    // Laisser le drapeau ici ferait donc une case « OCR français » qui MENT —
    // exactement le défaut que le dépôt refuse.
    //
    // ⭐ **L'OCR DE NAPS2 VIT DANS `arguments_assemblage`** : mesuré sur le même
    // fichier, l'assemblage passe de 12 993 à 42 965 octets, avec une police
    // `Type0` composite et un `/ToUnicode` — une **vraie couche texte
    // cherchable**. C'est le PDF qui porte l'OCR, pas la page.
    args.push("-n".to_string());
    args.push(PAGES_PAR_PASSE.to_string());
    // `-f` : une page réécrite ne doit pas faire ÉCHOUER le scan en silence — et
    // c'est aussi ce qui rend la réutilisation d'un nom de page sans danger.
    args.push("-f".to_string());
    // `-v` : de quoi dire POURQUOI un scan a échoué. Sans lui, NAPS2 ne dit rien
    // quand tout va bien — et rien non plus quand ça se passe mal.
    args.push("-v".to_string());
    args.push("-o".to_string());
    args.push(sortie.display().to_string());
    args
}

/// **LA LIGNE DE COMMANDE DE L'ASSEMBLAGE — PURE, et c'est elle qui fait le
/// multipage.**
///
/// ⭐ **`-n 0` EST CE QUI INTERDIT TOUT SCAN** : il dit à NAPS2 de ne pas
/// numériser et de se contenter de combiner les fichiers de `-i`. C'est ce que
/// l'aide décrit (« combine files or change file type without scanning »), et
/// c'est pourquoi « Terminer » n'a **pas besoin du scanner** — donc pas besoin
/// qu'il soit allumé, ni que la feuille soit posée.
///
/// ⚠️ **LES CHEMINS SONT SÉPARÉS PAR DES POINTS-VIRGULES**, c'est la syntaxe de
/// `-i`. Un chemin qui en contiendrait un casserait la liste : les nôtres se
/// nomment `page-N.jpg` sous `%TEMP%`, donc seul le préfixe système pourrait en
/// porter un. C'est écrit ici pour que ce soit su, pas découvert.
///
/// ⭐ **C'EST ICI QUE VIT L'OCR** (`ocr` = la case « OCR français »). Mesuré le
/// 2026-10-09 : le même assemblage passe de 12 993 à 42 965 octets selon le
/// drapeau, et le PDF gagne une police `Type0` composite ET un `/ToUnicode` —
/// c'est-à-dire une **vraie couche texte**, extractible par un lecteur.
///
/// ⛔ **Et l'OCR NE PEUT PAS vivre sur une PASSE** : NAPS2 ignore `--ocrlang`
/// quand la sortie est une image (voir `arguments`). Une page JPEG ne peut pas
/// porter de texte.
///
/// ⚠️ `--ocrlang` IMPLIQUE `--enableocr` ; `--disableocr` est posé EXPLICITEMENT,
/// sans quoi un profil NAPS2 qui active l'OCR le ferait quand même et la case
/// décochée mentirait.
pub fn arguments_assemblage(pages: &[PathBuf], sortie: &Path, ocr: bool) -> Vec<String> {
    let liste = pages.iter().map(|page| page.display().to_string()).collect::<Vec<_>>().join(";");
    let mut args = vec![
        "-n".to_string(),
        "0".to_string(),
        "-i".to_string(),
        liste,
    ];
    if ocr {
        args.push("--ocrlang".to_string());
        args.push(OCR_LANGUE.to_string());
    } else {
        args.push("--disableocr".to_string());
    }
    args.push("-f".to_string());
    args.push("-v".to_string());
    args.push("-o".to_string());
    args.push(sortie.display().to_string());
    args
}

// ---------------------------------------------------------------------------
// LES NOMS DES SORTIES — calculés ici, donc connus d'avance
// ---------------------------------------------------------------------------

/// **L'HORODATAGE LOCAL `AAAA-MM-JJ_hhmmss`**, comme NAPS2 l'écrirait.
fn horodatage() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}-{:02}-{:02}_{:02}{:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/// Le nom du **document assemblé** : `scan-AAAA-MM-JJ_hhmmss.<ext>`.
pub fn nom_de_document(extension: Extension) -> String {
    format!("scan-{}.{}", horodatage(), extension.nom())
}

/// Le nom d'une **page écrite une à une** — le cas du JPEG, qui ne s'assemble
/// pas. **Toutes les pages d'un même document partagent l'horodatage**, ce qui
/// les fait se trier ensemble dans l'explorateur.
///
/// ⭐ **PURE, ET C'EST POUR ÇA** : le seul nom de fichier que ce module compose
/// pour l'utilisateur se vérifie sans écrire quoi que ce soit.
pub fn nom_de_page(horodatage: &str, rang: usize, extension: Extension) -> String {
    format!("scan-{horodatage}_page-{rang}.{}", extension.nom())
}

// ---------------------------------------------------------------------------
// LES PROFILS — lus, jamais devinés
// ---------------------------------------------------------------------------

/// **LES PROFILS QUE NAPS2 CONNAÎT, LUS DANS SON PROPRE FICHIER.**
///
/// ⛔ **ON NE LES ÉCRIT PAS EN DUR.** Le nom d'un profil appartient à
/// l'utilisateur : les écrire dans la page serait une seconde source de vérité,
/// qui mentirait dès qu'il en crée un — et le dépôt a déjà payé ce défaut
/// plusieurs fois.
///
/// ⚠️ **L'ABSENCE N'EST PAS UNE ERREUR.** Pas de NAPS2, pas de fichier, un XML
/// d'une autre version : on rend une liste vide, et l'appelant retombe sur le
/// profil le plus récent de NAPS2 (aucun `-p`).
pub fn profils_naps2() -> Vec<String> {
    let Some(appdata) = std::env::var_os("APPDATA") else { return Vec::new() };
    let chemin = Path::new(&appdata).join("NAPS2").join("profiles.xml");
    let Ok(xml) = std::fs::read_to_string(&chemin) else { return Vec::new() };
    profils_dans(&xml)
}

/// **L'EXTRACTION, PURE** — séparée de la lecture pour être vérifiable, même
/// discipline que `notification::textes_du_payload`, qui lit du XML à la main
/// pour la même raison : une seule balise à prendre, aucune dépendance à ajouter.
///
/// ⚠️ **`<DisplayName>` ET NON `<Name>`** : `profiles.xml` porte les deux — le
/// second nomme l'APPAREIL (`Brother MFC-J480DW LAN`), le premier le PROFIL
/// (`TW-Brother MFC-J480DW LAN`). C'est le second que `-p` attend, et les
/// confondre donnerait « profil introuvable » sur le profil TWAIN.
pub fn profils_dans(xml: &str) -> Vec<String> {
    let mut trouves = Vec::new();
    let mut reste = xml;
    while let Some(debut) = reste.find("<DisplayName>") {
        let apres = &reste[debut + "<DisplayName>".len()..];
        let Some(fin) = apres.find("</DisplayName>") else { break };
        let nom = apres[..fin].trim();
        if !nom.is_empty() {
            trouves.push(nom.to_string());
        }
        reste = &apres[fin + "</DisplayName>".len()..];
    }
    trouves
}

// ---------------------------------------------------------------------------
// LA SESSION DE PAGES — l'onglet Scan est une liste de fichiers temporaires
// ---------------------------------------------------------------------------

/// **OÙ VIVENT LES PAGES D'UNE SESSION.** Sous `%TEMP%` : ce sont des
/// INTERMÉDIAIRES, pas des documents — le document naît à « Terminer ».
pub fn dossier_session() -> PathBuf {
    std::env::temp_dir().join(DOSSIER_SESSION)
}

/// **PURGE LES RÉSIDUS D'UN LANCEMENT PRÉCÉDENT.** Appelée une seule fois, à la
/// première session de chaque lancement.
///
/// ⚠️ **CE N'EST PAS DE LA PROPRETÉ, C'EST DE LA CONFIDENTIALITÉ** : une session
/// interrompue par un arrêt brutal laisserait dans `%TEMP%` les pages d'un
/// document que l'utilisateur n'a jamais enregistré — une pièce d'identité, une
/// ordonnance. Personne ne va les y chercher.
pub fn purger_residus() {
    let _ = std::fs::remove_dir_all(dossier_session());
}

/// **LA SESSION DE PAGES** — ce que l'onglet Scan accumule jusqu'à « Terminer ».
///
/// ⭐ **RIEN À INVENTER POUR LE MULTIPAGE** : NAPS2 sait combiner des fichiers
/// (`-i a;b;c` avec `-n 0`, voir `arguments_assemblage`). Une session n'est donc
/// qu'une liste de fichiers temporaires, plus de quoi les nommer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Session {
    /// Les pages, **DANS L'ORDRE D'ASSEMBLAGE** — c'est la liste qui fait le
    /// document, pas les noms de fichiers.
    pages: Vec<PathBuf>,
    /// **LE NOMBRE DE FICHIERS DÉJÀ NOMMÉS — JAMAIS REMIS À ZÉRO PAR UN RETRAIT.**
    /// Nommer d'après la POSITION ferait réécrire un fichier encore présent quand
    /// on retire une page du milieu puis qu'on en scanne une autre.
    nommes: u32,
}

/// **LE FORMAT D'UNE PAGE DE SESSION — UNE IMAGE, TOUJOURS.**
///
/// ⛔ **CE N'EST PAS UN RÉGLAGE, C'EST UNE NÉCESSITÉ MESURÉE** : le dépôt n'a
/// **aucun rastériseur PDF** (ni pdfium, ni `Windows.Data.Pdf`, ni la feature
/// `Data_Pdf`), donc une page PDF ne peut donner **ni vignette, ni aperçu, ni
/// image à un moteur d'OCR**. Une page doit être une image pour qu'on puisse la
/// MONTRER.
///
/// ⭐ **Et ça ne coûte rien** : NAPS2 assemble des images en PDF sans scanner —
/// mesuré le 2026-10-09, `-n 0 -i p1.png;p2.png -o doc.pdf` rend un vrai PDF de
/// deux pages. Le format du DOCUMENT reste `Reglages::extension`, et il se choisit
/// maintenant **à « Terminer »**, une fois les pages vues.
pub const EXTENSION_PAGE: Extension = Extension::Jpeg;

impl Session {
    pub fn pages(&self) -> &[PathBuf] {
        &self.pages
    }

    /// **Le chemin de la page à venir** — son nom porte un rang de FICHIER, qui
    /// n'est jamais réutilisé (voir `nommes`).
    pub fn chemin_a_venir(&self) -> PathBuf {
        dossier_session().join(format!("page-{}.{}", self.nommes + 1, EXTENSION_PAGE.nom()))
    }

    /// Ajoute une page.
    pub fn ajouter(&mut self, chemin: PathBuf) {
        self.nommes += 1;
        self.pages.push(chemin);
    }

    /// **RETIRE UNE PAGE PAR SON RANG (1-based)** et rend son chemin, pour que
    /// l'appelant efface le fichier.
    ///
    /// ⚠️ **UN RANG FAUX NE RETIRE RIEN** : il vient de la page, donc la liste a
    /// pu changer entre l'affichage et le clic. On rend `None` plutôt que de
    /// retirer la mauvaise page.
    pub fn retirer(&mut self, rang: usize) -> Option<PathBuf> {
        if rang == 0 || rang > self.pages.len() {
            return None;
        }
        Some(self.pages.remove(rang - 1))
    }

    /// **VIDE LA SESSION et rend ses fichiers.** « Terminer » comme
    /// « Abandonner » les effacent : c'est ce que l'appelant en a fait qui décide
    /// si le contenu a été conservé ailleurs.
    pub fn vider(&mut self) -> Vec<PathBuf> {
        self.nommes = 0;
        std::mem::take(&mut self.pages)
    }
}

/// **ÉCRIT LES PAGES UNE À UNE** — le seul cas où « Terminer » n'assemble pas.
///
/// ⛔ **UN JPEG NE PORTE QU'UNE IMAGE** : `-i p1;p2 -o out.jpg` ne peut pas
/// produire un document de deux pages. On écrit donc les pages telles quelles
/// dans la destination, sous un horodatage COMMUN, et le panneau le DIT au lieu
/// de laisser découvrir un dossier plein de fichiers.
pub fn copier_les_pages(pages: &[PathBuf], destination: &Path, extension: Extension) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(destination)
        .map_err(|erreur| format!("Dossier de destination impossible à créer : {erreur}"))?;
    let tampon = horodatage();
    let mut ecrits = Vec::with_capacity(pages.len());
    for (rang, page) in pages.iter().enumerate() {
        let cible = destination.join(nom_de_page(&tampon, rang + 1, extension));
        if let Err(erreur) = std::fs::copy(page, &cible) {
            // ⚠️ **ON NE DÉFAIT PAS CE QUI A RÉUSSI.** Les pages déjà écrites sont
            // VALIDES, et les effacer ferait perdre du travail que l'utilisateur
            // peut garder. Ce qui manquait, c'était de **le DIRE** : le message ne
            // nommait que la fautive, donc un dossier à moitié rempli passait pour
            // un échec total.
            return Err(match ecrits.len() {
                0 => format!("{} n'a pas pu être écrit : {erreur}", cible.display()),
                deja => format!(
                    "{} n'a pas pu être écrit : {erreur} — les {deja} page(s) précédente(s) SONT dans {}.",
                    cible.display(),
                    destination.display()
                ),
            });
        }
        ecrits.push(cible);
    }
    Ok(ecrits)
}

/// **ÉCRIT LES PAGES UNE À UNE À L'ENDROIT QUE L'UTILISATEUR A CHOISI** — le cas
/// « Enregistrer sous » d'un document JPEG.
///
/// ⚠️ **UN FICHIER NE PEUT PAS EN CONTENIR PLUSIEURS** : une seule page va
/// EXACTEMENT au chemin demandé ; plusieurs se rangent à côté, sous le même nom de
/// base avec un suffixe de rang. Le dire ici évite de chercher pourquoi le
/// troisième choix n'a pas été respecté.
pub fn copier_les_pages_vers(pages: &[PathBuf], cible: &Path, extension: Extension) -> Result<Vec<PathBuf>, String> {
    let dossier = cible.parent().filter(|d| !d.as_os_str().is_empty()).ok_or_else(|| {
        format!("{} n'a pas de dossier — choisissez un emplacement complet.", cible.display())
    })?;
    std::fs::create_dir_all(dossier).map_err(|erreur| format!("Dossier impossible à créer : {erreur}"))?;
    let base = cible.file_stem().map(|nom| nom.to_string_lossy().into_owned()).unwrap_or_else(|| "scan".to_string());
    let mut ecrits = Vec::with_capacity(pages.len());
    for (rang, page) in pages.iter().enumerate() {
        let cible_page = match pages.len() {
            1 => cible.to_path_buf(),
            _ => dossier.join(format!("{base}_page-{}.{}", rang + 1, extension.nom())),
        };
        std::fs::copy(page, &cible_page)
            .map_err(|erreur| format!("{} n'a pas pu être écrit : {erreur}", cible_page.display()))?;
        ecrits.push(cible_page);
    }
    Ok(ecrits)
}

/// **EFFACE UNE PAGE TEMPORAIRE** — sans bruit : un fichier déjà absent n'est pas
/// une erreur, et l'échec d'un effacement ne doit pas interrompre le panneau.
pub fn effacer_page(page: &Path) {
    let _ = std::fs::remove_file(page);
}

// ---------------------------------------------------------------------------
// LE LANCEMENT
// ---------------------------------------------------------------------------

/// Le verdict d'un scan, une fois terminé.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub ok: bool,
    /// Ce que NAPS2 a dit — ou ce que ce module a constaté à sa place.
    pub message: String,
    /// Le fichier, quand il existe vraiment sur le disque.
    pub fichier: Option<PathBuf>,
}

/// Un scan lancé, pas encore relevé — voir la doc de module pour le pourquoi de
/// `try_wait` plutôt que d'un fil.
pub struct Scan {
    enfant: Child,
    /// **LE FICHIER ATTENDU, CONNU D'AVANCE** — c'est nous qui nommons les
    /// sorties, donc il n'y a plus à chercher après coup « le plus récent du
    /// dossier » : on sait lequel on attend, et on vérifie qu'il est là.
    sortie: PathBuf,
}

impl Scan {
    /// **ABANDONNER LE SCAN EN COURS.** Rend `true` si un processus a bien été
    /// tué.
    ///
    /// ⚠️ **CE N'EST PAS UN CONFORT, C'EST UNE NÉCESSITÉ ICI** : l'appareil est
    /// **sur le réseau** (mesuré : Brother MFC-J480DW LAN). Un scanner éteint ou
    /// endormi laisse NAPS2 attendre son tour très longtemps — sans cette sortie,
    /// le panneau resterait bloqué sur « en cours » avec un bouton mort.
    ///
    /// ⛔ **ET TUER NAPS2 PEUT LAISSER L'APPAREIL OCCUPÉ** : c'est écrit ici pour
    /// que ce soit su. Un scan abandonné peut demander d'attendre, ou de relancer
    /// NAPS2 une fois, avant que le suivant parte. On préfère ce défaut-là à une
    /// attente sans fin.
    pub fn annuler(&mut self) -> bool {
        self.enfant.kill().is_ok()
    }

    /// `None` tant que le scan travaille ; sinon le verdict, rendu UNE fois.
    pub fn relever(&mut self) -> Option<Verdict> {
        let statut = match self.enfant.try_wait() {
            Ok(None) => return None,
            Ok(Some(statut)) => statut,
            Err(erreur) => {
                return Some(Verdict { ok: false, message: format!("le scan est perdu : {erreur}"), fichier: None })
            }
        };
        // ⚠️ **LES TUYAUX SONT VIDÉS APRÈS LA SORTIE, PAS PENDANT** : la sortie
        // d'un scan est de quelques lignes, donc très en deçà du tampon d'un
        // tuyau (~64 Ko) et le sous-processus ne peut pas s'y bloquer. Une sortie
        // qui grossirait au-delà demanderait un fil de drainage — c'est écrit ici
        // pour que ce soit su, pas découvert.
        let mut texte = String::new();
        if let Some(mut sortie) = self.enfant.stdout.take() {
            let _ = sortie.read_to_string(&mut texte);
        }
        if let Some(mut erreurs) = self.enfant.stderr.take() {
            let mut suite = String::new();
            let _ = erreurs.read_to_string(&mut suite);
            if !suite.trim().is_empty() {
                texte.push_str(&suite);
            }
        }
        if statut.success() {
            // **LE FICHIER EST LA SEULE PREUVE QUI VAUT.** NAPS2 peut sortir en 0
            // sans rien écrire (aucune page saisie) : annoncer un scan réussi sur
            // un code de sortie serait annoncer ce qu'on n'a pas vérifié.
            let fichier = self.fichier_ecrit();
            return Some(match fichier {
                Some(chemin) => Verdict {
                    // ⚠️ **PAS « SCAN ENREGISTRÉ »** : cette ligne sert aussi à
                    // l'ASSEMBLAGE, qui ne numérise rien. Le panneau, lui, dit
                    // « page N scannée » pour une passe (voir `cef_scanner`).
                    ok: true,
                    message: format!("Enregistré : {}", chemin.display()),
                    fichier: Some(chemin),
                },
                None => Verdict {
                    ok: false,
                    message: "NAPS2 s'est terminé sans rien écrire — aucune page n'a été numérisée.".to_string(),
                    fichier: None,
                },
            });
        }
        let detail = texte.lines().map(str::trim).filter(|l| !l.is_empty()).next_back().unwrap_or("");
        let message = if detail.is_empty() {
            format!("Le scan a échoué (code {}). L'appareil est peut-être éteint ou occupé.", statut.code().map_or("?".to_string(), |c| c.to_string()))
        } else {
            detail.to_string()
        };
        Some(Verdict { ok: false, message, fichier: None })
    }

    /// **LE FICHIER EST LA SEULE PREUVE QUI VAUT** — et il n'y a plus rien à
    /// chercher : on attend un chemin qu'on a nommé nous-mêmes.
    fn fichier_ecrit(&self) -> Option<PathBuf> {
        self.sortie.is_file().then(|| self.sortie.clone())
    }
}

/// **LE LANCEMENT COMMUN** — NAPS2, sans fenêtre, vers un chemin CONNU.
///
/// ⚠️ **LE DOSSIER DE SORTIE EST CRÉÉ ICI** : NAPS2 n'écrit pas dans un dossier
/// absent, et un échec pour cette raison serait incompréhensible depuis le
/// panneau (le message affiché serait celui de NAPS2, pas le vrai).
fn lancer_avec(arguments: Vec<String>, sortie: &Path) -> Result<Scan, String> {
    use std::os::windows::process::CommandExt;
    let exe = chemin_console().ok_or_else(|| {
        "NAPS2.Console.exe est introuvable : installez NAPS2, ou lancez-le une fois pour vérifier son emplacement.".to_string()
    })?;
    let dossier = sortie.parent().map(Path::to_path_buf).unwrap_or_default();
    if let Err(erreur) = std::fs::create_dir_all(&dossier) {
        return Err(format!("Dossier de sortie impossible à créer : {erreur}"));
    }
    let enfant = Command::new(&exe)
        .args(arguments)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|erreur| format!("NAPS2 n'a pas pu démarrer : {erreur}"))?;
    Ok(Scan { enfant, sortie: sortie.to_path_buf() })
}

/// **UNE PASSE DE SCAN : UNE PAGE**, vers le fichier temporaire que la session a
/// nommé.
pub fn lancer_la_page(reglages: &Reglages, sortie: &Path) -> Result<Scan, String> {
    lancer_avec(arguments(reglages, sortie), sortie)
}

/// **L'ASSEMBLAGE DU DOCUMENT** — aucun scan, donc l'appareil n'est pas touché :
/// ni allumé, ni libre. C'est ce qui rend « Terminer » possible même si le
/// scanner a été éteint entre deux pages.
pub fn lancer_l_assemblage(pages: &[PathBuf], sortie: &Path, ocr: bool) -> Result<Scan, String> {
    if pages.is_empty() {
        return Err("Aucune page à assembler.".to_string());
    }
    lancer_avec(arguments_assemblage(pages, sortie, ocr), sortie)
}

/// Le chemin de `NAPS2.Console.exe`, s'il existe.
pub fn chemin_console() -> Option<PathBuf> {
    EXE_CANDIDATS.iter().map(PathBuf::from).find(|chemin| chemin.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reglages_de_reference() -> Reglages {
        Reglages {
            profil: "TW-Brother MFC-J480DW LAN".to_string(),
            source: Source::Vitre,
            dpi: 300,
            couleur: Couleur::Couleur,
            format: "A4".to_string(),
            deskew: false,
            ocr: false,
            destination: PathBuf::from(r"C:\Users\moi\Documents\Scans"),
            extension: Extension::Pdf,
        }
    }

    /// Le chemin de la page que la session aurait nommée.
    fn sortie_de_reference() -> PathBuf {
        PathBuf::from(r"C:\Temp\neuradesk-scanner\page-1.pdf")
    }

    /// ⭐ **LA LIGNE DE COMMANDE ENTIÈRE, ET DANS L'ORDRE.** Comparer le vecteur
    /// complet plutôt que des présences éparses : c'est ce qui attrape un
    /// argument ajouté au mauvais endroit, ou un `-o` qui perd son chemin.
    #[test]
    fn la_ligne_de_commande_de_reference() {
        assert_eq!(
            arguments(&reglages_de_reference(), &sortie_de_reference()),
            vec![
                "-p",
                "TW-Brother MFC-J480DW LAN",
                "--source",
                "glass",
                "--dpi",
                "300",
                "--bitdepth",
                "color",
                "--pagesize",
                "A4",
                "-n",
                "1",
                "-f",
                "-v",
                "-o",
                r"C:\Temp\neuradesk-scanner\page-1.pdf",
            ]
        );
    }

    /// **CHAQUE RÉGLAGE DE LA PASSE CHANGE SON ARGUMENT, ET UN SEUL.** Un réglage
    /// qui n'agit pas est un réglage mort : c'est le défaut que ce test rend
    /// impossible.
    ///
    /// ⚠️ **`destination` ET `extension` N'Y SONT PLUS, ET CE N'EST PAS UN
    /// OUBLI** : depuis le multipage, c'est l'APPELANT qui nomme la sortie (voir
    /// `arguments`, doc). Ces deux-là disent *où et sous quel nom*, pas *comment
    /// numériser* — et c'est `cef_scanner::terminer` qui s'en sert.
    #[test]
    fn chaque_reglage_se_retrouve_dans_la_ligne() {
        let cas: [(Reglages, &str, &str); 4] = [
            (Reglages { source: Source::Chargeur, ..reglages_de_reference() }, "--source", "feeder"),
            (Reglages { dpi: 600, ..reglages_de_reference() }, "--dpi", "600"),
            (Reglages { couleur: Couleur::NoirBlanc, ..reglages_de_reference() }, "--bitdepth", "bw"),
            (Reglages { format: "Legal".to_string(), ..reglages_de_reference() }, "--pagesize", "Legal"),
        ];
        for (reglages, drapeau, attendu) in cas {
            let ligne = arguments(&reglages, &sortie_de_reference());
            let rang = ligne.iter().position(|a| a == drapeau).unwrap_or_else(|| panic!("{drapeau} absent"));
            assert_eq!(ligne[rang + 1], attendu, "{drapeau}");
        }
    }

    /// ⭐ **UNE PASSE NUMÉRISE TOUJOURS UNE SEULE PAGE** — c'est le multipage qui
    /// fait le document, pas `-n`. Le jour où quelqu'un rebrancherait un réglage
    /// de pages, ce test le dit avant que le panneau ne mente.
    #[test]
    fn une_passe_ne_numerise_qu_une_page() {
        let ligne = arguments(&reglages_de_reference(), &sortie_de_reference());
        let rang = ligne.iter().position(|a| a == "-n").expect("-n absent");
        assert_eq!(ligne[rang + 1], PAGES_PAR_PASSE.to_string());
    }

    /// Le redressement : présent quand il est coché, absent sinon.
    #[test]
    fn la_case_de_redressement_ajoute_son_drapeau() {
        let redresse = arguments(&Reglages { deskew: true, ..reglages_de_reference() }, &sortie_de_reference());
        assert!(redresse.contains(&"--deskew".to_string()));
        assert!(!arguments(&reglages_de_reference(), &sortie_de_reference()).contains(&"--deskew".to_string()));
    }

    /// ⛔ **AUCUN ARGUMENT D'OCR SUR UNE PASSE, ET C'EST UNE MESURE.**
    ///
    /// Le 2026-10-09 : `-n 0 -i page.jpg -o sortie.jpg --ocrlang fra` sort en
    /// **code 0 sans rien reconnaître et sans un mot**. Une passe écrit une IMAGE,
    /// donc y laisser le drapeau ferait une case « OCR français » qui MENT — le
    /// défaut exact que ce dépôt refuse.
    ///
    /// ⭐ **L'OCR vit sur l'ASSEMBLAGE**, où il produit une vraie couche texte —
    /// voir `l_ocr_change_l_assemblage`, qui le vérifie de l'autre côté.
    #[test]
    fn une_passe_ne_parle_jamais_d_ocr() {
        let ligne = arguments(&Reglages { ocr: true, ..reglages_de_reference() }, &sortie_de_reference());
        assert!(!ligne.contains(&"--ocrlang".to_string()), "une image ne porte pas de texte : {ligne:?}");
        assert!(!ligne.contains(&"--disableocr".to_string()), "et le desactiver ne veut rien dire non plus");
    }

    /// ⚠️ **UN PROFIL VIDE N'ÉMET PAS `-p`** — c'est le repli « profil le plus
    /// récent de NAPS2 », et poser `-p ""` ferait échouer le scan.
    #[test]
    fn un_profil_vide_n_emet_pas_le_drapeau() {
        let ligne = arguments(&Reglages { profil: "   ".to_string(), ..reglages_de_reference() }, &sortie_de_reference());
        assert!(!ligne.contains(&"-p".to_string()));
        assert_eq!(ligne.first().map(String::as_str), Some("--source"));
    }

    /// ⭐ **`<DisplayName>` ET NON `<Name>`** — les deux sont dans le fichier, et
    /// les confondre donnerait « profil introuvable » sur le profil TWAIN. La
    /// charge utile est recopiée de `%APPDATA%\NAPS2\profiles.xml`.
    #[test]
    fn les_profils_se_lisent_dans_display_name() {
        let xml = r#"<ArrayOfScanProfile>
            <ScanProfile><Device><Name>Brother MFC-J480DW LAN</Name></Device>
              <DriverName>wia</DriverName><DisplayName>Brother MFC-J480DW LAN</DisplayName></ScanProfile>
            <ScanProfile><Device><Name>TW-Brother MFC-J480DW LAN</Name></Device>
              <DriverName>twain</DriverName><DisplayName>TW-Brother MFC-J480DW LAN</DisplayName></ScanProfile>
        </ArrayOfScanProfile>"#;
        assert_eq!(
            profils_dans(xml),
            vec!["Brother MFC-J480DW LAN".to_string(), "TW-Brother MFC-J480DW LAN".to_string()]
        );
        assert!(profils_dans("<ArrayOfScanProfile></ArrayOfScanProfile>").is_empty());
        assert!(profils_dans("pas du xml").is_empty());
    }

    /// Les noms des réglages font l'aller-retour, et **la page n'a pas le droit
    /// d'en inventer un** : c'est le contrat que `cef_scanner` vérifie aussi.
    #[test]
    fn les_noms_de_reglages_font_l_aller_retour() {
        for (variante, texte, _libelle) in Source::TOUTES {
            assert_eq!(Source::depuis_nom(texte), Some(variante));
            assert_eq!(Source::depuis_nom(&texte.to_uppercase()), Some(variante));
        }
        for (variante, texte, _libelle) in Couleur::TOUTES {
            assert_eq!(Couleur::depuis_nom(texte), Some(variante));
        }
        for (variante, texte, _libelle) in Extension::TOUTES {
            assert_eq!(Extension::depuis_nom(texte), Some(variante));
        }
        assert_eq!(Source::depuis_nom("inconnu"), None);
    }

    /// Les valeurs que la page propose viennent de Rust — un format ou une
    /// résolution en dur dans la page serait une seconde liste.
    #[test]
    fn les_formats_et_resolutions_viennent_de_rust() {
        assert!(FORMATS.contains(&"A4"));
        assert_eq!(RESOLUTIONS, [200, 300, 600]);
        assert_eq!(Reglages::default().format, "A4");
        assert_eq!(Reglages::default().dpi, 300);
    }

    /// ⭐ **LES RÉGLAGES FONT L'ALLER-RETOUR PAR LE FICHIER.** C'est ce qui rend
    /// la mémorisation vérifiable sans écrire dans `%APPDATA%` — et ce qui
    /// attraperait un champ ajouté au struct mais oublié dans l'un des deux sens.
    #[test]
    fn les_reglages_font_l_aller_retour_par_le_texte() {
        let reglages = Reglages {
            profil: "Brother MFC-J480DW LAN".to_string(),
            source: Source::Chargeur,
            dpi: 600,
            couleur: Couleur::NoirBlanc,
            format: "Legal".to_string(),
            deskew: true,
            ocr: true,
            destination: PathBuf::from(r"D:\Numerisations"),
            extension: Extension::Jpeg,
        };
        let relu = Reglages::depuis_texte(&reglages.vers_texte(), &Reglages::default());
        assert_eq!(relu, reglages);
    }

    /// ⛔ **UNE VALEUR ILLISIBLE GARDE LE DÉFAUT, ELLE NE L'EFFACE PAS.** Le cas
    /// qui compte est le chemin RELATIF : accepté, il enverrait les scans dans le
    /// répertoire de travail du processus — que personne ne connaît.
    #[test]
    fn une_valeur_illisible_garde_le_defaut() {
        let defaut = Reglages { destination: PathBuf::from(r"C:\Repli"), ..Reglages::default() };
        let texte = "dpi=beaucoup\nsource=inconnu\nformat=A0\ndestination=Scans\nextension=docx\nclef absente\n";
        let relu = Reglages::depuis_texte(texte, &defaut);
        assert_eq!(relu.dpi, defaut.dpi, "un dpi illisible garde le defaut");
        assert_eq!(relu.source, defaut.source);
        assert_eq!(relu.format, defaut.format, "A0 n'est pas propose");
        assert_eq!(relu.destination, defaut.destination, "un chemin RELATIF est refuse");
        assert_eq!(relu.extension, defaut.extension);
    }

    /// ⭐ **UN FICHIER ÉCRIT AVANT LE MULTIPAGE RESTE LISIBLE.** `pages` était un
    /// réglage ; c'est devenu la liste de la `Session`. Une clé qui disparaît doit
    /// être **IGNORÉE**, pas refusée — sinon le panneau d'un utilisateur dont le
    /// `scanner_reglages.txt` date d'hier ne s'ouvrirait plus du tout.
    #[test]
    fn un_fichier_d_avant_le_multipage_reste_lisible() {
        let defaut = Reglages::neufs();
        let relu = Reglages::depuis_texte("dpi=600\npages=5\nextension=jpg\n", &defaut);
        assert_eq!(relu.dpi, 600, "les autres cles sont bien lues");
        assert_eq!(relu.extension, Extension::Jpeg);
    }

    // -----------------------------------------------------------------------
    // LA SESSION — le multipage
    // -----------------------------------------------------------------------

    /// ⭐ **L'ASSEMBLAGE EST PUR, DONC VÉRIFIABLE SANS SCANNER** — ce qui compte
    /// quand le seul appareil est sur le réseau et qu'une numérisation ne se
    /// simule pas.
    #[test]
    fn l_assemblage_nomme_chaque_page_et_interdit_le_scan() {
        let pages = vec![PathBuf::from(r"C:\Temp\neuradesk-scanner\page-1.jpg"), PathBuf::from(r"C:\Temp\neuradesk-scanner\page-2.jpg")];
        let ligne = arguments_assemblage(&pages, Path::new(r"C:\Scans\scan-2026-10-09_150000.pdf"), false);
        assert_eq!(
            ligne,
            vec![
                "-n",
                "0",
                "-i",
                r"C:\Temp\neuradesk-scanner\page-1.jpg;C:\Temp\neuradesk-scanner\page-2.jpg",
                "--disableocr",
                "-f",
                "-v",
                "-o",
                r"C:\Scans\scan-2026-10-09_150000.pdf",
            ]
        );
        let rang = ligne.iter().position(|a| a == "-n").expect("-n absent");
        assert_eq!(ligne[rang + 1], "0", "zero page : c'est ce qui INTERDIT tout scan");
        assert!(!ligne.contains(&"--source".to_string()), "un assemblage ne choisit pas de source");
    }

    /// ⭐ **C'EST L'ASSEMBLAGE QUI PORTE L'OCR** — mesuré le 2026-10-09 : le même
    /// assemblage passe de 12 993 à 42 965 octets, et le PDF gagne une police
    /// `Type0` composite ET un `/ToUnicode`, c'est-à-dire une **vraie couche
    /// texte** extractible. Sur une PASSE, le même drapeau ne faisait RIEN.
    ///
    /// ⛔ **`--disableocr` EST POSÉ, PAS OMIS** : sans lui, un profil NAPS2 qui
    /// active l'OCR le ferait quand même, et la case décochée mentirait.
    #[test]
    fn l_ocr_change_l_assemblage() {
        let pages = vec![PathBuf::from(r"C:\Temp\neuradesk-scanner\page-1.jpg")];
        let sortie = Path::new(r"C:\Scans\scan-2026-10-09_150000.pdf");

        let avec = arguments_assemblage(&pages, sortie, true);
        assert!(avec.contains(&"--ocrlang".to_string()) && avec.contains(&OCR_LANGUE.to_string()));
        assert!(!avec.contains(&"--disableocr".to_string()));

        let sans = arguments_assemblage(&pages, sortie, false);
        assert!(sans.contains(&"--disableocr".to_string()));
        assert!(!sans.contains(&"--ocrlang".to_string()));
    }

    /// ⭐ **L'ORDRE DE LA LISTE EST L'ORDRE DU DOCUMENT** — et un retrait ne fait
    /// pas réécrire un fichier encore présent : les noms ne sont JAMAIS réutilisés.
    #[test]
    fn la_session_garde_l_ordre_et_ne_reutilise_pas_un_nom() {
        let mut session = Session::default();
        let mut noms = Vec::new();
        for _ in 0..3 {
            let chemin = session.chemin_a_venir();
            session.ajouter(chemin.clone());
            noms.push(chemin);
        }
        assert!(noms[0].ends_with("page-1.jpg"), "{:?}", noms[0]);
        assert!(noms[1].ends_with("page-2.jpg"), "{:?}", noms[1]);
        assert!(noms[2].ends_with("page-3.jpg"), "{:?}", noms[2]);

        // On retire celle du MILIEU : les deux autres gardent leur ordre…
        assert_eq!(session.retirer(2), Some(noms[1].clone()));
        assert_eq!(session.pages(), &[noms[0].clone(), noms[2].clone()]);
        // …et la page suivante ne RÉÉCRIT pas `page-3`, qui est encore là.
        assert!(session.chemin_a_venir().ends_with("page-4.jpg"));
    }

    /// ⭐ **UNE PAGE EST TOUJOURS UNE IMAGE**, quel que soit le format du DOCUMENT
    /// choisi dans les Réglages.
    ///
    /// ⛔ **C'est une nécessité, pas un goût** : le dépôt n'a aucun rastériseur
    /// PDF, donc une page PDF ne pourrait être ni vignettée, ni montrée, ni lue
    /// par un moteur d'OCR. **Ce test ÉCHOUE sur l'ancien code**, qui nommait
    /// `page-1.pdf` dès que le réglage disait PDF.
    #[test]
    fn une_page_de_session_est_toujours_une_image() {
        let session = Session::default();
        assert!(session.chemin_a_venir().ends_with("page-1.jpg"), "{:?}", session.chemin_a_venir());
        assert_eq!(EXTENSION_PAGE, Extension::Jpeg);

        // Et le réglage du DOCUMENT n'y change rien.
        let reglages = Reglages { extension: Extension::Pdf, ..reglages_de_reference() };
        let ligne = arguments(&reglages, &session.chemin_a_venir());
        let rang = ligne.iter().position(|a| a == "-o").expect("-o absent");
        assert!(ligne[rang + 1].ends_with("page-1.jpg"), "une page reste une image : {}", ligne[rang + 1]);
    }

    /// ⚠️ **UN RANG FAUX NE RETIRE RIEN** : il vient de la page, donc la liste a
    /// pu changer entre l'affichage et le clic. Retirer la mauvaise page serait
    /// pire que ne rien faire.
    #[test]
    fn un_rang_hors_bornes_ne_retire_rien() {
        let mut session = Session::default();
        session.ajouter(PathBuf::from("page-1.jpg"));
        assert_eq!(session.retirer(0), None);
        assert_eq!(session.retirer(2), None);
        assert_eq!(session.pages().len(), 1);
    }

    /// Le document assemblé porte l'horodatage ; et **toutes les pages écrites une
    /// à une le PARTAGENT**, ce qui les trie ensemble dans l'explorateur.
    #[test]
    fn les_noms_de_document_et_de_page() {
        let nom = nom_de_document(Extension::Pdf);
        assert_eq!(nom.len(), "scan-2026-10-09_150000.pdf".len(), "{nom}");
        assert!(nom.starts_with("scan-2") && nom.ends_with(".pdf"), "{nom}");
        assert_eq!(nom_de_page("2026-10-09_150000", 3, Extension::Jpeg), "scan-2026-10-09_150000_page-3.jpg");
    }

    /// Un fichier vide ou absent n'est pas une erreur : on part des défauts.
    #[test]
    fn un_texte_vide_donne_les_defauts() {
        let defaut = Reglages::neufs();
        assert_eq!(Reglages::depuis_texte("", &defaut), defaut);
        assert!(defaut.destination.is_absolute(), "le dossier propose est absolu");
        assert!(defaut.destination.ends_with("Scans"));
    }
}
