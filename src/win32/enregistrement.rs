//! **CE QU'ON ENREGISTRE, ET LA CHAÎNE GSTREAMER QUI LE FAIT.**
//!
//! DEMANDES (2026-10-31) : « l'écran ou une zone sélectionnée de l'écran ou la
//! webcam avec ou sans le son », « je voudrais une applet dédiée, utilisons
//! gstreamer », « aussi la possibilité de n'enregistrer que du son », et
//! « l'incrustation de la webcam dans un coin ».
//!
//! Ce module est PUR : des réglages entrent, une description de chaîne
//! (`gst_parse_launch`) sort. Le moteur qui la fait tourner est
//! `win32::gst`, dans un sous-processus.
//!
//! ⭐ **CHAQUE ÉLÉMENT A ÉTÉ MESURÉ AVANT D'ÊTRE ÉCRIT** (GStreamer 1.28.8,
//! enregistrements de 5 s dont les pistes ont été relues) :
//! - `d3d11screencapturesrc` : 4K à 30 i/s, découpe par `crop-*` ;
//! - `wasapi2src loopback=true` : le son du PC — que libVLC 3 ne sait pas
//!   capturer (0x88890003) ;
//! - ⛔ `nvd3d11h264enc` a échoué une fois sur deux (`NV_ENC_ERR_INVALID_VERSION`) :
//!   l'encodeur est `mfh264enc`, celui de Windows, qui passe par le pilote
//!   de N'IMPORTE QUELLE carte ;
//! - ⛔ **`videorate` AVANT L'ENCODEUR EST OBLIGATOIRE** : le compositeur de
//!   l'incrustation émet ses deux premières images au MÊME horodatage, et
//!   `mfh264enc` rend alors un tampon sans horodatage, que `mp4mux` refuse
//!   (« Buffer has no PTS »). Mesuré : sans lui, aucun fichier ; avec lui,
//!   vidéo 4K + webcam + son.

/// Ce qu'on filme.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// Un écran, entier ou une zone : `moniteur` est le `HMONITOR` (valable dans
    /// tout le système), `zone` en pixels PHYSIQUES relatifs au coin de cet écran.
    Ecran { moniteur: isize, largeur: i32, hauteur: i32, zone: Option<[i32; 4]> },
    /// Une webcam, par son NOM (voir `branche_webcam`).
    Webcam { peripherique: String },
    /// Pas d'image.
    SonSeul,
}

/// Un coin de l'image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coin {
    HautGauche,
    HautDroit,
    BasGauche,
    BasDroit,
}

impl Coin {
    pub const TOUS: [Coin; 4] = [Coin::HautGauche, Coin::HautDroit, Coin::BasGauche, Coin::BasDroit];

    pub fn nom(self) -> &'static str {
        match self {
            Coin::HautGauche => "haut-gauche",
            Coin::HautDroit => "haut-droit",
            Coin::BasGauche => "bas-gauche",
            Coin::BasDroit => "bas-droit",
        }
    }

    pub fn depuis_nom(nom: &str) -> Option<Coin> {
        Coin::TOUS.into_iter().find(|c| c.nom() == nom)
    }
}

/// La webcam incrustée dans un coin d'un enregistrement d'écran.
#[derive(Clone, Debug, PartialEq)]
pub struct Incrustation {
    pub peripherique: String,
    pub coin: Coin,
    /// Largeur de la vignette, en pour cent de la largeur de l'image.
    pub taille_pct: u32,
}

/// Le format d'un enregistrement SANS image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatSon {
    M4a,
    Mp3,
    Wav,
}

impl FormatSon {
    pub fn extension(self) -> &'static str {
        match self {
            FormatSon::M4a => "m4a",
            FormatSon::Mp3 => "mp3",
            FormatSon::Wav => "wav",
        }
    }

    pub fn depuis_nom(nom: &str) -> Option<FormatSon> {
        [FormatSon::M4a, FormatSon::Mp3, FormatSon::Wav].into_iter().find(|f| f.extension() == nom)
    }
}

/// Le son à prendre.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Son {
    /// Le son du PC (ce qu'on entend), par le bouclage WASAPI.
    pub pc: bool,
    /// Le micro : `Some("")` = celui par défaut, `Some(id)` = l'identifiant
    /// WASAPI d'un périphérique, `None` = pas de micro.
    pub micro: Option<String>,
}

impl Son {
    pub fn aucun(&self) -> bool {
        !self.pc && self.micro.is_none()
    }
}

