//! **LE PILOTE DE L'ENREGISTREUR** — ce que l'applet « Enregistrer » a choisi,
//! le sous-processus qui enregistre, et ce qu'il raconte.
//!
//! DEMANDES (2026-10-31) : « l'écran ou une zone sélectionnée de l'écran ou la
//! webcam avec ou sans le son », « une applet dédiée, utilisons gstreamer »,
//! « n'enregistrer que du son », « l'incrustation de la webcam dans un coin ».
//!
//! Trois couches :
//! - `enregistrement` (PUR) : des réglages → une chaîne GStreamer ;
//! - `gst` : le moteur, dans `neuradesk.exe --enregistrer <fichier>` ;
//! - ICI : les CHOIX de la page (persistés), leur traduction en réglages
//!   (écran, zone, débit), et la vie du sous-processus.
//!
//! ⭐ **UN SOUS-PROCESSUS, ET C'EST MESURÉ** : avec libVLC, une caméra virtuelle
//! par défaut a tué le processus (segfault). Un pilote de capture qui plante
//! ne doit pas emporter les barres.
//!
//! ⭐ **`CREATE_BREAKAWAY_FROM_JOB`** : l'app vit dans un Job à
//! `KILL_ON_JOB_CLOSE` (voir `cef_app`). Sans la sortie du Job, un plantage de
//! NeuraDesk TUERAIT l'enregistreur au milieu d'un mp4 — fichier illisible,
//! sans `moov`. Hors du Job, il voit son entrée standard se fermer, envoie
//! l'EOS et referme proprement le fichier (voir `gst::faire_tourner`).

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::win32::enregistrement::{Coin, FormatSon, Incrustation, Reglages, Son, Source};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
/// Au-delà, un enregistreur qui ne répond plus à `stop` est tué — le fichier
/// sera perdu, mais NeuraDesk ne doit pas attendre éternellement.
const DELAI_ARRET: Duration = Duration::from_secs(20);
/// Le fichier des choix (`clé=valeur`, une par ligne — même format que le scanner).
const FICHIER_CHOIX: &str = "enregistrer_reglages.txt";
/// Les tailles de vignette proposées par la page, en pour cent de la largeur.
pub const TAILLES_VIGNETTE: [u32; 3] = [15, 25, 35];
/// Les cadences proposées par la page.
pub const CADENCES: [u32; 2] = [30, 60];

// ---------------------------------------------------------------------------
// LES CHOIX DE LA PAGE
// ---------------------------------------------------------------------------

/// Ce qu'on filme, tel que la page le nomme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceChoisie {
    Ecran,
    Zone,
    Webcam,
    Son,
}

impl SourceChoisie {
    const TOUTES: [SourceChoisie; 4] = [SourceChoisie::Ecran, SourceChoisie::Zone, SourceChoisie::Webcam, SourceChoisie::Son];

    pub fn nom(self) -> &'static str {
        match self {
            SourceChoisie::Ecran => "ecran",
            SourceChoisie::Zone => "zone",
            SourceChoisie::Webcam => "webcam",
            SourceChoisie::Son => "son",
        }
    }

    fn depuis_nom(nom: &str) -> Option<SourceChoisie> {
        SourceChoisie::TOUTES.into_iter().find(|s| s.nom() == nom)
    }
}

/// La qualité de l'image : un débit PAR PIXEL, pas un débit fixe — 4K et 720p
/// n'ont pas besoin du même.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Qualite {
    Basse,
    Normale,
    Haute,
}

impl Qualite {
    const TOUTES: [Qualite; 3] = [Qualite::Basse, Qualite::Normale, Qualite::Haute];

    pub fn nom(self) -> &'static str {
        match self {
            Qualite::Basse => "basse",
            Qualite::Normale => "normale",
            Qualite::Haute => "haute",
        }
    }

    fn depuis_nom(nom: &str) -> Option<Qualite> {
        Qualite::TOUTES.into_iter().find(|q| q.nom() == nom)
    }

    /// Bits par pixel et par image. Mesure de référence : 4K à 30 i/s en
    /// « normale » donne ~17 Mbit/s, l'ordre de grandeur d'OBS.
    fn bits_par_pixel(self) -> f64 {
        match self {
            Qualite::Basse => 0.04,
            Qualite::Normale => 0.07,
            Qualite::Haute => 0.12,
        }
    }
}

