//! **LE FORMAT DE GIMP** (XCF), lu et écrit à la main d'après
//! `docs/xcf-gimp.md` — la spécification officielle, copiée dans le dépôt.
//!
//! ⛔ **POURQUOI PAS UNE CRATE** : la seule disponible (`xcf` 0.4) est en
//! lecture seule et refuse les fichiers de GIMP 3 (`UnknownVersion` sur
//! `gimp xcf v023`, MESURÉ le 2026-09-25 sur un fichier enregistré par GIMP
//! 3.2.6). Aucune n'écrit.
//!
//! **LIRE** accepte ce que GIMP 2.10 et 3.x écrivent : pointeurs 64 bits (XCF
//! ≥ 11), toutes les précisions (8/16/32 bits entiers, 16/32/64 bits
//! flottants, linéaires ou non — ramenées en sRGB 8 bits), RVB, niveaux de
//! gris et couleurs indexées, tuiles brutes, RLE ou zlib, masques de calque
//! (appliqués à l'alpha), groupes (APLATIS : leurs calques gardent leur place,
//! leur visibilité et leur opacité héritent de celles du groupe).
//!
//! **ÉCRIRE** vise la PLUS PETITE version qui porte les modes de GIMP 2.10 —
//! XCF 10, comme le recommande la spécification (« GIMP's XCF writer
//! dynamically selects the lowest version… »).
//!
//! Tout accès au fichier est vérifié : un fichier corrompu rend une `Err`,
//! jamais une panique (le dock est compilé en `panic = "abort"`).

use super::document_image::{cote_valide, Budget, Calque, Document, Mode};

const TUILE: u32 = 64;

const PROP_END: u32 = 0;
const PROP_COLORMAP: u32 = 1;
const PROP_FLOATING_SELECTION: u32 = 5;
const PROP_OPACITY: u32 = 6;
const PROP_MODE: u32 = 7;
const PROP_VISIBLE: u32 = 8;
const PROP_APPLY_MASK: u32 = 11;
const PROP_OFFSETS: u32 = 15;
const PROP_COMPRESSION: u32 = 17;
const PROP_GROUP_ITEM: u32 = 29;
const PROP_ITEM_PATH: u32 = 30;
const PROP_FLOAT_OPACITY: u32 = 33;
const PROP_COMPOSITE_MODE: u32 = 35;
const PROP_COMPOSITE_SPACE: u32 = 36;
const PROP_BLEND_SPACE: u32 = 37;

const COMPRESSION_AUCUNE: u8 = 0;
const COMPRESSION_RLE: u8 = 1;
const COMPRESSION_ZLIB: u8 = 2;

/// La version écrite : la première qui connaît les modes 28 et suivants.
const VERSION_ECRITE: u32 = 10;
/// « 8-bit gamma integer » (XCF ≥ 7).
const PRECISION_8_BITS_GAMMA: u32 = 150;
/// L'espace « RVB non linéaire » (ex-« perceptuel » de GIMP 2.10) : celui où le
/// navigateur fusionne. Sans lui, GIMP fusionnerait `multiply`, `screen`… en
/// RVB LINÉAIRE et le fichier ne ressemblerait pas à l'écran de l'éditeur.
const ESPACE_RVB_NON_LINEAIRE: i32 = 2;
/// « Union » : le composite ordinaire d'un calque.
const COMPOSITE_UNION: i32 = 1;

// ── Lecture ─────────────────────────────────────────────────────────────────

struct Lecteur<'a> {
    o: &'a [u8],
    pos: usize,
    pointeurs_64: bool,
}