/// Tous les réglages d'un enregistrement.
#[derive(Clone, Debug, PartialEq)]
pub struct Reglages {
    pub source: Source,
    pub son: Son,
    pub curseur: bool,
    pub incrustation: Option<Incrustation>,
    pub images_par_seconde: u32,
    /// Débit vidéo en kbit/s.
    pub debit_video: u32,
    pub format_son: FormatSon,
}

/// Débit du son compressé (AAC et MP3), en kbit/s.
const DEBIT_SON: u32 = 160;
/// Plafond des modes de webcam demandés : au-delà, 4K inutile et lourd.
const WEBCAM_MAX: (i32, i32) = (1920, 1080);
/// Marge de la vignette au bord de l'image, en pour cent de la largeur.
const MARGE_PCT: i32 = 2;

impl Reglages {
    /// L'extension du fichier produit.
    pub fn extension(&self) -> &'static str {
        match self.source {
            Source::SonSeul => self.format_son.extension(),
            _ => "mp4",
        }
    }
}

/// Une chaîne entre guillemets pour `gst_parse_launch` : `\` et `"` échappés.
fn quoter(texte: &str) -> String {
    let mut sortie = String::with_capacity(texte.len() + 2);
    sortie.push('"');
    for c in texte.chars() {
        if c == '\\' || c == '"' {
            sortie.push('\\');
        }
        sortie.push(c);
    }
    sortie.push('"');
    sortie
}

/// Arrondi au PAIR inférieur : l'encodeur travaille en NV12, dont la
/// chrominance est sous-échantillonnée par deux.
fn pair(n: i32) -> i32 {
    n.max(2) & !1
}

/// La taille de l'image produite par une source d'écran.
fn taille_ecran(largeur: i32, hauteur: i32, zone: Option<[i32; 4]>) -> (i32, i32) {
    match zone {
        Some([_, _, l, h]) => (pair(l), pair(h)),
        None => (pair(largeur), pair(hauteur)),
    }
}

/// La place de la vignette `(x, y, largeur, hauteur)` dans une image `l × h`.
pub fn place_vignette(l: i32, h: i32, coin: Coin, taille_pct: u32) -> (i32, i32, i32, i32) {
    let pct = taille_pct.clamp(10, 50) as i32;
    let largeur = pair(l * pct / 100);
    let hauteur = pair(largeur * 9 / 16);
    let marge = l * MARGE_PCT / 100;
    let x = match coin {
        Coin::HautGauche | Coin::BasGauche => marge,
        Coin::HautDroit | Coin::BasDroit => l - largeur - marge,
    };
    let y = match coin {
        Coin::HautGauche | Coin::HautDroit => marge,
        Coin::BasGauche | Coin::BasDroit => h - hauteur - marge,
    };
    (x, y, largeur, hauteur)
}

/// **LA MÊME VIGNETTE QUE LA VIDÉO, MAIS DANS UNE PHOTO** — colle la webcam dans
/// le coin d'une image d'écran, à la géométrie EXACTE du compositor de `chaine` :
/// `place_vignette` pour la boîte, et `keep-aspect-ratio` pour la vignette —
/// elle tient DANS la boîte et y est centrée, sans déformation.
///
/// DEMANDE EXPLICITE (2026-11-04) : « pouvoir capturer l'écran avec une
/// incrustation de la webcam comme pour la vidéo ». Les deux images arrivent en
/// PNG (l'écran par `capture_ecran`, la webcam par `chaine_photo`).
pub fn composer_la_vignette(
    ecran_png: &[u8],
    vignette_png: &[u8],
    coin: Coin,
    taille_pct: u32,
) -> Result<Vec<u8>, String> {
    let mut fond = image::load_from_memory(ecran_png).map_err(|e| format!("écran non décodé : {e}"))?.to_rgba8();
    let vignette = image::load_from_memory(vignette_png).map_err(|e| format!("vignette non décodée : {e}"))?.to_rgba8();
    let (largeur, hauteur) = (fond.width() as i32, fond.height() as i32);
    let (x, y, boite_l, boite_h) = place_vignette(largeur, hauteur, coin, taille_pct);
    // `keep-aspect-ratio` : le facteur est le PLUS PETIT des deux rapports,
    // jamais le plus grand — sinon la vignette déborderait de sa boîte.
    let facteur = (boite_l as f32 / vignette.width().max(1) as f32)
        .min(boite_h as f32 / vignette.height().max(1) as f32);
    let (l, h) = (
        ((vignette.width() as f32 * facteur).round() as u32).max(1),
        ((vignette.height() as f32 * facteur).round() as u32).max(1),
    );
    let redim = if (l, h) == vignette.dimensions() {
        vignette
    } else {
        image::imageops::resize(&vignette, l, h, image::imageops::FilterType::Triangle)
    };
    let dx = x + (boite_l - l as i32) / 2;
    let dy = y + (boite_h - h as i32) / 2;
    image::imageops::overlay(&mut fond, &redim, dx as i64, dy as i64);
    let mut sortie = std::io::Cursor::new(Vec::new());
    fond.write_to(&mut sortie, image::ImageFormat::Png).map_err(|e| format!("PNG non écrit : {e}"))?;
    Ok(sortie.into_inner())
}