/// Tout ce que la page règle — et que `FICHIER_CHOIX` retient d'une fois sur
/// l'autre.
#[derive(Clone, Debug, PartialEq)]
pub struct Choix {
    pub source: SourceChoisie,
    /// Le `device_name` de l'écran (`\\.\DISPLAY1`) ; vide = le principal.
    pub ecran: String,
    /// La zone, en pixels PHYSIQUES du bureau virtuel : x, y, largeur, hauteur.
    pub zone: Option<[i32; 4]>,
    /// Le NOM de la webcam (voir `enregistrement::branche_webcam`) ; vide = la
    /// première de la liste.
    pub webcam: String,
    pub son_pc: bool,
    pub micro: bool,
    /// L'identifiant WASAPI du micro ; vide = celui par défaut.
    pub micro_id: String,
    pub curseur: bool,
    pub incruster: bool,
    pub coin: Coin,
    pub taille: u32,
    pub ips: u32,
    pub qualite: Qualite,
    pub format_son: FormatSon,
    /// Le dossier de sortie ; vide = Vidéos (Musique pour le son seul).
    pub dossier: String,
}

impl Default for Choix {
    fn default() -> Self {
        Choix {
            source: SourceChoisie::Ecran,
            ecran: String::new(),
            zone: None,
            webcam: String::new(),
            son_pc: true,
            micro: false,
            micro_id: String::new(),
            curseur: true,
            incruster: false,
            coin: Coin::BasDroit,
            taille: 25,
            ips: 30,
            qualite: Qualite::Normale,
            format_son: FormatSon::M4a,
            dossier: String::new(),
        }
    }
}