impl<'a> Lecteur<'a> {
    fn octets(&mut self, n: usize) -> Result<&'a [u8], String> {
        let fin = self.pos.checked_add(n).filter(|&f| f <= self.o.len()).ok_or("fichier XCF tronqué")?;
        let s = &self.o[self.pos..fin];
        self.pos = fin;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.octets(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.octets(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }

    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_bits(self.u32()?))
    }

    fn pointeur(&mut self) -> Result<usize, String> {
        if self.pointeurs_64 {
            let v = u64::from_be_bytes(self.octets(8)?.try_into().unwrap());
            usize::try_from(v).map_err(|_| "pointeur hors du fichier".to_string())
        } else {
            Ok(self.u32()? as usize)
        }
    }

    fn chaine(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        if n == 0 {
            return Ok(String::new());
        }
        let octets = self.octets(n)?;
        Ok(String::from_utf8_lossy(&octets[..n - 1]).into_owned())
    }

    fn aller(&mut self, pos: usize) -> Result<(), String> {
        if pos > self.o.len() {
            return Err("pointeur hors du fichier".into());
        }
        self.pos = pos;
        Ok(())
    }

    /// Lit une liste de propriétés : `f(type, lecteur_sur_la_charge)` pour
    /// chacune, puis se place APRÈS la charge annoncée — une propriété
    /// inconnue est sautée, une connue ne peut pas déborder.
    fn proprietes(&mut self, mut f: impl FnMut(u32, &mut Lecteur<'a>) -> Result<(), String>) -> Result<(), String> {
        loop {
            let genre = self.u32()?;
            let longueur = self.u32()? as usize;
            if genre == PROP_END {
                return Ok(());
            }
            let debut = self.pos;
            let charge = self.octets(longueur)?;
            let mut sous = Lecteur { o: charge, pos: 0, pointeurs_64: self.pointeurs_64 };
            f(genre, &mut sous)?;
            self.pos = debut + longueur;
        }
    }
}

/// Comment un composant de pixel est codé (voir le champ « precision »).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Composant {
    U8,
    U16,
    U32,
    F16,
    F32,
    F64,
}

impl Composant {
    fn taille(self) -> usize {
        match self {
            Composant::U8 => 1,
            Composant::U16 | Composant::F16 => 2,
            Composant::U32 | Composant::F32 => 4,
            Composant::F64 => 8,
        }
    }

    /// La valeur de 0 à 1 du composant au début de `o` (grand-boutien).
    fn lire(self, o: &[u8]) -> f32 {
        match self {
            Composant::U8 => f32::from(o[0]) / 255.0,
            Composant::U16 => f32::from(u16::from_be_bytes([o[0], o[1]])) / 65535.0,
            Composant::U32 => (f64::from(u32::from_be_bytes([o[0], o[1], o[2], o[3]])) / f64::from(u32::MAX)) as f32,
            Composant::F16 => demi_vers_f32(u16::from_be_bytes([o[0], o[1]])),
            Composant::F32 => f32::from_be_bytes([o[0], o[1], o[2], o[3]]),
            Composant::F64 => f64::from_be_bytes(o[..8].try_into().unwrap()) as f32,
        }
    }
}

/// Un flottant de 16 bits (IEEE 754 « half ») en `f32`.
fn demi_vers_f32(h: u16) -> f32 {
    let signe = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exposant = i32::from((h >> 10) & 0x1f);
    let mantisse = f32::from(h & 0x3ff);
    signe
        * match exposant {
            0 => mantisse * 2f32.powi(-24),
            31 => f32::INFINITY,
            e => (1.0 + mantisse / 1024.0) * 2f32.powi(e - 15),
        }
}

/// Le composant et la linéarité d'une précision, selon la version du fichier.
fn precision(version: u32, code: u32) -> Result<(Composant, bool), String> {
    use Composant::*;
    let r = match (version, code) {
        (0..=3, _) => (U8, false),
        (4, 0) => (U8, false),
        (4, 1) => (U16, false),
        (4, 2) => (U32, true),
        (4, 3) => (F16, true),
        (4, 4) => (F32, true),
        (5 | 6, 400) => (F16, true),
        (5 | 6, 450) => (F16, false),
        (5 | 6, 500) => (F32, true),
        (5 | 6, 550) => (F32, false),
        (_, 100) => (U8, true),
        (_, 150) => (U8, false),
        (_, 200) => (U16, true),
        (_, 250) => (U16, false),
        (_, 300) => (U32, true),
        (_, 350) => (U32, false),
        (7.., 500) => (F16, true),
        (7.., 550) => (F16, false),
        (7.., 600) => (F32, true),
        (7.., 650) => (F32, false),
        (7.., 700) => (F64, true),
        (7.., 750) => (F64, false),
        _ => return Err(format!("précision XCF inconnue : {code} (version {version})")),
    };
    Ok(r)
}