/// La source d'écran, en mémoire D3D11.
fn branche_ecran(r: &Reglages, moniteur: isize, zone: Option<[i32; 4]>) -> String {
    let mut s = format!(
        "d3d11screencapturesrc monitor-handle={} show-cursor={}",
        moniteur as usize,
        r.curseur
    );
    if let Some([x, y, l, h]) = zone {
        s += &format!(" crop-x={} crop-y={} crop-width={} crop-height={}", x.max(0), y.max(0), pair(l), pair(h));
    }
    s += &format!(" ! video/x-raw(memory:D3D11Memory),framerate={}/1", r.images_par_seconde);
    s
}

/// Une webcam, plafonnée à 1080p.
///
/// ⛔ **PAR SON NOM, PAS PAR `device-path`** : le chemin que rend le moniteur
/// de périphériques de GStreamer (`\\?\usb#…\global`) fait échouer
/// `mfvideosrc` au démarrage (« Failed to start »), échappé ou non — mesuré le
/// 2026-10-31, alors que `device-name` ouvre la même caméra.
fn branche_webcam(peripherique: &str) -> String {
    format!(
        "mfvideosrc device-name={} ! video/x-raw,width=[1,{}],height=[1,{}] ! videoconvert",
        quoter(peripherique),
        WEBCAM_MAX.0,
        WEBCAM_MAX.1
    )
}

/// **UNE PHOTO** : une seule image de la webcam, encodée en PNG — la même
/// source que l'enregistrement (`mfvideosrc`, par NOM), et un fin de flux
/// (`num-buffers=1`) qui referme le fichier sans qu'on ait à l'arrêter.
///
/// DEMANDE EXPLICITE (2026-11-04) : « proposer capture webcam aussi » dans
/// l'applet Capture d'écran. Mesuré le même jour : la chaîne rend un PNG
/// 3840 × 2160 en quelques secondes.
pub fn chaine_photo(peripherique: &str, fichier: &str) -> String {
    format!(
        "mfvideosrc device-name={} num-buffers=1 ! videoconvert ! pngenc ! filesink location={}",
        quoter(peripherique),
        quoter(fichier)
    )
}

/// De l'image brute au flux H.264 qui entre dans le multiplexeur `m`.
fn encodage_video(r: &Reglages, depuis_d3d11: bool) -> String {
    let telechargement = if depuis_d3d11 { "d3d11convert ! d3d11download ! " } else { "" };
    format!(
        "{telechargement}videorate ! video/x-raw,format=NV12,framerate={}/1 ! mfh264enc bitrate={} ! h264parse ! queue ! m.",
        r.images_par_seconde, r.debit_video
    )
}

/// La source de son, ramenée en 48 kHz stéréo.
fn source_son(son: &Son) -> Vec<String> {
    const BRUT: &str = "audioconvert ! audioresample ! audio/x-raw,rate=48000,channels=2";
    let mut sources = Vec::new();
    if son.pc {
        sources.push(format!("wasapi2src loopback=true low-latency=true ! {BRUT}"));
    }
    if let Some(id) = &son.micro {
        let device = if id.is_empty() { String::new() } else { format!(" device={}", quoter(id)) };
        sources.push(format!("wasapi2src{device} low-latency=true ! {BRUT}"));
    }
    sources
}

/// Les branches de son, mélangées s'il y en a deux, terminées par `encodeur`.
fn branches_son(son: &Son, encodeur: &str) -> String {
    let sources = source_son(son);
    match sources.as_slice() {
        [] => String::new(),
        [seule] => format!("{seule} ! {encodeur}"),
        plusieurs => {
            let mut s = format!("audiomixer name=mix ! {encodeur}");
            for source in plusieurs {
                s += &format!("  {source} ! queue ! mix.");
            }
            s
        }
    }
}