fn booleen(valeur: &str) -> Option<bool> {
    match valeur {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

impl Choix {
    /// **APPLIQUE UN RÉGLAGE DE LA PAGE** — rend `false` pour une clé ou une
    /// valeur inconnue, que l'appelant journalise. Une valeur hors liste est
    /// REFUSÉE plutôt que bornée : la page n'en envoie pas d'autres, et en
    /// accepter une serait cacher un défaut de la page.
    pub fn regler(&mut self, cle: &str, valeur: &str) -> bool {
        match cle {
            "source" => SourceChoisie::depuis_nom(valeur).map(|v| self.source = v).is_some(),
            "ecran" => {
                self.ecran = valeur.to_string();
                true
            }
            "webcam" => {
                self.webcam = valeur.to_string();
                true
            }
            "son_pc" => booleen(valeur).map(|v| self.son_pc = v).is_some(),
            "micro" => booleen(valeur).map(|v| self.micro = v).is_some(),
            "micro_id" => {
                self.micro_id = valeur.to_string();
                true
            }
            "curseur" => booleen(valeur).map(|v| self.curseur = v).is_some(),
            "incruster" => booleen(valeur).map(|v| self.incruster = v).is_some(),
            "coin" => Coin::depuis_nom(valeur).map(|v| self.coin = v).is_some(),
            "taille" => valeur.parse().ok().filter(|t| TAILLES_VIGNETTE.contains(t)).map(|v| self.taille = v).is_some(),
            "ips" => valeur.parse().ok().filter(|t| CADENCES.contains(t)).map(|v| self.ips = v).is_some(),
            "qualite" => Qualite::depuis_nom(valeur).map(|v| self.qualite = v).is_some(),
            "format_son" => FormatSon::depuis_nom(valeur).map(|v| self.format_son = v).is_some(),
            "dossier" => {
                self.dossier = valeur.to_string();
                true
            }
            "zone" => {
                let nombres: Vec<i32> = valeur.split(',').filter_map(|n| n.trim().parse().ok()).collect();
                match nombres[..] {
                    [x, y, l, h] if l > 0 && h > 0 => {
                        self.zone = Some([x, y, l, h]);
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// Lit `FICHIER_CHOIX` — une ligne illisible est ignorée, les autres comptent.
    pub fn depuis_texte(texte: &str) -> Choix {
        let mut choix = Choix::default();
        for ligne in texte.lines() {
            if let Some((cle, valeur)) = ligne.split_once('=') {
                let _ = choix.regler(cle.trim(), valeur.trim());
            }
        }
        choix
    }

    pub fn en_texte(&self) -> String {
        let b = |v: bool| if v { "1" } else { "0" };
        let mut lignes = vec![
            format!("source={}", self.source.nom()),
            format!("ecran={}", self.ecran),
            format!("webcam={}", self.webcam),
            format!("son_pc={}", b(self.son_pc)),
            format!("micro={}", b(self.micro)),
            format!("micro_id={}", self.micro_id),
            format!("curseur={}", b(self.curseur)),
            format!("incruster={}", b(self.incruster)),
            format!("coin={}", self.coin.nom()),
            format!("taille={}", self.taille),
            format!("ips={}", self.ips),
            format!("qualite={}", self.qualite.nom()),
            format!("format_son={}", self.format_son.extension()),
            format!("dossier={}", self.dossier),
        ];
        if let Some([x, y, l, h]) = self.zone {
            lignes.push(format!("zone={x},{y},{l},{h}"));
        }
        lignes.join("\n") + "\n"
    }

    pub fn charger() -> Choix {
        crate::config::config_path(FICHIER_CHOIX)
            .and_then(|chemin| std::fs::read_to_string(chemin).ok())
            .map(|texte| Choix::depuis_texte(&texte))
            .unwrap_or_default()
    }

    pub fn enregistrer(&self) -> bool {
        crate::config::write_config_atomique(FICHIER_CHOIX, self.en_texte())
    }

    /// Le dossier où va le fichier.
    pub fn dossier_effectif(&self) -> PathBuf {
        if !self.dossier.is_empty() {
            return PathBuf::from(&self.dossier);
        }
        use windows::Win32::UI::Shell::{FOLDERID_Music, FOLDERID_Videos};
        let id = if self.source == SourceChoisie::Son { &FOLDERID_Music } else { &FOLDERID_Videos };
        crate::win32::ssh::local::dossier_connu(id).unwrap_or_else(crate::win32::ssh::local::dossier_documents)
    }
}

// ---------------------------------------------------------------------------
// DES CHOIX AUX RÉGLAGES
// ---------------------------------------------------------------------------

/// Un écran tel que `monitors::enumerate` le voit — réduit à ce qui sert ici,
/// pour que la traduction reste testable sans écran.
#[derive(Clone, Debug, PartialEq)]
pub struct EcranVu {
    pub handle: isize,
    pub nom: String,
    /// x, y, largeur, hauteur — pixels physiques du bureau virtuel.
    pub rect: [i32; 4],
    pub principal: bool,
}

pub fn ecrans() -> Vec<EcranVu> {
    crate::win32::monitors::enumerate()
        .into_iter()
        .map(|m| EcranVu {
            handle: m.handle.0 as isize,
            nom: m.device_name,
            rect: [m.rect.left, m.rect.top, m.rect.right - m.rect.left, m.rect.bottom - m.rect.top],
            principal: m.is_primary,
        })
        .collect()
}

/// L'écran choisi par son nom, sinon le principal, sinon le premier.
fn ecran_choisi<'a>(nom: &str, ecrans: &'a [EcranVu]) -> Option<&'a EcranVu> {
    ecrans.iter().find(|e| e.nom == nom).or_else(|| ecrans.iter().find(|e| e.principal)).or_else(|| ecrans.first())
}

/// **LA ZONE, RAMENÉE À UN SEUL ÉCRAN** — `d3d11screencapturesrc` capture un
/// écran, pas le bureau virtuel. On prend l'écran qui contient le CENTRE de la
/// zone, et on rogne ce qui dépasse. Rend l'écran et la zone RELATIVE à son coin.
pub fn zone_sur_un_ecran(zone: [i32; 4], ecrans: &[EcranVu]) -> Option<(&EcranVu, [i32; 4])> {
    let [x, y, l, h] = zone;
    let (cx, cy) = (x + l / 2, y + h / 2);
    let ecran = ecrans.iter().find(|e| {
        let [ex, ey, el, eh] = e.rect;
        cx >= ex && cx < ex + el && cy >= ey && cy < ey + eh
    })?;
    let [ex, ey, el, eh] = ecran.rect;
    let gauche = x.max(ex);
    let haut = y.max(ey);
    let droite = (x + l).min(ex + el);
    let bas = (y + h).min(ey + eh);
    // Moins de 16 pixels : rien de filmable (et l'encodeur refuse les tailles
    // minuscules).
    if droite - gauche < 16 || bas - haut < 16 {
        return None;
    }
    Some((ecran, [gauche - ex, haut - ey, droite - gauche, bas - haut]))
}

/// Le débit vidéo, en kbit/s, pour une image de `pixels` à `ips` images/s.
fn debit(qualite: Qualite, pixels: i64, ips: u32) -> u32 {
    let kbits = pixels as f64 * f64::from(ips) * qualite.bits_par_pixel() / 1000.0;
    (kbits.round() as u32).clamp(1_000, 80_000)
}

/// La webcam : le nom choisi s'il est encore branché, sinon la première.
fn webcam_choisie(choix: &Choix, webcams: &[String]) -> Option<String> {
    if !choix.webcam.is_empty() && webcams.iter().any(|w| *w == choix.webcam) {
        return Some(choix.webcam.clone());
    }
    webcams.first().cloned()
}

/// **LES CHOIX DEVIENNENT DES RÉGLAGES** — ou une phrase qui dit à l'utilisateur
/// ce qui manque. `webcams` : les noms connus (une liste vide refuse la webcam).
pub fn reglages(choix: &Choix, ecrans: &[EcranVu], webcams: &[String]) -> Result<Reglages, String> {
    let son = Son {
        pc: choix.son_pc,
        micro: choix.micro.then(|| choix.micro_id.clone()),
    };
    let (source, pixels) = match choix.source {
        SourceChoisie::Ecran => {
            let ecran = ecran_choisi(&choix.ecran, ecrans).ok_or("Aucun écran trouvé.")?;
            let [_, _, l, h] = ecran.rect;
            (Source::Ecran { moniteur: ecran.handle, largeur: l, hauteur: h, zone: None }, i64::from(l) * i64::from(h))
        }
        SourceChoisie::Zone => {
            let zone = choix.zone.ok_or("Choisissez d'abord la zone à filmer.")?;
            let (ecran, relative) = zone_sur_un_ecran(zone, ecrans).ok_or("La zone choisie n'est plus sur un écran.")?;
            let [_, _, l, h] = ecran.rect;
            (
                Source::Ecran { moniteur: ecran.handle, largeur: l, hauteur: h, zone: Some(relative) },
                i64::from(relative[2]) * i64::from(relative[3]),
            )
        }
        SourceChoisie::Webcam => {
            let nom = webcam_choisie(choix, webcams).ok_or("Aucune webcam trouvée.")?;
            (Source::Webcam { peripherique: nom }, 1920 * 1080)
        }
        SourceChoisie::Son => {
            if son.aucun() {
                return Err("Choisissez le son du PC, le micro, ou les deux.".to_string());
            }
            (Source::SonSeul, 0)
        }
    };
    let filme_l_ecran = matches!(choix.source, SourceChoisie::Ecran | SourceChoisie::Zone);
    let incrustation = if filme_l_ecran && choix.incruster {
        let nom = webcam_choisie(choix, webcams).ok_or("Aucune webcam à incruster.")?;
        Some(Incrustation { peripherique: nom, coin: choix.coin, taille_pct: choix.taille })
    } else {
        None
    };
    Ok(Reglages {
        source,
        son,
        curseur: choix.curseur,
        incrustation,
        images_par_seconde: choix.ips,
        debit_video: debit(choix.qualite, pixels.max(1), choix.ips),
        format_son: choix.format_son,
    })
}

/// « Enregistrement 2026-10-31 22-30-59.mp4 » — l'heure LOCALE, triable.
fn nom_de_fichier(extension: &str) -> String {
    // SAFETY : `GetLocalTime` remplit une structure rendue par valeur.
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!(
        "Enregistrement {:04}-{:02}-{:02} {:02}-{:02}-{:02}.{extension}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    )
}

// ---------------------------------------------------------------------------
// LES PÉRIPHÉRIQUES
// ---------------------------------------------------------------------------

/// Un périphérique de capture : son nom affiché, et ce qu'on passe à GStreamer.
#[derive(Clone, Debug, PartialEq)]
pub struct Peripherique {
    pub nom: String,
    pub id: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Peripheriques {
    pub webcams: Vec<Peripherique>,
    pub micros: Vec<Peripherique>,
}

/// Ce que `--lister-peripheriques` imprime (voir `gst::lister`).
pub fn lire_peripheriques(texte: &str) -> Option<Peripheriques> {
    // Le moteur peut écrire d'autres lignes avant : on prend la DERNIÈRE qui
    // est un objet JSON.
    let json: serde_json::Value = texte.lines().rev().find_map(|l| serde_json::from_str(l.trim()).ok())?;
    let liste = |cle: &str| -> Vec<Peripherique> {
        json[cle]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| {
                        Some(Peripherique { nom: p["nom"].as_str()?.to_string(), id: p["id"].as_str()?.to_string() })
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    Some(Peripheriques { webcams: liste("webcams"), micros: liste("micros") })
}

/// L'état de la liste des périphériques : demandée une fois, en tâche de fond —
/// le moteur met une à deux secondes à démarrer.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum EtatListe {
    #[default]
    Jamais,
    EnCours,
    Prete(Peripheriques),
    Echec(String),
}

static LISTE: Mutex<EtatListe> = Mutex::new(EtatListe::Jamais);

pub fn liste() -> EtatListe {
    LISTE.lock().map(|l| l.clone()).unwrap_or_default()
}

/// Lance la liste si elle n'a jamais été faite, ou si `forcer`.
pub fn demander_la_liste(forcer: bool) {
    {
        let Ok(mut etat) = LISTE.lock() else { return };
        let deja = matches!(*etat, EtatListe::EnCours) || (!forcer && !matches!(*etat, EtatListe::Jamais));
        if deja {
            return;
        }
        *etat = EtatListe::EnCours;
    }
    std::thread::spawn(|| {
        let resultat = (|| -> Result<Peripheriques, String> {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let sortie = Command::new(exe)
                .arg(crate::win32::gst::DRAPEAU_LISTE)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|e| e.to_string())?;
            let texte = String::from_utf8_lossy(&sortie.stdout);
            lire_peripheriques(&texte).ok_or_else(|| format!("réponse illisible (code {:?})", sortie.status.code()))
        })();
        crate::diag!("[enregistrer] liste des peripheriques : {:?}", resultat.as_ref().map(|p| (p.webcams.len(), p.micros.len())));
        if let Ok(mut etat) = LISTE.lock() {
            *etat = match resultat {
                Ok(p) => EtatListe::Prete(p),
                Err(e) => EtatListe::Echec(e),
            };
        }
    });
}

// ---------------------------------------------------------------------------
// LA SESSION
// ---------------------------------------------------------------------------

/// Où en est l'enregistrement — ce que la page affiche.
#[derive(Clone, Debug, PartialEq)]
pub enum Etat {
    Repos,
    /// Lancé, le moteur n'a pas encore dit `en-cours`.
    Demarrage,
    EnCours { depuis: Instant },
    /// `stop` envoyé, on attend `fini`.
    Arret,
    Fini { fichier: PathBuf },
    Erreur(String),
}

struct Session {
    enfant: Child,
    entree: Option<ChildStdin>,
    fichier: PathBuf,
    chaine: PathBuf,
    lignes: Receiver<String>,
    arret_demande: Option<Instant>,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static ETAT: RefCell<Etat> = const { RefCell::new(Etat::Repos) };
}

pub fn etat() -> Etat {
    ETAT.with(|e| e.borrow().clone())
}

fn poser(etat: Etat) {
    ETAT.with(|e| *e.borrow_mut() = etat);
}

/// Vrai de `Demarrage` à `fini` — l'enregistreur tourne.
pub fn en_cours() -> bool {
    matches!(etat(), Etat::Demarrage | Etat::EnCours { .. } | Etat::Arret)
}

/// **DÉMARRE UN ENREGISTREMENT** avec les choix donnés. Une erreur est posée
/// dans l'état (la page l'affiche) ET rendue.
pub fn demarrer(choix: &Choix) -> Result<PathBuf, String> {
    let resultat = lancer(choix);
    match &resultat {
        Ok(fichier) => {
            crate::diag!("[enregistrer] demarre : {}", fichier.display());
            poser(Etat::Demarrage);
        }
        Err(e) => {
            crate::diag!("[enregistrer] refuse : {e}");
            poser(Etat::Erreur(e.clone()));
        }
    }
    resultat
}

fn lancer(choix: &Choix) -> Result<PathBuf, String> {
    if en_cours() {
        return Err("Un enregistrement est déjà en cours.".to_string());
    }
    if !crate::win32::gst::installe() {
        return Err("GStreamer n'est pas installé.".to_string());
    }
    let webcams: Vec<String> = match liste() {
        EtatListe::Prete(p) => p.webcams.into_iter().map(|w| w.id).collect(),
        _ => Vec::new(),
    };
    let reglages = reglages(choix, &ecrans(), &webcams)?;
    let dossier = choix.dossier_effectif();
    std::fs::create_dir_all(&dossier).map_err(|e| format!("Dossier impossible à créer : {e}"))?;
    let fichier = dossier.join(nom_de_fichier(reglages.extension()));
    let description = crate::win32::enregistrement::chaine(&reglages, &fichier.to_string_lossy())
        .ok_or("Rien à enregistrer : ni image ni son.")?;
    let chaine = std::env::temp_dir().join(format!("neuradesk-enregistrement-{}.txt", std::process::id()));
    std::fs::write(&chaine, &description).map_err(|e| format!("Chaîne impossible à écrire : {e}"))?;
    crate::diag!("[enregistrer] chaine : {description}");

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let commande = |drapeaux: u32| {
        Command::new(&exe)
            .arg(crate::win32::gst::DRAPEAU)
            .arg(&chaine)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(drapeaux)
            .spawn()
    };
    // Sortir du Job peut être refusé (un Job SANS `BREAKAWAY_OK`, posé par un
    // lanceur) : alors on reste dedans — mieux qu'aucun enregistrement.
    let mut enfant = commande(CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB)
        .or_else(|_| commande(CREATE_NO_WINDOW))
        .map_err(|e| format!("L'enregistreur ne démarre pas : {e}"))?;
    let sortie = enfant.stdout.take().ok_or("sortie standard absente")?;
    let (envoi, lignes) = mpsc::channel();
    std::thread::spawn(move || {
        for ligne in BufReader::new(sortie).lines().map_while(Result::ok) {
            if envoi.send(ligne).is_err() {
                break;
            }
        }
    });
    let entree = enfant.stdin.take();
    SESSION.with(|s| {
        *s.borrow_mut() = Some(Session { enfant, entree, fichier: fichier.clone(), chaine, lignes, arret_demande: None })
    });
    Ok(fichier)
}

/// **ARRÊTE PROPREMENT** : `stop` puis fermeture de l'entrée — le moteur envoie
/// l'EOS et referme le fichier. Rend `false` s'il n'y avait rien à arrêter.
pub fn arreter() -> bool {
    let arrete = SESSION.with(|s| {
        let mut s = s.borrow_mut();
        let Some(session) = s.as_mut() else { return false };
        if let Some(mut entree) = session.entree.take() {
            let _ = writeln!(entree, "stop");
            let _ = entree.flush();
        }
        session.arret_demande.get_or_insert_with(Instant::now);
        true
    });
    if arrete {
        poser(Etat::Arret);
    }
    arrete
}

/// Ce que `avancer` a vu passer — pour qu'`app::run` puisse réagir (voyant,
/// avis de fin).
#[derive(Clone, Debug, PartialEq)]
pub enum Evenement {
    Commence,
    Termine(PathBuf),
    Echoue(String),
}

/// **À APPELER À CHAQUE FRAME** — lit ce que le moteur a dit, et constate sa fin.
/// Les emprunts de `SESSION` sont relâchés avant que l'état ne change.
pub fn avancer() -> Option<Evenement> {
    let (lignes, sortie) = SESSION.with(|s| {
        let mut s = s.borrow_mut();
        let Some(session) = s.as_mut() else { return (Vec::new(), None) };
        let mut lignes = Vec::new();
        loop {
            match session.lignes.try_recv() {
                Ok(ligne) => lignes.push(ligne),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        let sortie = match session.enfant.try_wait() {
            Ok(Some(statut)) => Some(statut.code()),
            Ok(None) => {
                if session.arret_demande.is_some_and(|t| t.elapsed() > DELAI_ARRET) {
                    let _ = session.enfant.kill();
                    crate::diag!("[enregistrer] le moteur ne repond plus a stop : tue");
                }
                None
            }
            Err(_) => Some(None),
        };
        (lignes, sortie.map(|code| (code, session.fichier.clone(), session.chaine.clone())))
    });
    let mut evenement = None;
    for ligne in &lignes {
        crate::diag!("[enregistrer] moteur : {ligne}");
        if ligne == "en-cours" && etat() == Etat::Demarrage {
            poser(Etat::EnCours { depuis: Instant::now() });
            evenement = Some(Evenement::Commence);
        } else if let Some(message) = ligne.strip_prefix("erreur: ") {
            poser(Etat::Erreur(message.to_string()));
        }
    }
    let Some((code, fichier, chaine)) = sortie else { return evenement };
    SESSION.with(|s| *s.borrow_mut() = None);
    let _ = std::fs::remove_file(&chaine);
    let fini = lignes.iter().any(|l| l == "fini");
    let fin = match (fini || code == Some(0), etat()) {
        (_, Etat::Erreur(message)) => Evenement::Echoue(message),
        (true, _) if fichier.exists() => {
            poser(Etat::Fini { fichier: fichier.clone() });
            Evenement::Termine(fichier)
        }
        _ => {
            let message = format!("L'enregistreur s'est arrêté (code {code:?}).");
            poser(Etat::Erreur(message.clone()));
            Evenement::Echoue(message)
        }
    };
    crate::diag!("[enregistrer] fin : {fin:?}");
    Some(fin)
}

/// **L'AVIS DE FIN** — titre, message, et le fichier à proposer d'ouvrir.
/// `Commence` n'en a pas : le voyant REC le dit déjà.
pub fn avis_de_fin(evenement: &Evenement) -> Option<(String, String, Option<PathBuf>)> {
    match evenement {
        Evenement::Commence => None,
        Evenement::Termine(fichier) => {
            let nom = fichier.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let dossier = fichier.parent().map(|d| d.display().to_string()).unwrap_or_default();
            Some(("Enregistrement terminé".to_string(), format!("{nom}\n{dossier}"), Some(fichier.clone())))
        }
        Evenement::Echoue(message) => Some(("L'enregistrement a échoué".to_string(), message.clone(), None)),
    }
}

/// **ANNONCE LA FIN** par un toast Windows — cliquer dessus ouvre la vidéo, un
/// bouton ouvre le dossier. Rend `true` si Windows a pris le toast.
pub fn annoncer(evenement: &Evenement) -> bool {
    let Some((titre, message, fichier)) = avis_de_fin(evenement) else { return false };
    let Some(fichier) = fichier else {
        return crate::win32::bulle::notifier(&titre, &message);
    };
    let video = crate::win32::bulle::url_de_fichier(&fichier);
    let dossier = fichier.parent().map(crate::win32::bulle::url_de_fichier).unwrap_or_default();
    let mut boutons = vec![crate::win32::bulle::Lien { libelle: "Ouvrir", url: &video }];
    if !dossier.is_empty() {
        boutons.push(crate::win32::bulle::Lien { libelle: "Ouvrir le dossier", url: &dossier });
    }
    crate::win32::bulle::notifier_avec_liens(&titre, &message, Some(&video), &boutons)
}

/// La durée écoulée depuis `en-cours`.
pub fn duree() -> Option<Duration> {
    match etat() {
        Etat::EnCours { depuis } => Some(depuis.elapsed()),
        _ => None,
    }
}

/// Montre un fichier dans l'Explorateur, sélectionné ; ou ouvre un dossier.
pub fn montrer(chemin: &Path) {
    if chemin.is_file() {
        crate::win32::everything::ouvrir_l_emplacement(chemin);
    } else if let Err(err) = Command::new("explorer.exe").arg(chemin).spawn() {
        crate::diag!("[enregistrer] dossier impossible a ouvrir : {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **SONDE — WINDOWS OUVRE-T-IL UN LIEN `file:///` DEPUIS UN TOAST ?** Le
    /// code ne le dit pas : seul un clic le tranche. Envoie le VRAI avis de fin
    /// pour la vidéo `NEURADESK_SONDE_VIDEO`.
    ///
    /// Lancer : `NEURADESK_SONDE_VIDEO=<mp4> cargo test --bins -- --ignored --nocapture sonde_avis_de_fin`
    #[test]
    #[ignore = "sonde a la main — ENVOIE UN VRAI toast"]
    fn sonde_avis_de_fin() {
        let video = PathBuf::from(std::env::var("NEURADESK_SONDE_VIDEO").expect("NEURADESK_SONDE_VIDEO"));
        assert!(video.is_file(), "{}", video.display());
        let remis = annoncer(&Evenement::Termine(video));
        println!("
toast remis a Windows : {remis}
");
    }

    /// L'avis de fin nomme le fichier et son dossier ; un échec rend son message ;
    /// le début n'a pas d'avis.
    #[test]
    fn l_avis_de_fin_dit_ce_qui_s_est_passe() {
        assert!(avis_de_fin(&Evenement::Commence).is_none());
        let fichier = PathBuf::from(r"C:\Videos\Enregistrement 2026-11-01 03-20-00.mp4");
        let (titre, message, ouvrir) = avis_de_fin(&Evenement::Termine(fichier.clone())).unwrap();
        assert_eq!(titre, "Enregistrement terminé");
        assert_eq!(message, "Enregistrement 2026-11-01 03-20-00.mp4\nC:\\Videos");
        assert_eq!(ouvrir, Some(fichier));
        let (titre, message, ouvrir) = avis_de_fin(&Evenement::Echoue("micro absent".into())).unwrap();
        assert_eq!((titre.as_str(), message.as_str(), ouvrir), ("L'enregistrement a échoué", "micro absent", None));
    }

    fn deux_ecrans() -> Vec<EcranVu> {
        vec![
            EcranVu { handle: 11, nom: r"\.\DISPLAY2".into(), rect: [0, 0, 1920, 1080], principal: false },
            EcranVu { handle: 22, nom: r"\.\DISPLAY1".into(), rect: [1920, 0, 3840, 2160], principal: true },
        ]
    }

    /// Ce que la page règle survit au fichier — chaque clé, aller et retour.
    #[test]
    fn les_choix_survivent_au_fichier() {
        let mut c = Choix::default();
        for (cle, valeur) in [
            ("source", "zone"),
            ("ecran", r"\.\DISPLAY2"),
            ("webcam", "Web Camera"),
            ("son_pc", "0"),
            ("micro", "1"),
            ("micro_id", "{0.0.1.00000000}.{abc}"),
            ("curseur", "0"),
            ("incruster", "1"),
            ("coin", "haut-gauche"),
            ("taille", "35"),
            ("ips", "60"),
            ("qualite", "haute"),
            ("format_son", "wav"),
            ("dossier", r"D:\Films"),
            ("zone", "100,200,1280,720"),
        ] {
            assert!(c.regler(cle, valeur), "refusé : {cle}={valeur}");
        }
        assert_eq!(Choix::depuis_texte(&c.en_texte()), c);
    }

    /// Une valeur hors liste est REFUSÉE, et ne touche à rien.
    #[test]
    fn une_valeur_hors_liste_est_refusee() {
        let mut c = Choix::default();
        for (cle, valeur) in [("taille", "20"), ("ips", "25"), ("source", "fenetre"), ("son_pc", "oui"), ("zone", "1,2,0,4"), ("inconnue", "1")] {
            assert!(!c.regler(cle, valeur), "accepté : {cle}={valeur}");
        }
        assert_eq!(c, Choix::default());
    }

    /// La zone se ramène à l'écran qui contient son CENTRE, rognée, en
    /// coordonnées relatives à cet écran.
    #[test]
    fn la_zone_se_ramene_a_un_ecran() {
        let ecrans = deux_ecrans();
        let (ecran, z) = zone_sur_un_ecran([2000, 100, 1280, 720], &ecrans).unwrap();
        assert_eq!((ecran.handle, z), (22, [80, 100, 1280, 720]));
        // À cheval : le centre (1900+200/2 = 2000) est sur l'écran de droite, la
        // partie gauche est rognée.
        let (ecran, z) = zone_sur_un_ecran([1800, 0, 400, 300], &ecrans).unwrap();
        assert_eq!((ecran.handle, z), (22, [0, 0, 280, 300]));
        assert!(zone_sur_un_ecran([9000, 0, 100, 100], &ecrans).is_none(), "hors de tout écran");
    }

    #[test]
    fn l_ecran_par_defaut_est_le_principal() {
        let r = reglages(&Choix::default(), &deux_ecrans(), &[]).unwrap();
        assert_eq!(r.source, Source::Ecran { moniteur: 22, largeur: 3840, hauteur: 2160, zone: None });
        // 4K à 30 i/s en « normale » : ~17 Mbit/s.
        assert_eq!(r.debit_video, 17_418);
        assert_eq!(r.son, Son { pc: true, micro: None });
    }

    /// Ce qui manque est DIT, en clair.
    #[test]
    fn ce_qui_manque_est_dit() {
        let ecrans = deux_ecrans();
        let mut c = Choix::default();
        c.source = SourceChoisie::Zone;
        assert!(reglages(&c, &ecrans, &[]).unwrap_err().contains("zone"));
        c.source = SourceChoisie::Webcam;
        assert!(reglages(&c, &ecrans, &[]).unwrap_err().contains("webcam"));
        c.source = SourceChoisie::Son;
        c.son_pc = false;
        assert!(reglages(&c, &ecrans, &[]).unwrap_err().contains("micro"));
        c.source = SourceChoisie::Ecran;
        c.incruster = true;
        assert!(reglages(&c, &ecrans, &[]).unwrap_err().contains("webcam"));
    }

    /// L'incrustation et la webcam prennent la webcam CHOISIE si elle est
    /// branchée, sinon la première.
    #[test]
    fn la_webcam_choisie_ou_la_premiere() {
        let webcams = vec!["A".to_string(), "B".to_string()];
        let mut c = Choix::default();
        c.incruster = true;
        c.webcam = "B".into();
        let r = reglages(&c, &deux_ecrans(), &webcams).unwrap();
        assert_eq!(r.incrustation.map(|i| i.peripherique), Some("B".to_string()));
        c.webcam = "débranchée".into();
        c.source = SourceChoisie::Webcam;
        let r = reglages(&c, &deux_ecrans(), &webcams).unwrap();
        assert_eq!(r.source, Source::Webcam { peripherique: "A".into() });
        assert!(r.incrustation.is_none(), "pas d'incrustation dans une webcam");
    }

    #[test]
    fn le_micro_par_defaut_est_une_chaine_vide() {
        let mut c = Choix::default();
        c.micro = true;
        let r = reglages(&c, &deux_ecrans(), &[]).unwrap();
        assert_eq!(r.son.micro, Some(String::new()));
    }

    #[test]
    fn la_liste_des_peripheriques_se_lit_apres_le_bruit() {
        let texte = "avertissement quelconque\n{\"webcams\":[{\"nom\":\"Web Camera\",\"id\":\"Web Camera\"}],\"micros\":[{\"nom\":\"Micro\",\"id\":\"{0.0.1}\"}]}\n";
        let p = lire_peripheriques(texte).unwrap();
        assert_eq!(p.webcams[0].nom, "Web Camera");
        assert_eq!(p.micros[0].id, "{0.0.1}");
        assert!(lire_peripheriques("rien").is_none());
    }

    #[test]
    fn le_nom_de_fichier_est_date_et_triable() {
        let nom = nom_de_fichier("mp4");
        assert!(nom.starts_with("Enregistrement 20") && nom.ends_with(".mp4"), "{nom}");
        assert_eq!(nom.len(), "Enregistrement 2026-10-31 22-30-59.mp4".len());
    }
}