/// Linéaire → sRGB, sur une valeur de 0 à 1.
fn vers_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn octet(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Le mode d'un calque XCF dans ceux de la page — le plus proche quand la
/// page ne l'a pas (« Addition », « Grain merge »… deviennent Normal).
fn mode_depuis_xcf(code: u32) -> Mode {
    match code {
        3 | 30 => Mode::Multiply,
        4 | 31 => Mode::Screen,
        5 | 23 => Mode::Overlay,
        6 | 32 => Mode::Difference,
        9 | 35 | 54 => Mode::Darken,
        10 | 36 | 55 => Mode::Lighten,
        11 | 24 | 37 => Mode::Hue,
        12 | 25 | 38 => Mode::Saturation,
        13 | 26 | 39 => Mode::Color,
        14 | 27 | 40 | 56 => Mode::Luminosity,
        16 | 42 => Mode::ColorDodge,
        17 | 43 => Mode::ColorBurn,
        18 | 44 => Mode::HardLight,
        19 | 45 => Mode::SoftLight,
        52 => Mode::Exclusion,
        _ => Mode::Normal,
    }
}

/// Le mode de GIMP écrit pour un mode de la page — ceux que GIMP choisit
/// lui-même en important un PSD (HSV pour teinte, saturation et valeur, HSL
/// pour la couleur), pour qu'un PSD et un XCF de l'éditeur s'ouvrent pareil.
fn mode_vers_xcf(mode: Mode) -> u32 {
    match mode {
        Mode::Normal => 28,
        Mode::Multiply => 30,
        Mode::Screen => 31,
        Mode::Overlay => 23,
        Mode::Darken => 35,
        Mode::Lighten => 36,
        Mode::ColorDodge => 42,
        Mode::ColorBurn => 43,
        Mode::HardLight => 44,
        Mode::SoftLight => 45,
        Mode::Difference => 32,
        Mode::Exclusion => 52,
        Mode::Hue => 37,
        Mode::Saturation => 38,
        Mode::Color => 39,
        Mode::Luminosity => 40,
    }
}

/// Ce que l'en-tête dit de la façon de lire les pixels.
struct Contexte {
    base: u32,
    composant: Composant,
    lineaire: bool,
    compression: u8,
    palette: Vec<[u8; 3]>,
}

/// Un calque tel que lu, avant l'aplatissement des groupes.
struct Lu {
    calque: Calque,
    groupe: bool,
    chemin: Option<Vec<u32>>,
}

/// Lit un fichier XCF (voir la doc du module).
pub fn lire(o: &[u8]) -> Result<Document, String> {
    let mut l = Lecteur { o, pos: 0, pointeurs_64: false };
    if l.octets(9)? != b"gimp xcf " {
        return Err("ce n'est pas un fichier XCF".into());
    }
    let etiquette = l.octets(4)?;
    let version = match etiquette {
        b"file" => 0,
        [b'v', a, b, c] if [a, b, c].iter().all(|d| d.is_ascii_digit()) => {
            u32::from(a - b'0') * 100 + u32::from(b - b'0') * 10 + u32::from(c - b'0')
        }
        _ => return Err("version XCF illisible".into()),
    };
    if l.u8()? != 0 {
        return Err("en-tête XCF invalide".into());
    }
    let largeur = l.u32()?;
    let hauteur = l.u32()?;
    if !cote_valide(largeur, hauteur) {
        return Err(format!("taille d'image XCF refusée : {largeur}×{hauteur}"));
    }
    let base = l.u32()?;
    if base > 2 {
        return Err(format!("mode de couleur XCF inconnu : {base}"));
    }
    let code_precision = if version >= 4 { l.u32()? } else { 150 };
    let (composant, lineaire) = precision(version, code_precision)?;
    l.pointeurs_64 = version >= 11;

    let mut compression = COMPRESSION_RLE;
    let mut palette = Vec::new();
    l.proprietes(|genre, p| {
        match genre {
            PROP_COMPRESSION => compression = p.u8()?,
            PROP_COLORMAP => {
                let n = p.u32()?.min(256);
                palette = (0..n).map(|_| Ok([p.u8()?, p.u8()?, p.u8()?])).collect::<Result<_, String>>()?;
            }
            _ => {}
        }
        Ok(())
    })?;
    if compression > COMPRESSION_ZLIB {
        return Err(format!("compression XCF inconnue : {compression}"));
    }
    let contexte = Contexte { base, composant, lineaire, compression, palette };

    let mut pointeurs = Vec::new();
    loop {
        match l.pointeur()? {
            0 => break,
            p => pointeurs.push(p),
        }
    }
    let mut budget = Budget::nouveau();
    let mut lus = Vec::new();
    for p in pointeurs {
        l.aller(p)?;
        if let Some(lu) = lire_calque(&mut l, &contexte, &mut budget)? {
            lus.push(lu);
        }
    }
    let calques = aplatir_groupes(lus);
    if calques.is_empty() {
        return Err("le fichier XCF n'a aucun calque".into());
    }
    Ok(Document { largeur, hauteur, calques })
}

/// Un calque à `l.pos` — `None` pour une sélection flottante, qui n'est pas un
/// calque de l'image.
fn lire_calque(l: &mut Lecteur, c: &Contexte, budget: &mut Budget) -> Result<Option<Lu>, String> {
    let largeur = l.u32()?;
    let hauteur = l.u32()?;
    let genre = l.u32()?;
    let nom = l.chaine()?;
    if !cote_valide(largeur, hauteur) {
        return Err(format!("calque « {nom} » de taille refusée : {largeur}×{hauteur}"));
    }
    let mut calque = Calque { nom, x: 0, y: 0, largeur, hauteur, rgba: Vec::new(), opacite: 1.0, visible: true, mode: Mode::Normal };
    let (mut groupe, mut flottant, mut appliquer_masque) = (false, false, true);
    let mut chemin = None;
    l.proprietes(|p, v| {
        match p {
            PROP_OPACITY => calque.opacite = (v.u32()?.min(255) as f32) / 255.0,
            PROP_FLOAT_OPACITY => calque.opacite = v.f32()?.clamp(0.0, 1.0),
            PROP_VISIBLE => calque.visible = v.u32()? != 0,
            PROP_MODE => calque.mode = mode_depuis_xcf(v.u32()?),
            PROP_OFFSETS => {
                calque.x = v.i32()?;
                calque.y = v.i32()?;
            }
            PROP_GROUP_ITEM => groupe = true,
            PROP_FLOATING_SELECTION => flottant = true,
            PROP_APPLY_MASK => appliquer_masque = v.u32()? != 0,
            PROP_ITEM_PATH => chemin = Some((0..v.o.len() / 4).map(|_| v.u32()).collect::<Result<Vec<_>, _>>()?),
            _ => {}
        }
        Ok(())
    })?;
    if flottant {
        return Ok(None);
    }
    let hierarchie = l.pointeur()?;
    let masque = l.pointeur()?;
    if !groupe {
        budget.prendre(largeur, hauteur)?;
        let avec_alpha = genre % 2 == 1;
        calque.rgba = lire_pixels(l, c, hierarchie, largeur, hauteur, avec_alpha)?;
        if masque != 0 && appliquer_masque {
            appliquer_un_masque(l, c, masque, &mut calque, budget)?;
        }
    }
    Ok(Some(Lu { calque, groupe, chemin }))
}

/// Les pixels d'une hiérarchie, en RGBA 8 bits sRGB.
fn lire_pixels(l: &mut Lecteur, c: &Contexte, hierarchie: usize, largeur: u32, hauteur: u32, avec_alpha: bool) -> Result<Vec<u8>, String> {
    let canaux = match c.base {
        0 => 3,
        1 => 1,
        _ => 1,
    } + usize::from(avec_alpha);
    // Les index d'une image indexée sont toujours sur un octet.
    let composant = if c.base == 2 { Composant::U8 } else { c.composant };
    let brut = lire_hierarchie(l, c, hierarchie, largeur, hauteur, canaux * composant.taille())?;
    let n = composant.taille();
    let mut rgba = Vec::with_capacity(brut.len() / (canaux * n) * 4);
    let couleur = |v: f32| octet(if c.lineaire { vers_srgb(v) } else { v });
    for px in brut.chunks_exact(canaux * n) {
        let lire = |i: usize| composant.lire(&px[i * n..]);
        let alpha = if avec_alpha { octet(lire(canaux - 1)) } else { 255 };
        match c.base {
            0 => rgba.extend_from_slice(&[couleur(lire(0)), couleur(lire(1)), couleur(lire(2)), alpha]),
            1 => {
                let g = couleur(lire(0));
                rgba.extend_from_slice(&[g, g, g, alpha]);
            }
            _ => {
                let [r, g, b] = c.palette.get(usize::from(px[0])).copied().unwrap_or([0, 0, 0]);
                rgba.extend_from_slice(&[r, g, b, alpha]);
            }
        }
    }
    Ok(rgba)
}

/// Multiplie l'alpha du calque par son masque (un canal gris sans alpha).
fn appliquer_un_masque(l: &mut Lecteur, c: &Contexte, masque: usize, calque: &mut Calque, budget: &mut Budget) -> Result<(), String> {
    l.aller(masque)?;
    let largeur = l.u32()?;
    let hauteur = l.u32()?;
    let _nom = l.chaine()?;
    l.proprietes(|_, _| Ok(()))?;
    let hierarchie = l.pointeur()?;
    if (largeur, hauteur) != (calque.largeur, calque.hauteur) {
        return Err(format!("masque {largeur}×{hauteur} pour le calque « {} »", calque.nom));
    }
    budget.prendre(largeur, hauteur)?;
    let gris = Contexte { base: 1, palette: Vec::new(), ..*c };
    let valeurs = lire_pixels(l, &gris, hierarchie, largeur, hauteur, false)?;
    for (px, m) in calque.rgba.chunks_exact_mut(4).zip(valeurs.chunks_exact(4)) {
        px[3] = ((u16::from(px[3]) * u16::from(m[0]) + 127) / 255) as u8;
    }
    Ok(())
}

/// Les octets bruts (`bpp` par pixel, ligne à ligne) d'une hiérarchie.
fn lire_hierarchie(l: &mut Lecteur, c: &Contexte, hierarchie: usize, largeur: u32, hauteur: u32, bpp: usize) -> Result<Vec<u8>, String> {
    l.aller(hierarchie)?;
    let (hl, hh, hbpp) = (l.u32()?, l.u32()?, l.u32()? as usize);
    if (hl, hh) != (largeur, hauteur) || hbpp != bpp {
        return Err(format!("hiérarchie {hl}×{hh}×{hbpp} pour un calque {largeur}×{hauteur}×{bpp}"));
    }
    let niveau = l.pointeur()?;
    l.aller(niveau)?;
    let (nl, nh) = (l.u32()?, l.u32()?);
    if (nl, nh) != (largeur, hauteur) {
        return Err("niveau de taille incohérente".into());
    }
    let (tx, ty) = (largeur.div_ceil(TUILE), hauteur.div_ceil(TUILE));
    let tuiles = (0..tx * ty).map(|_| l.pointeur()).collect::<Result<Vec<_>, _>>()?;
    let mut sortie = vec![0u8; largeur as usize * hauteur as usize * bpp];
    for (i, &debut) in tuiles.iter().enumerate() {
        let (cx, cy) = (i as u32 % tx * TUILE, i as u32 / tx * TUILE);
        let (tl, th) = ((largeur - cx).min(TUILE) as usize, (hauteur - cy).min(TUILE) as usize);
        // Une tuile compressée finit où commence la suivante ; la dernière, au
        // plus loin à la fin du fichier.
        let fin = tuiles.get(i + 1).copied().filter(|&f| f > debut).unwrap_or(l.o.len()).min(l.o.len());
        let donnees = l.o.get(debut..fin).ok_or("tuile hors du fichier")?;
        let pixels = decoder_tuile(c.compression, donnees, tl * th, bpp)?;
        for ligne in 0..th {
            let dest = ((cy as usize + ligne) * largeur as usize + cx as usize) * bpp;
            sortie[dest..dest + tl * bpp].copy_from_slice(&pixels[ligne * tl * bpp..(ligne + 1) * tl * bpp]);
        }
    }
    Ok(sortie)
}

/// Les `n` pixels d'une tuile, entrelacés (`bpp` octets par pixel).
fn decoder_tuile(compression: u8, donnees: &[u8], n: usize, bpp: usize) -> Result<Vec<u8>, String> {
    let taille = n * bpp;
    match compression {
        COMPRESSION_AUCUNE => donnees.get(..taille).map(<[u8]>::to_vec).ok_or_else(|| "tuile tronquée".to_string()),
        COMPRESSION_ZLIB => {
            let v = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(donnees, taille).map_err(|e| format!("tuile zlib illisible : {e:?}"))?;
            if v.len() != taille {
                return Err("tuile zlib de taille incohérente".into());
            }
            Ok(v)
        }
        _ => decoder_rle(donnees, n, bpp),
    }
}

/// Le RLE de XCF : un flux par octet du pixel (tous les premiers octets, puis
/// tous les deuxièmes…), chacun fait d'opérations « répète » et « copie ».
fn decoder_rle(donnees: &[u8], n: usize, bpp: usize) -> Result<Vec<u8>, String> {
    let mut sortie = vec![0u8; n * bpp];
    let mut pos = 0;
    let mut lire = |k: usize| -> Result<&[u8], String> {
        let s = donnees.get(pos..pos + k).ok_or("tuile RLE tronquée")?;
        pos += k;
        Ok(s)
    };
    for plan in 0..bpp {
        let mut i = 0;
        while i < n {
            let op = lire(1)?[0];
            let (repete, compte) = match op {
                0..=126 => (true, usize::from(op) + 1),
                127 => {
                    let pq = lire(2)?;
                    (true, usize::from(pq[0]) * 256 + usize::from(pq[1]))
                }
                128 => {
                    let pq = lire(2)?;
                    (false, usize::from(pq[0]) * 256 + usize::from(pq[1]))
                }
                _ => (false, 256 - usize::from(op)),
            };
            if i + compte > n {
                return Err("tuile RLE qui déborde".into());
            }
            if repete {
                let v = lire(1)?[0];
                for k in i..i + compte {
                    sortie[k * bpp + plan] = v;
                }
            } else {
                for (k, &v) in lire(compte)?.iter().enumerate() {
                    sortie[(i + k) * bpp + plan] = v;
                }
            }
            i += compte;
        }
    }
    Ok(sortie)
}

/// Retire les groupes : leurs calques restent à leur place, avec la visibilité
/// et l'opacité de tous leurs groupes parents. Le fichier range du HAUT vers
/// le BAS ; le document, du bas vers le haut.
fn aplatir_groupes(lus: Vec<Lu>) -> Vec<Calque> {
    // Le chemin de chaque élément : celui de `PROP_ITEM_PATH`, ou sa place parmi
    // les éléments du premier niveau (qui n'en ont pas).
    let mut premier_niveau = 0;
    let chemins: Vec<Vec<u32>> = lus
        .iter()
        .map(|lu| {
            lu.chemin.clone().unwrap_or_else(|| {
                premier_niveau += 1;
                vec![premier_niveau - 1]
            })
        })
        .collect();
    let groupes: Vec<(&Vec<u32>, bool, f32)> = lus
        .iter()
        .zip(&chemins)
        .filter(|(lu, _)| lu.groupe)
        .map(|(lu, chemin)| (chemin, lu.calque.visible, lu.calque.opacite))
        .collect();
    let mut calques: Vec<Calque> = lus
        .into_iter()
        .zip(&chemins)
        .filter(|(lu, _)| !lu.groupe)
        .map(|(lu, chemin)| {
            let mut calque = lu.calque;
            for (parent, visible, opacite) in &groupes {
                if parent.len() < chemin.len() && chemin.starts_with(parent) {
                    calque.visible &= visible;
                    calque.opacite *= opacite;
                }
            }
            calque
        })
        .collect();
    calques.reverse();
    calques
}

// ── Écriture ────────────────────────────────────────────────────────────────

struct Ecrivain(Vec<u8>);

impl Ecrivain {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }

    fn chaine(&mut self, s: &str) {
        // Un NUL dans le nom couperait la chaîne à la lecture.
        let s = s.replace('\0', "");
        self.u32(s.len() as u32 + 1);
        self.0.extend_from_slice(s.as_bytes());
        self.0.push(0);
    }

    fn propriete(&mut self, genre: u32, charge: &[u8]) {
        self.u32(genre);
        self.u32(charge.len() as u32);
        self.0.extend_from_slice(charge);
    }

    fn propriete_u32(&mut self, genre: u32, v: u32) {
        self.propriete(genre, &v.to_be_bytes());
    }

    /// Réserve un pointeur (32 bits en XCF 10), à remplir par [`Self::poser`].
    fn reserver(&mut self) -> usize {
        self.u32(0);
        self.0.len() - 4
    }

    /// Écrit à `place` la position COURANTE, où va commencer ce qu'elle pointe.
    fn poser(&mut self, place: usize) -> Result<(), String> {
        let ici = u32::try_from(self.0.len()).map_err(|_| "fichier XCF de plus de 4 Go".to_string())?;
        self.0[place..place + 4].copy_from_slice(&ici.to_be_bytes());
        Ok(())
    }
}