/// **LA CHAÎNE** — `None` si les réglages ne produisent rien (son seul sans
/// aucun son).
pub fn chaine(r: &Reglages, fichier: &str) -> Option<String> {
    let sortie = format!("filesink location={}", quoter(fichier));
    let aac = format!("mfaacenc bitrate={} ! aacparse ! queue ! m.", DEBIT_SON * 1000);
    match &r.source {
        Source::SonSeul => {
            let encodeur = match r.format_son {
                FormatSon::M4a => format!("mfaacenc bitrate={} ! aacparse ! mp4mux ! {sortie}", DEBIT_SON * 1000),
                FormatSon::Mp3 => format!("lamemp3enc target=bitrate bitrate={DEBIT_SON} cbr=true ! {sortie}"),
                FormatSon::Wav => format!("wavenc ! {sortie}"),
            };
            if r.son.aucun() {
                return None;
            }
            Some(branches_son(&r.son, &encodeur))
        }
        Source::Webcam { peripherique } => {
            let mut s = format!("mp4mux name=m ! {sortie}  {} ! {}", branche_webcam(peripherique), encodage_video(r, false));
            if !r.son.aucun() {
                s += &format!("  {}", branches_son(&r.son, &aac));
            }
            Some(s)
        }
        Source::Ecran { moniteur, largeur, hauteur, zone } => {
            let (l, h) = taille_ecran(*largeur, *hauteur, *zone);
            let ecran = branche_ecran(r, *moniteur, *zone);
            let mut s = format!("mp4mux name=m ! {sortie}  ");
            match &r.incrustation {
                None => s += &format!("{ecran} ! {}", encodage_video(r, true)),
                Some(inc) => {
                    let (x, y, lv, hv) = place_vignette(l, h, inc.coin, inc.taille_pct);
                    s += &format!(
                        "d3d11compositor name=c background=black sink_1::xpos={x} sink_1::ypos={y} sink_1::width={lv} sink_1::height={hv} sink_1::sizing-policy=keep-aspect-ratio \
                         ! video/x-raw(memory:D3D11Memory),width={l},height={h},framerate={}/1 ! {}  \
                         {ecran} ! queue ! c.sink_0  {} ! d3d11upload ! queue ! c.sink_1",
                        r.images_par_seconde,
                        encodage_video(r, true),
                        branche_webcam(&inc.peripherique)
                    );
                }
            }
            if !r.son.aucun() {
                s += &format!("  {}", branches_son(&r.son, &aac));
            }
            Some(s)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ecran(zone: Option<[i32; 4]>) -> Reglages {
        Reglages {
            source: Source::Ecran { moniteur: 0x1234, largeur: 3840, hauteur: 2160, zone },
            son: Son::default(),
            curseur: true,
            incrustation: None,
            images_par_seconde: 30,
            debit_video: 16000,
            format_son: FormatSon::M4a,
        }
    }

    #[test]
    fn un_ecran_sans_son_passe_par_l_encodeur_de_windows_apres_videorate() {
        let c = chaine(&ecran(None), r"C:\Vidéos\a.mp4").unwrap();
        assert!(c.contains("monitor-handle=4660 show-cursor=true"), "{c}");
        assert!(c.contains("d3d11download ! videorate ! video/x-raw,format=NV12,framerate=30/1 ! mfh264enc bitrate=16000"), "{c}");
        assert!(c.contains(r#"filesink location="C:\\Vidéos\\a.mp4""#), "{c}");
        assert!(!c.contains("wasapi2src"), "sans son : {c}");
        assert!(!c.contains("nvd3d11h264enc") && !c.contains("x264enc"), "{c}");
    }

    #[test]
    fn une_zone_est_decoupee_aux_dimensions_paires() {
        let c = chaine(&ecran(Some([101, 50, 1281, 721])), "a.mp4").unwrap();
        assert!(c.contains("crop-x=101 crop-y=50 crop-width=1280 crop-height=720"), "{c}");
    }

    #[test]
    fn le_son_du_pc_et_le_micro_sont_melanges() {
        let mut r = ecran(None);
        r.son = Son { pc: true, micro: Some(String::new()) };
        let c = chaine(&r, "a.mp4").unwrap();
        assert!(c.contains("audiomixer name=mix ! mfaacenc"), "{c}");
        assert!(c.contains("wasapi2src loopback=true"), "{c}");
        assert_eq!(c.matches("! queue ! mix.").count(), 2, "{c}");
        // Un seul : pas de mélangeur.
        r.son = Son { pc: false, micro: Some("{0.0.1}.{abc}".into()) };
        let c = chaine(&r, "a.mp4").unwrap();
        assert!(!c.contains("audiomixer") && c.contains(r#"wasapi2src device="{0.0.1}.{abc}""#), "{c}");
    }

    #[test]
    fn l_incrustation_compose_l_ecran_et_la_webcam_dans_le_coin() {
        let mut r = ecran(None);
        r.incrustation = Some(Incrustation { peripherique: r#"Web "Cam""#.into(), coin: Coin::BasDroit, taille_pct: 20 });
        let c = chaine(&r, "a.mp4").unwrap();
        let (x, y, l, h) = place_vignette(3840, 2160, Coin::BasDroit, 20);
        assert_eq!((l, h), (768, 432));
        assert_eq!((x + l, y + h), (3840 - 76, 2160 - 76), "marge de 2 %");
        assert!(c.contains(&format!("sink_1::xpos={x} sink_1::ypos={y} sink_1::width=768 sink_1::height=432")), "{c}");
        assert!(c.contains("width=3840,height=2160"), "{c}");
        assert!(c.contains(r#"device-name="Web \"Cam\"""#), "{c}");
        assert!(c.contains("! queue ! c.sink_0") && c.contains("d3d11upload ! queue ! c.sink_1"), "{c}");
        // Le `videorate` qui rend un horodatage à chaque image (voir la doc du module).
        assert!(c.contains("videorate ! video/x-raw,format=NV12"), "{c}");
    }

    /// **LA PHOTO INCRUSTE COMME LA VIDÉO** : même boîte (`place_vignette`), même
    /// règle `keep-aspect-ratio`, et l'image d'écran garde ses dimensions.
    #[test]
    fn la_photo_incruste_la_webcam_au_meme_coin_que_la_video() {
        fn png(image: image::RgbaImage) -> Vec<u8> {
            let mut sortie = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(image).write_to(&mut sortie, image::ImageFormat::Png).unwrap();
            sortie.into_inner()
        }
        let ecran = png(image::RgbaImage::from_pixel(640, 360, image::Rgba([10, 20, 30, 255])));
        // Une caméra 16:9, comme celle de la vidéo.
        let webcam = png(image::RgbaImage::from_pixel(320, 180, image::Rgba([200, 0, 0, 255])));
        let compose = composer_la_vignette(&ecran, &webcam, Coin::BasDroit, 25).unwrap();
        let sortie = image::load_from_memory(&compose).unwrap().to_rgba8();
        assert_eq!(sortie.dimensions(), (640, 360), "l'écran a changé de taille");
        let (x, y, l, h) = place_vignette(640, 360, Coin::BasDroit, 25);
        assert_eq!(
            sortie.get_pixel((x + l / 2) as u32, (y + h / 2) as u32),
            &image::Rgba([200, 0, 0, 255]),
            "la vignette n'est pas au coin bas-droit"
        );
        assert_eq!(sortie.get_pixel(2, 2), &image::Rgba([10, 20, 30, 255]), "le fond a été touché");
        // Une caméra 4:3 tient DANS la boîte, centrée : ses coins ne la débordent pas.
        let carree = png(image::RgbaImage::from_pixel(400, 300, image::Rgba([200, 0, 0, 255])));
        let compose = composer_la_vignette(&ecran, &carree, Coin::HautGauche, 25).unwrap();
        let sortie = image::load_from_memory(&compose).unwrap().to_rgba8();
        let (x, y, _, h) = place_vignette(640, 360, Coin::HautGauche, 25);
        assert_eq!(sortie.get_pixel((x + 1) as u32, (y + h / 2) as u32), &image::Rgba([10, 20, 30, 255]));
    }

    #[test]
    fn la_vignette_reste_dans_l_image_a_chaque_coin() {
        for coin in Coin::TOUS {
            for pct in [0, 20, 90] {
                let (x, y, l, h) = place_vignette(1280, 720, coin, pct);
                assert!(x >= 0 && y >= 0 && x + l <= 1280 && y + h <= 720, "{coin:?} {pct} : {x},{y} {l}x{h}");
            }
        }
        assert_eq!(Coin::depuis_nom("haut-gauche"), Some(Coin::HautGauche));
        assert_eq!(Coin::depuis_nom("milieu"), None);
    }

    #[test]
    fn le_son_seul_choisit_son_format_et_rien_ne_sort_sans_son() {
        let mut r = ecran(None);
        r.source = Source::SonSeul;
        assert_eq!(chaine(&r, "a.m4a"), None, "son seul sans aucun son : rien à enregistrer");
        r.son.micro = Some(String::new());
        assert!(chaine(&r, "a.m4a").unwrap().contains("mfaacenc bitrate=160000 ! aacparse ! mp4mux ! filesink"));
        r.format_son = FormatSon::Mp3;
        assert!(chaine(&r, "a.mp3").unwrap().contains("lamemp3enc target=bitrate bitrate=160 cbr=true ! filesink"));
        r.format_son = FormatSon::Wav;
        let c = chaine(&r, "a.wav").unwrap();
        assert!(c.contains("wavenc ! filesink") && !c.contains("d3d11"), "{c}");
        assert_eq!(r.extension(), "wav");
        assert_eq!(ecran(None).extension(), "mp4");
    }

    #[test]
    fn la_webcam_seule_ne_passe_pas_par_d3d11() {
        let mut r = ecran(None);
        r.source = Source::Webcam { peripherique: "cam".into() };
        r.son.micro = Some(String::new());
        let c = chaine(&r, "a.mp4").unwrap();
        assert!(c.contains("mfvideosrc device-name=\"cam\" ! video/x-raw,width=[1,1920],height=[1,1080]"), "{c}");
        assert!(!c.contains("d3d11"), "{c}");
        assert!(c.contains("mfaacenc"), "{c}");
    }

    /// ⚠️ **ESSAI SUR LE MATÉRIEL, À LA DEMANDE** :
    /// `NEURADESK_ESSAI_CHAINES=<dossier> cargo test ecrire_les_chaines_d_essai -- --ignored`
    /// écrit une chaîne par cas dans `<dossier>`, à faire tourner ensuite par
    /// `neuradesk.exe --enregistrer <fichier>` (les vrais écran, webcam et micro).
    #[test]
    #[ignore]
    fn ecrire_les_chaines_d_essai() {
        let dossier = std::path::PathBuf::from(std::env::var("NEURADESK_ESSAI_CHAINES").expect("dossier"));
        let moniteur: isize = std::env::var("NEURADESK_ESSAI_MONITEUR").unwrap().parse().unwrap();
        let webcam = std::env::var("NEURADESK_ESSAI_WEBCAM").unwrap();
        let base = Reglages {
            source: Source::Ecran { moniteur, largeur: 3840, hauteur: 2160, zone: None },
            ..ecran(None)
        };
        let son_pc_micro = Son { pc: true, micro: Some(String::new()) };
        let cas: Vec<(&str, Reglages)> = vec![
            ("ecran", base.clone()),
            ("zone_pc", Reglages { source: Source::Ecran { moniteur, largeur: 3840, hauteur: 2160, zone: Some([480, 300, 1280, 720]) }, son: Son { pc: true, micro: None }, ..base.clone() }),
            ("incrustation_deux", Reglages { incrustation: Some(Incrustation { peripherique: webcam.clone(), coin: Coin::BasDroit, taille_pct: 20 }), son: son_pc_micro.clone(), ..base.clone() }),
            ("webcam_micro", Reglages { source: Source::Webcam { peripherique: webcam }, son: Son { pc: false, micro: Some(String::new()) }, ..base.clone() }),
            ("son_m4a", Reglages { source: Source::SonSeul, son: son_pc_micro.clone(), ..base.clone() }),
            ("son_mp3", Reglages { source: Source::SonSeul, son: son_pc_micro.clone(), format_son: FormatSon::Mp3, ..base.clone() }),
            ("son_wav", Reglages { source: Source::SonSeul, son: Son { pc: false, micro: Some(String::new()) }, format_son: FormatSon::Wav, ..base }),
        ];
        for (nom, r) in cas {
            let sortie = dossier.join(format!("{nom}.{}", r.extension()));
            std::fs::write(dossier.join(format!("{nom}.chaine")), chaine(&r, &sortie.to_string_lossy()).unwrap()).unwrap();
        }
    }

    #[test]
    fn les_guillemets_et_barres_obliques_sont_echappes() {
        assert_eq!(quoter(r#"a"b\c"#), r#""a\"b\\c""#);
    }
}