/// Écrit `document` en XCF 10, 8 bits sRGB, tuiles zlib.
pub fn ecrire(document: &Document) -> Result<Vec<u8>, String> {
    let mut e = Ecrivain(Vec::new());
    e.0.extend_from_slice(format!("gimp xcf v{VERSION_ECRITE:03}\0").as_bytes());
    e.u32(document.largeur);
    e.u32(document.hauteur);
    e.u32(0); // RVB
    e.u32(PRECISION_8_BITS_GAMMA);
    e.propriete(PROP_COMPRESSION, &[COMPRESSION_ZLIB]);
    e.propriete(PROP_END, &[]);
    // Du HAUT vers le BAS.
    let places: Vec<usize> = document.calques.iter().map(|_| e.reserver()).collect();
    e.u32(0); // fin des calques
    e.u32(0); // aucun canal
    for (calque, place) in document.calques.iter().rev().zip(places) {
        e.poser(place)?;
        ecrire_calque(&mut e, calque)?;
    }
    Ok(e.0)
}

fn ecrire_calque(e: &mut Ecrivain, c: &Calque) -> Result<(), String> {
    if !cote_valide(c.largeur, c.hauteur) || c.rgba.len() != (c.largeur * c.hauteur * 4) as usize {
        return Err(format!("calque « {} » incohérent", c.nom));
    }
    e.u32(c.largeur);
    e.u32(c.hauteur);
    e.u32(1); // RVB avec alpha
    e.chaine(&c.nom);
    let opacite = c.opacite.clamp(0.0, 1.0);
    e.propriete_u32(PROP_OPACITY, (opacite * 255.0).round() as u32);
    e.propriete(PROP_FLOAT_OPACITY, &opacite.to_be_bytes());
    e.propriete_u32(PROP_VISIBLE, u32::from(c.visible));
    e.propriete_u32(PROP_MODE, mode_vers_xcf(c.mode));
    let mut decalage = c.x.to_be_bytes().to_vec();
    decalage.extend_from_slice(&c.y.to_be_bytes());
    e.propriete(PROP_OFFSETS, &decalage);
    e.propriete(PROP_BLEND_SPACE, &ESPACE_RVB_NON_LINEAIRE.to_be_bytes());
    e.propriete(PROP_COMPOSITE_SPACE, &ESPACE_RVB_NON_LINEAIRE.to_be_bytes());
    e.propriete(PROP_COMPOSITE_MODE, &COMPOSITE_UNION.to_be_bytes());
    e.propriete(PROP_END, &[]);
    let hierarchie = e.reserver();
    e.u32(0); // pas de masque
    e.poser(hierarchie)?;
    e.u32(c.largeur);
    e.u32(c.hauteur);
    e.u32(4);
    let niveau = e.reserver();
    e.u32(0); // aucun niveau factice
    e.poser(niveau)?;
    e.u32(c.largeur);
    e.u32(c.hauteur);
    let (tx, ty) = (c.largeur.div_ceil(TUILE), c.hauteur.div_ceil(TUILE));
    let tuiles: Vec<usize> = (0..tx * ty).map(|_| e.reserver()).collect();
    e.u32(0);
    for (i, place) in tuiles.into_iter().enumerate() {
        e.poser(place)?;
        let (cx, cy) = (i as u32 % tx * TUILE, i as u32 / tx * TUILE);
        let (tl, th) = ((c.largeur - cx).min(TUILE) as usize, (c.hauteur - cy).min(TUILE) as usize);
        let mut brut = Vec::with_capacity(tl * th * 4);
        for ligne in 0..th {
            let debut = ((cy as usize + ligne) * c.largeur as usize + cx as usize) * 4;
            brut.extend_from_slice(&c.rgba[debut..debut + tl * 4]);
        }
        e.0.extend(miniz_oxide::deflate::compress_to_vec_zlib(&brut, 6));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::win32::document_image::tests::exemple;

    #[test]
    fn ecrire_puis_lire_rend_le_document() {
        let doc = exemple();
        let octets = ecrire(&doc).unwrap();
        assert!(octets.starts_with(b"gimp xcf v010\0"));
        let relu = lire(&octets).unwrap();
        // L'opacité passe par un flottant 32 bits : exacte pour 0,5 et 1.
        assert_eq!(relu, doc);
    }

    /// Un fichier coupé n'importe où rend une erreur, jamais une panique.
    #[test]
    fn un_fichier_tronque_est_une_erreur() {
        let octets = ecrire(&exemple()).unwrap();
        for n in (0..octets.len()).step_by(97) {
            assert!(lire(&octets[..n]).is_err(), "coupé à {n}");
        }
    }

    #[test]
    fn le_rle_se_decode() {
        // Plan 1 : 3 × 7 puis « 1 2 » copiés ; plan 2 : 5 × 9 (forme longue).
        let donnees = [2, 7, 254, 1, 2, 127, 0, 5, 9];
        assert_eq!(decoder_rle(&donnees, 5, 2).unwrap(), vec![7, 9, 7, 9, 7, 9, 1, 9, 2, 9]);
        assert!(decoder_rle(&[10, 1], 5, 1).is_err(), "une répétition qui déborde");
    }

    #[test]
    fn les_precisions_se_ramenent_a_8_bits() {
        assert_eq!(demi_vers_f32(0x3c00), 1.0);
        assert_eq!(demi_vers_f32(0x3800), 0.5);
        assert_eq!(octet(vers_srgb(0.214_041_14)), 128);
        assert_eq!(precision(23, 150).unwrap(), (Composant::U8, false));
        assert_eq!(precision(23, 600).unwrap(), (Composant::F32, true));
        assert!(precision(23, 999).is_err());
    }

    /// Les groupes s'aplatissent : un calque d'un groupe caché est caché, et
    /// son opacité est multipliée par celle du groupe.
    #[test]
    fn un_groupe_transmet_visibilite_et_opacite() {
        let calque = |nom: &str, visible, opacite| Calque { nom: nom.into(), x: 0, y: 0, largeur: 1, hauteur: 1, rgba: vec![0; 4], opacite, visible, mode: Mode::Normal };
        let lus = vec![
            Lu { calque: calque("dessus", true, 1.0), groupe: false, chemin: None },
            Lu { calque: calque("groupe", false, 0.5), groupe: true, chemin: None },
            Lu { calque: calque("dedans", true, 0.5), groupe: false, chemin: Some(vec![1, 0]) },
            Lu { calque: calque("fond", true, 1.0), groupe: false, chemin: None },
        ];
        let plats = aplatir_groupes(lus);
        let noms: Vec<_> = plats.iter().map(|c| (c.nom.as_str(), c.visible, c.opacite)).collect();
        assert_eq!(noms, vec![("fond", true, 1.0), ("dedans", false, 0.25), ("dessus", true, 1.0)]);
    }

    /// **SONDE** (ignorée) : `NEURADESK_SONDE_SORTIE=<dossier>` — y écrit le
    /// document d'essai en `exemple.xcf` et `exemple.psd`, pour les faire ouvrir
    /// par GIMP.
    #[test]
    #[ignore]
    fn sonde_ecrire() {
        let dossier = std::path::PathBuf::from(std::env::var("NEURADESK_SONDE_SORTIE").expect("NEURADESK_SONDE_SORTIE"));
        let doc = exemple();
        std::fs::write(dossier.join("exemple.xcf"), ecrire(&doc).unwrap()).unwrap();
        let composite = vec![255u8; (doc.largeur * doc.hauteur * 4) as usize];
        let envoi = crate::win32::document_image::Envoi { document: doc, composite };
        std::fs::write(dossier.join("exemple.psd"), crate::win32::format_psd::ecrire(&envoi).unwrap()).unwrap();
    }

    /// **SONDE** (ignorée) : `NEURADESK_SONDE_XCF=<fichier>` — lit un vrai
    /// fichier de GIMP et en affiche les calques.
    #[test]
    #[ignore]
    fn sonde_xcf() {
        let chemin = std::env::var("NEURADESK_SONDE_XCF").expect("NEURADESK_SONDE_XCF");
        let doc = lire(&std::fs::read(&chemin).unwrap()).unwrap();
        println!("{}×{}", doc.largeur, doc.hauteur);
        for c in &doc.calques {
            println!("  {:?} ({}, {}) {}×{} {:?} opacité {} visible {} px0 {:?}", c.nom, c.x, c.y, c.largeur, c.hauteur, c.mode, c.opacite, c.visible, &c.rgba[..4]);
        }
    }
}
