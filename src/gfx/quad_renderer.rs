//! Renderer minimal : dessine une liste de rectangles à coins arrondis
//! (les icônes du dock) en un seul draw call instancié. Les coins arrondis
//! et l'anti-aliasing sont calculés dans le pixel shader via une distance
//! signée (SDF), donc aucune texture n'est nécessaire pour ce prototype.

use std::mem::size_of;

use windows::core::{s, Result};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32G32B32A32_FLOAT;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32_FLOAT;

use crate::gfx::material;
use crate::gfx::shader::{blob_bytes, compile_shader};

/// Nombre maximum de quads qu'un seul appel de dessin peut porter (l'instance
/// buffer est alloué une fois pour cette taille).
///
/// ⭐ 16 384 DEPUIS (2026-11-05) : les effets se découpent au grain du plugin
/// Windhawk — 5 px, soit jusqu'à 191 000 cases sur une fenêtre maximisée (voir
/// `ouverture_fenetres::effets::COTE_MOD`). À 4 096 il fallait 47 `Map`/`Unmap`
/// du tampon par image et autant d'appels de dessin ; à 16 384, douze.
pub const MAX_INSTANCES: usize = 16_384;

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    local_pos: [f32; 2],
}

/// Une icône à dessiner. Coordonnées en pixels, origine en haut à gauche
/// du dock (comme les coordonnées souris Win32).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct QuadInstance {
    /// x, y, largeur, hauteur en pixels.
    pub rect: [f32; 4],
    /// r, g, b, a en alpha "droit" (non prémultiplié), 0.0..1.0. Pour
    /// `TexMode::Backdrop`, l'alpha sert de force de teinte (mélange avec
    /// l'arrière-plan flouté) plutôt que de transparence de sortie.
    pub color: [f32; 4],
    /// Rayon des coins arrondis, en pixels.
    pub radius: f32,
    /// Échelle d'agrandissement (1.0 = taille normale, >1.0 = survolé).
    pub scale: f32,
    /// Mode de couleur : 0.0 = `color` seule (uni) ; 1.0 = texture pure
    /// (icônes réelles, voir `new_textured`) ; 2.0 = texture d'arrière-plan
    /// teintée par `color` (voir `new_backdrop`) ; 3.0 = glyphe blanc
    /// teinté par `color` (voir `new_glyph`).
    pub tex_mode: f32,
    /// Rotation, en RADIANS, autour du CENTRE du rectangle — 0.0 par défaut,
    /// donc toutes les instances existantes sont inchangées. Sert au
    /// balancement des icônes magnifiées (voir `ui::dock::hover_wobble_angle`)
    /// et au losange de l'indicateur d'application (voir `ui::dock::
    /// RunningDotShape`).
    ///
    /// A pris la place de l'ancien champ de remplissage `_pad`, ce qui avait
    /// laissé la structure à 48 octets ; `ring` en ajoute 4 (voir sa doc).
    pub rotation: f32,
    /// ÉPAISSEUR d'un ANNEAU, en pixels — `0.0` pour tout ce qui n'est PAS un
    /// anneau, donc toutes les instances existantes sont inchangées (voir
    /// `QuadInstance::new_ring`).
    ///
    /// Quand elle est non nulle, le fragment shader dessine un CONTOUR au lieu
    /// d'un aplat : la bande comprise entre le bord de la forme et ce même bord
    /// rentré d'autant, et rien au centre. C'est la seule façon d'obtenir une
    /// forme CREUSE avec ce pipeline — le masque plein ne sait pas en faire une
    /// (voir `ui::dock::RunningDotShape::Ring`).
    ///
    /// **Ajoute 4 octets à la structure, et un élément au layout d'entrée**
    /// (voir `create_input_layout`) : les deux DOIVENT rester d'accord. Un champ
    /// AJOUTÉ sans son élément échoue bruyamment (`CreateInputLayout` valide le
    /// layout contre la signature du vertex shader, qui exige `RING`) ; un champ
    /// RÉORDONNÉ, lui, est SILENCIEUX — les décalages glissent, le layout
    /// réussit, le GPU lit des octets faux. C'est le second cas que le test
    /// `quad_instance_layout_matches_the_input_layout` ferme.
    pub ring: f32,
    /// ⭐ BASCULE en arrière autour de l'axe HORIZONTAL du centre, en radians
    /// (positive : le haut part en arrière ; négative : le bas),
    /// vue en PERSPECTIVE — `0.0` pour tout le reste, donc toutes les instances
    /// existantes sont inchangées. C'est le repliement des icônes du lanceur
    /// de Unity (voir `ui::empilement`) : `rect` est alors l'icône ENTIÈRE,
    /// avant bascule, et le vertex shader la projette comme `IconRenderer.cpp`
    /// (caméra à une demi-hauteur d'écran, profondeur triplée, recul de
    /// `FOLDED_Z_DISTANCE` = 10 px par radian).
    ///
    /// Même contrat que `ring` : un élément de layout à son décalage (52).
    pub bascule: f32,
    /// ⭐ **SOUS-RECTANGLE DE LA TEXTURE** `(u0, v0, u1, v1)`, en 0..1 —
    /// `(0, 0, 1, 1)` par défaut, donc toutes les instances existantes sont
    /// inchangées. Il n'existe que pour les EFFETS EN TUILES (éclatement,
    /// génie : voir `win32::ouverture_fenetres::effets`) : sans lui, une
    /// fenêtre ne peut être dessinée qu'entière, en un seul quadrilatère.
    ///
    /// Même contrat que `ring` : un élément de layout à son décalage (56), et
    /// ⛔ un champ RÉORDONNÉ est SILENCIEUX (voir le test
    /// `quad_instance_layout_matches_the_input_layout`).
    pub uv: [f32; 4],
}

/// Mode de coloration d'un [`QuadInstance`] — voir `tex_mode`.
#[derive(Clone, Copy)]
enum TexMode {
    Solid = 0,
    Texture = 1,
    Backdrop = 2,
    Glyph = 3,
    /// Voir `QuadInstance::new_calque_texture`.
    CalqueTexture = 4,
    /// Voir `QuadInstance::avec_texture`.
    AplatTexture = 5,
}

impl QuadInstance {
    pub fn new(rect: [f32; 4], color: [f32; 4], radius: f32, scale: f32) -> Self {
        Self::with_mode(rect, color, radius, scale, TexMode::Solid)
    }

    /// Quad texturé (icône image) : `color` ne sert qu'en secours si la
    /// texture n'est pas liée, seule son alpha réelle vient de l'image.
    pub fn new_textured(rect: [f32; 4], radius: f32, scale: f32) -> Self {
        Self::with_mode(rect, [1.0, 1.0, 1.0, 1.0], radius, scale, TexMode::Texture)
    }

    /// Quad "glyphe" (texte/icône vectorielle rendue par `gfx::text::
    /// render_text` — horloge, glyphes de la barre du haut, etc.) : la
    /// texture ne porte que la couverture anti-aliasée dans son canal alpha
    /// (RGB toujours blanc, voir la doc de `render_text`), teintée ici par
    /// `color` plutôt qu'utilisée telle quelle — contrairement à
    /// `new_textured`, qui affiche la texture SANS AUCUNE recoloration
    /// (nécessaire pour une vraie icône d'application multicolore, où
    /// teindre casserait ses couleurs réelles). C'est cette distinction qui
    /// permet à l'utilisateur de choisir la couleur des éléments de la barre
    /// du haut (voir `win32::menu`) sans affecter les icônes du dock.
    pub fn new_glyph(rect: [f32; 4], color: [f32; 3]) -> Self {
        Self::with_mode(rect, [color[0], color[1], color[2], 1.0], 0.0, 1.0, TexMode::Glyph)
    }

    /// Comme `new_glyph`, mais avec un facteur d'échelle explicite (voir
    /// `scale`) — nécessaire pour les glyphes monochromes du DOCK (Lanceur,
    /// Presse-papiers, voir `ui::dock::Dock::external_glyph_instance`) qui,
    /// contrairement aux glyphes de la barre du haut, gardent l'effet de
    /// magnification au survol comme les autres icônes du dock.
    pub fn new_glyph_scaled(rect: [f32; 4], color: [f32; 3], scale: f32) -> Self {
        Self::with_mode(rect, [color[0], color[1], color[2], 1.0], 0.0, scale, TexMode::Glyph)
    }

    /// Quad d'arrière-plan flouté (voir `gfx::backdrop`) teinté par `tint`
    /// (RGB = couleur, A = force du mélange, 0.0 = flou brut, 1.0 = couleur
    /// pleine) — reproduit l'effet Mica/Acrylic de la barre des tâches
    /// Windows : flou du bureau en dessous + teinte adoucie. `radius` (px) :
    /// coins arrondis de la fenêtre ENTIÈRE (pas seulement des widgets à
    /// l'intérieur, voir `win32::popup_menu`) — le pixel shader (SDF, voir
    /// `RoundedBoxSDF`) rend les coins hors du rectangle arrondi totalement
    /// transparents (alpha prémultiplié, anti-aliasé), DWM compose alors une
    /// vraie fenêtre à coins arrondis sans le moindre découpage GDI
    /// (`SetWindowRgn`) : BUG RÉEL constaté en pratique — passer `0.0` ici
    /// (comme avant l'ajout de ce paramètre) laisse le fond flouté couvrir
    /// tout le rectangle de la fenêtre à bord droit, quels que soient les
    /// coins arrondis des boutons dessinés PAR-DESSUS par `egui` — la
    /// fenêtre elle-même reste carrée tant que ce fond ne l'est pas aussi.
    pub fn new_backdrop(rect: [f32; 4], tint: [f32; 4], radius: f32) -> Self {
        Self::with_mode(rect, tint, radius, 1.0, TexMode::Backdrop)
    }

    /// ⭐ **LE CALQUE DE TEXTURE** : la texture posée par `QuadRenderer::
    /// poser_texture`, SEULE, en alpha DROIT — blanc où elle éclaircit, noir où
    /// elle assombrit, l'alpha valant l'écart. Une image à poser PAR-DESSUS une
    /// couleur qu'on ne connaît pas ici : le fond d'un terminal, peint par sa page
    /// (voir `win32::texture_terminaux`).
    pub fn new_calque_texture(rect: [f32; 4]) -> Self {
        Self::with_mode(rect, [0.0; 4], 0.0, 1.0, TexMode::CalqueTexture)
    }

    /// ⭐ **L'APLAT TEXTURÉ** — DEMANDE EXPLICITE (2026-10-16) : « que la texture
    /// principale soit présente sur les indicateurs d'application ». Un aplat
    /// (`new`, `new_ring`) qui reçoit la texture de l'interface comme les fonds
    /// (voir `gfx::texture_interface`) : même nature, même échelle, en
    /// coordonnées écran. Sa forme (anneau compris) ne change pas.
    pub fn avec_texture(mut self) -> Self {
        self.tex_mode = TexMode::AplatTexture as u8 as f32;
        self
    }

    /// **CONGÉ** (voir `ui::dock::conges`) : l'arrière-plan teinté d'une barre,
    /// sur le carré `rect` MOINS le quart de disque centré sur son coin `code`
    /// (`ui::dock::code_conge`, de -1 à -4). Le code passe par `ring`, NÉGATIF :
    /// un anneau n'a jamais d'épaisseur négative (`new_ring` la borne à zéro).
    pub fn new_conge(rect: [f32; 4], tint: [f32; 4], code: f32) -> Self {
        let mut instance = Self::with_mode(rect, tint, 0.0, 1.0, TexMode::Backdrop);
        instance.ring = code;
        instance
    }

    /// ANNEAU : la bande d'épaisseur `thickness` comprise entre le bord de la
    /// forme et ce même bord rentré d'autant, et RIEN au centre — un contour,
    /// pas un aplat (voir `ring`). C'est la seule forme CREUSE que ce pipeline
    /// sache rendre : un quad ne peut pas en composer une autrement, et le
    /// masque plein ne sait pas en faire une (voir `ui::dock::
    /// RunningDotShape::Ring`, qui l'utilise pour entourer une icône).
    pub fn new_ring(rect: [f32; 4], color: [f32; 4], radius: f32, thickness: f32) -> Self {
        let mut instance = Self::with_mode(rect, color, radius, 1.0, TexMode::Solid);
        // Bornée à zéro : une épaisseur négative retomberait dans la branche du
        // masque plein (le shader teste `> 0.0`) et rendrait un APLAT au lieu de
        // rien — un repli surprenant pour un constructeur public.
        instance.ring = thickness.max(0.0);
        instance
    }

    fn with_mode(rect: [f32; 4], color: [f32; 4], radius: f32, scale: f32, mode: TexMode) -> Self {
        Self {
            rect,
            color,
            radius,
            scale,
            tex_mode: mode as u8 as f32,
            // Aucune rotation par défaut : toutes les instances existantes sont
            // donc inchangées (voir `rotation`).
            rotation: 0.0,
            // Aucun anneau par défaut : idem (voir `ring`).
            ring: 0.0,
            // Aucune bascule par défaut : idem (voir `bascule`).
            bascule: 0.0,
            // La texture ENTIÈRE par défaut : idem (voir `uv`).
            uv: [0.0, 0.0, 1.0, 1.0],
        }
    }
}

/// Paramètres de matériau (voir `gfx::material::PostParams`) lus par
/// `PSMain`, `TexMode::Backdrop` — un seul matériau actif pour toute
/// l'appli à la fois, d'où un réglage global dans ce constant buffer plutôt
/// que par instance (`QuadInstance`).
///
/// RÈGLE DE PACKING HLSL À RESPECTER SI CE STRUCT EST MODIFIÉ : un `float3`
/// ne doit jamais chevaucher une frontière de 16 octets. `hue_bias` démarre
/// pile à l'offset 32 parce que les sept `f32` qui précèdent (`size`,
/// `dither_amount`, `rim`, `gloss`, `saturation`, `tint_scale`,
/// `refraction`) totalisent exactement 32 octets — réordonner ces champs
/// SANS recalculer cet alignement casse silencieusement le rendu (pas
/// d'erreur de compilation, juste des paramètres lus au mauvais offset côté
/// GPU), même classe de bug que celle documentée plus bas sur
/// `PSSetConstantBuffers`. `size_of::<ViewportConstants>()` doit rester un
/// multiple de 16 (ici 48 + 160) — voir le test `viewport_constants_size_is_16_
/// byte_aligned` en fin de fichier, qui protège ce packing.
#[repr(C)]
struct ViewportConstants {
    size: [f32; 2],
    /// Voir `dither_amount` — amplitude du bruit de dithering appliqué au
    /// fond flouté (`PSMain`, `TexMode::Backdrop`), = `gfx::material::
    /// PostParams::noise` du matériau actif, jamais sous `gfx::material::
    /// MIN_NOISE`.
    dither_amount: f32,
    /// `gfx::material::PostParams::rim`.
    rim: f32,
    /// `gfx::material::PostParams::gloss`.
    gloss: f32,
    /// `gfx::material::PostParams::saturation`.
    saturation: f32,
    /// `gfx::material::PostParams::tint_scale` — voir sa doc : jamais
    /// persisté, appliqué UNIQUEMENT ici.
    tint_scale: f32,
    /// `gfx::material::PostParams::refraction`, en pixels.
    refraction: f32,
    /// `gfx::material::PostParams::hue_bias`.
    hue_bias: [f32; 3],
    /// **LE RELIEF EST-IL DEMANDÉ PAR LE STYLE ?** — 0.0 ou 1.0, lu de
    /// `ui::style::relief_degrade`/`relief_liserel`.
    ///
    /// ⚠️ **IL OCCUPE LE `_padding` D'ORIGINE, ET CE N'EST PAS UN HASARD.** Ce
    /// rembourrage existe pour que `hue_bias` (un `float3`) ne chevauche pas une
    /// frontière de 16 octets, et pour que `degrade` reste à l'offset 48 (voir le
    /// test `viewport_constants_size_is_16_byte_aligned`, qui vérifie les DEUX).
    /// Le réutiliser garde ces invariants **sans déplacer un seul octet**.
    ///
    /// ⚠️ **IL COMMANDE LA FORME, PAS LA FORCE** : la force est déjà fondue dans
    /// `rim`/`gloss` ci-dessus. Ici il ne dit qu'une chose — « dessine un BISEAU
    /// à deux tons » (clair sur le bord éclairé, sombre sur son opposé) au lieu
    /// du liseré d'origine, qui souligne le bord quel qu'il soit. Sans lui,
    /// ajouter un style changerait l'aspect des trois autres.
    relief_actif: f32,
    /// Dégradé animé (voir `gfx::degrade`) — commence à l'offset 48, sur une
    /// frontière de 16 octets.
    degrade: crate::gfx::degrade::DegradeGpu,
    /// Les lueurs (voir `gfx::lueur`) — un en-tête et deux tableaux, juste après.
    lueur: crate::gfx::lueur::LueurGpu,
    /// Les FONDUS (voir `ui::dock::bandes_de_fondu`), gauche puis droite :
    /// `(x, y, largeur, hauteur)` en pixels de la FENÊTRE — hauteur 0 : aucun.
    /// Posés pour UN rendu (`QuadRenderer::poser_fondu`).
    fondu: [[f32; 4]; 2],
    /// La texture de l'interface (voir `gfx::texture_interface::constantes`) :
    /// nature, force, échelle, clarté, puis la chroma et un remplissage — DEUX
    /// registres, les derniers.
    texture: [[f32; 4]; 2],
    /// La texture PERSONNALISÉE (voir `gfx::texture_perso::constantes`) : ses
    /// réglages à leur index, la phase de l'animation en dernier.
    texture_perso: [[f32; 4]; crate::gfx::texture_perso::REGISTRES],
    /// Les OMBRES des barres : `x` à 1 quand leur calque est lié au registre
    /// `t2`, à zéro sinon (`QuadRenderer::poser_ombres`). ⭐ `y` signale qu'il
    /// n'y a PAS de fond capturé — uniquement quand le mode « visible dans les
    /// captures » est actif ET que la loupe (`gfx::mag_capture`) est
    /// indisponible ; le shader sort alors l'alpha de l'utilisateur au lieu
    /// d'un aplat opaque. Le dernier registre.
    ombre: [f32; 4],
}

const SHADER_SOURCE: &str = r#"
cbuffer Viewport : register(b0) {
    float2 ViewportSize;
    float  DitherAmount;
    float  MatRim;
    float  MatGloss;
    float  MatSaturation;
    float  MatTintScale;
    float  MatRefraction;
    float3 MatHueBias;
    // Relief du STYLE : prends la lumière du bord de l'écran que ce quad touche
    // (voir `ViewportConstants::relief_actif`, qui porte le pourquoi).
    float  MatReliefActif;
    // Dégradé animé — voir `gfx::degrade::DegradeGpu`, même ordre.
    float2 DegOrigin;
    float  DegTime;
    float  DegActif;
    float4 DegEcran;
    float  DegEchelle;
    float  DegDouceur;
    float  DegWarp;
    float  DegVitesse;
    float  DegLuminosite;
    float  DegContraste;
    float  DegSaturation;
    float  DegGrain;
    float4 DegCouleurs[5];
    float  DegGraine;
    float DegFlou;
    // Relief (voir `Degrade`), puis la direction de la lumière en tours.
    float DegRelief;
    float DegLumiere;
    float2 DegOrigine;
    // Dans le remplissage du registre de `DegOrigine` — voir `DegradeGpu`.
    float DegCosinus;
    float DegPerspective;
    // Les remous (troisième registre) — voir `DegRemousTorsion`.
    float DegRemousForce;
    float DegRemousTaille;
    float DegRemousVitesse;
    float DegRemousDensite;
    // Les lueurs — voir `gfx::lueur::LueurGpu`, même ordre.
    float LueurNombre;
    float LueurAssombrissement;
    float2 LueurRemplissage;
    // ⚠️ 20 = `gfx::lueur::MAX_ELEMENTS`, 80 = × `MAX_ARRETS` (test
    // `les_tableaux_du_shader_ont_la_taille_des_lueurs`).
    float4 LueurDisques[20];
    float4 LueurTeintes[20];
    float4 LueurFormes[20];
    float4 LueurDegrades[20];
    float4 LueurArrets[80];
    // Les fondus en haut des colonnes, gauche puis droite — voir
    // `ViewportConstants::fondu`.
    float4 FonduBande;
    float4 FonduBandeDroite;
    // La texture de l'interface — voir `gfx::texture_interface::constantes`.
    float TexNature;
    float TexForce;
    float TexEchelle;
    float TexClarte;
    float3 TexChroma;
    float TexRemplissage;
    // La texture personnalisée — voir `gfx::texture_perso::REGLAGES`, même ordre.
    float4 TexPerso[9];
    // Les ombres des barres : `x` à 1 quand leur calque (`OmbresTex`) est lié,
    // `y` à 1 quand il n'y a PAS de fond capturé et que l'alpha de sortie doit
    // être la transparence de l'utilisateur (voir `ViewportConstants::ombre`).
    float4 Ombres;
};

Texture2D    IconTex     : register(t0);
SamplerState IconSampler : register(s0);
// Les images des lueurs (voir `gfx::lueur_images`) : une tranche par image, avec
// ses mips — lues avec le MÊME échantillonneur (linéaire, mips, bords bloqués).
Texture2DArray LueurImages : register(t1);
// Le calque des ombres des barres (voir `win32::ombre_barre::Calque`).
Texture2D OmbresTex : register(t2);

struct VSInput {
    float2 LocalPos  : LOCALPOS;
    float4 Rect      : RECT;
    float4 Color     : COLOR;
    float  Radius    : RADIUS;
    float  Scale     : SCALE;
    float  TexMode   : TEXMODE;
    float  Rotation  : ROTATION;
    float  Ring      : RING;
    float  Bascule   : BASCULE;
    float4 Uv        : UV; // sous-rectangle de la texture (u0, v0, u1, v1)
};

struct PSInput {
    float4 Position : SV_POSITION;
    float2 LocalUV  : TEXCOORD0; // position en pixels dans le repère de l'icône (mise à l'échelle incluse)
    float2 HalfSize : TEXCOORD1; // demi-taille en pixels (mise à l'échelle incluse)
    float  Radius   : TEXCOORD2;
    float4 Color    : COLOR0;
    float2 TexUV    : TEXCOORD3; // 0..1, indépendant de l'échelle de survol
    float  TexMode  : TEXCOORD4;
    float  Ring     : TEXCOORD5;
};

PSInput VSMain(VSInput input) {
    PSInput output;

    float2 halfSize = (input.Rect.zw * 0.5) * input.Scale;
    float2 center = input.Rect.xy + input.Rect.zw * 0.5;
    // PIVOT autour du CENTRE du rectangle (voir `QuadInstance::rotation`) : on
    // tourne le décalage local, pas le rectangle — c'est ce qui fait tourner
    // l'icône sur place au lieu de la faire orbiter.
    float2 offset = input.LocalPos * input.Rect.zw * input.Scale;
    float s = sin(input.Rotation);
    float c = cos(input.Rotation);
    float2 rotated = float2(offset.x * c - offset.y * s, offset.x * s + offset.y * c);
    float2 pixelPos = center + rotated;
    // BASCULE (voir `QuadInstance::bascule`) : la perspective du lanceur de
    // Unity, centrée sur l'icône. Le haut part en arrière, le bas avance.
    // `w` porte la profondeur : le rasteriseur divise par lui, et interpole
    // donc la texture et `LocalUV` EN PERSPECTIVE, sans pli à la diagonale.
    float w = 1.0;
    if (abs(input.Bascule) > 0.0001) {
        float sb = sin(input.Bascule);
        float cb = cos(input.Bascule);
        float camera = ViewportSize.y * 0.5;
        // Bascule NÉGATIVE (repliement du haut) : le bord du BAS part en arrière.
        float z = 3.0 * (-offset.y * sb + 10.0 * abs(input.Bascule));
        float f = camera / max(camera + z, 1.0);
        pixelPos = center + float2(offset.x, offset.y * cb) * f;
        w = 1.0 / f;
    }

    float2 ndc = float2(
        (pixelPos.x / ViewportSize.x) * 2.0 - 1.0,
        1.0 - (pixelPos.y / ViewportSize.y) * 2.0
    );

    output.Position = float4(ndc * w, 0.0, w);
    // `LocalUV` reste la position NON TOURNÉE : le masque de coins arrondis du
    // fragment shader (`HalfSize`, qui est la demi-taille du rectangle, elle
    // aussi non tournée) se compare à elle, et les deux doivent donc être dans
    // le MÊME repère — celui de l'icône. Les avoir mis dans des repères
    // différents (position tournée, demi-taille droite) rognait l'icône au bord
    // de son rectangle dès qu'elle pivotait — DÉFAUT RÉEL SIGNALÉ : « les icones
    // sont rognées quand elles tournent ». La rotation du quad, elle, vient de
    // `pixelPos` ci-dessus : le contenu suit.
    output.LocalUV = offset;
    output.HalfSize = halfSize;
    output.Radius = min(input.Radius, min(halfSize.x, halfSize.y));
    output.Color = input.Color;
    output.TexUV = input.Uv.xy + (input.LocalPos + 0.5) * input.Uv.zw;
    output.TexMode = input.TexMode;
    // Épaisseur d'anneau, bornée par la demi-taille COMME `Radius` : au-delà,
    // la bande ferait le tour du centre et la forme redeviendrait pleine.
    output.Ring = min(input.Ring, min(halfSize.x, halfSize.y));
    return output;
}

// Distance signée à un rectangle à coins arrondis (Inigo Quilez,
// https://iquilezles.org/articles/distfunctions2d/).
float RoundedBoxSDF(float2 p, float2 halfSize, float radius) {
    float2 q = abs(p) - halfSize + radius;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;
}

// Bruit pseudo-aléatoire par pixel (hash trigonométrique classique) — sert
// UNIQUEMENT à dispenser le "dithering" ci-dessous (TexMode::Backdrop), voir
// son commentaire. Pas de texture de bruit chargée depuis le disque
// (contrairement à `dash_noise.png` d'Unity/Compiz, voir l'historique) :
// un hash procédural coûte un `sin`/`frac` par pixel plutôt qu'un
// échantillonnage de texture, et n'a pas de taille de tuile à assortir à la
// résolution native (le flou lui-même est produit à une résolution plus
// petite que l'affichage final, voir `gfx::blur::STOP_LEVELS_FROM_NATIVE` —
// un bruit texturé y serait lui-même flouté/agrandi par le rééchantillonnage
// bilinéaire avant d'atteindre cette passe).
float DitherHash(float2 p) {
    return frac(sin(dot(p, float2(12.9898, 78.233))) * 43758.5453);
}

// Dégradé animé (voir `gfx::degrade`). Bruit de gradient 2D : un vecteur
// pseudo-aléatoire par coin de cellule, interpolation lissée — valeurs dans
// [-1, 1] environ.
float2 DegGradient(float2 cell) {
    // La graine (réglage « graine ») décale le réseau d'un nombre ENTIER de
    // cellules : un autre tirage du même bruit, sans couture.
    cell += floor(DegGraine) * float2(7.0, 13.0);
    float2 h = float2(dot(cell, float2(127.1, 311.7)), dot(cell, float2(269.5, 183.3)));
    return frac(sin(h) * 43758.5453) * 2.0 - 1.0;
}

float DegBruit(float2 p) {
    float2 i = floor(p);
    float2 f = frac(p);
    float2 u = f * f * (3.0 - 2.0 * f);
    float a = dot(DegGradient(i), f);
    float b = dot(DegGradient(i + float2(1, 0)), f - float2(1, 0));
    float c = dot(DegGradient(i + float2(0, 1)), f - float2(0, 1));
    float d = dot(DegGradient(i + float2(1, 1)), f - float2(1, 1));
    return lerp(lerp(a, b, u.x), lerp(c, d, u.x), u.y) * 1.4;
}

// Tirage par cellule des motifs, décalé par la graine comme `DegGradient`.
float DegHash(float2 p) {
    return DitherHash(p + floor(DegGraine) * float2(5.0, 11.0));
}

// Somme de quatre octaves de `DegBruit` — environ [-1, 1].
float DegFbm(float2 p) {
    float s = 0.0;
    float a = 0.5;
    [unroll] for (int i = 0; i < 4; i++) {
        s += a * DegBruit(p);
        p = p * 2.03 + float2(1.7, 9.2);
        a *= 0.5;
    }
    return s * 1.6;
}

// **LA PALETTE COSINUS — celle du shader de raymarching de l'utilisateur** :
// `a + b·cos(2π(c·x + d))`.
//
// ⚠️ **LA FORME EST À LUI, LES PARAMÈTRES SONT AU THÈME.** Ses `a`/`b`/`c` sont
// des constantes de SA scène ; les recopier ici aurait figé la palette et cassé
// ce qui fait qu'un préréglage change le motif. On garde donc la FORME — un
// cosinus, donc cyclique, plus riche qu'une rampe qui ne fait que traverser
// cinq points — en gardant la SOURCE : les couleurs que l'utilisateur a
// choisies.
//
// Sa phase `d` est, chez lui, une COULEUR (`vec3(...)` passée en phase) : on
// fait pareil, avec la couleur du MILIEU de la rampe, qui donne le meilleur
// départ.
float3 DegPaletteCosinus(float x) {
    float3 a = 0.5 * (DegCouleurs[0].rgb + DegCouleurs[4].rgb);
    float3 b = 0.5 * abs(DegCouleurs[4].rgb - DegCouleurs[0].rgb) + 0.18;
    float3 d = DegCouleurs[2].rgb;
    return saturate(a + b * cos(6.2831853 * (x + d)));
}

// Les cinq couleurs en rampe : 0 → première, 1 → dernière.
float3 DegPalette(float x) {
    float u = saturate(x);
    int i = min((int)floor(u * 4.0), 3);
    float3 rampe = lerp(DegCouleurs[i].rgb, DegCouleurs[i + 1].rgb, u * 4.0 - (float)i);
    // À 0 — le défaut — la rampe seule, et le cosinus n'est même pas évalué :
    // `DegPalette` est appelée par la plupart des motifs, donc dix-sept fois par
    // pixel avec le flou.
    if (DegCosinus <= 0.001) {
        return rampe;
    }
    return lerp(rampe, DegPaletteCosinus(u), DegCosinus);
}

// Cellules de Voronoï qui dérivent : x = distance au point le plus proche,
// y = au second, z = valeur pseudo-aléatoire (0..1) de la cellule la plus
// proche.
float3 DegCellules(float2 g, float t) {
    float2 i = floor(g);
    float2 f = frac(g);
    float d1 = 8.0;
    float d2 = 8.0;
    float id = 0.0;
    [unroll] for (int y = -1; y <= 1; y++) {
        [unroll] for (int x = -1; x <= 1; x++) {
            float2 n = float2(x, y);
            float2 h = DegGradient(i + n) * 0.5 + 0.5;
            float2 o = 0.5 + 0.4 * sin(t * 1.5 + 6.2831 * h);
            float d = length(n + o - f);
            if (d < d1) { d2 = d1; d1 = d; id = h.x; }
            else if (d < d2) { d2 = d; }
        }
    }
    return float3(d1, d2, id);
}

// « Mesh » — voir `gfx::degrade`, doc de module.
float4 DegMesh(float2 p, float aspect, float t) {
    // Déformation du domaine : les frontières entre couleurs ondulent.
    float2 q = p * DegEchelle;
    p += DegWarp * float2(DegBruit(q + float2(0.0, t * 1.3)),
                          DegBruit(q + float2(5.2, 1.3) - t * 1.1));

    // Cinq points en orbite lente, pondération inverse à la distance :
    // l'exposant (la « douceur ») règle la netteté des taches.
    float k = lerp(3.0, 0.8, DegDouceur);
    float3 acc = 0.0;
    float somme = 0.0;
    // Hauteur : la part du point DOMINANT — elle culmine sur les cinq points
    // et s'effondre entre eux, ce qui donne cinq dômes sous le relief.
    float wmax = 0.0;
    [unroll] for (int i = 0; i < 5; i++) {
        float fi = (float)i;
        float2 c = float2(
            aspect * (0.5 + 0.42 * sin(t * (1.0 + 0.23 * fi) + fi * 1.7)),
            0.5 + 0.42 * cos(t * (0.8 + 0.17 * fi) + fi * 2.3));
        float2 d = p - c;
        float w = 1.0 / pow(dot(d, d) + 1e-4, k);
        acc += w * DegCouleurs[i].rgb;
        somme += w;
        wmax = max(wmax, w);
    }
    return float4(acc / max(somme, 1e-6), wmax / max(somme, 1e-6));
}

// « Aurore » : la première couleur en fond, les quatre autres en rideaux qui
// ondulent — bord net en haut, fondu vers le bas (y croît vers le bas).
float4 DegAurore(float2 p, float aspect, float t) {
    float3 rgb = DegCouleurs[0].rgb;
    float chute = lerp(14.0, 3.0, DegDouceur);
    float relief = 0.0;
    [unroll] for (int i = 1; i < 5; i++) {
        float fi = (float)i;
        float x = p.x * DegEchelle * 0.6 + fi * 1.37;
        float bord = 0.12 + 0.13 * fi + (0.1 + DegWarp * 0.35) * DegBruit(float2(x, t * 0.6 + fi * 3.1));
        float d = p.y - bord;
        float rideau = exp(-max(d, 0.0) * chute) * smoothstep(-0.015 - 0.08 * DegDouceur, 0.0, d);
        float rayons = 0.55 + 0.45 * DegBruit(float2(x * 7.0, t * 0.25 + fi * 5.0));
        rgb += DegCouleurs[i].rgb * rideau * rayons * 0.9;
        relief += rideau * rayons;
    }
    return float4(rgb, relief);
}

// « Plasma » : somme de sinus qui se croisent, lue dans la rampe de couleurs.
float4 DegPlasma(float2 p, float aspect, float t) {
    float2 q = p * DegEchelle * 3.0;
    float2 centre = float2(aspect * 1.5 + 2.0 * sin(t * 0.9), 1.5 + 2.0 * cos(t * 0.7));
    float v = sin(q.x + t * 2.0) + sin(q.y * 0.8 - t * 1.7)
            + sin((q.x + q.y) * 0.7 + t * 1.3) + sin(length(q - centre) * 1.2 - t * 2.0);
    v += DegWarp * 2.0 * DegBruit(q * 0.5 + t * 0.3);
    float x = 0.5 + 0.125 * v;
    return float4(DegPalette(0.5 + (x - 0.5) * lerp(2.2, 0.9, DegDouceur)), x);
}

// « Bandes » : un bruit lent découpé en paliers, comme des lignes de niveau.
float4 DegBandes(float2 p, float aspect, float t) {
    float2 q = p * DegEchelle;
    q += DegWarp * float2(DegBruit(q + t * 0.4), DegBruit(q + float2(3.1, 7.4) - t * 0.4));
    float f = saturate(0.5 + 0.5 * DegFbm(q + float2(t * 0.15, -t * 0.1)));
    float n = lerp(14.0, 4.0, DegDouceur);
    float palier = floor(f * n) / max(n - 1.0, 1.0);
    // Liseré sombre à chaque frontière de palier.
    float bord = smoothstep(0.0, 0.08, frac(f * n)) * smoothstep(0.0, 0.08, 1.0 - frac(f * n));
    return float4(DegPalette(palier) * lerp(0.7, 1.0, bord), palier);
}

// « Soie » : bruit fractal déformé deux fois par lui-même — volutes de fumée
// ou plis de tissu.
float4 DegSoie(float2 p, float aspect, float t) {
    float2 q0 = p * DegEchelle;
    float2 q = float2(DegFbm(q0 + t * 0.2), DegFbm(q0 + float2(5.2, 1.3) - t * 0.2));
    float2 r = float2(DegFbm(q0 + 3.0 * q + float2(1.7, 9.2) + t * 0.3),
                      DegFbm(q0 + 3.0 * q + float2(8.3, 2.8) - t * 0.25));
    float f = DegFbm(q0 + (1.0 + 3.0 * DegWarp) * r);
    float x = saturate(0.5 + 0.5 * f);
    x = 0.5 + (x - 0.5) * lerp(1.8, 0.9, DegDouceur);
    return float4(lerp(DegPalette(x), DegPalette(saturate(length(q) * 0.7)), 0.3), f);
}

// « Voronoï » : des cellules d'une des cinq couleurs, liseré sombre entre
// elles.
float4 DegVoronoi(float2 p, float aspect, float t) {
    float2 g = p * DegEchelle * 2.0;
    g += DegWarp * 0.6 * float2(DegBruit(g * 0.5 + t * 0.3), DegBruit(g * 0.5 + float2(4.0, 1.0) - t * 0.3));
    float3 c = DegCellules(g, t);
    int idx = min((int)floor(c.z * 5.0), 4);
    float bord = smoothstep(0.0, lerp(0.03, 0.2, DegDouceur), c.y - c.x);
    return float4(DegCouleurs[idx].rgb * lerp(0.45, 1.0, bord), bord);
}

// « Caustiques » : le réseau lumineux d'un fond de piscine — les frontières
// de deux couches de cellules déformées, allumées par-dessus un fond d'eau.
float4 DegCaustiques(float2 p, float aspect, float t) {
    float2 g = p * DegEchelle * 3.0;
    g += (0.2 + DegWarp) * 0.5 * float2(DegBruit(g * 0.4 + t * 0.5), DegBruit(g * 0.4 + float2(2.0, 6.0) - t * 0.5));
    float3 a = DegCellules(g, t);
    float3 b = DegCellules(g * 1.3 + float2(3.7, 1.1), -t * 0.8);
    float k = lerp(9.0, 3.0, DegDouceur);
    float lumiere = pow(saturate(1.0 - (a.y - a.x) * k), 3.0) + 0.6 * pow(saturate(1.0 - (b.y - b.x) * k), 3.0);
    float3 fond = lerp(DegCouleurs[0].rgb, DegCouleurs[1].rgb, saturate(p.y));
    return float4(fond + lumiere * lerp(DegCouleurs[3].rgb, DegCouleurs[4].rgb, 0.5) * 1.2
                       + lumiere * lumiere * DegCouleurs[2].rgb * 0.4,
                  lumiere);
}

// « Synthwave » : ciel, soleil à bandes et sol quadrillé en perspective qui
// défile. L'horizon est à 55 % de la hauteur de l'écran virtuel.
float4 DegSynthwave(float2 p, float aspect, float t) {
    const float horizon = 0.55;
    float lissage = lerp(0.002, 0.02, DegDouceur);
    if (p.y < horizon) {
        float3 ciel = lerp(DegCouleurs[0].rgb, DegCouleurs[1].rgb, saturate(p.y / horizon));
        float2 s = float2(p.x - aspect * 0.5, p.y - (horizon - 0.2));
        float rayon = 0.18;
        float disque = smoothstep(rayon + lissage, rayon - lissage, length(s));
        // Bandes horizontales dans la moitié basse du soleil, qui descendent.
        float v = saturate(s.y / rayon);
        // Entailles de plus en plus larges vers le bas ; la moitié haute reste pleine.
        float bande = s.y <= 0.0 ? 1.0 : step(v * 0.55, frac(s.y * 25.0 * DegEchelle / 1.6 - t * 2.0));
        float3 soleil = lerp(DegCouleurs[4].rgb, DegCouleurs[3].rgb, saturate(0.5 + s.y / (2.0 * rayon)));
        // Hauteur : l'ÉCLAT DU SOLEIL — le disque, entailuré par ses bandes.
        return float4(lerp(ciel, soleil, disque * saturate(bande)), disque * saturate(bande));
    }
    float dy = max(p.y - horizon, 1e-3);
    float z = 0.15 / dy;
    float gx = (p.x - aspect * 0.5) * z * DegEchelle * 2.0;
    float gz = z * DegEchelle * 2.0 + t * 3.0;
    float2 cellule = abs(frac(float2(gx, gz)) - 0.5);
    float largeur = 0.03 + lissage * 4.0;
    float ligne = max(smoothstep(0.5 - largeur, 0.5, cellule.x), smoothstep(0.5 - largeur, 0.5, cellule.y));
    float brume = saturate(dy * 4.0);
    float3 sol = lerp(DegCouleurs[1].rgb, DegCouleurs[0].rgb, brume);
    // Hauteur : les LIGNES du sol — un quadrillage en creux plutôt qu'en relief,
    // ce que dit le signe.
    return float4(lerp(sol, DegCouleurs[2].rgb, ligne * brume), -ligne * brume);
}

// « Tramé » : le Mesh rendu en points de trame (grille à 45°, en PIXELS
// écran), le rayon d'un point suivant la luminosité.
float4 DegTrame(float2 screen, float2 p, float aspect, float t) {
    float3 base = DegMesh(p, aspect, t).rgb;
    float pas = 3.0 + 3.0 * DegEchelle;
    float2 r = float2(screen.x + screen.y, screen.y - screen.x) * 0.70710678 / pas;
    float2 f = frac(r) - 0.5;
    float luma = dot(base, float3(0.2126, 0.7152, 0.0722));
    float rayon = sqrt(saturate(1.0 - luma)) * 0.62;
    float lissage = lerp(0.04, 0.25, DegDouceur);
    float encre = smoothstep(rayon + lissage, rayon - lissage, length(f));
    float3 papier = lerp(base, 1.0, 0.25);
    return float4(lerp(papier, base * 0.3, encre), encre);
}

// Onde triangle de période 1 (0 → 1 → 0) : une rampe lue en aller-retour n'a
// pas de saut au bout.
float DegAllerRetour(float x) {
    return 1.0 - abs(2.0 * frac(x) - 1.0);
}

// Paliers de la rampe quand la douceur est nulle, rampe continue à 1.
float DegPaliers(float x) {
    return lerp(floor(saturate(x) * 4.999) / 4.0, x, DegDouceur);
}

// **UNION DOUCE — prise du shader de raymarching de l'utilisateur**, dont c'est
// le minimum polynomial. Deux champs se fondent au lieu de se couper, et
// l'arête qui les sépare s'ARRONDIT.
//
// ⚠️ **POURQUOI ELLE COMPTE ICI, ET PAS SEULEMENT POUR L'ESTHÉTIQUE** : une
// arête vive a un gradient infini. Le relief en prend la dérivée écran, qui y
// saute d'un coup — et une normale qui saute d'un pixel au suivant CRÉPITE.
// Fondre l'arête, c'est rendre la normale continue, donc le relief stable.
float DegFondu(float a, float b, float k) {
    float h = saturate(0.5 + 0.5 * (b - a) / k);
    return lerp(b, a, h) - k * h * (1.0 - h);
}

/// Le MAXIMUM doux : c'est `DegFondu` appliqué aux opposés.
float DegFonduMax(float a, float b, float k) {
    return -DegFondu(-a, -b, k);
}

// « Doux » : de grandes taches gaussiennes des couleurs 1 à 4 sur la
// première — le Mesh sans frontières.
float4 DegDoux(float2 p, float aspect, float t) {
    float3 rgb = DegCouleurs[0].rgb;
    float taille = lerp(0.18, 0.45, DegDouceur) * 1.6 / DegEchelle;
    float poids = 0.0;
    [unroll] for (int i = 1; i < 5; i++) {
        float fi = (float)i;
        float2 c = float2(aspect * (0.5 + 0.45 * sin(t * (0.7 + 0.13 * fi) + fi * 2.1)),
                          0.5 + 0.45 * cos(t * (0.6 + 0.19 * fi) + fi * 1.3));
        c += DegWarp * 0.2 * float2(DegBruit(c * 3.0 + t), DegBruit(c * 3.0 - t));
        float2 d = p - c;
        float g = exp(-dot(d, d) / (taille * taille));
        rgb = lerp(rgb, DegCouleurs[i].rgb, g);
        poids = max(poids, g);
    }
    return float4(rgb, poids);
}

// « Cannelé » : le Mesh vu à travers du verre cannelé — des lamelles
// verticales qui agrandissent chacune ce qu'elles montrent, reflet sur le
// bord droit.
float4 DegCannele(float2 p, float aspect, float t) {
    float n = 12.0 * DegEchelle;
    float u = p.x * n;
    float f = frac(u) - 0.5;
    float2 q = float2((floor(u) + 0.5 + f * (1.5 + 3.0 * DegWarp)) / n, p.y);
    float3 rgb = DegMesh(q, aspect, t).rgb;
    float relief = lerp(0.35, 0.12, DegDouceur);
    return float4(rgb * (1.0 - relief * f * f * 4.0) + relief * 0.5 * smoothstep(0.3, 0.5, f), 1.0 - f * f * 4.0);
}

// « Concentrique » : des anneaux autour d'un centre qui dérive.
float4 DegConcentrique(float2 p, float aspect, float t) {
    float2 c = float2(aspect * (0.5 + 0.2 * sin(t * 0.5)), 0.5 + 0.2 * cos(t * 0.4));
    float r = length(p - c) + DegWarp * 0.15 * DegFbm(p * DegEchelle + t * 0.2);
    float v = DegAllerRetour(r * DegEchelle * 1.2 - t * 0.5);
    return float4(DegPalette(DegPaliers(v)), v);
}

// « Fumée » : bruit « en crêtes » (1 − |bruit|) — des filets nets.
float4 DegFumee(float2 p, float aspect, float t) {
    float2 q = p * DegEchelle;
    q += DegWarp * float2(DegFbm(q + t * 0.15), DegFbm(q + float2(3.3, 8.1) - t * 0.15));
    float r = 0.0;
    float a = 0.5;
    float2 s = q + float2(0.0, t * 0.3);
    [unroll] for (int i = 0; i < 4; i++) {
        r += a * (1.0 - abs(DegBruit(s)));
        s = s * 2.1 + float2(4.1, 2.7);
        a *= 0.5;
    }
    float x = saturate(pow(saturate(r / 0.9375), lerp(4.0, 1.5, DegDouceur)));
    return float4(lerp(DegCouleurs[0].rgb, DegPalette(x), saturate(x * 1.5)), r);
}

// « Rayons » : des rais de lumière tombant d'une source au-dessus de l'écran.
float4 DegRayons(float2 p, float aspect, float t) {
    float2 d = p - float2(aspect * (0.5 + 0.25 * sin(t * 0.3)), -0.3);
    float angle = atan2(d.x, d.y);
    float n = 10.0 * DegEchelle;
    float rais = 0.5 + 0.5 * DegBruit(float2(angle * n, t * 0.5)) + 0.25 * DegBruit(float2(angle * n * 2.7, -t * 0.7));
    rais = pow(saturate(rais), lerp(4.0, 1.2, DegDouceur));
    float chute = exp(-length(d) * (1.2 - DegWarp * 0.8));
    float3 fond = lerp(DegCouleurs[0].rgb, DegCouleurs[1].rgb, saturate(p.y));
    float3 lumiere = lerp(DegCouleurs[3].rgb, DegCouleurs[4].rgb, 0.5 + 0.5 * sin(angle * 3.0 + t));
    return float4(fond + rais * chute * (lumiere + DegCouleurs[2].rgb * 0.3) * 1.3, rais * chute);
}

// « Prisme » : la rampe lue avec un décalage par canal — les franges irisées
// d'une dispersion.
float4 DegPrisme(float2 p, float aspect, float t) {
    float2 q = p * DegEchelle;
    float v = 0.5 * DegFbm(q * 0.7 + float2(t * 0.2, -t * 0.15)) + 0.25 * sin((p.x + p.y) * 6.0 * DegEchelle + t);
    float ecart = 0.04 + 0.2 * DegWarp;
    float3 irise = float3(DegPalette(DegAllerRetour(v - ecart)).r,
                          DegPalette(DegAllerRetour(v)).g,
                          DegPalette(DegAllerRetour(v + ecart)).b);
    float3 rgb = lerp(DegPalette(DegAllerRetour(v)), irise, lerp(1.0, 0.6, DegDouceur));
    return float4(rgb + pow(DegAllerRetour(v * 3.0), 8.0) * 0.15, v);
}

// « Vortex » : le Mesh enroulé autour du centre, d'autant plus près de lui.
float4 DegVortex(float2 p, float aspect, float t) {
    float2 c = float2(aspect * 0.5, 0.5);
    float2 d = p - c;
    float a = (1.0 + 3.0 * DegWarp) * 0.6 / (length(d) + 0.15) + t;
    float sa = sin(a);
    float ca = cos(a);
    // `DegMesh` rend DÉJÀ un float4 (couleur + hauteur) : on le transmet tel
    // quel. L'enrober donnait `float4(float4, float)`, que le compilateur refuse.
    return DegMesh(c + float2(d.x * ca - d.y * sa, d.x * sa + d.y * ca), aspect, t * 0.5);
}

// « Marbre » : des veines claires (sinus déformé par le bruit) sur un fond
// nuageux des premières couleurs.
float4 DegMarbre(float2 p, float aspect, float t) {
    float2 q = p * DegEchelle;
    float f = DegFbm(q + float2(t * 0.1, 0.0));
    float v = sin((q.x + q.y) * 3.0 + (2.0 + 6.0 * DegWarp) * f + t * 0.3);
    float veine = pow(1.0 - abs(v), lerp(12.0, 3.0, DegDouceur));
    float3 fond = DegPalette(saturate(0.5 + 0.5 * DegFbm(q * 0.5 - t * 0.05)) * 0.6);
    return float4(lerp(fond, DegCouleurs[4].rgb * 1.4, veine), veine);
}

// « Vagues » : quatre couches de collines ondulées empilées sur la première
// couleur, la plus lointaine en haut.
float4 DegVagues(float2 p, float aspect, float t) {
    float3 rgb = DegCouleurs[0].rgb;
    float lissage = lerp(0.002, 0.04, DegDouceur);
    float relief = 0.0;
    [unroll] for (int i = 1; i < 5; i++) {
        float fi = (float)i;
        float x = p.x * DegEchelle * (0.8 + 0.3 * fi) + fi * 3.7;
        float crete = 0.15 + 0.19 * fi + (0.04 + 0.1 * DegWarp)
                    * (sin(x * 2.0 + t * (0.6 + 0.2 * fi)) + 0.6 * DegBruit(float2(x, t * 0.3 + fi)));
        float dedans = smoothstep(-lissage, lissage, p.y - crete);
        // Ombre sous la crête : un léger relief.
        float3 c = DegCouleurs[i].rgb * (1.0 - 0.25 * exp(-max(p.y - crete, 0.0) * 12.0));
        rgb = lerp(rgb, c, dedans);
        relief = lerp(relief, fi * 0.25, dedans);
    }
    return float4(rgb, relief);
}

// « Ruban » : quatre rubans sinueux et lumineux sur la première couleur.
float4 DegRuban(float2 p, float aspect, float t) {
    float3 rgb = DegCouleurs[0].rgb;
    float largeur = lerp(0.01, 0.06, DegDouceur);
    float relief = 0.0;
    float x = p.x * DegEchelle;
    [unroll] for (int i = 1; i < 5; i++) {
        float fi = (float)i;
        float y = 0.5 + 0.28 * sin(x * (0.9 + 0.2 * fi) + t * (0.5 + 0.15 * fi) + fi * 1.9)
                + (0.05 + 0.2 * DegWarp) * DegBruit(float2(x * 0.8 + fi * 5.0, t * 0.4));
        rgb += DegCouleurs[i].rgb * (largeur / (abs(p.y - y) + largeur * 0.5)) * 0.35;
        relief += (largeur / (abs(p.y - y) + largeur * 0.5)) * 0.35;
    }
    return float4(rgb, relief);
}

// « Tunnel » : des anneaux qui avancent vers le spectateur, en coordonnées
// polaires (profondeur = 1 / rayon), tordus par l'ondulation.
float4 DegTunnel(float2 p, float aspect, float t) {
    float2 d = p - float2(aspect * 0.5 + 0.1 * sin(t * 0.4), 0.5 + 0.08 * cos(t * 0.3));
    float r = max(length(d), 1e-3);
    float a = atan2(d.y, d.x) / 6.2831853;
    float z = 0.3 / r + t * 1.5;
    // Nombre ENTIER de secteurs : pas de couture à ±π.
    float n = max(round(3.0 * DegEchelle), 1.0);
    float v = 0.7 * DegAllerRetour(z * 0.5) + 0.3 * DegAllerRetour(a * n + DegWarp * 2.0 * sin(z * 0.7));
    return float4(DegPalette(DegPaliers(v)) * saturate(r * 5.0), v);
}

// « Kaléidoscope » : du bruit replié en miroir dans huit secteurs.
float4 DegKaleidoscope(float2 p, float aspect, float t) {
    float2 d = p - float2(aspect * 0.5, 0.5);
    float r = length(d);
    const float secteur = 6.2831853 / 8.0;
    float a = atan2(d.y, d.x) + t * 0.2 + 100.0 * secteur;
    a = abs(fmod(a, secteur) - secteur * 0.5);
    float2 q = r * float2(cos(a), sin(a)) * DegEchelle * 2.0;
    q += DegWarp * float2(DegBruit(q + t * 0.3), DegBruit(q - t * 0.3));
    float v = DegAllerRetour(0.5 + 0.5 * DegFbm(q + float2(t * 0.3, 0.0)) + r * 0.5 - t * 0.1);
    return float4(DegPalette(0.5 + (v - 0.5) * lerp(1.6, 1.0, DegDouceur)), v);
}

// « Lave » : une lampe à lave — six boules qui montent, descendent et
// fusionnent (champ additif seuillé).
float4 DegLave(float2 p, float aspect, float t) {
    p += DegWarp * 0.05 * float2(DegBruit(p * 6.0 + t), DegBruit(p * 6.0 - t));
    float champ = 0.0;
    float3 teinte = 0.0;
    [unroll] for (int i = 0; i < 6; i++) {
        float fi = (float)i;
        float2 c = float2(aspect * (0.15 + 0.7 * frac(fi * 0.618 + 0.1)) + 0.08 * sin(t * 0.7 + fi),
                          0.5 + 0.45 * sin(t * (0.3 + 0.07 * fi) + fi * 2.4));
        float rayon = (0.08 + 0.03 * sin(fi * 3.1)) * 1.6 / DegEchelle;
        float2 d = p - c;
        float w = rayon * rayon / (dot(d, d) + 1e-4);
        champ += w;
        teinte += w * DegCouleurs[1 + i % 4].rgb;
    }
    teinte /= max(champ, 1e-4);
    float lissage = lerp(0.03, 0.5, DegDouceur);
    float dedans = smoothstep(1.0 - lissage, 1.0 + lissage, champ);
    float3 fond = DegCouleurs[0].rgb + teinte * 0.15 * saturate(champ * 0.5);
    return float4(lerp(fond, teinte, dedans), dedans);
}

// « Nébuleuse » : nuages de bruit déformé et étoiles fixes qui scintillent.
float4 DegNebuleuse(float2 p, float aspect, float t) {
    float2 q = p * DegEchelle;
    float2 w = float2(DegFbm(q + t * 0.05), DegFbm(q + float2(7.1, 3.3) - t * 0.05));
    float nuage = saturate(0.5 + 0.5 * DegFbm(q + (0.5 + 2.0 * DegWarp) * w));
    nuage = pow(nuage, lerp(2.5, 1.2, DegDouceur));
    float3 rgb = lerp(DegCouleurs[0].rgb * 0.6, DegPalette(saturate(length(w) * 0.8)), nuage);
    // Une étoile possible par cellule (60 par hauteur d'écran virtuel).
    float2 g = p * 60.0;
    float h = DegHash(floor(g));
    float2 place = 0.2 + 0.6 * frac(float2(h * 13.7, h * 71.3));
    float etoile = step(0.92, h) * smoothstep(0.08, 0.0, length(frac(g) - place))
                 * (0.6 + 0.4 * sin(t * 3.0 + h * 40.0));
    return float4(rgb + etoile * 0.8, nuage);
}

// « Conique » : la rampe tout autour d'un centre qui dérive, et qui tourne.
float4 DegConique(float2 p, float aspect, float t) {
    float2 d = p - float2(aspect * (0.5 + 0.15 * sin(t * 0.3)), 0.5 + 0.15 * cos(t * 0.4));
    float a = atan2(d.y, d.x) / 6.2831853 + t * 0.1 + DegWarp * 0.15 * DegBruit(p * DegEchelle * 2.0 + t * 0.2);
    // Nombre ENTIER de tours de rampe : pas de couture à ±π.
    float n = max(round(DegEchelle / 1.6), 1.0);
    float v = DegAllerRetour(a * n);
    return float4(DegPalette(DegPaliers(v)), v);
}

// « Topographie » : les lignes de niveau d'un relief de bruit, une sur cinq
// plus marquée.
float4 DegTopographie(float2 p, float aspect, float t) {
    float2 q = p * DegEchelle;
    q += DegWarp * 0.5 * float2(DegBruit(q + t * 0.2), DegBruit(q + float2(5.0, 2.0) - t * 0.2));
    float f = saturate(0.5 + 0.5 * DegFbm(q * 0.8 + float2(t * 0.05, t * 0.03)));
    float g = f * 18.0;
    float d = min(frac(g), 1.0 - frac(g));
    float trait = 1.0 - smoothstep(0.0, lerp(0.05, 0.15, DegDouceur), d);
    float majeure = step(fmod(round(g), 5.0), 0.5);
    return float4(lerp(DegPalette(f) * 0.6, saturate(DegPalette(1.0 - f) * 1.8), trait * lerp(0.55, 1.0, majeure)), f);
}

// « Pixels » : le Mesh en mosaïque de blocs (en PIXELS écran), joints sombres.
float4 DegPixels(float2 screen, float2 p, float aspect, float t) {
    float taille = 6.0 + 10.0 * DegEchelle;
    float2 bloc = (floor(screen / taille) + 0.5) * taille;
    float3 rgb = DegMesh((bloc - DegEcran.xy) / DegEcran.w, aspect, t).rgb;
    float2 f = abs(frac(screen / taille) - 0.5);
    float joint = smoothstep(0.5 - lerp(0.12, 0.01, DegDouceur), 0.5, max(f.x, f.y));
    return float4(rgb * (1.0 - 0.35 * joint), 1.0 - joint);
}

// « Moiré » : deux jeux d'anneaux qui se croisent, centres en dérive.
float4 DegMoire(float2 p, float aspect, float t) {
    float2 a = float2(aspect * (0.5 + 0.3 * sin(t * 0.3)), 0.5 + 0.2 * cos(t * 0.25));
    float2 b = float2(aspect * (0.5 + 0.3 * cos(t * 0.27)), 0.5 + 0.2 * sin(t * 0.33));
    float k = 40.0 * DegEchelle;
    float v = sin(length(p - a) * k) * sin(length(p - b) * k) + DegWarp * DegBruit(p * DegEchelle * 2.0 + t * 0.3);
    return float4(DegPalette(saturate(0.5 + 0.5 * v * lerp(1.5, 0.8, DegDouceur))), v);
}

// « Lueurs » : des disques flous (bokeh) qui dérivent sur une rampe de fond,
// deux couches de tailles différentes.
float4 DegLueurs(float2 p, float aspect, float t) {
    float3 rgb = lerp(DegCouleurs[0].rgb, DegCouleurs[1].rgb, saturate(p.y));
    float2 g = p * DegEchelle * 2.0 + float2(t * 0.2, -t * 0.3);
    g += DegWarp * float2(DegBruit(g * 0.3 + t * 0.2), DegBruit(g * 0.3 - t * 0.2));
    float bord = lerp(0.02, 0.2, DegDouceur);
    float relief = 0.0;
    [unroll] for (int couche = 0; couche < 2; couche++) {
        float2 gc = g * (1.0 + couche * 0.7) + couche * 5.3;
        float h = DegHash(floor(gc) + couche * 31.0);
        float2 centre = 0.3 + 0.4 * float2(h, frac(h * 17.3));
        float r = 0.1 + 0.18 * frac(h * 7.1);
        float disque = smoothstep(r, r - bord, length(frac(gc) - centre)) * step(0.35, frac(h * 3.7));
        rgb += disque * DegCouleurs[2 + (int)(frac(h * 5.3) * 2.999)].rgb * 0.5;
        relief += disque;
    }
    return float4(rgb, relief);
}

// « Damier » : un damier liquide, ondulé, sur une rampe lente.
float4 DegDamier(float2 p, float aspect, float t) {
    float2 q = p * DegEchelle * 3.0;
    q += (0.3 + DegWarp) * float2(sin(q.y * 0.9 + t), sin(q.x * 0.8 - t * 1.1));
    float2 s = sin(q * 3.14159265);
    float w = lerp(0.02, 0.5, DegDouceur);
    float c = smoothstep(-w, w, s.x * s.y);
    float3 fond = DegPalette(saturate(0.5 + 0.5 * DegBruit(q * 0.2 + t * 0.1))) * 0.8;
    return float4(lerp(fond, DegCouleurs[4].rgb * 1.3, c), c);
}

// `screen` en pixels ÉCRAN. Le repère est l'écran virtuel, en unités de sa
// HAUTEUR (pas d'étirement quand il est large). Aiguille sur `DEG_MODE` (voir
// `gfx::degrade::MODES`, même ordre), une CONSTANTE de compilation posée par
// `pixel_source` : le pixel shader est compilé pour le seul motif choisi.
// Aiguiller à l'exécution (sur un index lu dans le cbuffer) inlinait tous les motifs et faisait
// passer la compilation de 0,4 à 60 s — elle croît plus vite que leur nombre.
// « Ondes » : des ronds dans l'eau — trois sources en dérive dont les ondes
// se croisent, éclairées par leur pente.
float4 DegOndes(float2 p, float aspect, float t) {
    float k = 30.0 * DegEchelle / 1.6;
    float h = 0.0;
    float2 pente = 0.0;
    [unroll] for (int i = 0; i < 3; i++) {
        float fi = (float)i;
        float2 d = p - float2(aspect * (0.5 + 0.35 * sin(t * 0.2 + fi * 2.1)), 0.5 + 0.35 * cos(t * 0.17 + fi * 1.7));
        float r = length(d) + 1e-4;
        float amorti = exp(-r * lerp(4.0, 1.0, DegWarp));
        float phase = r * k - t * 4.0;
        h += sin(phase) * amorti;
        pente += cos(phase) * amorti * d / r;
    }
    float3 n = normalize(float3(-pente * lerp(0.6, 0.2, DegDouceur), 1.0));
    float lumiere = dot(n, normalize(float3(-0.4, -0.6, 0.7)));
    return float4(DegPalette(saturate(0.5 + 0.25 * h)) * (0.6 + 0.6 * lumiere), h);
}

// « Fibres » : des brins fins qui suivent un courant — bruit étiré le long
// d'une direction qui tourne avec le bruit.
float4 DegFibres(float2 p, float aspect, float t) {
    float2 q = p * DegEchelle;
    float angle = DegBruit(q * 0.4 + t * 0.1) * (1.0 + 2.0 * DegWarp);
    float2 dir = float2(cos(angle), sin(angle));
    float2 s = float2(dot(q, dir) * 0.8, dot(q, float2(-dir.y, dir.x)) * 40.0);
    float brins = pow(saturate(0.5 + 0.5 * DegBruit(s + float2(t * 0.5, 0.0))), lerp(3.0, 1.0, DegDouceur));
    return float4(DegPalette(saturate(0.5 + 0.5 * DegFbm(q * 0.5 + t * 0.05))) * (0.55 + 0.7 * brins), brins);
}

// « Irisé » : une nappe d'huile — l'épaisseur (bruit déformé) fait tourner
// la teinte comme les interférences d'un film mince.
float4 DegIrise(float2 p, float aspect, float t) {
    float2 q = p * DegEchelle;
    float2 w = float2(DegFbm(q + t * 0.1), DegFbm(q + float2(4.3, 1.7) - t * 0.1));
    float epaisseur = DegFbm(q * 0.7 + (0.5 + 2.0 * DegWarp) * w) * 1.5 + t * 0.05;
    float3 film = 0.5 + 0.5 * cos(6.2831853 * (epaisseur * 2.0 + float3(0.0, 0.33, 0.67)));
    float3 base = DegPalette(DegAllerRetour(epaisseur));
    return float4(lerp(base, base * 0.5 + film * 0.5, lerp(0.8, 0.45, DegDouceur)), epaisseur);
}

// « Holographique » : une feuille métallisée froissée — bandes d'arc-en-ciel
// qui suivent les plis, reflets vifs.
float4 DegHolo(float2 p, float aspect, float t) {
    float2 q = p * DegEchelle;
    float pli = DegFbm(q * 1.5 + float2(t * 0.1, 0.0)) * (0.5 + 1.5 * DegWarp);
    float v = (p.x * 0.7 + p.y * 0.4) * 2.0 + pli + t * 0.2;
    float3 arc = 0.5 + 0.5 * cos(6.2831853 * (v + float3(0.0, 0.33, 0.67)));
    float reflet = pow(DegAllerRetour(v * 1.5 + pli), lerp(16.0, 4.0, DegDouceur));
    return float4(lerp(DegPalette(DegAllerRetour(v * 0.5)), arc * 0.7, 0.45) + reflet * 0.35, pli);
}

// « Braises » : des étincelles qui montent en vacillant sur un fond qui
// rougeoie par le bas.
float4 DegBraises(float2 p, float aspect, float t) {
    float3 rgb = lerp(DegCouleurs[0].rgb, DegCouleurs[1].rgb, saturate(p.y * p.y));
    float taille = lerp(0.04, 0.12, DegDouceur);
    float relief = 0.0;
    [unroll] for (int couche = 0; couche < 3; couche++) {
        float fc = (float)couche;
        // `+ t` en y : une étincelle fixe dans la grille MONTE à l'écran.
        float2 g = p * DegEchelle * (6.0 + 4.0 * fc) + float2(0.0, t * (2.0 + fc));
        g.x += (0.3 + DegWarp) * sin(g.y * 0.7 + fc * 2.0 + t);
        float h = DegHash(floor(g) + fc * 17.0);
        float2 place = 0.25 + 0.5 * float2(h, frac(h * 23.1));
        float braise = step(0.75, h) * smoothstep(taille * 2.0, 0.0, length(frac(g) - place))
                     * (0.5 + 0.5 * sin(t * 8.0 + h * 50.0)) * saturate(p.y * 1.2);
        rgb += braise * lerp(DegCouleurs[3].rgb, DegCouleurs[4].rgb, frac(h * 7.3)) * 1.5;
        relief += braise;
    }
    return float4(rgb, relief);
}

// « Pluie » : des gouttes sur une vitre devant le Mesh — chacune le réfracte
// (à l'envers, comme une lentille), naît, vit et s'efface.
float4 DegPluie(float2 p, float aspect, float t) {
    float grille = DegEchelle * 5.0;
    float2 g = p * grille;
    float h = DegHash(floor(g));
    float2 place = 0.3 + 0.4 * float2(h, frac(h * 31.7));
    float vie = frac(t * 0.1 * (0.5 + h) + h);
    float r = (0.1 + 0.1 * frac(h * 5.1)) * smoothstep(0.0, 0.1, vie) * smoothstep(1.0, 0.7, vie);
    float2 d = frac(g) - place;
    float dedans = smoothstep(r, r - lerp(0.01, 0.06, DegDouceur), length(d)) * step(0.6 - 0.5 * DegWarp, h);
    float3 fond = DegMesh(p - d * 2.5 / grille * dedans, aspect, t).rgb;
    float reflet = dedans * smoothstep(r * 0.5, 0.0, length(d + 0.35 * r));
    return float4(fond * lerp(0.85, 1.05, dedans) + reflet * 0.3, dedans);
}

// « Aura » : un halo doux qui respire au centre, ses couleurs en couronnes.
float4 DegAura(float2 p, float aspect, float t) {
    float2 d = p - float2(aspect * (0.5 + 0.1 * sin(t * 0.3)), 0.5 + 0.08 * cos(t * 0.25));
    float r = length(d) * DegEchelle / 1.6;
    // Déformation lue sur la direction (cos, sin) : pas de couture à ±π.
    float2 u = d / max(length(d), 1e-4);
    r *= 1.0 + DegWarp * 0.35 * DegBruit(u * 1.5 + t * 0.4);
    float x = saturate(r / (0.6 * (1.0 + 0.08 * sin(t * 1.5))));
    return float4(DegPalette(1.0 - pow(x, lerp(1.5, 0.7, DegDouceur))), 1.0 - x);
}

// « Éclosion » : une fleur de lumière — quatre corolles de pétales doux, qui
// tournent en sens alternés.
float4 DegEclosion(float2 p, float aspect, float t) {
    float2 d = p - float2(aspect * 0.5, 0.5);
    float r = length(d) * DegEchelle / 1.6;
    float a = atan2(d.y, d.x);
    float lissage = lerp(0.01, 0.12, DegDouceur);
    float3 rgb = DegCouleurs[0].rgb;
    float relief = 0.0;
    [unroll] for (int i = 1; i < 5; i++) {
        float fi = (float)i;
        float sens = (i % 2 == 0) ? 1.0 : -1.0;
        // Nombre ENTIER de pétales : pas de couture à ±π.
        float petale = 0.5 + 0.5 * cos((3.0 + fi) * (a + sens * t * 0.1) + fi);
        float bord = (0.55 - 0.1 * fi) * (0.6 + 0.4 * petale) * (1.0 + 0.1 * sin(t + fi))
                   + DegWarp * 0.05 * DegBruit(d * 8.0 + t);
        float dedans = smoothstep(bord + lissage, bord - lissage, r);
        rgb = lerp(rgb, DegCouleurs[i].rgb, dedans);
        relief = max(relief, dedans * fi * 0.25);
    }
    return float4(rgb, relief);
}

// « Drapé » : un tissu suspendu — plis verticaux qui ondulent, ombrés par
// leur pente.
float4 DegDrape(float2 p, float aspect, float t) {
    float2 q = p * DegEchelle;
    float ph = q.x * 6.0 + (1.0 + 3.0 * DegWarp) * DegBruit(float2(q.x * 0.5, q.y * 0.6 + t * 0.2))
             + 0.5 * sin(q.x * 2.3 + t * 0.5);
    float lumiere = pow(0.5 + 0.5 * cos(ph), lerp(3.0, 1.0, DegDouceur));
    float3 base = DegPalette(saturate(0.5 + 0.35 * sin(q.x * 1.3 + t * 0.2) + 0.15 * sin(ph)));
    return float4(base * (0.45 + 0.75 * lumiere), lumiere);
}

// « Duo » : deux teintes de part et d'autre d'une frontière qui ondule en
// diagonale, chacune en léger dégradé.
float4 DegDuo(float2 p, float aspect, float t) {
    float2 q = p * DegEchelle / 1.6;
    float frontiere = (p.x / aspect - p.y) + (0.08 + 0.25 * DegWarp) * sin(q.x * 3.0 + q.y * 2.0 + t)
                    + 0.1 * DegBruit(q * 2.0 + t * 0.3);
    float lissage = lerp(0.005, 0.25, DegDouceur);
    float3 a = lerp(DegCouleurs[0].rgb, DegCouleurs[1].rgb, saturate(p.y));
    float3 b = lerp(DegCouleurs[3].rgb, DegCouleurs[2].rgb, saturate(p.y));
    return float4(lerp(a, b, smoothstep(-lissage, lissage, frontiere)), frontiere);
}

// « Satin » : de larges reflets brillants qui glissent sur une rampe douce.
float4 DegSatin(float2 p, float aspect, float t) {
    float2 q = p * DegEchelle;
    float3 base = DegPalette(saturate(p.y * 0.8 + 0.1 + 0.15 * DegBruit(q * 0.5 + t * 0.1)));
    float v = dot(q, float2(0.8, 0.6)) * 1.5 + (0.5 + 2.0 * DegWarp) * DegFbm(q * 0.4 + t * 0.08) - t * 0.4;
    float brillance = pow(DegAllerRetour(v * 0.5), lerp(24.0, 4.0, DegDouceur));
    return float4(base + brillance * lerp(DegCouleurs[4].rgb, 1.0, 0.5) * 0.5, brillance);
}

// ---- VINGT MOTIFS DE PLUS — DEMANDE EXPLICITE (2026-09-27) -----------------
//
// Même contrat que les précédents : `p` en unités de HAUTEUR d'écran, `aspect`
// le rapport largeur/hauteur, `t` le temps (graine et vitesse comprises, voir
// `DegMotif`). Deux règles tenues partout, parce qu'un motif qui les viole ne
// se voit qu'à l'usage :
//
// - **Lisible à l'échelle 1.** Le dégradé passe sous un flou (voir `Degrade`) :
//   un motif plus fin que quelques pixels de haut devient du bruit gris.
// - **Les cinq couleurs servent.** Un motif qui n'en utilise qu'une ne réagit
//   ni au préréglage ni à la graine, et l'utilisateur croit le réglage cassé.

// « Toile » : un tissu — des bandes croisées qui passent dessus/dessous.
float4 DegToile(float2 p, float aspect, float t) {
    float n = 14.0 * DegEchelle;
    float2 g = p * n;
    float2 c = floor(g);
    float2 f = frac(g) - 0.5;
    // Dessus/dessous alterne en damier, décalé par la graine.
    float dessus = step(0.5, frac((c.x + c.y) * 0.5 + DegGraine * 0.01));
    float fil = lerp(f.x, f.y, dessus);
    float3 rgb = DegPalette(frac((dessus > 0.5 ? c.x : c.y) / n * 0.8 + DegGraine * 0.02));
    float relief = 1.0 - abs(fil) * 2.0;
    return float4(rgb * lerp(0.5, 1.3, relief * relief), relief);
}

// « Écailles » : des arcs qui se recouvrent en rangées décalées.
float4 DegEcailles(float2 p, float aspect, float t) {
    float n = 9.0 * DegEchelle;
    float2 g = float2(p.x, p.y * 2.0) * n;
    g.x += floor(g.y) * 0.5; // une demi-écaille de décalage par rangée
    float2 c = floor(g);
    float2 f = frac(g);
    float d = length(f - float2(0.5, 1.0));
    float bord = smoothstep(0.40, 0.50, d);
    float3 rgb = DegPalette(frac(c.x * 0.13 + c.y * 0.07 + t * 0.02 + DegGraine * 0.05));
    return float4(lerp(rgb, rgb * 0.45, bord), 1.0 - bord);
}

// « Zèbre » : des rayures dont le contour ondule — le bruit déforme la
// FRONTIÈRE, pas l'intérieur.
float4 DegZebre(float2 p, float aspect, float t) {
    float f = p.x * 6.0 * DegEchelle
            + (0.6 + 2.0 * DegWarp) * DegBruit(p * 3.0 * DegEchelle + t * 0.2);
    float bande = DegAllerRetour(f);
    float net = lerp(0.5, 0.05, DegDouceur);
    return float4(lerp(DegCouleurs[0].rgb, DegCouleurs[3].rgb, smoothstep(0.5 - net, 0.5 + net, bande)), bande);
}

// « Cristal » : des facettes anguleuses, arêtes vives — le contraire de
// « Voronoï », qui adoucit les mêmes cellules.
float4 DegCristal(float2 p, float aspect, float t) {
    float3 c = DegCellules(p * 5.0 * DegEchelle, t * 0.3);
    float net = lerp(0.9, 0.2, DegDouceur);
    float arete = smoothstep(net - 0.08, net, c.y - c.x);
    return float4(lerp(DegPalette(c.z), DegPalette(c.z) * 2.2, arete), arete);
}

// « Brume » : un brouillard très doux, sans structure — le repos entre deux
// motifs chargés.
float4 DegBrume(float2 p, float aspect, float t) {
    float n = DegFbm(p * 1.1 * DegEchelle + t * 0.05);
    float3 rgb = DegPalette(saturate(0.5 + 0.35 * n + p.y * 0.15));
    return float4(lerp(rgb, DegCouleurs[4].rgb, 0.35 * saturate(n)), n);
}

// « Sillons » : des anneaux CARRÉS — même famille que « Concentrique », dont
// ils sont la version anguleuse.
float4 DegSillons(float2 p, float aspect, float t) {
    float2 q = abs(p + DegWarp * 0.15 * float2(DegBruit(p * 3.0 + t), DegBruit(p * 3.0 - t)));
    // Arête ARRIE depuis le shader de l'utilisateur (voir `DegFondu`).
    float d = DegFonduMax(q.x, q.y, 0.10);
    float v = DegAllerRetour(d * 7.0 * DegEchelle - t * 0.15);
    return float4(lerp(DegCouleurs[1].rgb, DegCouleurs[4].rgb, pow(v, lerp(3.0, 0.6, DegDouceur))), v);
}

// « Chevrón » : des zigzags empilés.
float4 DegChevron(float2 p, float aspect, float t) {
    float x = p.x * 5.0 * DegEchelle;
    float zig = abs(frac(x) * 2.0 - 1.0);
    float v = DegAllerRetour((p.y + zig * 0.5) * 4.0 * DegEchelle - t * 0.1);
    return float4(lerp(DegCouleurs[0].rgb, DegCouleurs[2].rgb, pow(v, lerp(2.0, 0.5, DegDouceur))), v);
}

// « Étoiles » : des points qui scintillent — chacun a sa phase, tirée de sa
// cellule, donc ils ne clignotent pas ensemble.
float4 DegEtoiles(float2 p, float aspect, float t) {
    float n = 24.0 * DegEchelle;
    float2 g = p * n;
    float2 c = floor(g);
    float h = DegHash(c);
    float2 centre = c + 0.5 + 0.35 * DegGradient(c);
    float d = length(g - centre);
    float scintille = 0.5 + 0.5 * sin(t * (1.0 + 3.0 * h) + h * 20.0);
    float halo = exp(-d * d * lerp(9.0, 2.0, DegDouceur)) * step(0.55, h);
    return float4(DegCouleurs[0].rgb + halo * scintille * 1.6 * DegCouleurs[4].rgb, halo);
}

// « Réseau » : un maillage de lignes lumineuses sur une rampe.
float4 DegReseau(float2 p, float aspect, float t) {
    float n = 12.0 * DegEchelle;
    float2 f = abs(frac(p * n) - 0.5);
    float d = min(f.x, f.y);
    float trait = smoothstep(lerp(0.05, 0.25, DegDouceur), 0.0, d);
    return float4(DegPalette(saturate(p.y * 0.7 + 0.15)) * (0.55 + 0.9 * trait), trait);
}

// « Marée » : des bandes dont la hauteur varie lentement — un trait de côte.
float4 DegMaree(float2 p, float aspect, float t) {
    float v = p.y * 8.0 * DegEchelle
            + (0.4 + 1.6 * DegWarp) * DegBruit(p * 2.0 * DegEchelle + float2(0.0, t * 0.15));
    float3 rgb = DegPalette(saturate(frac(v) * 0.9 + p.y * 0.1));
    float lisere = smoothstep(0.85, 1.0, frac(v));
    return float4(lerp(rgb, rgb * 1.4, lisere * lerp(0.2, 0.9, DegDouceur)), frac(v));
}

// « Confettis » : des carrés épars, chacun de sa couleur, sur un fond sourd.
float4 DegConfettis(float2 p, float aspect, float t) {
    float n = 7.0 * DegEchelle;
    float2 g = p * n;
    float2 c = floor(g);
    float h = DegHash(c);
    float2 d = abs(g - (c + 0.5));
    float carre = step(0.62, h) * (1.0 - smoothstep(0.28, 0.36, max(d.x, d.y)));
    float3 fond = DegPalette(saturate(p.y * 0.5 + 0.2)) * 0.35;
    return float4(lerp(fond, DegPalette(frac(h * 3.7)), carre), carre);
}

// « Spirale » : un bras qui s'enroule — la distance angulaire sert de phase.
float4 DegSpirale(float2 p, float aspect, float t) {
    float2 q = (p - 0.5) * float2(aspect, 1.0) * DegEchelle * 2.0;
    float r = length(q) + 1e-4;
    float a = atan2(q.y, q.x);
    float v = DegAllerRetour((a / 6.2831 + r * 0.9) * 2.0 - t * 0.08);
    return float4(lerp(DegCouleurs[0].rgb, DegCouleurs[3].rgb, pow(v, lerp(3.0, 0.7, DegDouceur))), v);
}

// « Écho » : des anneaux concentriques qui se propagent depuis des centres qui
// dérivent — chacun d'une couleur.
float4 DegEcho(float2 p, float aspect, float t) {
    float3 rgb = DegCouleurs[0].rgb;
    // Hauteur : l'intensité cumulée des anneaux — les ondes se SOMMENT là où
    // elles se croisent, donc le relief y fait des crêtes.
    float relief = 0.0;
    [unroll] for (int i = 0; i < 5; i++) {
        float fi = (float)i;
        float2 c = float2(0.5 + 0.25 * sin(t * 0.30 + fi * 1.1), 0.5 + 0.25 * cos(t * 0.24 + fi * 0.9));
        float d = length(p - c) - (0.12 + 0.16 * fi);
        float onde = exp(-d * d * 40.0);
        rgb += onde * DegCouleurs[i].rgb * (0.6 / (1.0 + fi));
        relief += onde * (0.6 / (1.0 + fi));
    }
    return float4(rgb, relief);
}

// « Prairie » : des brins verticaux qui ondulent, chacun de sa phase et de sa
// hauteur — PLANTÉS EN BAS, comme de l'herbe.
float4 DegPrairie(float2 p, float aspect, float t) {
    float n = 30.0 * DegEchelle;
    float x = p.x * n;
    float h = DegHash(float2(floor(x), 0.0));
    float hauteur = 0.25 + 0.5 * h;
    float balance = (0.10 + 0.08 * DegWarp) * sin(t * 1.2 + h * 6.2831);
    // ⚠️ **`p.y` CROÎT VERS LE BAS** (voir `DegMotif`) : c'est `1.0 - p.y` qui
    // mesure la hauteur AU-DESSUS du bas de l'écran. Avec `p.y` seul — la
    // première version — les brins pendaient du bord SUPÉRIEUR : une prairie au
    // plafond. Même classe de défaut que « Neige », qui tombait vers le ciel :
    // dans ce repère, le sens de l'axe ne se relit pas, il se MESURE.
    float au_dessus = (1.0 - p.y) + balance * (p.y - 0.5);
    float y = au_dessus / hauteur;
    float dedans = step(0.0, y) * step(y, 1.0) * step(0.35, frac(x));
    float3 brin = lerp(DegCouleurs[0].rgb, DegCouleurs[2].rgb, saturate(y));
    return float4(lerp(DegPalette(saturate(p.y * 0.4 + 0.1)) * 0.4, brin, dedans), dedans);
}

// « Amande » : des losanges en quinconce — des feuilles, pas des disques.
float4 DegAmande(float2 p, float aspect, float t) {
    float n = 6.0 * DegEchelle;
    float2 g = p * n;
    g.x += floor(g.y) * 0.5;
    float2 f = frac(g) - 0.5;
    float amande = 1.0 - smoothstep(0.0, 1.0, abs(f.x) * 3.0 + abs(f.y) * 1.2);
    float3 rgb = DegPalette(frac(floor(g.x) * 0.11 + floor(g.y) * 0.19 + t * 0.02));
    return float4(lerp(rgb * 0.35, rgb, amande), amande);
}

// « Voile » : deux réseaux de fines diagonales qui interfèrent — un moiré
// lent, sur un fond de brouillard.
float4 DegVoile(float2 p, float aspect, float t) {
    float2 q = p * DegEchelle;
    float a = sin((q.x + q.y) * 20.0 + t * 0.6);
    float b = sin((q.x - q.y) * 17.0 - t * 0.5);
    float moire = 0.5 + 0.25 * (a + b);
    float3 rgb = DegPalette(saturate(0.5 + 0.4 * DegFbm(q * 0.6 + t * 0.05)));
    return float4(lerp(rgb, rgb * 1.5, moire * lerp(0.15, 0.7, DegDouceur)), moire);
}

// « Tisons » : des braises chaudes sur un fond éteint — plus sèches que
// « Braises », dont les taches sont larges et fondues.
float4 DegTisons(float2 p, float aspect, float t) {
    float bruit = DegFbm(p * 2.2 * DegEchelle + t * 0.12);
    float braise = smoothstep(lerp(0.25, 0.6, DegDouceur), 1.0, bruit);
    float3 fond = DegPalette(saturate(p.y * 0.5)) * 0.25;
    float3 chaud = lerp(DegCouleurs[2].rgb, DegCouleurs[4].rgb, saturate(bruit));
    return float4(lerp(fond, chaud * 1.5, braise), braise);
}

// « Ondulation » : une seule onde qui traverse l'écran — deux rampes
// s'échangent, l'une en haut, l'autre en bas.
float4 DegOndulation(float2 p, float aspect, float t) {
    float phase = p.y * 3.0 * DegEchelle
                + (0.5 + DegWarp) * DegBruit(p * 1.5 * DegEchelle + t * 0.1) + t * 0.25;
    float v = DegAllerRetour(phase);
    float3 haute = lerp(DegCouleurs[0].rgb, DegCouleurs[1].rgb, v);
    float3 basse = lerp(DegCouleurs[2].rgb, DegCouleurs[3].rgb, v);
    return float4(lerp(haute, basse, saturate(p.y)), v);
}

// « Vitrail » : des cellules de couleur séparées par un plomb sombre — le
// contraire de « Cristal », qui éclaire les arêtes.
float4 DegVitrail(float2 p, float aspect, float t) {
    float3 c = DegCellules(p * 4.5 * DegEchelle, t * 0.18);
    float plein = smoothstep(0.0, 0.12, c.y - c.x);
    float3 verre = DegPalette(frac(c.z * 2.3 + 0.1)) * 1.35;
    return float4(lerp(DegCouleurs[0].rgb * 0.12, verre, plein), plein);
}

// « Neige » : des flocons qui descendent — la grille défile, donc les flocons
// tombent et de nouveaux entrent par le haut.
//
// ⚠️ **LE SIGNE COMPTE, ET IL ÉTAIT À L'ENVERS.** Le flocon de la cellule `c`
// s'affiche à `p.y = (c.y + 0.5 + t·1.2) / n` : quand `t` augmente, `p.y`
// augmente, donc le flocon DESCEND. Avec `+=` (la première version), `p.y`
// diminuait et la neige tombait vers le ciel — défaut signalé, et invisible
// dans un code où les deux lignes se ressemblent à un caractère près.
float4 DegNeige(float2 p, float aspect, float t) {
    float n = 16.0 * DegEchelle;
    float2 g = p * n;
    // `p.y` CROÎT vers le bas (voir `DegMotif`) : soustraire fait donc tomber.
    g.y -= t * 1.2;
    float2 c = floor(g);
    float h = DegHash(c);
    float2 centre = c + 0.5 + 0.3 * sin(t * 0.7 + h * 6.2831) * float2(1.0, 0.2);
    float d = length(g - centre);
    float flocon = exp(-d * d * lerp(40.0, 10.0, DegDouceur)) * step(0.62, h);
    return float4(DegPalette(saturate(p.y * 0.4 + 0.15)) * 0.45 + flocon * DegCouleurs[4].rgb * 1.2, flocon);
}

// ---- DOUZE MOTIFS D'AUTRES FAMILLES — DEMANDE EXPLICITE (2026-09-27) :
//      « cherches sur internet de la doc pour faire de nouveaux shaders jamais
//      vus ». Les soixante précédents tournent tous autour des mêmes cinq ou six
//      familles (bruit fbm, cellules de Voronoï, sommes de sinus, warp de
//      domaine). Ceux-ci viennent de familles DOCUMENTÉES qui n'y figuraient
//      pas — la source est citée sur chacun, aucune n'est devinée.
//
//      ⚠️ Deux d'entre eux sortent volontairement de `DegPalette` (Newton, et
//      la teinte des cellules du Nid d'abeille) : la couleur y est une DONNÉE de
//      la technique, pas une rampe. Le commentaire le dit à chaque fois.

// Orientation locale d'une tuile de Truchet — la fonction du « Book of
// Shaders » (chapitre *Generative designs*) : le tirage de la cellule choisit
// l'une des quatre orientations, ce qui suffit à faire courir des arcs d'un
// bord à l'autre.
float2 DegTuileTruchet(float2 f, float tirage) {
    float i = frac((tirage - 0.5) * 2.0);
    if (i > 0.75) { return float2(1.0, 1.0) - f; }
    if (i > 0.50) { return float2(1.0 - f.x, f.y); }
    if (i > 0.25) { return float2(f.x, 1.0 - f.y); }
    return f;
}

// « Truchet » : des arcs tirés par cellule, qui se RACCORDENT d'une tuile à
// l'autre et dessinent des chemins sans plan d'ensemble.
// Source : The Book of Shaders, « Generative designs » (`truchetPattern`).
float4 DegTruchet(float2 p, float aspect, float t) {
    float n = 8.0 * DegEchelle;
    float2 g = p * n;
    float2 c = floor(g);
    float2 f = DegTuileTruchet(frac(g), DegHash(c));
    // Jonction des arcs ARRIE (voir `DegFondu`).
    float d = DegFondu(length(f), length(f - 1.0), 0.08);
    float epaisseur = 0.24 + 0.10 * DegDouceur;
    float trait = smoothstep(epaisseur + 0.04, epaisseur, d);
    float3 fond = DegPalette(saturate(p.y * 0.5 + 0.15)) * 0.40;
    return float4(lerp(fond, DegCouleurs[3].rgb, trait), trait);
}

// « Labyrinthe » : la variante à LIGNES DROITES des mêmes tuiles — un dédale
// plutôt que des courbes.
// Source : même chapitre, variante `abs(abs(gv.x + gv.y) - .5)`.
float4 DegLabyrinthe(float2 p, float aspect, float t) {
    float n = 10.0 * DegEchelle;
    float2 g = p * n;
    float2 c = floor(g);
    float2 f = DegTuileTruchet(frac(g), DegHash(c));
    float d = abs(abs(f.x + f.y) - 0.5);
    float trait = smoothstep(0.16, 0.08, d);
    float3 fond = DegPalette(saturate(p.y * 0.6 + 0.1)) * 0.35;
    return float4(lerp(fond, DegCouleurs[1].rgb * 1.2, trait), trait);
}

// Le champ d'une plaque CARRÉE qui vibre — `melange` vaut -1 pour le mode
// antisymétrique (le dessin classique) et +1 pour le symétrique, qui donne une
// toute autre figure.
// Source : φ = cos(nπx)cos(mπy) + α·cos(mπx)cos(nπy), α = ±1.
float DegChladni(float2 p, float2 modes, float melange) {
    float2 q = p * 3.14159265;
    return cos(modes.x * q.x) * cos(modes.y * q.y) + melange * cos(modes.y * q.x) * cos(modes.x * q.y);
}

// « Chladni » : les lignes NODALES d'une plaque vibrante — là où le sable se
// rassemble, parce que la plaque n'y bouge pas. Les modes changent lentement,
// et la figure se réorganise d'un coup quand ils passent à l'entier suivant.
float4 DegChladniMotif(float2 p, float aspect, float t) {
    float2 modes = floor(float2(2.0 + 4.0 * (0.5 + 0.5 * sin(t * 0.07)),
                                3.0 + 4.0 * (0.5 + 0.5 * cos(t * 0.05))));
    float phi = DegChladni(p - 0.5, modes, -1.0);
    float nodal = 1.0 - smoothstep(0.0, 0.06 + 0.30 * DegDouceur, abs(phi));
    return float4(lerp(DegPalette(saturate(0.5 + 0.6 * phi)), DegCouleurs[4].rgb * 1.4, nodal), phi);
}

// « Rosace » : le même dessin en mode SYMÉTRIQUE (α = +1) — des lignes droites
// et des diagonales au lieu de courbes.
float4 DegRosace(float2 p, float aspect, float t) {
    float2 modes = floor(float2(3.0 + 4.0 * (0.5 + 0.5 * cos(t * 0.06)),
                                2.0 + 5.0 * (0.5 + 0.5 * sin(t * 0.04))));
    float phi = DegChladni(p - 0.5, modes, 1.0);
    float nodal = 1.0 - smoothstep(0.0, 0.05 + 0.25 * DegDouceur, abs(phi));
    return float4(lerp(DegPalette(saturate(0.5 - 0.7 * phi)), DegCouleurs[0].rgb * 1.5, nodal), phi);
}

// « Quasicristal » : une somme d'ondes planes également réparties autour du
// cercle, à SEPT directions. L'ordre est parfait et le motif **ne se répète
// jamais** — c'est la définition d'un quasicristal, et c'est ce qui le
// distingue de tous les autres motifs de cette liste, qui sont tous périodiques
// ou pseudo-périodiques.
// Source : somme de m ondes planes d'angle a = (0:m-1)·π/m.
float4 DegQuasicristal(float2 p, float aspect, float t) {
    const int DIRECTIONS = 7;
    float somme = 0.0;
    [unroll] for (int i = 0; i < DIRECTIONS; i++) {
        float a = 3.14159265 * (float)i / (float)DIRECTIONS + t * 0.02;
        somme += cos((p.x * cos(a) + p.y * sin(a)) * DegEchelle * 9.0 + a * 3.0);
    }
    return float4(DegPalette(saturate(0.5 + 0.9 * somme / (float)DIRECTIONS)), somme / (float)DIRECTIONS);
}

// « Guilloché » : une HYPOTROCHOÏDE — la courbe des machines à guillocher, qui
// décorent les billets et les fonds de montre. On ne trace pas une courbe
// paramétrée, on prend la DISTANCE du pixel à 48 échantillons de la courbe :
// c'est ce qui la remplit au lieu de laisser un trait d'un pixel.
// Source : x(φ) = (R−r)cos φ + p·cos((R−r)/r·φ), y(φ) = (R−r)sin φ − p·sin(…).
float4 DegGuilloche(float2 p, float aspect, float t) {
    const float R = 9.0;
    const float r = 4.0;
    const float plume = 1.6;
    float2 q = (p - 0.5) * float2(aspect, 1.0) * 2.0;
    float ca = cos(t * 0.03), sa = sin(t * 0.03);
    q = float2(q.x * ca - q.y * sa, q.x * sa + q.y * ca);
    float pente = (R - r) / r;
    float dmin = 8.0;
    // ⚠️ `[unroll]`, comme partout dans ce fichier, et un nombre d'échantillons
    // choisi pour rester déroulable. J'avais écrit `[loop]` ici d'abord, et je
    // l'ai accusé à tort de l'échec `X3528` — la cause était le `[branch]` du
    // `switch` (voir `DegMotif`). Je n'ai donc AUCUNE mesure disant que `[loop]`
    // passe sur ce profil, et je ne l'affirme pas.
    [unroll] for (int i = 0; i < 32; i++) {
        float phi = 6.2831853 * (float)i / 32.0;
        float2 c = float2((R - r) * cos(phi) + plume * cos(pente * phi),
                          (R - r) * sin(phi) - plume * sin(pente * phi)) / R;
        dmin = min(dmin, length(q - c));
    }
    dmin += 0.10 * DegWarp * DegBruit(q * 2.0 + t * 0.1);
    float trait = 1.0 - smoothstep(0.0, 0.05 + 0.16 * DegDouceur, dmin);
    return float4(lerp(DegPalette(saturate(0.30 + 1.8 * dmin)), DegCouleurs[4].rgb * 1.5, trait), trait);
}

// « Phyllotaxie » : l'arrangement des graines de tournesol — chaque grain à
// l'angle d'or du précédent, à une distance en √n. C'est le seul motif de la
// liste dont la structure vient d'une PLANTE, et il ne se répète pas non plus.
// Source : modèle de Vogel, θ = n·φ_or (φ_or ≈ 137,5°), r = c·√n.
float4 DegPhyllotaxie(float2 p, float aspect, float t) {
    const int GRAINS = 64;
    const float OR = 2.39996323; // π(3 − √5), l'angle d'or
    float2 q = (p - 0.5) * float2(aspect, 1.0) * 2.0;
    float dmin = 8.0;
    // `[unroll]` : voir « Guilloché ».
    [unroll] for (int i = 0; i < GRAINS; i++) {
        float fi = (float)i;
        float a = fi * OR + t * 0.05;
        float r = 0.92 * DegEchelle * sqrt(fi / (float)GRAINS);
        dmin = min(dmin, length(q - r * float2(cos(a), sin(a))));
    }
    float grain = 1.0 - smoothstep(0.0, 0.06 + 0.16 * DegDouceur, dmin);
    return float4(lerp(DegPalette(saturate(0.25 + 1.6 * dmin)), DegCouleurs[2].rgb * 1.3, grain), grain);
}

// Le RIDGED MULTIFRACTAL de Musgrave : valeur absolue du bruit, inversée,
// ÉLEVÉE AU CARRÉ, puis chaque octave pondérée par la précédente — c'est ce qui
// fait des ARÊTES au lieu de collines.
// Source : `noise_musgrave_ridged_multi_fractal` (Blender/Cycles, `svm_musgrave.h`).
float DegRide(float2 p) {
    const float H = 0.9;
    const float LACUNARITE = 2.1;
    const float GAIN = 2.0;
    float2 q = p;
    float pwHL = pow(LACUNARITE, -H);
    float pwr = pwHL;
    float signal = 1.0 - abs(DegBruit(q));
    signal *= signal;
    float valeur = signal;
    [unroll] for (int i = 1; i < 5; i++) {
        q *= LACUNARITE;
        float poids = saturate(signal * GAIN);
        signal = 1.0 - abs(DegBruit(q));
        signal *= poids;
        valeur += signal * pwr;
        pwr *= pwHL;
    }
    return valeur;
}

// « Ridé » : une crête rocheuse — ce que le ridged multifractal a été inventé
// pour produire.
float4 DegRideMotif(float2 p, float aspect, float t) {
    float h = DegRide(p * 2.4 * DegEchelle + t * 0.03);
    return float4(DegPalette(saturate(h * 0.75)), h);
}

// « Newton » : les anneaux d'interférence d'une lame mince — la couleur vient
// du `cos(2π·2nd/λ)`, avec une LONGUEUR D'ONDE PAR CANAL. C'est le seul motif
// qui ne passe pas par `DegPalette` : un arc-en-ciel est ici la donnée de la
// technique, pas une rampe de thème. Il s'y mêle quand même à 40 %, pour ne pas
// jurer avec le reste.
float4 DegNewton(float2 p, float aspect, float t) {
    float2 q = (p - 0.5) * float2(aspect, 1.0) * 2.0;
    float epaisseur = length(q) * 9.0 * DegEchelle - t * 0.4;
    // Longueurs d'onde RELATIVES du rouge, du vert et du bleu. ⚠️ Surtout PAS
    // nommées `lambda` : c'est un MOT RÉSERVÉ de HLSL, et l'erreur qu'il produit
    // (« Erreur non spécifiée ») ne dit ni la ligne ni le nom — elle fait
    // échouer la compilation des SOIXANTE ET ONZE motifs d'un coup, y compris
    // ceux qui n'y touchent pas.
    float3 longueurs = float3(0.62, 0.53, 0.45);
    float3 irise = 0.5 + 0.5 * cos(6.2831853 * 2.0 * epaisseur / longueurs + DegGraine);
    float melange = 0.40 + 0.35 * DegDouceur;
    return float4(lerp(DegPalette(saturate(0.5 + length(q) * 0.8)), irise, melange), epaisseur);
}

// « Nid d'abeille » : le pavage hexagonal — le seul régulier qui ne soit ni
// carré ni triangulaire. La cellule se décrit comme l'intersection de trois
// bandes à 60°.
float4 DegNidDabeille(float2 p, float aspect, float t) {
    float n = 9.0 * DegEchelle;
    float2 g = p * n;
    // Une rangée sur deux décalée d'une demi-cellule.
    float rangee = floor(g.y / 1.7320508);
    g.x += 0.5 * (rangee - 2.0 * floor(rangee * 0.5));
    float2 f = abs(frac(g) - 0.5);
    // Arête ARRIE (voir `DegFondu`).
    float d = DegFonduMax(f.x * 0.8660254 + f.y * 0.5, f.y, 0.06);
    float trait = smoothstep(0.44, 0.36, d);
    return float4(lerp(DegPalette(saturate(p.y * 0.5 + 0.1)) * 0.35, DegCouleurs[2].rgb * 1.3, trait), trait);
}

// « Résonance » : la plaque est cette fois CIRCULAIRE — les lignes nodales
// suivent une fonction de Bessel. HLSL n'en a pas, mais son développement
// asymptotique donne exactement les anneaux qui comptent ici.
// Source : J_n(kr) ≈ cos(kr − nπ/2 − π/4)/√r, multiplié par cos(nθ).
float4 DegResonance(float2 p, float aspect, float t) {
    float2 q = (p - 0.5) * float2(aspect, 1.0) * 2.0;
    float r = length(q) + 1e-3;
    float ordre = 1.0 + floor(2.0 + 3.0 * (0.5 + 0.5 * sin(t * 0.05)));
    float phi = cos(9.0 * DegEchelle * r - ordre * 1.5707963 - 0.7853982) / sqrt(r);
    phi *= cos(ordre * atan2(q.y, q.x));
    float nodal = 1.0 - smoothstep(0.0, 0.05 + 0.25 * DegDouceur, abs(phi));
    return float4(lerp(DegPalette(saturate(0.5 + 0.8 * phi)), DegCouleurs[3].rgb * 1.4, nodal), phi);
}

// « Facetgrad » : chaque carreau est un DÉGRADÉ DIAGONAL à deux arrêts, et son
// axe comme ses deux couleurs sont tirés par la cellule — les facettes se
// ré-éclairent et se ré-orientent quand la graine change.
//
// Source : Tabbied, `facetgrad` — « Each tile a two-stop diagonal gradient, the
// facets re-lighting and re-aiming on reseed ». Vérifié sur son APERÇU
// (tabbied.com/previews/facetgrad.webp) et non sur la seule description : c'est
// là qu'on voit que la transition ne remplit pas tout le carreau, et qu'il reste
// donc une grande plage unie avec un seul coin qui bascule.
float4 DegFacetgrad(float2 p, float aspect, float t) {
    float n = 8.0 * DegEchelle;
    float2 g = p * n;
    float2 c = floor(g);
    float2 f = frac(g) - 0.5;
    float h = DegHash(c);
    // L'AXE : un angle par carreau — c'est le « re-aiming ».
    float angle = h * 6.2831853;
    float2 axe = float2(cos(angle), sin(angle));
    // Les DEUX ARRÊTS : deux couleurs de la rampe du THÈME, tirées par la même
    // cellule — c'est le « re-lighting ». Le thème plutôt qu'une palette figée :
    // c'est ce qui fait qu'un préréglage change le motif.
    float3 a = DegPalette(frac(h * 3.1));
    float3 b = DegPalette(frac(h * 7.7 + 0.37));
    // La projection du point sur l'axe. Le facteur décide de la PART du carreau
    // que la transition occupe : au-delà de 1, les deux extrémités saturent et
    // il reste une plage unie de chaque côté.
    float v = saturate(dot(f, axe) * lerp(2.4, 1.1, DegDouceur) + 0.5);
    float3 rgb = lerp(a, b, smoothstep(0.0, 1.0, v));
    // Hauteur : une facette est PLATE, donc une valeur PAR CARREAU. C'est ce qui
    // fait que le relief allume les arêtes des carreaux, et que la perspective
    // les empile au lieu de les fondre.
    return float4(rgb, h);
}

// ---- PLAN D'EAU — d'après `ShaderGradient` (type « waterPlane ») -----------
//
// DEMANDE EXPLICITE (2026-10-03) : « ajouter un motif en t'inspirant de ce
// code » — un composant `<ShaderGradient type="waterPlane">` (uDensity 1,2,
// uStrength 3,4, uSpeed 0,1, caméra à 60° de la verticale, plan basculé de 45°,
// reflet 0,4, trois couleurs : magenta, orange, rouge très sombre).
//
// Ce que fait l'original : un PLAN maillé dont chaque sommet est déplacé par un
// bruit 3D lent, vu de biais, coloré selon sa position et éclairé par une carte
// d'environnement. Ce qui est repris, en 2D et sans maillage :
//   * la VUE DE BIAIS — l'axe vertical est comprimé vers le haut de l'écran
//     (le loin), ce qui étire les ondulations en nappes comme sur le plan
//     basculé ;
//   * le DÉPLACEMENT — un bruit de grande échelle, déformé par un second bruit
//     (le domaine qui coule), qui dérive à la vitesse `uSpeed` ;
//   * les TROIS COULEURS — la rampe du thème le long de la nappe, et la couleur
//     la plus SOMBRE du thème au fond des creux (le « color3 » de l'original) ;
//   * le REFLET — une normale tirée du déplacement, un spéculaire large et une
//     fresnel douce : c'est ce qui donne le satin des crêtes.
// La hauteur rendue est le DÉPLACEMENT lui-même, la grandeur sur laquelle la
// nappe est construite — le relief et la perspective s'en servent.
float DegNappe(float2 q, float t) {
    float2 coule = float2(DegBruit(q * 0.7 + float2(t * 0.6, 3.1)), DegBruit(q * 0.7 + float2(8.3, -t * 0.5)));
    return DegBruit(q + coule * 1.2 + float2(0.0, t));
}

float4 DegPlanDeau(float2 p, float aspect, float t) {
    // La vue de biais : plus on monte, plus c'est loin, donc plus c'est serré.
    float profondeur = 1.0 / (0.55 + 0.9 * saturate(1.0 - p.y));
    float2 q = float2((p.x - 0.5 * aspect) * profondeur, p.y * profondeur * 1.6) * 1.2 * DegEchelle;
    float tt = t * 0.25;
    float d = DegNappe(q, tt);
    // La normale, par différence finie — trois évaluations DANS ce motif, le
    // `switch` n'en voit qu'une.
    float e = 0.05;
    float dx = DegNappe(q + float2(e, 0.0), tt) - d;
    float dy = DegNappe(q + float2(0.0, e), tt) - d;
    float3 n = normalize(float3(-dx, -dy, e * lerp(0.6, 1.6, DegDouceur)));
    // La couleur : la rampe le long de la nappe déplacée, puis le fond sombre
    // dans les creux.
    float le_long = saturate(0.5 + 0.35 * (q.x * 0.35 + d * 0.9));
    float3 rgb = DegPalette(le_long);
    float3 sombre = DegCouleurs[0].rgb;
    rgb = lerp(rgb, sombre, smoothstep(0.1, -0.9, d) * 0.85);
    // Le reflet (`reflection` 0,4) : spéculaire large + fresnel douce.
    float3 lumiere = normalize(float3(-0.35, 0.55, 0.75));
    float3 demi = normalize(lumiere + float3(0.0, 0.0, 1.0));
    float spec = pow(saturate(dot(n, demi)), 28.0);
    float fresnel = pow(1.0 - saturate(n.z), 2.0);
    rgb += (spec * 0.4 + fresnel * 0.12) * lerp(rgb, 1.0, 0.6);
    return float4(saturate(rgb), d);
}

// ---- REMOUS — variante de « Plan d'eau » -----------------------------------
//
// DEMANDE EXPLICITE (2026-10-03) : « une variante avec des rotations de surfaces
// et une impression de mélanges en spirale ». Même nappe que `DegPlanDeau`
// (déplacement, reflet, fond sombre dans les creux), mais :
//   * DEUX SURFACES qui tournent en sens CONTRAIRE autour du centre, à des
//     vitesses différentes, et dont les déplacements s'additionnent — c'est ce
//     qui fait qu'aucune ne semble fixe ;
//   * une TORSION EN SPIRALE : l'angle de rotation croît vers le centre (en
//     1/r, borné), donc les nappes s'enroulent et les couleurs se mélangent en
//     bras de spirale ;
//   * la couleur suit l'ANGLE tordu plutôt qu'une direction fixe — ce sont les
//     bras eux-mêmes qui portent la rampe du thème.
// Le centre dérive lentement, pour que la spirale ne soit jamais clouée au même
// point. La hauteur rendue reste le déplacement.
float2 DegTourne(float2 q, float a) {
    float c = cos(a);
    float s = sin(a);
    return float2(c * q.x - s * q.y, s * q.x + c * q.y);
}

float DegRemousNappe(float2 q, float t) {
    float r = length(q);
    // La torsion : forte au centre, nulle loin — bornée pour ne pas crépiter.
    float torsion = 2.6 / (r + 0.35);
    float a = DegNappe(DegTourne(q, t * 0.9 + torsion), t);
    float b = DegNappe(DegTourne(q * 1.35 + 4.0, -t * 0.6 - torsion * 0.7), t * 1.3);
    return 0.6 * a + 0.4 * b;
}

float4 DegRemous(float2 p, float aspect, float t) {
    float tt = t * 0.25;
    float2 centre = float2(0.5 * aspect, 0.5) + 0.12 * float2(sin(tt * 0.7), cos(tt * 0.5));
    // Même vue de biais que le plan d'eau, plus douce : la spirale doit rester
    // ronde à l'œil.
    float profondeur = 1.0 / (0.7 + 0.6 * saturate(1.0 - p.y));
    float2 q = float2(p.x - centre.x, (p.y - centre.y) * 1.3) * profondeur * 2.2 * DegEchelle;
    float d = DegRemousNappe(q, tt);
    float e = 0.05;
    float dx = DegRemousNappe(q + float2(e, 0.0), tt) - d;
    float dy = DegRemousNappe(q + float2(0.0, e), tt) - d;
    float3 n = normalize(float3(-dx, -dy, e * lerp(0.6, 1.6, DegDouceur)));
    // La couleur suit l'ANGLE tordu : les bras de la spirale portent la rampe.
    float r = length(q);
    float angle = atan2(q.y, q.x) + 2.6 / (r + 0.35) - tt * 0.5;
    float bras = 0.5 + 0.5 * sin(angle * 2.0 + d * 2.5);
    float3 rgb = DegPalette(saturate(lerp(bras, 0.5 + 0.5 * d, 0.35)));
    rgb = lerp(rgb, DegCouleurs[0].rgb, smoothstep(0.1, -0.9, d) * 0.8);
    float3 lumiere = normalize(float3(-0.35, 0.55, 0.75));
    float3 demi = normalize(lumiere + float3(0.0, 0.0, 1.0));
    float spec = pow(saturate(dot(n, demi)), 28.0);
    float fresnel = pow(1.0 - saturate(n.z), 2.0);
    rgb += (spec * 0.4 + fresnel * 0.12) * lerp(rgb, 1.0, 0.6);
    return float4(saturate(rgb), d);
}

// ---- BULLES — bulles de savon poussées par des coups de vent ----------------
//
// DEMANDE EXPLICITE (2026-10-11) : une bulle CSS fournie (pellicule aux bords
// flous cyan #0fb4ff, rose #ff4484, jaune #ffeb3b et blanc tourné de 330°, un
// second anneau rose, une lueur intérieure, deux reflets), « avec de nombreuses
// bulles et des turbulences de vent », puis « qu'elles se déplacent de façon
// aléatoire et des effets comme s'il y avait des coups de vent ».
//
//   * TROIS COUCHES de bulles (lointaines : petites, pâles, lentes) ; chaque
//     bulle ERRE au hasard autour de sa cellule par un bruit qui lui est
//     propre. Les 2×2 cellules les plus proches suffisent : l'errance (0,35) et
//     le rayon dessiné (0,32 × 1,25 = 0,4) n'atteignent jamais un pixel à plus
//     de 0,75 du centre de la cellule, et la cellule écartée en est à 1 au
//     moins. Une seule bulle par couche est dessinée (voir plus bas) ;
//   * DEUX SYSTÈMES DE RAFALES de périodes non commensurables (vers la droite,
//     et en biais vers le haut) : le chemin parcouru est l'INTÉGRALE de leur
//     vitesse, en forme close — un shader n'a pas de mémoire d'une image à
//     l'autre, et déplacer selon la vitesse ferait revenir les bulles en
//     arrière à la fin de chaque rafale. Le front de rafale traverse l'écran de
//     gauche à droite ;
//   * pendant une rafale, la turbulence et le tremblement des bulles
//     s'amplifient, et des TRAÎNÉES de vent filent à travers l'écran.
//
// Les bulles gardent les couleurs du CSS ; le fond suit le thème. Curseurs :
// Échelle (taille et nombre), Warp (force du vent), Douceur (flou de la
// pellicule). La hauteur rendue est l'épaisseur de pellicule.
static const float3 DegBulleCyan  = float3(0.059, 0.706, 1.000);
static const float3 DegBulleRose  = float3(1.000, 0.267, 0.518);
static const float3 DegBulleJaune = float3(1.000, 0.922, 0.231);

// La force d'une rafale (0 au calme, 1 au plus fort), une fois par période 2π/w.
float DegRafaleForce(float t, float w) {
    float b = 0.5 - 0.5 * cos(w * t);
    return b * b;
}

// Le chemin parcouru sous une vitesse 1 + force·DegRafaleForce :
// ((1 - cos wt)/2)² = 3/8 - cos(wt)/2 + cos(2wt)/8, intégré terme à terme.
float DegRafaleChemin(float t, float w, float force) {
    return t + force * (0.375 * t - sin(w * t) / (2.0 * w) + sin(2.0 * w * t) / (16.0 * w));
}

// Une bulle, en coordonnées de la bulle : rayon 1, y vers le BAS comme en CSS.
// Rend la couleur et l'opacité ; `film` reçoit l'épaisseur de pellicule.
float4 DegBulle(float2 d, float phase, out float film) {
    film = 0.0;
    float r = length(d);
    if (r > 1.25) return float4(0.0, 0.0, 0.0, 0.0);

    float flou = 0.05 + 0.2 * DegDouceur;
    float bord = smoothstep(0.45, 0.95, r) * (1.0 - smoothstep(0.95, 0.95 + flou, r));

    // La pellicule tourne lentement : ses couleurs glissent autour de la bulle.
    float2 n = DegTourne(d / max(r, 1e-4), phase);
    // Le bord blanc du bas est tourné de 330° dans le CSS.
    float blanc = saturate(-0.5 * n.x + 0.866 * n.y) * 0.8;
    float3 couleur = DegBulleCyan * saturate(-n.x) + DegBulleRose * saturate(n.x)
                   + DegBulleJaune * saturate(-n.y) + blanc;
    couleur /= max(saturate(-n.x) + saturate(n.x) + saturate(-n.y) + blanc, 1e-3);

    // Le second anneau rose, plus à l'intérieur.
    float anneau = exp(-pow((r - 0.62) / (0.08 + 0.1 * DegDouceur), 2.0)) * saturate(-n.x);
    couleur = lerp(couleur, DegBulleRose, anneau * 0.6);

    // La lueur intérieure blanche, puis les deux reflets.
    float lueur = smoothstep(0.6, 1.0, r) * step(r, 1.0) * 0.25;
    float reflet = 1.0 - smoothstep(0.08, 0.2, length(d - float2(-0.4, -0.35)));
    reflet = max(reflet, 1.0 - smoothstep(0.04, 0.14, length(d - float2(-0.1, -0.1))));

    float a = saturate(max(bord * 0.85 + anneau * 0.4, lueur));
    float3 rgb = lerp(couleur, 1.0, lueur / max(a, 1e-3) * 0.5);
    rgb = lerp(rgb, 1.0, reflet);
    film = max(bord, reflet);
    return float4(rgb, max(a, reflet * 0.95));
}

float4 DegBulles(float2 p, float aspect, float t) {
    float3 rgb = DegPalette(saturate(0.5 - 0.8 * (p.y - 0.5) + 0.12 * DegFbm(p * 1.5 + t * 0.05)));
    float hauteur = 0.0;

    float forceVent = 1.5 + 6.0 * DegWarp;
    // Le front de rafale arrive par la gauche : le temps local est en retard.
    // Il ne règle que l'INTENSITÉ (traînées, tremblement, turbulence). ⛔ Le
    // chemin, lui, ne dépend que du temps : calculé au temps local, il variait
    // avec x et les bulles s'écrasaient en ovales au passage de la rafale.
    float tl = t - 0.25 * p.x;
    float rafale = DegRafaleForce(tl, 0.83) + 0.7 * DegRafaleForce(tl + 3.7, 0.53);
    float2 chemin = float2(-0.5 * DegRafaleChemin(t, 0.83, forceVent), 0.0)
                  + float2(-0.25, 0.18) * DegRafaleChemin(t + 3.7, 0.53, forceVent);

    for (int couche = 0; couche < 3; couche++) {
        float fc = (float)couche;
        float k = fc * 0.5;
        float taille = lerp(7.0, 2.6, k) * DegEchelle;
        float vitesse = lerp(0.3, 0.7, k);
        float opacite = lerp(0.45, 1.0, k);

        // Montée lente, plus le vent ; la parallaxe vient du facteur par couche.
        float2 q = p * taille + float2(fc * 13.7, fc * 5.3);
        q += float2(0.0, t * vitesse) + chemin * lerp(0.6, 1.0, k);
        // La turbulence, plus forte pendant les rafales. Ses tourbillons sont
        // LARGES (0,18) : elle DÉPLACE les bulles, elle ne doit pas les
        // déformer — à 0,35 elles passaient en ovales.
        float tourbillon = (0.2 + 0.15 * DegWarp) * (1.0 + 0.6 * rafale);
        q += tourbillon * float2(DegBruit(q * 0.18 + t * 0.3), DegBruit(q * 0.18 + float2(7.7, 3.1) - t * 0.3));

        // Parmi les quatre, la bulle la plus proche RAPPORTÉE À SON RAYON est la
        // seule dessinée. ⛔ Dessiner les quatre déroulait douze copies de
        // `DegBulle` (le relief dérive la hauteur : la boucle ne peut pas rester
        // une boucle) — 7,4 s de compilation sur le fil de l'interface. Seul se
        // perd le chevauchement de deux bulles d'une même couche, rare.
        float2 base = floor(q - 0.5);
        float2 proche = float2(9.0, 9.0);
        float hProche = 1.0;
        float iciProche = 0.0;
        for (int i = 0; i < 4; i++) {
            float2 cellule = base + float2(i & 1, i >> 1);
            float h = DegHash(cellule + fc * 31.7);
            // Une cellule sur deux environ reste vide.
            float ici = step(h, lerp(0.45, 0.6, k));
            float rayon = lerp(0.18, 0.32, DegHash(cellule + 3.1 + fc));
            // L'errance, ±0,35 cellule : deux sinus par axe, de fréquences et de
            // phases propres à chaque bulle et non commensurables — le chemin ne
            // se répète pas à l'œil. Puis un tremblement quand la rafale passe.
            // ⛔ Avec `DegBruit` (quatre par cellule, quarante-huit en tout), la
            // compilation montait à 8 s, sur le fil de l'interface.
            float4 frequence = 0.2 + 0.35 * float4(h, DegHash(cellule + 7.3), DegHash(cellule + 4.9), DegHash(cellule + 8.6));
            float4 phase = h * float4(91.0, 57.0, 23.0, 71.0);
            float4 ondes = sin(t * frequence * float4(1.0, 1.0, 1.7, 1.9) + phase);
            float2 errance = 0.35 * (0.6 * ondes.xy + 0.4 * ondes.zw);
            errance += 0.06 * rafale * sin(t * float2(9.0, 11.0) + phase.yx);
            float2 d = (q - (cellule + 0.5 + errance)) / rayon;
            bool retenue = ici > 0.0 && dot(d, d) < dot(proche, proche);
            proche = retenue ? d : proche;
            hProche = retenue ? h : hProche;
            iciProche = retenue ? ici : iciProche;
        }

        float film;
        float4 b = DegBulle(proche, t * 0.3 + hProche * 50.0, film);
        rgb = lerp(rgb, b.rgb, b.a * iciProche * opacite);
        hauteur = max(hauteur, film * iciProche * opacite);
    }

    // Les traînées : longues et fines, elles filent avec le vent et ne se
    // montrent que là où la rafale souffle.
    float trainee = DegBruit(float2(p.x * 2.0 - t * 5.0, p.y * 30.0));
    float zone = saturate(DegBruit(p * 1.5 - float2(t * 0.8, 0.0)) + 0.3);
    rgb = lerp(rgb, 1.0, smoothstep(0.35, 0.8, trainee) * zone * rafale * 0.3);
    return float4(saturate(rgb), hauteur);
}

// Le vertex shader se compile sans motif : n'importe lequel convient.
#ifndef DEG_MODE
#define DEG_MODE 0
#endif

// Écran → unités de motif, et l'INVERSE — indispensables à la marche de
// perspective, qui doit rééchantillonner le motif à des positions qu'elle
// fabrique elle-même (`DegMotif` prend des coordonnées ÉCRAN).
float2 DegEnMotif(float2 screen) {
    return (screen - DegEcran.xy) / DegEcran.w - DegOrigine;
}

float2 DegVersEcran(float2 p) {
    return (p + DegOrigine) * DegEcran.w + DegEcran.xy;
}

// **LES REMOUS — DES TOURBILLONS SEMÉS PARTOUT** (DEMANDE EXPLICITE,
// 2026-10-03 : « des remous paramétrables partout »). Une grille de cellules ;
// chaque cellule porte UN tourbillon (centre, sens, force tirés par la cellule),
// et le point est tourné autour des centres voisins, d'autant plus qu'il en est
// proche. La torsion s'applique AUX COORDONNÉES, avant l'évaluation du motif :
// c'est ce qui la rend valable pour tous les motifs sans en toucher un seul.
//
// Neuf cellules (la sienne et ses voisines) : un tourbillon déborde de sa case,
// et ne regarder que la sienne ferait des coutures aux bords. Le profil
// `(1 − d²/R²)²` s'annule au rayon, donc deux tourbillons voisins se raccordent
// sans saut — la normale du relief ne crépite pas.
float2 DegRemousTorsion(float2 p, float aspect, float t) {
    float n = lerp(1.5, 9.0, DegRemousDensite);
    float2 g = p * n;
    float2 c = floor(g);
    float rayon = DegRemousTaille;
    float2 decalage = 0.0;
    // `[loop]` et non `[unroll]` : moins de code généré, donc une compilation
    // plus courte (voir l'appel, dans `Degrade`).
    [loop] for (int j = -1; j <= 1; j++) {
        [loop] for (int i = -1; i <= 1; i++) {
            float2 cellule = c + float2(i, j);
            float h = DegHash(cellule);
            float h2 = DegHash(cellule + 17.3);
            // Le centre erre un peu dans sa case, pour que la grille ne se voie pas.
            float2 centre = cellule + 0.5 + 0.3 * float2(sin(t * 0.3 + h * 6.28), cos(t * 0.27 + h2 * 6.28));
            float2 v = g - centre;
            float d2 = dot(v, v) / (rayon * rayon);
            if (d2 < 1.0) {
                float poids = (1.0 - d2) * (1.0 - d2);
                // Sens et force par tourbillon ; l'angle tourne avec le temps.
                float sens = h < 0.5 ? -1.0 : 1.0;
                float angle = sens * poids * DegRemousForce * (2.5 + 2.0 * h2)
                            * (1.0 + 0.4 * sin(t * (0.5 + DegRemousVitesse * 3.0) + h * 6.28));
                float cs = cos(angle);
                float sn = sin(angle);
                decalage += float2(cs * v.x - sn * v.y, sn * v.x + cs * v.y) - v;
            }
        }
    }
    return p + decalage / n;
}

// **LE MOTIF DE L'UTILISATEUR** — voir `gfx::shader_utilisateur`. Ce marqueur
// est remplacé par son code (`source_utilisateur`), qui définit
// `float4 Motif(float2 p, float aspect, float t)` ; ici, APRÈS toutes les aides
// qu'il peut appeler (`DegPalette`, `DegFbm`…), AVANT `DegMotif` qui l'appelle.
//@@MOTIF_UTILISATEUR@@

float4 DegMotif(float2 screen) {
    // `DegOrigine` (voir `gfx::degrade::decalage_origine`) amène le centre du
    // motif à l'origine réglée — tous les motifs à la fois.
    float2 p = (screen - DegEcran.xy) / DegEcran.w - DegOrigine;
    float aspect = DegEcran.z / DegEcran.w;
    // La graine avance aussi la phase du temps : les motifs purement
    // géométriques (sources, centres, bandes) changent eux aussi de disposition.
    float t = DegTime * DegVitesse + DegGraine * 37.0;

    int mode = DEG_MODE;
    // **CHAQUE MOTIF REND `rgb` ET SA HAUTEUR** (voir `Degrade`, qui s'en sert
    // pour le relief). La hauteur voyage DANS la valeur de retour et non par un
    // second appel : un motif évalué deux fois ferait inliner les 72 branches du
    // `switch` à chaque site d'appel, et le dépôt a déjà mesuré ce que ça coûte
    // — la compilation passe de 3 à 60 s.
    //
    // **LES SOIXANTE ET ONZE ONT LA LEUR** — décidée motif par motif, et c'est
    // toujours la grandeur sur laquelle il est CONSTRUIT : le déplacement d'une
    // plaque pour Chladni et Résonance, la crête du bruit pour Ridé et Fumée,
    // l'épaisseur de lame pour Newton et Irisé, la distance pour les cellules et
    // les grains. Aucune n'est la luminance de la couleur : ce serait du
    // repoussage, c'est-à-dire du relief qui ne dit rien de la forme.
    float4 rgb;
    // ⛔ **PAS DE `[branch]` ICI, ET C'EST UNE CORRECTION MESURÉE.** L'attribut y
    // était depuis l'origine ; au-delà d'une soixantaine de motifs il fait
    // échouer la compilation de TOUS, avec un message qui ne dit ni la cause ni
    // l'attribut :
    //
    //   error X3528: can't use flow control on this profile
    //
    // — rapporté sur une ligne `case`, donc en accusant un motif qui n'a ni
    // boucle ni branchement (`DegChladniMotif`). Le retirer fait passer les
    // 71 motifs.
    //
    // Il était de toute façon sans objet : `DEG_MODE` est une CONSTANTE de
    // compilation posée par `pixel_source`, donc le `switch` est replié sur un
    // seul appel et il n'y a rien à aiguiller à l'exécution.
#ifdef DEG_UTILISATEUR
    rgb = Motif(p, aspect, t);
    // Un motif de l'utilisateur peut rendre n'importe quoi : un NaN se
    // propagerait par le flou à toute la barre.
    rgb = (rgb == rgb) ? rgb : float4(0.0, 0.0, 0.0, 0.0);
    return float4(saturate(rgb.rgb), rgb.a);
#endif
    switch (mode) {
        case 1: rgb = DegAurore(p, aspect, t); break;
        case 2: rgb = DegPlasma(p, aspect, t); break;
        case 3: rgb = DegBandes(p, aspect, t); break;
        case 4: rgb = DegSoie(p, aspect, t); break;
        case 5: rgb = DegVoronoi(p, aspect, t); break;
        case 6: rgb = DegCaustiques(p, aspect, t); break;
        case 7: rgb = DegSynthwave(p, aspect, t); break;
        case 8: rgb = DegTrame(screen, p, aspect, t); break;
        case 9: rgb = DegDoux(p, aspect, t); break;
        case 10: rgb = DegCannele(p, aspect, t); break;
        case 11: rgb = DegConcentrique(p, aspect, t); break;
        case 12: rgb = DegFumee(p, aspect, t); break;
        case 13: rgb = DegRayons(p, aspect, t); break;
        case 14: rgb = DegPrisme(p, aspect, t); break;
        case 15: rgb = DegVortex(p, aspect, t); break;
        case 16: rgb = DegMarbre(p, aspect, t); break;
        case 17: rgb = DegVagues(p, aspect, t); break;
        case 18: rgb = DegRuban(p, aspect, t); break;
        case 19: rgb = DegTunnel(p, aspect, t); break;
        case 20: rgb = DegKaleidoscope(p, aspect, t); break;
        case 21: rgb = DegLave(p, aspect, t); break;
        case 22: rgb = DegNebuleuse(p, aspect, t); break;
        case 23: rgb = DegConique(p, aspect, t); break;
        case 24: rgb = DegTopographie(p, aspect, t); break;
        case 25: rgb = DegPixels(screen, p, aspect, t); break;
        case 26: rgb = DegMoire(p, aspect, t); break;
        case 27: rgb = DegLueurs(p, aspect, t); break;
        case 28: rgb = DegDamier(p, aspect, t); break;
        case 29: rgb = DegOndes(p, aspect, t); break;
        case 30: rgb = DegFibres(p, aspect, t); break;
        case 31: rgb = DegIrise(p, aspect, t); break;
        case 32: rgb = DegHolo(p, aspect, t); break;
        case 33: rgb = DegBraises(p, aspect, t); break;
        case 34: rgb = DegPluie(p, aspect, t); break;
        case 35: rgb = DegAura(p, aspect, t); break;
        case 36: rgb = DegEclosion(p, aspect, t); break;
        case 37: rgb = DegDrape(p, aspect, t); break;
        case 38: rgb = DegDuo(p, aspect, t); break;
        case 39: rgb = DegSatin(p, aspect, t); break;
        case 40: rgb = DegToile(p, aspect, t); break;
        case 41: rgb = DegEcailles(p, aspect, t); break;
        case 42: rgb = DegZebre(p, aspect, t); break;
        case 43: rgb = DegCristal(p, aspect, t); break;
        case 44: rgb = DegBrume(p, aspect, t); break;
        case 45: rgb = DegSillons(p, aspect, t); break;
        case 46: rgb = DegChevron(p, aspect, t); break;
        case 47: rgb = DegEtoiles(p, aspect, t); break;
        case 48: rgb = DegReseau(p, aspect, t); break;
        case 49: rgb = DegMaree(p, aspect, t); break;
        case 50: rgb = DegConfettis(p, aspect, t); break;
        case 51: rgb = DegSpirale(p, aspect, t); break;
        case 52: rgb = DegEcho(p, aspect, t); break;
        case 53: rgb = DegPrairie(p, aspect, t); break;
        case 54: rgb = DegAmande(p, aspect, t); break;
        case 55: rgb = DegVoile(p, aspect, t); break;
        case 56: rgb = DegTisons(p, aspect, t); break;
        case 57: rgb = DegOndulation(p, aspect, t); break;
        case 58: rgb = DegVitrail(p, aspect, t); break;
        case 59: rgb = DegNeige(p, aspect, t); break;
        case 60: rgb = DegTruchet(p, aspect, t); break;
        case 61: rgb = DegLabyrinthe(p, aspect, t); break;
        case 62: rgb = DegChladniMotif(p, aspect, t); break;
        case 63: rgb = DegRosace(p, aspect, t); break;
        case 64: rgb = DegQuasicristal(p, aspect, t); break;
        case 65: rgb = DegGuilloche(p, aspect, t); break;
        case 66: rgb = DegPhyllotaxie(p, aspect, t); break;
        case 67: rgb = DegRideMotif(p, aspect, t); break;
        case 68: rgb = DegNewton(p, aspect, t); break;
        case 69: rgb = DegNidDabeille(p, aspect, t); break;
        case 70: rgb = DegResonance(p, aspect, t); break;
        case 71: rgb = DegFacetgrad(p, aspect, t); break;
        case 72: rgb = DegPlanDeau(p, aspect, t); break;
        case 73: rgb = DegRemous(p, aspect, t); break;
        case 74: rgb = DegBulles(p, aspect, t); break;
        default: rgb = DegMesh(p, aspect, t); break;
    }
    return rgb;
}

// Échantillons du flou, en plus du centre.
#define DEG_FLOU_TAPS 16

// **LA MARCHE DE PERSPECTIVE — la vraie 3D** (DEMANDE EXPLICITE : « fais le »).
//
// Le plan cesse d'être vu de face : on INCLINE le regard et on avance le long
// du rayon dans le champ de hauteur, jusqu'à passer SOUS la surface. Le point
// qu'on échantillonne se déplace alors selon la hauteur — c'est la parallaxe,
// et avec elle l'auto-occlusion et les silhouettes qui bougent : exactement ce
// que le relief seul ne peut pas donner.
//
// ⚠️ **UNE SEULE BOUCLE, DONC UN SEUL SITE D'APPEL à `DegMotif`.** Un second
// site d'appel fait inliner les 72 branches du `switch` à chaque appel, et la
// compilation passe de 3 à 60 s — le dépôt l'a mesuré. Le coût de cette marche
// est donc en EXÉCUTION, et c'est le seul endroit où il est acceptable.
//
// À `DegPerspective` = 0, la direction du regard est DROITE (0, 0, −1) : le
// décalage est nul à chaque pas, donc la sortie est celle d'avant, au bit près.
float2 DegMarchePerspective(float2 screen) {
    const int MARCHES = 16;
    float2 base = DegEnMotif(screen);
    float3 vers = normalize(float3(0.55 * DegPerspective, 0.32 * DegPerspective, -1.0));
    float epaisseur = 0.22 * DegPerspective;
    float pas = epaisseur / (float)MARCHES;
    float z = epaisseur;
    float2 q = base;
    [unroll] for (int i = 0; i < MARCHES; i++) {
        // À cette hauteur au-dessus du plan, le rayon est passé par là.
        float2 essai = base - vers.xy * (z / -vers.z);
        // La hauteur du motif à CET endroit, remise à l'échelle de l'épaisseur.
        float h = DegMotif(DegVersEcran(essai)).w * epaisseur;
        q = essai;
        if (z <= h) {
            break;
        }
        z -= pas;
    }
    return DegVersEcran(q);
}

float3 Degrade(float2 screen) {
    // Flou : moyenne sur un disque (spirale à l'angle d'or, tournée d'un angle
    // FIXE par pixel pour changer le motif des échantillons en grain plutôt
    // qu'en images fantômes). Rayon jusqu'à 15 % de la hauteur de l'écran
    // virtuel. Sans flou, un seul tour (l'échantillon 0 est le centre).
    // UN SEUL appel à `DegMotif`, dans la boucle : un second appel hors de la
    // boucle faisait passer la compilation de 3 à 60 s (FXC inline le switch
    // de tous les motifs à chaque appel).
    int taps = DegFlou > 0.001 ? DEG_FLOU_TAPS + 1 : 1;
    float rayon = DegFlou * 0.15 * DegEcran.w;
    float rotation = DitherHash(floor(screen) + 17.0) * 6.2831853;
    // **LA HAUTEUR EST FLOUTÉE EN MÊME TEMPS QUE LA COULEUR** — un canal de plus
    // sur ce que la boucle calcule DÉJÀ, aucun appel de plus. C'est ce qui rend
    // le relief abordable, et c'est aussi ce qui le rend STABLE : la normale est
    // prise sur une hauteur lissée, donc elle ne crépite pas.
    // **LA PERSPECTIVE recentre tout ce qui suit.** À 0, `centre` vaut `screen`
    // et rien ne change. Au-delà, le flou lui-même se fait AUTOUR DU POINT
    // TROUVÉ par la marche — la parallaxe et le flou restent donc compatibles,
    // au lieu de s'exclure l'un l'autre.
    // **LES REMOUS — UNE FOIS PAR PIXEL, AVANT TOUT LE RESTE** (voir
    // `DegRemousTorsion`). Ils tordent la coordonnée du pixel ; la perspective et
    // le flou partent ensuite de ce point tordu. Placés dans `DegMotif`, ils
    // étaient recopiés à chacun de ses sites d'appel, et la compilation des 74
    // shaders passait de 111 s à 190 s — MESURÉ. À force nulle, rien n'est
    // calculé : la sortie reste celle d'avant le réglage.
    // Le pixel d'ORIGINE est gardé pour le grain : il doit rester fixe, et les
    // remous, eux, bougent — un grain tordu scintillerait.
    float2 pixel = screen;
    if (DegRemousForce > 0.0) {
        float2 p = DegEnMotif(screen);
        float aspect = DegEcran.z / DegEcran.w;
        screen = DegVersEcran(DegRemousTorsion(p, aspect, DegTime * (0.2 + DegRemousVitesse) + DegGraine * 37.0));
    }
    float2 centre = DegPerspective > 0.001 ? DegMarchePerspective(screen) : screen;
    float4 acc = 0.0;
    [loop] for (int i = 0; i < taps; i++) {
        float r = rayon * sqrt((float)i / DEG_FLOU_TAPS);
        float a = i * 2.3999632 + rotation;
        acc += DegMotif(centre + r * float2(cos(a), sin(a)));
    }
    float4 moyenne = acc / (float)taps;
    float3 rgb = moyenne.rgb;

    // Étalonnage : contraste, luminosité, saturation, grain FIXE (un grain qui
    // changerait à chaque frame scintillerait).
    rgb = (rgb - 0.5) * DegContraste + 0.5;
    rgb *= DegLuminosite;
    float luma = dot(rgb, float3(0.2126, 0.7152, 0.0722));
    rgb = lerp(luma.xxx, rgb, DegSaturation);
    rgb += (DitherHash(floor(pixel)) - 0.5) * DegGrain;

    // **LE RELIEF** (DEMANDE EXPLICITE : « ajoute de la 3d dans les shaders »).
    //
    // On ne raymarche RIEN : la normale vient de la DÉRIVÉE ÉCRAN de la hauteur
    // floutée, ce qui coûte quelques instructions au lieu des dizaines de pas
    // par pixel d'un vrai rendu volumétrique — hors de prix ici, où le motif est
    // déjà évalué dix-sept fois par pixel et jusqu'à 144 fois par seconde.
    //
    // Le MODÈLE D'ÉCLAIRAGE, lui, vient du shader de raymarching fourni par
    // l'utilisateur : trois termes, et c'est le troisième qui fait le « 3D ».
    //   - diffus : la lumière qui frappe la pente ;
    //   - FRESNEL : les bords rasants s'allument — c'est lui qui détache la
    //     forme de son fond, et sans lui un bas-relief reste plat ;
    //   - SPÉCULAIRE : un reflet blanc là où la pente fait miroir.
    // `emission` n'est pas repris : sur un dégradé, la couleur EST déjà
    // l'émission, et l'ajouter aurait juste éclairci l'ensemble.
    //
    // `DegEcran.w` ramène la dérivée en unités de motif : sans lui, l'effet
    // dépendrait de la résolution, et un même réglage ne donnerait pas le même
    // relief sur deux écrans.
    //
    // À `DegRelief` = 0, la sortie est EXACTEMENT celle d'avant.
    if (DegRelief > 0.001) {
        // **MICRO-RELIEF — pris du shader de l'utilisateur.** Le sien ajoute
        // `(fbm(q * …) - 0.5) * 0.12` à sa distance pour donner de la MATIÈRE à
        // sa surface. Même idée ici, appliquée à la hauteur : quelques dixièmes
        // de grain, et le relief cesse d'être une surface lisse pour devenir une
        // matière.
        //
        // ⚠️ Ajouté APRÈS le flou, donc à PLEINE RÉSOLUTION : c'est ce qui le
        // fait survivre au lissage au lieu d'être moyenné avec le reste — et
        // c'est précisément ce que la hauteur floutée ne peut pas porter.
        float matiere = (DegFbm(screen * 0.30 / max(0.2, DegEchelle)) - 0.5) * 0.15;
        float hauteur = moyenne.w + matiere;
        float3 normale = normalize(float3(-ddx(hauteur) * DegEcran.w, -ddy(hauteur) * DegEcran.w, 1.0 / max(0.05, DegRelief * 2.0)));
        // **LA LUMIÈRE DÉRIVE** — celle de l'utilisateur orbite autour de sa
        // scène (`2.4 * sin(t * 0.75)`). C'est ce qui vend la profondeur : une
        // surface immobile sous une lumière immobile reste une image.
        //
        // Elle suit `DegVitesse`, donc à vitesse NULLE elle est fixe — un
        // dégradé arrêté ne doit pas s'agiter.
        float angle = DegLumiere * 6.2831853 + DegTime * DegVitesse * 0.35;
        float3 vers = normalize(float3(cos(angle), sin(angle), 0.65));
        // Le relief est vu DE FACE : ce n'est pas une scène, c'est une surface
        // bombée sous nos yeux. La vue est donc la normale au plan, et
        // `normale.z` joue directement le rôle de `dot(n, vue)`.
        float3 vue = float3(0.0, 0.0, 1.0);
        float3 demi = normalize(vers + vue);
        float diffus = saturate(dot(normale, vers));
        float fresnel = pow(1.0 - saturate(normale.z), 3.4);
        float speculaire = pow(saturate(dot(normale, demi)), 64.0);
        float3 relief = rgb * (0.18 + 1.25 * diffus) + rgb * fresnel * 1.2 + 0.9 * speculaire;
        rgb = lerp(rgb, saturate(relief), DegRelief);
    }
    return saturate(rgb);
}

// La distance NORMALISÉE d'un pixel ÉCRAN à la lueur `li` (0 au cœur, 1 au bord),
// dans le repère tourné de son angle : cercle, rectangle, segment (une gélule),
// droite. Voir `gfx::lueur::Figure`.
float LueurDistance(int li, float2 ecran) {
    float4 disque = LueurDisques[li];
    float4 forme = LueurFormes[li];
    float2 v = ecran - disque.xy;
    float sn, cs;
    sincos(forme.y, sn, cs);
    float2 q = float2(cs * v.x + sn * v.y, -sn * v.x + cs * v.y);
    if (forme.x < 0.5) return length(v) / disque.z;
    // Le rectangle, et le cadre d'une IMAGE (code 4).
    if (forme.x < 1.5 || forme.x > 3.5) return max(abs(q.x) / forme.z, abs(q.y) / disque.z);
    if (forme.x < 2.5) return length(float2(max(abs(q.x) - forme.z, 0.0), q.y)) / disque.z;
    return abs(q.y) / disque.z;
}

// Le pixel ÉCRAN dans le repère TOURNÉ de la lueur `li` (x le long de son grand axe).
float2 LueurRepere(int li, float2 ecran) {
    float2 v = ecran - LueurDisques[li].xy;
    float sn, cs;
    sincos(LueurFormes[li].y, sn, cs);
    return float2(cs * v.x + sn * v.y, -sn * v.x + cs * v.y);
}

// L'image d'une lueur de figure Image : PRÉMULTIPLIÉE, floutée par les mips (le
// `flou` choisit le niveau). Hors de son cadre, ou sans image : transparente.
float4 LueurImage(int li, float2 ecran) {
    float tranche = LueurDegrades[li].z;
    if (tranche < -0.5) return 0.0;
    float2 q = LueurRepere(li, ecran);
    float2 uv = q / float2(LueurFormes[li].z, LueurDisques[li].z) * 0.5 + 0.5;
    if (any(uv < 0.0) || any(uv > 1.0)) return 0.0;
    return LueurImages.SampleLevel(IconSampler, float3(uv, tranche), LueurTeintes[li].w * 7.0);
}

// Le POIDS de la lueur `li` au pixel : 1 au cœur, 0 au bord, fondu selon son `flou`
// — l'alpha de son image pour une figure Image.
float LueurPoids(int li, float2 ecran) {
    if (LueurFormes[li].x > 3.5) return LueurImage(li, ecran).a;
    float debut = 1.0 - max(LueurTeintes[li].w, 0.001);
    return 1.0 - smoothstep(debut, 1.0, LueurDistance(li, ecran));
}

// `c` dont la TEINTE a tourné de `tours` (la même roue que `gfx::lueur::
// tourner_la_teinte` : la teinte HSV est celle de HSL).
float3 LueurTourneTeinte(float3 c, float tours) {
    float4 K = float4(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    float4 p = lerp(float4(c.bg, K.wz), float4(c.gb, K.xy), step(c.b, c.g));
    float4 q = lerp(float4(p.xyw, c.r), float4(c.r, p.yzx), step(p.x, c.r));
    float d = q.x - min(q.w, q.y);
    float h = abs(q.z + (q.w - q.y) / (6.0 * d + 1e-10));
    float s = d / (q.x + 1e-10);
    float3 roue = abs(frac(h + tours + float3(1.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0);
    return q.x * lerp(1.0, saturate(roue - 1.0), s);
}

// La couleur de l'arrêt `k` de la lueur `li` — 0 : sa couleur principale.
float3 LueurArret(int li, int k) {
    return k == 0 ? LueurTeintes[li].rgb : LueurArrets[li * 4 + k - 1].rgb;
}

// ⭐ LA COULEUR de la lueur `li` au pixel : la sienne, son DÉGRADÉ (voir
// `gfx::lueur::Degrade`), ou celle de son image.
float3 LueurCouleur(int li, float2 ecran) {
    float4 forme = LueurFormes[li];
    if (forme.x > 3.5) {
        float4 s = LueurImage(li, ecran);
        float3 c = s.rgb / max(s.a, 1e-4);
        // Sa teinte qui tourne (voir `gfx::lueur::Element::teinte`).
        return forme.w != 0.0 ? LueurTourneTeinte(c, forme.w) : c;
    }
    float4 dg = LueurDegrades[li];
    if (dg.x < 0.5) return LueurTeintes[li].rgb;
    float2 q = LueurRepere(li, ecran);
    float rayon = LueurDisques[li].z;
    float u;
    if (dg.x < 1.5) {
        // Le long du grand axe : du bout gauche au bout droit de la figure. Une
        // droite n'a pas de bout : AU TRAVERS d'elle.
        float demi = forme.x < 0.5 ? rayon : (forme.x < 1.5 ? forme.z : forme.z + rayon);
        u = forme.x > 2.5 ? q.y / rayon * 0.5 + 0.5 : q.x / max(demi, 1.0) * 0.5 + 0.5;
    } else if (dg.x < 2.5) {
        u = LueurDistance(li, ecran);
    } else {
        u = atan2(q.y, q.x) / 6.2831853 + 0.5;
    }
    int n = (int)dg.y + 1;
    // L'angulaire BOUCLE : après la dernière couleur, il revient à la première.
    // Un dégradé qui DÉFILE aussi (`dg.w` : sa phase, -1 s'il est immobile) — sans
    // quoi ses couleurs sortiraient de la forme sans revenir.
    bool boucle = dg.x > 2.5 || dg.w > -0.5;
    if (dg.w > -0.5) u = frac(saturate(u) + dg.w);
    float pos = saturate(u) * (boucle ? n : n - 1);
    int k = min((int)pos, boucle ? n - 1 : n - 2);
    float3 a = LueurArret(li, k);
    float3 b = LueurArret(li, boucle && k == n - 1 ? 0 : k + 1);
    return lerp(a, b, smoothstep(0.0, 1.0, pos - k));
}

// ⭐ LE FONDU EN HAUT DE LA COLONNE DE GAUCHE (voir `ui::dock::bande_de_fondu`) :
// 0 contre la barre du haut, 1 à `FonduBande.w` pixels plus bas. Hors de la bande
// — au-dessus compris, où vit la barre du haut —, 1.
float FonduDansBande(float4 bande, float2 p) {
    if (bande.w <= 0.0) return 1.0;
    if (p.x < bande.x || p.x > bande.x + bande.z || p.y < bande.y) return 1.0;
    return smoothstep(bande.y, bande.y + bande.w, p.y);
}

// Les deux colonnes ont chacune SA bande (gauche, droite) : elles ne se
// chevauchent pas, le produit vaut celle qui contient le pixel.
float FonduVisibilite(float2 p) {
    return FonduDansBande(FonduBande, p) * FonduDansBande(FonduBandeDroite, p);
}


// La texture FLOUTÉE sur `rayon` (en UV) : seize prises sur deux cercles, plus le
// centre, moyennées en alpha PRÉMULTIPLIÉ — sans quoi les texels transparents
// (RGB noir) assombriraient le bord de l'icône.
float4 EchantillonFlou(float2 uv, float rayon) {
    float4 centre = IconTex.Sample(IconSampler, uv);
    if (rayon <= 0.0005) return centre;
    float4 somme = float4(centre.rgb * centre.a, centre.a);
    [unroll] for (int k = 0; k < 16; k++) {
        float angle = k * 0.39269908;
        float r = (k & 1) ? rayon : rayon * 0.5;
        float4 s = IconTex.Sample(IconSampler, uv + float2(cos(angle), sin(angle)) * r);
        somme += float4(s.rgb * s.a, s.a);
    }
    somme /= 17.0;
    return float4(somme.rgb / max(somme.a, 1e-4), somme.a);
}

// LA TEXTURE DE L'INTERFACE (voir `gfx::texture_interface`) : une variation de
// luminosité dans [-1, 1], fonction du pixel ÉCRAN — la même matière sous toutes
// les fenêtres. Bruit de valeur et fbm maison : quelques `frac`/`sin`, aucune
// image. ⚠️ L'index `n` est celui de `texture_interface::NATURES`.
float TexHash(float2 p) {
    return frac(sin(dot(p, float2(127.1, 311.7))) * 43758.5453);
}
float TexValeur(float2 p) {
    float2 i = floor(p);
    float2 f = frac(p);
    float2 u = f * f * (3.0 - 2.0 * f);
    return lerp(lerp(TexHash(i), TexHash(i + float2(1.0, 0.0)), u.x),
                lerp(TexHash(i + float2(0.0, 1.0)), TexHash(i + float2(1.0, 1.0)), u.x), u.y);
}
float TexFbm(float2 p) {
    float somme = 0.0;
    float poids = 0.5;
    [unroll] for (int o = 0; o < 4; o++) {
        somme += poids * TexValeur(p);
        p = p * 2.03 + 17.0;
        poids *= 0.5;
    }
    return somme / 0.9375;
}
// LA TEXTURE PERSONNALISÉE (voir `gfx::texture_perso`) : un bruit de base
// (`TexBruit`, dans [0, 1] par canal), puis chaque fonction cochée dans l'applet
// Textures.
// ⛔ **UN SEUL SITE D'APPEL DE `TexBruit`, DANS UN `[loop]`** : FXC inline tout,
// et chaque copie du bruit (les cellules surtout) multiplie le temps de
// compilation — mesuré sur le test de compilation des 72 motifs : 197 s sans
// cette texture, 223 s avec trois sites (distorsion ×2 + fractal), 200 s avec un
// seul. La distorsion prend donc les DEUX PREMIERS tours de la boucle du fractal,
// et le FLOU répète la boucle entière, une PRISE après l'autre, dans la même.
// Pour la même raison, les natures de l'interface (grain, papier…) SONT des
// bruits de base : `TextureFond` les fait passer par ici, sans aucun réglage.
float TexMod(float x, float y) {
    return x - y * floor(x / y);
}
float TexPerlinPente(float2 i, float2 f) {
    float a = TexHash(i) * 6.2831853;
    return dot(float2(cos(a), sin(a)), f);
}
float TexPerlin(float2 p) {
    float2 i = floor(p);
    float2 f = frac(p);
    float2 u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    float n = lerp(lerp(TexPerlinPente(i, f), TexPerlinPente(i + float2(1.0, 0.0), f - float2(1.0, 0.0)), u.x),
                   lerp(TexPerlinPente(i + float2(0.0, 1.0), f - float2(0.0, 1.0)), TexPerlinPente(i + float2(1.0, 1.0), f - float2(1.0, 1.0)), u.x), u.y);
    return saturate(n * 0.7071 + 0.5);
}
// Simplexe : trois sommets au lieu de quatre, sans l'aspect quadrillé du Perlin.
float TexSimplexe(float2 p) {
    float2 i = floor(p + (p.x + p.y) * 0.36602540);
    float2 x0 = p - i + (i.x + i.y) * 0.21132487;
    float2 o = x0.x > x0.y ? float2(1.0, 0.0) : float2(0.0, 1.0);
    float2 x1 = x0 - o + 0.21132487;
    float2 x2 = x0 - 1.0 + 0.42264973;
    float3 t = max(0.5 - float3(dot(x0, x0), dot(x1, x1), dot(x2, x2)), 0.0);
    t = t * t;
    t = t * t;
    float3 g = float3(TexPerlinPente(i, x0), TexPerlinPente(i + o, x1), TexPerlinPente(i + 1.0, x2));
    return saturate(0.5 + 35.0 * dot(t, g));
}
// Les CELLULES (Worley) : la distance au germe le plus proche (x), au second (y),
// et un tirage propre à la cellule la plus proche (z) — cellules, fissures,
// vitrail et ondulations en sont tirés, d'UN appel. Les germes tournent avec la
// phase quand l'animation est cochée.
float3 TexWorley(float2 p, float phase) {
    float2 i = floor(p);
    float2 f = frac(p);
    float d1 = 8.0;
    float d2 = 8.0;
    float id = 0.0;
    [loop] for (int y = -1; y <= 1; y++) {
        [loop] for (int x = -1; x <= 1; x++) {
            float2 o = float2(x, y);
            float2 h = float2(TexHash(i + o), TexHash(i + o + 19.19));
            h = 0.5 + 0.5 * sin(phase + 6.2831853 * h);
            float d = length(o + h - f);
            if (d < d1) {
                d2 = d1;
                d1 = d;
                id = TexHash(i + o + 7.7);
            } else if (d < d2) {
                d2 = d;
            }
        }
    }
    return float3(d1, d2, id);
}
// Les NATURES de l'interface (`texture_interface::NATURES`, index `n`), dans
// [-1, 1] : `p` en pixels à 100 %. Le grain et le sable tirent un pixel par
// `cellule` (1 sans flou, voir `TexPersoFond`).
float TexNatureFond(float2 p, int n, float cellule) {
    float grain = TexHash(floor(p / cellule)) - 0.5;
    // Grain : un tirage par pixel, comme une pellicule.
    if (n == 1) return grain * 2.0;
    // Papier : des nappes de fibres (fbm) et un grain fin.
    if (n == 2) return (TexFbm(p * 0.06) - 0.5) * 1.8 + grain * 0.6;
    // Toile : un tissage serré (fils croisés) et des irrégularités.
    if (n == 3) {
        float2 q = p * (3.14159 / 3.0);
        float fils = sin(q.x) * sin(q.y);
        return fils * 0.8 + (TexValeur(p * 0.3) - 0.5) * 0.8;
    }
    // Lin : des fibres horizontales, longues et fines.
    if (n == 4) return (TexValeur(float2(p.x * 0.015, p.y * 0.7)) - 0.5) * 1.8 + grain * 0.4;
    // Trame : une grille de points, comme une impression en demi-teinte.
    if (n == 5) {
        float2 c = frac(p / 6.0) - 0.5;
        float rond = 1.0 - smoothstep(0.18, 0.26, length(c));
        return (rond - 0.15) * 1.6;
    }
    // Sable : des grains épars, clairs ou sombres, sur un grain très fin.
    if (n == 6) {
        float2 g = floor(p / cellule);
        float tirage = TexHash(g + 3.7);
        float signe = TexHash(g + 11.3) > 0.5 ? 1.0 : -1.0;
        return (tirage > 0.92 ? signe * 1.4 : 0.0) + grain * 0.3;
    }
    // Nuages : de grandes nappes douces.
    if (n == 7) return (TexFbm(p * 0.006) - 0.5) * 2.2;
    return 0.0;
}
// Les MOTIFS GÉOMÉTRIQUES, dans [0, 1] : une maille de 50 pixels à 100 %.
float TexGeometrie(float2 p, int base, float phase) {
    // Damier.
    if (base == 15) return TexMod(floor(p.x) + floor(p.y), 2.0);
    // Rayures.
    if (base == 16) return step(0.5, frac(p.x));
    // Pois.
    if (base == 17) return 1.0 - smoothstep(0.22, 0.3, length(frac(p) - 0.5));
    // Hexagones : clairs sur leurs arêtes.
    if (base == 18) {
        float2 r = float2(1.0, 1.7320508);
        float2 a = float2(TexMod(p.x, r.x), TexMod(p.y, r.y)) - r * 0.5;
        float2 b = float2(TexMod(p.x - 0.5, r.x), TexMod(p.y - r.y * 0.5, r.y)) - r * 0.5;
        float2 g = abs(dot(a, a) < dot(b, b) ? a : b);
        return smoothstep(0.40, 0.48, max(dot(g, float2(0.5, 0.8660254)), g.x));
    }
    // Briques : un rang sur deux décalé, des joints sombres, chaque brique sa nuance.
    if (base == 19) {
        float2 q = p * float2(1.0, 2.0);
        q.x += 0.5 * TexMod(floor(q.y), 2.0);
        float2 f = frac(q);
        float joint = min(min(f.x, 1.0 - f.x), min(f.y, 1.0 - f.y) * 0.5);
        return smoothstep(0.02, 0.05, joint) * (0.65 + 0.35 * TexHash(floor(q)));
    }
    // Moiré : deux jeux d'anneaux qui interfèrent.
    if (base == 21) {
        float2 c = float2(1.3, 0.7) + 0.3 * float2(cos(phase), sin(phase));
        return 0.5 + 0.25 * (sin(length(p) * 12.0) + sin(length(p - c) * 12.0));
    }
    // Écailles : des arcs en quinconce, ombrés vers leur bord.
    float2 q = p * float2(1.0, 2.0);
    q.x += 0.5 * TexMod(floor(q.y), 2.0);
    float2 f = (frac(q) - float2(0.5, 0.0)) * float2(1.0, 0.5);
    float d = length(f) * 2.0;
    return saturate(1.0 - d) * 0.8 + smoothstep(0.08, 0.0, abs(d - 1.0)) * 0.6;
}
// Les MATIÈRES (bases 23 à 32, sauf le cuir et l'eau, tirés des cellules), dans
// [0, 1] — le plasma est en COULEUR, comme le brouillage RVB.
float3 TexMatiere(float2 p, int base, float phase) {
    // Bois : des cernes serrés, tordus par un bruit étiré dans le fil.
    if (base == 23) {
        float cerne = frac(p.y * 2.0 + TexValeur(p * float2(0.35, 1.4)) * 2.5);
        return (0.35 + 0.65 * cerne * cerne * (3.0 - 2.0 * cerne) * (0.8 + 0.2 * TexValeur(p * float2(0.2, 9.0)))).xxx;
    }
    // Marbre : des veines fines et sombres, tordues par plusieurs octaves.
    if (base == 24) return (0.2 + 0.8 * sqrt(abs(sin(p.x * 2.0 + TexFbm(p * 0.7) * 8.0)))).xxx;
    // Granit : des grains clairs et sombres serrés sur une nappe.
    if (base == 25) {
        float g = TexHash(floor(p * 14.0));
        float tache = g > 0.88 ? 1.0 : (g < 0.18 ? 0.0 : 0.5);
        return (tache * 0.65 + TexValeur(p * 2.5) * 0.35).xxx;
    }
    // Métal brossé : de longues stries horizontales et un grain fin.
    if (base == 26) {
        float strie = TexValeur(float2(p.x * 0.08, p.y * 24.0));
        return saturate(0.5 + (strie - 0.5) * 0.9 + (TexHash(floor(p * float2(3.0, 60.0))) - 0.5) * 0.25).xxx;
    }
    // Fibre de carbone : des mèches croisées, un carré sur deux dans l'autre sens.
    if (base == 27) {
        // Sergé 2/2 : les mèches changent de sens par paires décalées, d'où les diagonales.
        float2 q = p * 6.0;
        float2 f = frac(q);
        bool trame = TexMod(floor(q.x) + floor(q.y), 4.0) < 1.5;
        float meche = trame ? f.y : f.x;
        float reflet = trame ? f.x : f.y;
        return (0.08 + 0.55 * sin(3.14159 * meche) * (0.55 + 0.45 * sin(3.14159 * reflet))).xxx;
    }
    // Plasma : des sinus mêlés, colorés en arc-en-ciel.
    if (base == 30) {
        float v = sin(p.x * 1.3 + phase) + sin((p.y + phase) * 0.9) + sin((p.x + p.y) * 0.7 + phase * 0.5)
                + sin(length(p - float2(sin(phase * 0.3), cos(phase * 0.5)) * 3.0) * 1.2);
        return 0.5 + 0.5 * sin(v * 1.6 + float3(0.0, 2.094, 4.189));
    }
    // Tissage : des brins larges, alternés comme un panier.
    if (base == 31) {
        float2 q = p * 3.0;
        float2 f = frac(q);
        bool horizontal = TexMod(floor(q.x) + floor(q.y), 2.0) > 0.5;
        float travers = horizontal ? f.y : f.x;
        float long_ = horizontal ? f.x : f.y;
        return (0.15 + 0.75 * sin(3.14159 * travers) * (0.75 + 0.25 * sin(3.14159 * long_))).xxx;
    }
    // Truchet : des quarts de cercle, tournés au hasard dans chaque case.
    float2 c = floor(p * 2.0);
    float2 f = frac(p * 2.0);
    if (TexHash(c + 5.3) > 0.5) f.x = 1.0 - f.x;
    float d = min(abs(length(f) - 0.5), abs(length(f - 1.0) - 0.5));
    return (1.0 - smoothstep(0.07, 0.11, d)).xxx;
}
// MOTIFS (bases 33 à 42), dans [0, 1]. Pas de `fwidth` ici : la fonction est
// appelée sous `[branch]`, une dérivée y serait refusée par FXC.
float3 TexMotif(float2 p, int base, float phase) {
    // Topographie : les courbes de niveau d'un relief fractal.
    if (base == 33) {
        float niveau = abs(frac(TexFbm(p * 0.5) * 12.0 + phase * 0.05) - 0.5);
        return (0.25 + 0.65 * smoothstep(0.43, 0.49, niveau)).xxx;
    }
    // Zébrures : des bandes franches, tordues par un bruit.
    if (base == 34) {
        float bande = sin(p.x * 4.0 + TexFbm(p * 0.5) * 5.0 + phase);
        return (0.1 + 0.8 * smoothstep(-0.2, 0.2, bande)).xxx;
    }
    // Labyrinthe : une diagonale tirée au sort par case.
    if (base == 35) {
        float2 f = frac(p * 2.0);
        if (TexHash(floor(p * 2.0) + 11.7) > 0.5) f.x = 1.0 - f.x;
        return (0.15 + 0.75 * (1.0 - smoothstep(0.08, 0.14, abs(f.x - f.y)))).xxx;
    }
    // Chevrons : des bandes en zigzag.
    if (base == 36) {
        float2 q = p * 2.0;
        float bande = abs(frac(q.y + abs(frac(q.x) - 0.5) + phase * 0.1) - 0.5) * 2.0;
        return (0.2 + 0.65 * smoothstep(0.4, 0.6, bande)).xxx;
    }
    // Spirales : une spirale à deux bras par case, fondue sur ses bords.
    if (base == 37) {
        float2 f = frac(p * 0.5) - 0.5;
        float r = length(f);
        float bras = 0.5 + 0.5 * sin(2.0 * atan2(f.y, f.x) - r * 40.0 + phase * 3.0);
        return (0.3 + 0.6 * bras * smoothstep(0.5, 0.4, r)).xxx;
    }
    // Étoiles : une étoile au hasard dans certaines cases, qui scintille, sur un voile.
    if (base == 38) {
        float2 c = floor(p * 1.5);
        float2 f = frac(p * 1.5);
        float tirage = TexHash(c);
        // Centre dans [0,3 ; 0,7] et rayon ≤ 0,3 : l'éclat ne déborde jamais de sa case (sinon des carrés).
        float2 centre = float2(TexHash(c + 3.1), TexHash(c + 7.7)) * 0.4 + 0.3;
        float rayon = length(f - centre) / (tirage > 0.85 ? 0.3 : 0.14);
        float eclat = pow(saturate(1.0 - rayon), 2.5)
                    * step(0.4, tirage) * (0.6 + 0.4 * sin(phase * 3.0 + tirage * 40.0));
        return saturate(0.04 + 0.12 * TexFbm(p * 0.3) + eclat).xxx;
    }
    // Bulles : deux couches de cercles, de tailles tirées au sort, qui montent.
    if (base == 39) {
        float2 q = p * 1.5 + float2(0.0, phase * 0.2);
        float2 c = floor(q);
        float d = abs(length(frac(q) - 0.5) - (0.15 + 0.25 * TexHash(c + 2.3)));
        float2 q2 = p * 3.0 + float2(0.5, phase * 0.35);
        float2 c2 = floor(q2);
        float d2 = abs(length(frac(q2) - 0.5) - (0.1 + 0.3 * TexHash(c2 + 8.1)));
        float bulle = max(1.0 - smoothstep(0.0, 0.04, d), 0.6 * (1.0 - smoothstep(0.0, 0.06, d2)));
        return (0.15 + 0.75 * bulle).xxx;
    }
    // Circuit : des pistes droites et des pastilles, comme une carte électronique.
    if (base == 40) {
        float2 c = floor(p * 3.0);
        float2 f = frac(p * 3.0);
        float piste = 1.0 - smoothstep(0.05, 0.08, TexHash(c + 1.9) > 0.5 ? abs(f.y - 0.5) : abs(f.x - 0.5));
        float pastille = step(0.7, TexHash(c + 4.4)) * (1.0 - smoothstep(0.04, 0.07, abs(length(f - 0.5) - 0.15)));
        return (0.12 + 0.7 * max(piste, pastille)).xxx;
    }
    // Papier millimétré : un trait fort par case, quatre fins entre deux.
    if (base == 41) {
        float2 f = frac(p);
        float2 g = frac(p * 5.0);
        float fort = min(min(f.x, 1.0 - f.x), min(f.y, 1.0 - f.y));
        float fin = min(min(g.x, 1.0 - g.x), min(g.y, 1.0 - g.y));
        return (0.2 + 0.7 * max(1.0 - smoothstep(0.015, 0.03, fort), 0.45 * (1.0 - smoothstep(0.03, 0.07, fin)))).xxx;
    }
    // Losanges : un damier tourné d'un quart d'angle droit, rayé en travers (écossais).
    float2 r = float2(p.x + p.y, p.x - p.y) * 0.7071 * 1.5;
    float2 f = frac(r);
    float teinte = TexMod(floor(r.x) + floor(r.y), 2.0) > 0.5 ? 0.35 : 0.6;
    float trait = 1.0 - smoothstep(0.02, 0.04, min(abs(f.x - 0.5), abs(f.y - 0.5)));
    return lerp(teinte, 0.9, trait * 0.6).xxx;
}
// LE BRUIT DE BASE — l'index est celui de `texture_perso::BASES`. Le brouillage
// RVB est le SEUL en couleur : un tirage par canal, à un pixel par `cellule`.
float3 TexBruit(float2 p, int base, float phase, float cellule) {
    [branch] if (base >= 4 && base <= 10) return (0.5 + 0.5 * TexNatureFond(p * 50.0, base - 3, cellule)).xxx;
    [branch] if (base == 11) {
        float2 q = p * 50.0 / cellule - 0.5;
        return float3(TexValeur(q), TexValeur(q + 31.7), TexValeur(q + 67.3));
    }
    [branch] if (base == 2 || base == 13 || base == 14 || base == 20 || base == 28 || base == 29) {
        float2 pw = base == 28 ? p * 2.0 : p;
        float3 w = TexWorley(pw, phase);
        // Cuir : des sillons entre des bosses arrondies.
        if (base == 28) return saturate(saturate((w.y - w.x) * 6.0) * 0.7 + 0.3 * (1.0 - w.x)).xxx;
        // Eau : les caustiques, des filets de lumière là où deux cellules se touchent.
        if (base == 29) return (0.15 + 0.85 * pow(saturate(1.0 - (w.y - w.x) * 2.5), 6.0)).xxx;
        // Fissures : les arêtes entre cellules.
        if (base == 13) return saturate((w.y - w.x) * 2.5).xxx;
        // Vitrail : chaque cellule d'une seule nuance.
        if (base == 14) return w.zzz;
        // Ondulations : des ronds dans l'eau autour de chaque germe.
        if (base == 20) return (0.5 + 0.5 * sin(w.x * 18.0 - phase * 4.0)).xxx;
        return saturate(w.x).xxx;
    }
    [branch] if (base == 1) return TexPerlin(p).xxx;
    [branch] if (base == 12) return TexSimplexe(p).xxx;
    [branch] if (base >= 33) return TexMotif(p, base, phase);
    [branch] if (base >= 23) return TexMatiere(p, base, phase);
    [branch] if (base >= 15) return TexGeometrie(p, base, phase).xxx;
    if (base == 3) return (0.5 + 0.25 * (sin(p.x * 1.7 + p.y * 0.3 + phase) + sin(p.y * 1.3 - p.x * 0.6 + 1.7 - phase))).xxx;
    return TexValeur(p).xxx;
}
// ⭐ LE FLOU (`pp_flou`, en pixels) : la MOYENNE du motif ENTIER — fractal,
// veines, postérisation comprises — sur un disque de ce rayon, en
// `TEX_PRISES_FLOU` prises en spirale (Vogel), tournée d'un angle tiré par pixel
// (sans quoi le disque laisse des bandes). Un tirage au pixel (grain, sable,
// brouillage) passe alors à un tirage par `cellule` de la taille du flou : la
// moyenne de douze pixels au hasard serait encore un grain, à peine plus faible,
// alors qu'un bruit flouté est une nappe douce.
static const int TEX_PRISES_FLOU = 12;
float3 TexPersoFond(float2 ecran, float4 P[9]) {
    float pp_base = P[0].x;
    float pp_frequence = P[0].y;
    float pp_fractal = P[0].z;
    float pp_octaves = P[0].w;
    float pp_lacunarite = P[1].x;
    float pp_gain = P[1].y;
    float pp_cretes = P[1].z;
    float pp_turbulence = P[1].w;
    float pp_distorsion = P[2].x;
    float pp_distorsion_force = P[2].y;
    float pp_distorsion_frequence = P[2].z;
    float pp_angle = P[2].w;
    float pp_etirement = P[3].x;
    float pp_veines = P[3].y;
    float pp_veines_frequence = P[3].z;
    float pp_posterisation = P[3].w;
    float pp_niveaux = P[4].x;
    float pp_contraste = P[4].y;
    float pp_inverser = P[4].z;
    float pp_evoluer = P[4].w;
    float pp_vitesse = P[5].x;
    float pp_flou = P[5].y;
    float pp_bicolore = P[5].z;
    float pp_seconde_teinte = P[5].w;
    float pp_bicolore_force = P[6].x;
    float pp_relief = P[6].y;
    float pp_relief_force = P[6].z;
    float pp_relief_distance = P[6].w;
    float pp_pixeliser = P[7].x;
    float pp_pixel_taille = P[7].y;
    float pp_symetrie = P[7].z;
    float pp_branches = P[7].w;
    float phase = P[8].w;
    int base = (int)(pp_base + 0.5);
    float echelle = max(TexEchelle, 0.1);
    float a = pp_angle * 0.0174533;
    float etirement = max(pp_etirement, 0.05);
    // ⭐ LE RELIEF prend DEUX prises — le point, et son voisin vers le bas à
    // droite — à la place de celles du flou : leur différence est la pente.
    bool relief = pp_relief > 0.5;
    int prises = relief ? 2 : (pp_flou > 0.25 ? TEX_PRISES_FLOU : 1);
    float flou = relief ? 0.0 : pp_flou;
    // Un tirage au pixel ne descend pas sous la taille du flou (voir plus haut),
    // en pixels du MOTIF (fréquence et échelle comprises).
    float cellule = max(1.0, flou * pp_frequence * 50.0 / echelle);
    // ⭐ PIXELISER : tout le carré prend la valeur de son centre.
    if (pp_pixeliser > 0.5) {
        float taille = max(pp_pixel_taille, 1.0);
        ecran = (floor(ecran / taille) + 0.5) * taille;
    }
    // ⭐ KALÉIDOSCOPE : l'angle autour du centre de l'écran est replié en
    // `branches` secteurs, chacun le miroir de son voisin.
    if (pp_symetrie > 0.5) {
        float2 centre = ViewportSize * 0.5;
        float2 d = ecran - centre;
        float secteur = 6.2831853 / max(round(pp_branches), 2.0);
        float angle = abs(TexMod(atan2(d.y, d.x), secteur) - secteur * 0.5);
        ecran = centre + length(d) * float2(cos(angle), sin(angle));
    }
    float3 premiere = 0.0;
    float tourne = TexHash(floor(ecran) + 0.31) * 6.2831853;
    // Par prise : les tours 0 et 1 font la DISTORSION (le bruit décale ses
    // propres coordonnées), puis le FRACTAL — des octaves de plus en plus fines,
    // que crêtes et turbulence replient.
    int debut = pp_distorsion > 0.5 ? 2 : 0;
    int parPrise = debut + (pp_fractal > 0.5 ? (int)(pp_octaves + 0.5) : 1);
    int fin = prises * parPrise;
    float3 cumul = 0.0;
    float3 somme = 0.0;
    float total = 0.0;
    float poids = 1.0;
    float2 p = 0.0;
    float2 q = 0.0;
    float decalage = 0.0;
    int prise = 0;
    int o = 0;
    [loop] for (int k = 0; k < 160; k++) {
        if (k >= fin) break;
        if (o == 0) {
            // Le point de la prise sur le disque, puis angle, étirement, fréquence.
            float2 e = ecran;
            if (relief) {
                e += prise * max(pp_relief_distance, 0.25) * float2(0.7071, 0.7071);
            } else if (prises > 1) {
                float t = prise * 2.3999632 + tourne;
                e += flou * sqrt((prise + 0.5) / prises) * float2(cos(t), sin(t));
            }
            p = e / echelle;
            p = float2(cos(a) * p.x + sin(a) * p.y, -sin(a) * p.x + cos(a) * p.y);
            p.x /= etirement;
            p *= pp_frequence;
            if (pp_evoluer > 0.5) p += float2(phase, phase * 0.6);
            q = p;
            somme = 0.0;
            total = 0.0;
            poids = 1.0;
        }
        float2 ou = o < debut ? p * pp_distorsion_frequence + o * 5.2 : q;
        float3 n = TexBruit(ou, base, phase, cellule);
        if (o < debut) {
            if (o == 0) {
                decalage = n.x;
            } else {
                p += (float2(decalage, n.x) - 0.5) * 2.0 * pp_distorsion_force;
                q = p;
            }
        } else {
            if (pp_cretes > 0.5) n = 1.0 - abs(2.0 * n - 1.0);
            else if (pp_turbulence > 0.5) n = abs(2.0 * n - 1.0);
            somme += poids * n;
            total += poids;
            q = q * pp_lacunarite + 17.0;
            poids *= pp_gain;
        }
        o++;
        if (o == parPrise) {
            // La prise est finie : veines (un sinus le long de x, tordu par le
            // bruit — marbre ou bois) et postérisation, puis la moyenne.
            float3 v = somme / max(total, 1e-4);
            if (pp_veines > 0.5) v = 0.5 + 0.5 * sin(p.x * pp_veines_frequence * 0.3 + v * 8.0);
            if (pp_posterisation > 0.5) {
                float niveaux = max(pp_niveaux, 2.0);
                v = min(floor(saturate(v) * niveaux), niveaux - 1.0) / (niveaux - 1.0);
            }
            if (prise == 0) premiere = v;
            cumul += v;
            o = 0;
            prise++;
        }
    }
    // Le relief : le motif adouci, plus sa pente — clair là où il monte vers la
    // lumière (en haut à gauche), sombre là où il descend.
    float3 moyenne = relief ? lerp(premiere, 0.5, 0.6) + (premiere - (cumul - premiere)) * pp_relief_force * 3.0 : cumul / prises;
    // Le contraste est linéaire : l'appliquer après la moyenne revient au même.
    float3 v = (moyenne - 0.5) * 2.0 * pp_contraste;
    v = pp_inverser > 0.5 ? -v : v;
    // ⭐ BICOLORE : les parties claires prennent la seconde teinte, ramenée à
    // une couleur SANS luminosité propre (centrée) — elle colore sans éclaircir.
    if (pp_bicolore > 0.5) {
        float3 c = saturate(abs(frac(pp_seconde_teinte / 360.0 + float3(0.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0) - 1.0);
        c -= dot(c, 1.0 / 3.0);
        v += pp_bicolore_force * c * saturate(0.5 + 0.5 * dot(v, 1.0 / 3.0));
    }
    return v;
}

// LA TEXTURE DU FOND, par canal. « Personnalisée » (8) lit la composition de
// l'applet Textures ; une autre nature est le bruit de base du même nom (voir
// `texture_perso::BASES`), sans aucun réglage — fréquence 0.02, soit le pixel à
// 100 % (`TexBruit` la multiplie par 50).
float3 TextureFond(float2 ecran) {
    int n = (int)(TexNature + 0.5);
    float4 nature[9] = {
        float4(3.0 + n, 0.02, 0.0, 4.0),
        float4(2.0, 0.5, 0.0, 0.0),
        float4(0.0, 0.8, 0.5, 0.0),
        float4(1.0, 0.0, 5.0, 0.0),
        float4(4.0, 1.0, 0.0, 0.0),
        float4(0.3, 0.0, 0.0, 200.0),
        float4(1.0, 0.0, 1.5, 1.5),
        float4(0.0, 8.0, 0.0, 6.0),
        float4(0.0, 0.0, 0.0, 0.0),
    };
    float4 P[9];
    [unroll] for (int i = 0; i < 9; i++) P[i] = n == 8 ? TexPerso[i] : nature[i];
    return TexPersoFond(ecran, P);
}

// L'APLAT TEXTURÉ (voir `QuadInstance::avec_texture`) : la texture y est
// RELATIVE à la couleur — vers le blanc quand elle éclaircit, vers le noir quand
// elle assombrit. ⛔ Ajoutée telle quelle (comme sur un fond sombre), elle
// disparaissait sur un indicateur vif : même écart absolu, mais un contraste
// relatif 2 à 3 fois plus faible (mesuré le 2026-10-16). Le gain 5 ramène celui
// d'un fond bleu nuit (environ 0,19). Par CANAL : le brouillage RVB en a trois.
float3 AplatTexture(float3 rgb, float3 delta) {
    float ecart = saturate(abs(delta) * 5.0);
    rgb = delta > 0.0 ? lerp(rgb, 1.0, ecart) : rgb * (1.0 - ecart);
    return saturate(rgb + ecart * TexChroma);
}

float4 PSMain(PSInput input) : SV_TARGET {
    float dist = RoundedBoxSDF(input.LocalUV, input.HalfSize, input.Radius);
    // Le fondu ne touche que le CONTENU : un fond de barre (arrière-plan flouté,
    // congé) reste entier sous les icônes qui s'effacent.
    bool fond = input.TexMode > 1.5 && input.TexMode < 2.5;
    float visible = fond ? 1.0 : FonduVisibilite(input.Position.xy);
    // Le flou grandit à mesure que le contenu s'efface — un dixième de l'icône au
    // plus, là où elle a déjà presque disparu.
    float flou = (1.0 - visible) * 0.1;

    // LA TEXTURE DE L'INTERFACE (voir `TextureFond`), calculée UNE fois pour le
    // fond (2), le calque (4) et l'aplat texturé (5) : ⛔ ce site est le SEUL
    // appel de `TextureFond` — un second doublerait la compilation (voir
    // `TexPersoFond`). La CLARTÉ la décale vers le clair ou le sombre.
    float3 texDelta = 0.0;
    bool texturable = fond || input.TexMode > 3.5;
    [branch] if (TexForce > 0.0 && texturable) {
        texDelta = (TextureFond(input.Position.xy + DegOrigin) + TexClarte) * TexForce;
    }

    // Arête vive (sans anti-aliasing) pour les rectangles à coins droits
    // (Radius == 0, typiquement les fonds de barres) : plusieurs de ces
    // quads sont censés se toucher exactement bord à bord (les trois
    // barres du dock, voir `ui::dock`). Un bord anti-aliasé estompe sa
    // propre couverture sur ~1px ; deux bords estompés qui se touchent
    // laissent alors une fine ligne moins opaque à la jonction. Seules les
    // formes à coins arrondis (icônes) gardent l'anti-aliasing, qui n'a
    // pas ce problème de jonction.
    float shapeAlpha;
    // CONGÉ (voir `QuadInstance::new_conge`) : tout le carré SAUF le quart de
    // disque de rayon « côté du carré » centré sur le coin que `-Ring` désigne
    // (1 + x à droite + 2 × y en bas). `LocalUV` est centré, y vers le bas.
    bool conge = input.Ring < 0.0;
    if (conge) {
        int code = (int)(-input.Ring + 0.5) - 1;
        float2 coin = float2((code & 1) ? 1.0 : -1.0, (code & 2) ? 1.0 : -1.0) * input.HalfSize;
        float horsDisque = length(input.LocalUV - coin) - 2.0 * input.HalfSize.x;
        float aa = max(fwidth(horsDisque), 0.0001);
        shapeAlpha = smoothstep(-aa, aa, horsDisque);
        // Aucun effet de BORD : le congé prolonge une barre, il n'a pas de
        // contour à lui (liseré, réfraction, reflet, biseau).
        dist = -1.0e6;
    } else if (input.Ring > 0.0) {
        // ANNEAU (voir `QuadInstance::new_ring`) : un CONTOUR au lieu d'un
        // aplat — la bande d'épaisseur `input.Ring` comprise entre le bord de
        // la forme et ce même bord rentré d'autant, et RIEN au centre.
        //
        // `dist` vaut zéro SUR le bord, négatif à l'intérieur, positif
        // au-dehors : la bande est donc `-Ring <= dist <= 0`, et elle a DEUX
        // bords à anti-aliaser (le sien et celui de la forme), d'où les deux
        // `smoothstep` multipliés. C'est ce que le masque plein ci-dessous ne
        // sait pas faire, et la raison d'être de ce mode : un quad ne peut pas
        // rendre de creux autrement.
        // La rampe d'anti-aliasing est BORNÉE par la demi-épaisseur : sur un
        // anneau d'un pixel, deux rampes d'un pixel se recouvriraient et le
        // produit ne culminerait plus à 1 — l'anneau serait translucide au lieu
        // d'être fin (mesuré : un pic à ~0,7). Les deux bords gardent ainsi leur
        // AA sans jamais se chevaucher.
        float aa = min(max(fwidth(dist), 0.0001), input.Ring * 0.5);
        shapeAlpha = smoothstep(-input.Ring - aa, -input.Ring + aa, dist)
                   * (1.0 - smoothstep(-aa, aa, dist));
    } else if (input.Radius <= 0.0) {
        shapeAlpha = dist <= 0.0 ? 1.0 : 0.0;
    } else {
        // Anti-aliasing ~1px basé sur la dérivée de la distance.
        float aa = max(fwidth(dist), 0.0001);
        shapeAlpha = saturate(1.0 - smoothstep(-aa, aa, dist));
    }

    float4 baseColor;
    if (input.TexMode > 2.5 && input.TexMode < 3.5) {
        // Glyphe (texte/icône vectorielle blanche, voir gfx::text::
        // render_text et QuadInstance::new_glyph) : la texture ne porte que
        // la couverture anti-aliasée dans son alpha (RGB toujours blanc) —
        // on la teinte avec Color plutôt que d'utiliser sa couleur telle
        // quelle, pour permettre de choisir la couleur des éléments de la
        // barre du haut sans affecter les vraies icônes d'application.
        float4 glyphSample = EchantillonFlou(input.TexUV, flou);
        baseColor = float4(input.Color.rgb, glyphSample.a);
    } else if (input.TexMode > 1.5 && input.TexMode < 4.5) {
        // Arrière-plan flouté (voir gfx::backdrop), post-composé par
        // matériau (voir gfx::material::MATERIALS) puis teinté par Color :
        // Color.a est la force du mélange choisie par l'utilisateur, pas
        // une transparence de sortie — le résultat est opaque, exactement
        // comme le bureau flouté qu'il représente (voir gfx::backdrop pour
        // la limite de fraîcheur).

        // Proximité du bord intérieur, 1.0 sur le contour, 0.0 à `band` px
        // vers l'intérieur. `dist` est NÉGATIVE à l'intérieur (voir
        // RoundedBoxSDF). `band` borné à une fraction de la demi-épaisseur :
        // les barres du dock ne font que 72px (36px de demi-hauteur), un
        // liseré de 12px fixe y couvrirait un tiers de la surface.
        float band = min(12.0, min(input.HalfSize.x, input.HalfSize.y) * 0.35);
        float edge = saturate(1.0 + dist / max(band, 1.0));

        // 1. Réfraction de bord (Verre liquide) : pousse l'UV échantillonné
        //    vers l'extérieur près du contour, comme la lentille d'un vrai
        //    bord de verre. Direction par AXE (LocalUV/HalfSize), PAS
        //    normalize() : sur une barre 2000x72 une direction radiale
        //    normalisée pointerait presque à l'horizontale partout, alors
        //    que l'effet attendu est vertical près des bords haut/bas — et
        //    normalize(0) produirait un NaN au centre exact du quad. Le
        //    sampler est en CLAMP (gfx::texture::create_sampler_state),
        //    donc un UV hors [0,1] retombe sur le texel de bord, jamais un
        //    repli.
        float2 axis = clamp(input.LocalUV / max(input.HalfSize, 1.0), -1.0, 1.0);
        float2 uv = input.TexUV + axis * (edge * edge) * MatRefraction
                                / max(input.HalfSize * 2.0, 1.0);
        float4 backdrop = IconTex.Sample(IconSampler, uv);

        // 2. Teinte — comportement historique, Color.a = force du mélange.
        //    MatTintScale ne fait que MODULER ce que l'utilisateur a réglé ;
        //    la valeur persistée n'est jamais touchée (voir gfx::material).
        float tint = saturate(input.Color.a * MatTintScale);
        // SANS FOND CAPTURÉ — le cas de repli du réglage « visible dans les
        //    captures » quand la LOUPE (gfx::mag_capture) est indisponible sur
        //    la machine : aucun fond n'a pu être capturé, la teinte est prise
        //    PLEINE et l'alpha de sortie devient la transparence choisie par
        //    l'utilisateur — DWM compose le bureau derrière (fenêtres en
        //    modèle BitBlt, voir gfx::swapchain). Quand la loupe fonctionne,
        //    Ombres.y vaut 0 : le fond flouté est là et le rendu est le même
        //    qu'hors du mode capturable. Le drapeau voyage dans Ombres.y, à
        //    côté du calque des ombres en Ombres.x.
        bool sans_fond = Ombres.y > 0.5;
        float3 rgb = sans_fond ? input.Color.rgb : lerp(backdrop.rgb, input.Color.rgb, tint);
        // Dégradé animé (mode capturable seulement, voir `gfx::degrade`) :
        // il REMPLACE la teinte, évalué au pixel ÉCRAN — d'où l'origine de la
        // fenêtre ajoutée à `SV_POSITION` (pixels de la fenêtre).
        if (DegActif > 0.5) {
            rgb = Degrade(input.Position.xy + DegOrigin);
        }
        // Les lueurs (voir `gfx::lueur`) : chaque disque mêle sa couleur à celle
        // des barres, fondue sur la largeur de son `flou` ; au-delà de TOUS les
        // disques, un assombrissement léger. Pixel ÉCRAN, comme le dégradé : une
        // seule scène pour toute l'interface.
        if (LueurNombre > 0.5) {
            float2 ecran = input.Position.xy + DegOrigin;
            // ⛔ **L'ASSOMBRISSEMENT SE MESURAIT EN RAYONS** — défaut SIGNALÉ
            // (2026-10-14) : « l'assombrissement ne semble plus fonctionner ». Il
            // commençait à UN rayon du disque le plus proche et culminait à TROIS :
            // pour le disque par défaut (60 % de la largeur), le coin le plus
            // lointain de l'écran n'était qu'à 1,25 rayon — 4 % d'effet au mieux.
            // Il assombrit désormais tout ce que les lueurs NE COUVRENT PAS, AVANT
            // qu'elles ne se posent dessus : le curseur se voit sur tout l'écran.
            float couvert = 0.0;
            [loop] for (int lc = 0; lc < (int)LueurNombre; lc++) {
                // Une lueur à moins de 25 % d'intensité (une qui s'allume ou
                // s'éteint en fondu) protège d'autant moins.
                couvert = max(couvert, LueurPoids(lc, ecran) * saturate(LueurDisques[lc].w * 4.0));
            }
            rgb = lerp(rgb, 0.0, (1.0 - couvert) * LueurAssombrissement);
            [loop] for (int li = 0; li < (int)LueurNombre; li++) {
                float poids = LueurPoids(li, ecran) * LueurDisques[li].w;
                // Rien à mêler : on ne calcule ni le dégradé ni l'image.
                if (poids > 0.0005) rgb = lerp(rgb, LueurCouleur(li, ecran), poids);
            }
        }

        // 3. Saturation + biais de teinte (Frutiger Aero : cyan/bleu vif).
        float luma = dot(rgb, float3(0.2126, 0.7152, 0.0722));
        rgb = lerp(luma.xxx, rgb, MatSaturation) * MatHueBias;

        // 4. Bandeau glossy (Frutiger Aero) : hauteur en PIXELS plafonnée,
        //    pas en fraction du quad — sur une barre latérale pleine
        //    hauteur, une fraction donnerait un dégradé de 1000px au lieu
        //    d'un reflet.
        //
        // ⚠️ **LA LUMIÈRE VIENT D'EN HAUT, POUR LES QUATRE BARRES** — c'est la
        //    convention de l'industrie (clair en haut et à gauche, sombre en bas
        //    et à droite), et c'est une décision de l'utilisateur du 2026-10-05.
        //    La version précédente éclairait chaque barre DEPUIS LE BORD DE
        //    L'ÉCRAN qu'elle touche — quatre directions distinctes, ce que la
        //    littérature déconseille : une lumière incohérente fait lire les
        //    formes à l'envers. **Cette formule est donc celle d'ORIGINE, et elle
        //    vaut pour tout le monde** — les matériaux comme le style de relief.
        float glossPx = min(input.HalfSize.y * 2.0, 48.0);
        float top = conge ? 0.0 : saturate(1.0 - (input.LocalUV.y + input.HalfSize.y) / max(glossPx, 1.0));
        rgb += MatGloss * pow(top, 2.0) * 0.35;

        // 5. **LE BISEAU — DEUX TONS, ET C'EST TOUT LE SUJET.**
        //
        //    ⛔ **CE QUI A ÉTÉ SIGNALÉ : « on dirait que les barres sont
        //    ENCADRÉES ».** Et c'était exact, au sens des mots : un liseré d'UNE
        //    SEULE COULEUR sur les QUATRE bords, c'est la définition d'une
        //    BORDURE. Un BISEAU, lui, est un contour à DEUX couleurs — « clair sur
        //    les bords éclairés, sombre sur les bords opposés » — et c'est ce qui
        //    donne l'impression de relief.
        //
        //    D'où les deux termes ci-dessous : clair sur le bord ÉCLAIRÉ (le
        //    haut), sombre sur son OPPOSÉ (le bas). La couleur sombre est retirée,
        //    pas ajoutée — c'est le côté qui tourne le dos à la lumière.
        //
        //    ⚠️ **LES MATÉRIAUX GARDENT LEUR LISERÉ D'ORIGINE**, qui souligne le
        //    bord quel qu'il soit : leur aspect ne doit pas bouger d'un pixel.
        //    C'est le seul endroit où le style change le dessin des quads.
        if (MatReliefActif > 0.5 && !conge) {
            float bord_haut = saturate(1.0 - (input.LocalUV.y + input.HalfSize.y) / max(band, 1.0));
            float bord_bas  = saturate(1.0 - (input.HalfSize.y - input.LocalUV.y) / max(band, 1.0));
            rgb += MatRim * pow(bord_haut, 3.0);
            rgb -= MatRim * 0.75 * pow(bord_bas, 3.0);
        } else {
            rgb += MatRim * pow(edge, 3.0) * lerp(0.35, 1.0, top);
        }

        // 6. La TEXTURE DE L'INTERFACE (`texDelta`, calculée en tête) : à plat,
        //    sans lumière — une variation de luminosité, au pixel ÉCRAN ; la
        //    CHROMA (sans luminance, voir `texture_interface::chroma_de_teinte`)
        //    la colore dans les deux sens, d'où la valeur absolue.
        if (TexForce > 0.0) {
            float3 delta = texDelta;
            // Le CALQUE (TexMode 4, voir `QuadInstance::new_calque_texture`) sort
            // ici, la texture seule : l'alpha est le plus grand écart, la couleur
            // va de 0 (le canal assombrit de tout l'écart) à 1 (il éclaircit
            // d'autant). Gris, c'est exactement du blanc ou du noir ; en
            // couleur, un canal moins écarté que l'alpha est approché.
            if (input.TexMode > 3.5) {
                float ecart = max(max(abs(delta.r), abs(delta.g)), abs(delta.b));
                return float4(saturate(0.5 + 0.5 * delta / max(ecart, 1e-5) + TexChroma), saturate(ecart));
            }
            rgb += delta + abs(delta) * TexChroma;
        }

        baseColor = float4(saturate(rgb), sans_fond ? tint : 1.0);

        // 7. Dithering (voir `DitherHash`) — TOUJOURS en dernier, sur le
        //    résultat DÉJÀ mélangé/composé ci-dessus, PAS avant : BUG RÉEL
        //    CONSTATÉ EN PRATIQUE (voir l'historique, "le curseur Style de
        //    flou n'a aucun effet visible") — l'ajouter à `backdrop.rgb`
        //    puis mélanger avec `input.Color` écrasait jusqu'à 85% de
        //    l'effet (la force de teinte par défaut, voir `color::
        //    DEFAULT_BACKDROP_TINT_STRENGTH`) : avec `Color.a = 0.85`, le
        //    résultat final ne retient que 15% de `backdrop.rgb` (et donc
        //    de son dithering), quelle que soit l'amplitude choisie.
        //    Appliqué ici, sur `baseColor` (le résultat DÉJÀ mélangé), le
        //    grain reste visible à pleine amplitude quel que soit le
        //    réglage de transparence des barres — un flou gaussien/Kawase
        //    lisse un dégradé continu en bandes de couleur visibles une
        //    fois quantifié sur 8 bits/canal ("banding"), même technique
        //    que le `dash_noise.png` d'Unity (`BackgroundEffectHelper::
        //    GetBlurRegion`) ou le bruit du matériau "Acrylic" de Windows,
        //    mais procédural plutôt que texturé (voir `DitherHash`).
        //    `input.Position.xy` : coordonnées PIXEL de sortie (natives),
        //    pas `TexUV` — le bruit doit rester à l'échelle native
        //    affichée, pas à celle (plus petite) de la texture de flou
        //    rééchantillonnée bilinéairement par le `Sample` ci-dessus.
        float dither = (DitherHash(input.Position.xy) - 0.5) * DitherAmount;
        baseColor.rgb += dither;
    } else {
        // Icône texturée (ex: logo NeuraDesk) : la couleur ET l'alpha
        // viennent de l'image, le masque SDF ne fait qu'arrondir les coins
        // par-dessus. TexMode == 0 revient à garder Color telle quelle.
        // L'APLAT TEXTURÉ (5, voir `QuadInstance::avec_texture`) garde sa
        // couleur, et reçoit la texture des fonds.
        float4 texSample = EchantillonFlou(input.TexUV, flou);
        baseColor = lerp(input.Color, texSample, input.TexMode > 4.5 ? 0.0 : input.TexMode);
        if (input.TexMode > 4.5) baseColor.rgb = AplatTexture(baseColor.rgb, texDelta);
        // L'OPACITÉ d'une texture (mode 1) : `Color.a`, que `new_textured` pose
        // à 1 — le fondu des fenêtres de NeuraDesk (`win32::ouverture_fenetres`).
        if (input.TexMode > 0.5 && input.TexMode < 1.5) baseColor.a *= input.Color.a;
        // Et sa TEINTE : `new_textured` pose un blanc, sans effet ; une tranche de la
        // pile des barres latérales s'y assombrit (`ui::empilement`).
        if (input.TexMode > 0.5 && input.TexMode < 1.5) baseColor.rgb *= input.Color.rgb;
    }

    // ⭐ LES OMBRES DES BARRES, SOUS LES ICÔNES ET LES TEXTES — DEMANDE EXPLICITE
    // (2026-10-19) : leur part sur l'interface, composée par
    // `win32::ombre_barre::Calque` (BGRA prémultiplié, au pixel de la fenêtre), se
    // pose sur les FONDS de barre seulement — le contenu peint par-dessus reste
    // intact.
    if (fond && Ombres.x > 0.5) {
        float4 ombre = OmbresTex.Load(int3(input.Position.xy, 0));
        baseColor.rgb = baseColor.rgb * (1.0 - ombre.a) + ombre.rgb;
    }
    float alpha = shapeAlpha * baseColor.a * visible;

    // Sortie prémultipliée : DWM attend RGB déjà multiplié par A.
    return float4(baseColor.rgb * alpha, alpha);
}
"#;

/// Un lot de quads : une texture, des instances, et **un découpage**.
///
/// Un lot = un `DrawInstanced` (voir `QuadRenderer::render`), donc le
/// découpage se pose naturellement par lot : c'est ce que le pipeline permet
/// (un état de rastérisation + un rectangle de scissor, pas un par instance),
/// et ça couvre tous les besoins du dock — une barre latérale découpe d'un
/// bloc, le reste ne découpe pas.
pub struct QuadBatch<'a> {
    /// Texture échantillonnée par les instances dont `tex_mode` vaut 1.0 ou
    /// 2.0 ; `None` pour un lot 100 % couleur unie.
    pub texture: Option<&'a ID3D11ShaderResourceView>,
    pub instances: &'a [QuadInstance],
    /// Rectangle de découpe (`x, y, largeur, hauteur` en pixels, coordonnées
    /// de la fenêtre) — voir `QuadBatch::clipped`. `None` = aucun découpage.
    pub clip: Option<[f32; 4]>,
    /// Le lot subit le FONDU du haut des colonnes (`QuadRenderer::poser_fondu`).
    /// Vrai pour le seul contenu DÉFILANT des colonnes latérales — leurs lots
    /// découpés (`clipped`).
    ///
    /// ⛔ **DÉFAUT SIGNALÉ (2026-11-13)** : « les icônes de la barre du haut
    /// passent sous la barre de gauche quand elles sont magnifiées ». Le fondu
    /// s'appliquait à TOUT pixel de contenu dans la bande, donc aussi à une
    /// icône de la barre du haut qui, agrandie, y descend : elle s'effaçait
    /// comme si la colonne passait devant.
    pub fondu: bool,
}

impl<'a> QuadBatch<'a> {
    /// Lot ordinaire, non découpé.
    pub fn new(texture: Option<&'a ID3D11ShaderResourceView>, instances: &'a [QuadInstance]) -> Self {
        Self { texture, instances, clip: None, fondu: false }
    }

    /// Lot découpé à `clip` : rien de ce qu'il dessine n'apparaîtra en dehors.
    ///
    /// **Sert aux barres LATÉRALES du dock, dont le contenu défile.** Sans ce
    /// découpage, une icône remontée au-dessus du bord de sa barre se dessine
    /// par-dessus la barre du haut : la région de la fenêtre est l'**union**
    /// des trois barres (`Window::set_dock_region`), donc la zone de la barre
    /// du haut en fait partie et le système ne coupe rien — c'est l'hypothèse
    /// fausse qui a produit le défaut, corrigée ici (voir aussi
    /// `ui::dock::scroll_bar`, dont la doc la reprenait).
    pub fn clipped(
        texture: Option<&'a ID3D11ShaderResourceView>,
        instances: &'a [QuadInstance],
        clip: [f32; 4],
    ) -> Self {
        Self { texture, instances, clip: Some(clip), fondu: true }
    }
}

/// Accumulateur d'instances groupées par DÉCOUPAGE (voir `QuadBatch::clip`).
///
/// Les deux chemins de rendu répartissaient jusqu'ici leurs instances en
/// « barres latérales » / « barre du haut ». Depuis les séparateurs de zone
/// (`ui::dock::Icon::zone_height`), deux icônes d'une MÊME barre latérale
/// peuvent demander deux découpages DIFFÉRENTS — et un lot n'en porte qu'un.
/// Grouper par découpage plutôt que par barre règle le cas sans éclater le
/// rendu en un lot par icône : **le nombre de lots ne dépend que du nombre de
/// ZONES**, celui des icônes restant sans effet.
///
/// **Réordonner les groupes ne change rien à ce qui se recouvre**, ce qui est
/// la propriété qui rend ce regroupement sûr : les boîtes de zone sont
/// DISJOINTES entre elles (`ui::dock::BarLayout`), la barre du haut ne
/// chevauche aucune barre latérale, et la SEULE chose qui puisse déborder de
/// son emplacement — une icône magnifiée — ne passe jamais par ici (les deux
/// chemins la mettent de côté pour la dessiner en dernier, sans découpage).
#[derive(Default)]
pub struct ClipGroups {
    groups: Vec<(Option<[f32; 4]>, Vec<QuadInstance>)>,
}

impl ClipGroups {
    pub fn new() -> Self {
        Self::default()
    }

    /// Range `instance` dans le groupe de `clip`, créé à la première instance
    /// qui le demande.
    pub fn push(&mut self, clip: Option<[f32; 4]>, instance: QuadInstance) {
        match self.groups.iter_mut().find(|(existing, _)| *existing == clip) {
            Some((_, instances)) => instances.push(instance),
            None => self.groups.push((clip, vec![instance])),
        }
    }

    /// Ajoute à `batches` un lot par groupe, dans l'ordre où les découpages
    /// ont été rencontrés — voir la doc du type pour ce que cet ordre doit
    /// garantir. Un groupe vide ne produit rien (un découpage de largeur nulle
    /// ne dessinerait rien de toute façon, mais un lot en moins est un appel en
    /// moins).
    pub fn push_batches<'a>(
        &'a self,
        texture: Option<&'a ID3D11ShaderResourceView>,
        batches: &mut Vec<QuadBatch<'a>>,
    ) {
        for (clip, instances) in &self.groups {
            if instances.is_empty() {
                continue;
            }
            batches.push(match clip {
                Some(clip) => QuadBatch::clipped(texture, instances, *clip),
                None => QuadBatch::new(texture, instances),
            });
        }
    }
}

/// Pousse un **lot mono-instance** pour une icône du dock, découpé ou non.
///
/// Factorise les sites qui écrivaient tous
/// `QuadBatch::new(Some(&tex.srv), std::slice::from_ref(instance))` — il y en a
/// des dizaines, dans `app::run` comme dans `dock_instance::DockInstance::render`.
/// `clip` vient de la barre de l'icône (voir `ui::dock::Dock::lateral_clip`) :
/// une icône latérale remontée par le défilement doit disparaître sous le bord
/// de sa barre, pas se dessiner sur la barre du haut.
pub fn push_icon_batch<'a>(
    batches: &mut Vec<QuadBatch<'a>>,
    texture: &'a crate::gfx::Texture,
    instance: &'a QuadInstance,
    clip: Option<[f32; 4]>,
) {
    let instances = std::slice::from_ref(instance);
    batches.push(match clip {
        Some(clip) => QuadBatch::clipped(Some(&texture.srv), instances, clip),
        None => QuadBatch::new(Some(&texture.srv), instances),
    });
}

/// Rectangle de scissor D3D11 correspondant à `clip`, **borné au viewport** —
/// D3D11 exige un rectangle contenu dans la cible de rendu, et une barre
/// masquée (largeur 0, voir `app::effective_primary_thicknesses`) ou un
/// découpage à cheval sur le bord en produirait un hors bornes. Un rectangle
/// vide (`right == left`) est légal et ne dessine rien.
fn scissor_rect(clip: Option<[f32; 4]>, viewport_w: f32, viewport_h: f32) -> RECT {
    let [x, y, w, h] = clip.unwrap_or([0.0, 0.0, viewport_w, viewport_h]);
    let left = x.max(0.0).min(viewport_w).round() as i32;
    let top = y.max(0.0).min(viewport_h).round() as i32;
    let right = (x + w).max(0.0).min(viewport_w).round() as i32;
    let bottom = (y + h).max(0.0).min(viewport_h).round() as i32;
    RECT { left, top, right: right.max(left), bottom: bottom.max(top) }
}

/// `SHADER_SOURCE` précédée de `#define DEG_MODE <mode>` — une source par
/// motif, construite une fois : le cache de `gfx::shader` reconnaît une source
/// à son adresse.
fn pixel_source(mode: usize) -> &'static str {
    static SOURCES: [std::sync::OnceLock<String>; crate::gfx::degrade::MODES.len()] =
        [const { std::sync::OnceLock::new() }; crate::gfx::degrade::MODES.len()];
    let mode = mode.min(SOURCES.len() - 1);
    SOURCES[mode].get_or_init(|| format!("#define DEG_MODE {mode}\n{SHADER_SOURCE}"))
}

/// Le marqueur que `source_utilisateur` remplace.
const MARQUEUR_UTILISATEUR: &str = "//@@MOTIF_UTILISATEUR@@";

/// **LA SOURCE COMPLÈTE D'UN MOTIF DE L'UTILISATEUR** — `SHADER_SOURCE`, avec
/// `code` à la place du marqueur et `DEG_UTILISATEUR` défini.
///
/// `#line 1 "motif"` fait que le compilateur désigne une erreur DANS `code` par
/// sa ligne à lui (`motif(3,5): error …`), et non par celle de la source
/// complète — c'est ce que l'utilisateur verra (voir
/// `shader_utilisateur::erreurs_lisibles`). Le `#line` qui suit rend leurs
/// numéros aux lignes d'après.
pub(crate) fn source_utilisateur(code: &str) -> String {
    let (avant, apres) = SHADER_SOURCE.split_once(MARQUEUR_UTILISATEUR).expect("le marqueur du motif utilisateur");
    let ligne_d_apres = avant.lines().count() + 2;
    format!("#define DEG_MODE 0\n#define DEG_UTILISATEUR 1\n{avant}\n#line 1 \"motif\"\n{code}\n#line {ligne_d_apres} \"quad\"\n{apres}")
}

fn create_pixel_shader(device: &ID3D11Device, mode: usize) -> Result<ID3D11PixelShader> {
    let blob = compile_shader(pixel_source(mode), "PSMain", "ps_5_0")?;
    creer_depuis_les_octets(device, blob_bytes(&blob))
}

fn creer_depuis_les_octets(device: &ID3D11Device, octets: &[u8]) -> Result<ID3D11PixelShader> {
    unsafe {
        let mut out = None;
        device.CreatePixelShader(octets, None, Some(&mut out))?;
        Ok(out.unwrap())
    }
}

/// **LE PIXEL SHADER À DESSINER, ET SA CLÉ** — le motif de l'utilisateur s'il y
/// en a un d'actif (sa génération), sinon le motif intégré. La clé dit au
/// `QuadRenderer` quand recréer le sien.
///
/// ⛔ **RIEN NE SE COMPILE ICI POUR L'UTILISATEUR** : ses octets sont déjà
/// compilés, sur un fil à part (voir `shader_utilisateur::choisir`) ; ce chemin
/// tourne sur le fil d'interface, au rendu. Si le pilote refuse les octets, le
/// motif est désactivé et l'intégré reprend.
fn pixel_shader_courant(device: &ID3D11Device) -> Result<(ID3D11PixelShader, (usize, u64))> {
    let mode = crate::gfx::degrade::mode_index();
    if let Some((generation, octets)) = crate::gfx::shader_utilisateur::octets_actifs() {
        match creer_depuis_les_octets(device, &octets) {
            Ok(ps) => return Ok((ps, (mode, generation))),
            Err(err) => crate::gfx::shader_utilisateur::refuser_par_le_pilote(generation, &err.message()),
        }
    }
    Ok((create_pixel_shader(device, mode)?, (mode, 0)))
}

fn cle_courante() -> (usize, u64) {
    (crate::gfx::degrade::mode_index(), crate::gfx::shader_utilisateur::generation_active())
}

pub struct QuadRenderer {
    vs: ID3D11VertexShader,
    /// Le pixel shader du motif du dégradé animé, recréé au rendu quand sa clé
    /// change (voir `pixel_shader_courant`).
    ps: std::cell::RefCell<ID3D11PixelShader>,
    ps_cle: std::cell::Cell<(usize, u64)>,
    input_layout: ID3D11InputLayout,
    vertex_buffer: ID3D11Buffer,
    instance_buffer: ID3D11Buffer,
    constant_buffer: ID3D11Buffer,
    blend_state: ID3D11BlendState,
    sampler: ID3D11SamplerState,
    /// Lié au début de chaque `render`, **découpage ACTIVÉ** (`ScissorEnable:
    /// true`) — le découpage par lot est une fonctionnalité (voir
    /// `QuadBatch::clipped`), plus seulement quelque chose à neutraliser.
    ///
    /// Historique à connaître, parce que le contexte est PARTAGÉ par toutes
    /// les fenêtres de l'application (dock, popups CEF, infobulle, horloge
    /// plein écran) et que c'est **global au contexte, pas au render target** :
    /// `egui_directx11::Renderer` activait le découpage pour ses zones de clip
    /// et ne le désactivait pas en repartant ; le `QuadRenderer::render`
    /// suivant, sur une AUTRE fenêtre rendue la même frame, héritait donc d'un
    /// rectangle sans rapport — souvent hors de la fenêtre — et masquait tout
    /// ou partie des barres du dock (barres noires, ou partiellement noires).
    /// D'où la règle appliquée ici : **un `RSSetScissorRects` est TOUJOURS
    /// posé** — le viewport entier par défaut, le rectangle du lot quand il en
    /// porte un — jamais un état laissé au hasard du rendu précédent.
    rasterizer_state: ID3D11RasterizerState,
    /// Les fondus du PROCHAIN rendu (voir `poser_fondu`) — remis à zéro par lui.
    fondu: std::cell::Cell<[[f32; 4]; 2]>,
    /// Le calque des ombres du PROCHAIN rendu (voir `poser_ombres`) — remis à
    /// `None` par lui.
    ombres: std::cell::Cell<Option<ID3D11ShaderResourceView>>,
    /// La texture du PROCHAIN rendu, à la place de celle de l'interface (voir
    /// `poser_texture`) — remise à `None` par lui.
    texture: std::cell::Cell<Option<TextureDuRendu>>,
}

impl QuadRenderer {
    /// Pose le fondu en haut de la colonne de gauche (voir
    /// `ui::dock::bande_de_fondu`) pour le PROCHAIN `render_at` seulement : un
    /// rendu ne l'hérite jamais d'un autre (une infobulle, un popup).
    pub fn poser_fondu(&self, bandes: [Option<[f32; 4]>; 2]) {
        self.fondu.set(bandes.map(Option::unwrap_or_default));
    }

    /// Pose le calque des ombres des barres (voir `win32::ombre_barre::
    /// vue_des_ombres`) pour le PROCHAIN `render_at` seulement, comme
    /// `poser_fondu`.
    pub fn poser_ombres(&self, ombres: Option<ID3D11ShaderResourceView>) {
        self.ombres.set(ombres);
    }

    /// Pose d'AUTRES constantes de texture (voir `texture_interface::
    /// ReglagesTexture::constantes`), et leur composition personnalisée (voir
    /// `texture_perso::Composition::constantes`), pour le PROCHAIN `render_at`
    /// seulement — celles des terminaux, pour leur calque.
    pub fn poser_texture(&self, constantes: [[f32; 4]; 2], perso: [[f32; 4]; crate::gfx::texture_perso::REGISTRES]) {
        self.texture.set(Some((constantes, perso)));
    }

    pub fn new(device: &ID3D11Device) -> Result<Self> {
        let vs_blob = compile_shader(SHADER_SOURCE, "VSMain", "vs_5_0")?;
        let (ps, ps_cle) = pixel_shader_courant(device)?;

        let vs = unsafe {
            let mut out = None;
            device.CreateVertexShader(blob_bytes(&vs_blob), None, Some(&mut out))?;
            out.unwrap()
        };

        let input_layout = unsafe { create_input_layout(device, &vs_blob)? };

        let vertex_buffer = create_vertex_buffer(device)?;
        let instance_buffer = create_instance_buffer(device)?;
        let constant_buffer = create_constant_buffer(device)?;
        let blend_state = create_blend_state(device)?;
        let sampler = crate::gfx::texture::create_sampler_state(device)?;
        let rasterizer_state = create_rasterizer_state(device)?;

        Ok(Self {
            vs,
            ps: std::cell::RefCell::new(ps),
            ps_cle: std::cell::Cell::new(ps_cle),
            input_layout,
            vertex_buffer,
            instance_buffer,
            constant_buffer,
            blend_state,
            sampler,
            rasterizer_state,
            fondu: std::cell::Cell::new([[0.0; 4]; 2]),
            ombres: std::cell::Cell::new(None),
            texture: std::cell::Cell::new(None),
        })
    }

    /// Dessine une série de `batches` sur `rtv` (un draw call par lot).
    /// `viewport_w`/`viewport_h` sont la taille du back buffer en pixels
    /// physiques. Chaque lot associe une texture optionnelle (échantillonnée
    /// par les instances dont `tex_mode` vaut 1.0 ou 2.0, voir
    /// `QuadInstance::new_textured`/`new_backdrop` ; `None` pour un lot 100%
    /// couleur unie) à ses instances — plusieurs textures distinctes (logo,
    /// horloge, glyphe d'alimentation, arrière-plans floutés des barres) ne
    /// peuvent pas partager un seul draw call puisque le pipeline ne lie
    /// qu'une texture à la fois.
    pub fn render(
        &self,
        ctx: &ID3D11DeviceContext,
        rtv: &ID3D11RenderTargetView,
        viewport_w: f32,
        viewport_h: f32,
        batches: &[QuadBatch],
    ) -> Result<()> {
        self.render_at(ctx, rtv, viewport_w, viewport_h, batches, (0, 0))
    }

    /// `render` pour une fenêtre dont le coin supérieur gauche est
    /// `screen_origin` (pixels écran) : le dégradé animé (`gfx::degrade`) est
    /// évalué en coordonnées écran, et c'est ce qui le rend continu d'une
    /// fenêtre à l'autre. Sans lui (`render`), une fenêtre montrerait le coin
    /// haut-gauche du dégradé quelle que soit sa place.
    pub fn render_at(
        &self,
        ctx: &ID3D11DeviceContext,
        rtv: &ID3D11RenderTargetView,
        viewport_w: f32,
        viewport_h: f32,
        batches: &[QuadBatch],
        screen_origin: (i32, i32),
    ) -> Result<()> {
        unsafe {
            // Fond entièrement transparent : en alpha prémultiplié, (0,0,0,0)
            // veut dire "rien ici", laissant voir le bureau à travers DWM.
            ctx.ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, 0.0]);

            let ombres = self.ombres.take();
            let drapeau = [if ombres.is_some() { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0];
            let (fondu, texture) = (self.fondu.take(), self.texture.take());
            let sans_fondu = [[0.0; 4]; 2];
            // Le fondu n'est posé que pour les lots qui le demandent (voir
            // `QuadBatch::fondu`) ; on ne réécrit les constantes qu'au
            // changement, donc deux fois au plus par colonne.
            let mut fondu_pose = false;
            update_constant_buffer(ctx, &self.constant_buffer, viewport_w, viewport_h, screen_origin, sans_fondu, drapeau, texture)?;

            let viewport = D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: viewport_w,
                Height: viewport_h,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };
            ctx.RSSetViewports(Some(&[viewport]));
            // Voir la doc du champ `rasterizer_state` : sans ce
            // `RSSetState` explicite, le découpage laissé actif par
            // `egui_directx11::Renderer` (partage le même contexte, voir
            // `win32::egui_overlay`) restreignait aussi CE rendu-ci.
            ctx.RSSetState(&self.rasterizer_state);
            // Découpage plein viewport AVANT la boucle : quoi qu'il arrive
            // ensuite (liste de lots vide, ou premier lot vide), le contexte
            // ne conserve jamais un rectangle du rendu PRÉCÉDENT — c'est
            // exactement le bug que documente la doc du champ
            // `rasterizer_state`, et la raison pour laquelle un rectangle est
            // toujours posé, jamais « hérité ».
            ctx.RSSetScissorRects(Some(&[scissor_rect(None, viewport_w, viewport_h)]));

            ctx.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            ctx.OMSetBlendState(&self.blend_state, None, 0xFFFFFFFF);

            ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            ctx.IASetInputLayout(&self.input_layout);

            let strides = [size_of::<Vertex>() as u32, size_of::<QuadInstance>() as u32];
            let offsets = [0u32, 0u32];
            ctx.IASetVertexBuffers(
                0,
                2,
                Some([Some(self.vertex_buffer.clone()), Some(self.instance_buffer.clone())].as_ptr()),
                Some(strides.as_ptr()),
                Some(offsets.as_ptr()),
            );

            ctx.VSSetShader(&self.vs, None);
            if cle_courante() != self.ps_cle.get() {
                // Un échec garde l'ancien motif plutôt que de ne rien dessiner.
                if let Ok((ps, cle)) = ctx.GetDevice().and_then(|device| pixel_shader_courant(&device)) {
                    if let Ok(mut slot) = self.ps.try_borrow_mut() {
                        *slot = ps;
                        self.ps_cle.set(cle);
                    }
                }
            }
            match self.ps.try_borrow() {
                Ok(ps) => ctx.PSSetShader(&*ps, None),
                Err(_) => return Ok(()),
            }
            ctx.VSSetConstantBuffers(0, Some(&[Some(self.constant_buffer.clone())]));
            // BUG RÉEL CONSTATÉ EN PRATIQUE (curseur "Style de flou" sans
            // aucun effet, vérifié par relecture pixel du back buffer —
            // toujours identique quel que soit le préréglage) : D3D11 a des
            // tables de liaison de constant buffers SÉPARÉES par étage de
            // pipeline — lier ce buffer au VS (ci-dessus) ne le rend PAS
            // visible au PS. `PSMain` ne lisait jamais rien de `cbuffer
            // Viewport` avant l'ajout de `DitherAmount` (voir son commentaire
            // dans `SHADER_SOURCE`), donc l'absence de liaison PS passait
            // inaperçue ; maintenant qu'il lit `DitherAmount`, il lit un
            // registre jamais lié côté PS — qui revient à 0 en pratique sur
            // ce pilote, quel que soit ce qui est réellement écrit dans le
            // buffer côté CPU.
            ctx.PSSetConstantBuffers(0, Some(&[Some(self.constant_buffer.clone())]));
            ctx.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            // Les images des lueurs, au registre `t1` (voir `gfx::lueur_images`).
            crate::gfx::lueur_images::lier(ctx);
            // Le calque des ombres, au registre `t2` — TOUJOURS posé, `None`
            // compris : jamais celui d'un autre rendu.
            ctx.PSSetShaderResources(2, Some(&[ombres]));

            for batch in batches {
                // ⛔ **LE LOT EST DÉCOUPÉ, PAS TRONQUÉ** : le tampon d'instances
                // ne tient que `MAX_INSTANCES` ; `min(...)` laissait tomber en
                // SILENCE tout ce qui dépassait. Les effets fins (poussière,
                // glitch) demandent plus de mille quads pour une seule fenêtre :
                // on les dessine en plusieurs appels.
                if batch.instances.is_empty() {
                    continue;
                }
                let veut_fondu = batch.fondu && fondu != sans_fondu;
                if veut_fondu != fondu_pose {
                    let bandes = if veut_fondu { fondu } else { sans_fondu };
                    update_constant_buffer(ctx, &self.constant_buffer, viewport_w, viewport_h, screen_origin, bandes, drapeau, texture)?;
                    fondu_pose = veut_fondu;
                }
                ctx.RSSetScissorRects(Some(&[scissor_rect(batch.clip, viewport_w, viewport_h)]));
                ctx.PSSetShaderResources(0, Some(&[batch.texture.cloned()]));
                for tranche in batch.instances.chunks(MAX_INSTANCES) {
                    update_instance_buffer(ctx, &self.instance_buffer, tranche)?;
                    ctx.DrawInstanced(4, tranche.len() as u32, 0, 0);
                }
            }
        }

        Ok(())
    }
}

unsafe fn create_input_layout(device: &ID3D11Device, vs_blob: &ID3DBlob) -> Result<ID3D11InputLayout> {
    let elements = [
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("LOCALPOS"),
            SemanticIndex: 0,
            Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32G32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 0,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("RECT"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            InputSlot: 1,
            AlignedByteOffset: 0,
            InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
            InstanceDataStepRate: 1,
        },
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("COLOR"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            InputSlot: 1,
            AlignedByteOffset: 16,
            InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
            InstanceDataStepRate: 1,
        },
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("RADIUS"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32_FLOAT,
            InputSlot: 1,
            AlignedByteOffset: 32,
            InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
            InstanceDataStepRate: 1,
        },
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("SCALE"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32_FLOAT,
            InputSlot: 1,
            AlignedByteOffset: 36,
            InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
            InstanceDataStepRate: 1,
        },
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("TEXMODE"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32_FLOAT,
            InputSlot: 1,
            AlignedByteOffset: 40,
            InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
            InstanceDataStepRate: 1,
        },
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("ROTATION"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32_FLOAT,
            InputSlot: 1,
            AlignedByteOffset: 44,
            InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
            InstanceDataStepRate: 1,
        },
        // DERNIER champ de `QuadInstance` (voir sa doc) : l'épaisseur d'anneau.
        // Un champ RÉORDONNÉ dans la structure rendrait ce décalage faux SANS
        // rien signaler — voir le test `quad_instance_layout_matches_the_input_layout`,
        // qui asserte les sept offsets.
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("RING"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32_FLOAT,
            InputSlot: 1,
            AlignedByteOffset: 48,
            InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
            InstanceDataStepRate: 1,
        },
        // La BASCULE (voir `QuadInstance::bascule`), après l'anneau.
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("BASCULE"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32_FLOAT,
            InputSlot: 1,
            AlignedByteOffset: 52,
            InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
            InstanceDataStepRate: 1,
        },
        // Le SOUS-RECTANGLE DE TEXTURE (voir `QuadInstance::uv`), en dernier.
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("UV"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            InputSlot: 1,
            AlignedByteOffset: 56,
            InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
            InstanceDataStepRate: 1,
        },
    ];

    let mut layout = None;
    device.CreateInputLayout(&elements, blob_bytes(vs_blob), Some(&mut layout))?;
    Ok(layout.unwrap())
}

fn create_vertex_buffer(device: &ID3D11Device) -> Result<ID3D11Buffer> {
    // Quad unitaire en triangle-strip : (bas-gauche, bas-droit, haut-gauche, haut-droit).
    let verts = [
        Vertex { local_pos: [-0.5, -0.5] },
        Vertex { local_pos: [0.5, -0.5] },
        Vertex { local_pos: [-0.5, 0.5] },
        Vertex { local_pos: [0.5, 0.5] },
    ];

    let desc = D3D11_BUFFER_DESC {
        ByteWidth: (verts.len() * size_of::<Vertex>()) as u32,
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_FLAG(0).0 as u32,
        MiscFlags: D3D11_RESOURCE_MISC_FLAG(0).0 as u32,
        StructureByteStride: 0,
    };
    let data = D3D11_SUBRESOURCE_DATA {
        pSysMem: verts.as_ptr() as *const _,
        SysMemPitch: 0,
        SysMemSlicePitch: 0,
    };

    let mut buffer = None;
    unsafe { device.CreateBuffer(&desc, Some(&data), Some(&mut buffer))? };
    Ok(buffer.unwrap())
}

fn create_instance_buffer(device: &ID3D11Device) -> Result<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: (MAX_INSTANCES * size_of::<QuadInstance>()) as u32,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: D3D11_RESOURCE_MISC_FLAG(0).0 as u32,
        StructureByteStride: 0,
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer))? };
    Ok(buffer.unwrap())
}

fn create_constant_buffer(device: &ID3D11Device) -> Result<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: size_of::<ViewportConstants>() as u32,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: D3D11_RESOURCE_MISC_FLAG(0).0 as u32,
        StructureByteStride: 0,
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer))? };
    Ok(buffer.unwrap())
}

fn create_blend_state(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = true.into();
    // Alpha prémultiplié : la couleur source est déjà multipliée par son alpha.
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;

    let mut state = None;
    unsafe { device.CreateBlendState(&desc, Some(&mut state))? };
    Ok(state.unwrap())
}

/// Voir `QuadRenderer::rasterizer_state`, doc de champ : `ScissorEnable: TRUE`
/// est une fonctionnalité (le découpage par lot de `QuadBatch::clipped`), pas
/// un réglage neutre. Le reste (remplissage plein, pas de culling — des
/// rectangles 2D n'ont pas de face arrière à masquer) correspond à ce que ce
/// renderer attend implicitement depuis toujours.
///
/// Sans effet tant qu'aucun `RSSetScissorRects` n'est posé, d'où l'invariant
/// tenu par `render` : il en pose TOUJOURS un.
fn create_rasterizer_state(device: &ID3D11Device) -> Result<ID3D11RasterizerState> {
    let desc = D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: D3D11_CULL_NONE,
        DepthClipEnable: true.into(),
        ScissorEnable: true.into(),
        ..Default::default()
    };

    let mut state = None;
    unsafe { device.CreateRasterizerState(&desc, Some(&mut state))? };
    Ok(state.unwrap())
}

/// Les constantes d'une texture posée par `QuadRenderer::poser_texture` : ses
/// réglages, puis sa composition personnalisée.
type TextureDuRendu = ([[f32; 4]; 2], [[f32; 4]; crate::gfx::texture_perso::REGISTRES]);

unsafe fn update_constant_buffer(
    ctx: &ID3D11DeviceContext,
    buffer: &ID3D11Buffer,
    viewport_w: f32,
    viewport_h: f32,
    screen_origin: (i32, i32),
    fondu: [[f32; 4]; 2],
    ombre: [f32; 4],
    texture: Option<TextureDuRendu>,
) -> Result<()> {
    let post = material::current().post;
    // ⭐ **LE RELIEF DU STYLE S'AJOUTE À CELUI DU MATÉRIAU** — il ne le remplace
    // pas : les deux axes restent indépendants, c'est tout le modèle du système
    // de thèmes (style ≠ couleurs ≠ matériau). Borné à 1.0, comme la validation
    // du TOML le fait déjà de son côté.
    let (relief_degrade, relief_liserel) = (crate::ui::style::relief_degrade(), crate::ui::style::relief_liserel());
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    ctx.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))?;
    let constants = ViewportConstants {
        size: [viewport_w, viewport_h],
        dither_amount: post.noise.max(material::MIN_NOISE),
        rim: (post.rim + relief_liserel).min(1.0),
        gloss: (post.gloss + relief_degrade).min(1.0),
        saturation: post.saturation,
        tint_scale: post.tint_scale,
        refraction: post.refraction,
        hue_bias: post.hue_bias,
        relief_actif: if relief_degrade > 0.0 || relief_liserel > 0.0 { 1.0 } else { 0.0 },
        degrade: crate::gfx::degrade::constantes(screen_origin),
        lueur: crate::gfx::lueur::constantes(),
        fondu,
        texture: texture.map_or_else(crate::gfx::texture_interface::constantes, |(t, _)| t),
        texture_perso: texture.map_or_else(crate::gfx::texture_perso::constantes, |(_, p)| p),
        // `y` : pas de fond capturé. Dans le mode « visible dans les captures »
        // le fond vient de la LOUPE (`gfx::mag_capture`), qui exclut nos
        // fenêtres de notre propre capture — il y a donc bien un fond, et le
        // shader rend comme d'habitude. Le drapeau ne monte que si la loupe
        // est indisponible sur cette machine : le shader sort alors l'alpha de
        // l'utilisateur au lieu d'un aplat opaque (voir sa doc).
        ombre: [
            ombre[0],
            if crate::gfx::capture_visibility::est_capturable() && !crate::gfx::mag_capture::disponible() {
                1.0
            } else {
                0.0
            },
            ombre[2],
            ombre[3],
        ],
    };
    std::ptr::copy_nonoverlapping(&constants, mapped.pData as *mut ViewportConstants, 1);
    ctx.Unmap(buffer, 0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Garde le packing HLSL de `ViewportConstants` sous test — voir sa doc
    /// de champ : un `float3` (`hue_bias`) à cheval sur une frontière de 16
    /// octets casserait silencieusement le rendu (pas d'erreur de
    /// compilation, juste des paramètres lus au mauvais offset côté GPU).
    /// ⛔ Les tableaux du `cbuffer` portent leur taille EN DUR : un `MAX_ELEMENTS`
    /// changé sans eux ferait lire les fondus au mauvais endroit, SANS ERREUR.
    #[test]
    fn les_tableaux_du_shader_ont_la_taille_des_lueurs() {
        let n = crate::gfx::lueur::MAX_ELEMENTS;
        for nom in ["LueurDisques", "LueurTeintes", "LueurFormes", "LueurDegrades"] {
            assert!(SHADER_SOURCE.contains(&format!("float4 {nom}[{n}];")), "{nom}");
        }
        assert!(SHADER_SOURCE.contains(&format!("float4 LueurArrets[{}];", n * crate::gfx::lueur::MAX_ARRETS)));
        assert!(SHADER_SOURCE.contains(&format!("LueurArrets[li * {} + k - 1]", crate::gfx::lueur::MAX_ARRETS)));
        // En-tête (1 registre) + quatre tableaux + les arrêts.
        assert_eq!(size_of::<crate::gfx::lueur::LueurGpu>(), 16 * (1 + 4 * n + n * crate::gfx::lueur::MAX_ARRETS));
    }

    #[test]
    fn viewport_constants_size_is_16_byte_aligned() {
        assert_eq!(size_of::<ViewportConstants>() % 16, 0);
        assert_eq!(std::mem::offset_of!(ViewportConstants, degrade), 48);
        // `160` → `176` avec le relief et la lumière (voir `DegradeGpu`, et son
        // `_remplissage` : le `cbuffer` se lit par registres de 16 octets).
        assert_eq!(size_of::<ViewportConstants>(), 48 + 192 + size_of::<crate::gfx::lueur::LueurGpu>() + 32 + 32 + 16 * crate::gfx::texture_perso::REGISTRES + 16, "les lueurs, les deux fondus, la texture (2 registres), la texture perso, l'ombre");
        assert_eq!(std::mem::offset_of!(ViewportConstants, lueur), 48 + 192);
        // Chaque fondu est UN registre entier (`float4 FonduBande`, puis
        // `FonduBandeDroite`), juste après.
        assert_eq!(std::mem::offset_of!(ViewportConstants, fondu), 48 + 192 + size_of::<crate::gfx::lueur::LueurGpu>());
        // La texture de l'interface : deux registres, puis la texture perso, la DERNIÈRE.
        assert_eq!(std::mem::offset_of!(ViewportConstants, texture), 48 + 192 + size_of::<crate::gfx::lueur::LueurGpu>() + 32);
        assert_eq!(std::mem::offset_of!(ViewportConstants, texture_perso), 48 + 192 + size_of::<crate::gfx::lueur::LueurGpu>() + 64);
        // Les ombres des barres : UN registre, le dernier.
        assert_eq!(std::mem::offset_of!(ViewportConstants, ombre) + 16, size_of::<ViewportConstants>());
    }

    /// ⭐ LE CALQUE DE TEXTURE (voir `QuadInstance::new_calque_texture`) : la
    /// texture SEULE, en alpha DROIT — du blanc où elle éclaircit, du noir où elle
    /// assombrit, l'alpha valant l'écart — calculée au MÊME site d'appel que celle
    /// des fonds (un second appel de `TextureFond` doublerait la compilation, voir
    /// `TexPersoFond`). Les constantes sont celles posées par `poser_texture`.
    #[test]
    fn le_calque_de_texture_sort_la_texture_seule() {
        let fond = SHADER_SOURCE.split("} else if (input.TexMode > 1.5 && input.TexMode < 4.5) {").nth(1).unwrap();
        let fond = fond.split("// Icône texturée").next().unwrap();
        let calque = fond.find("if (input.TexMode > 3.5) {").expect("le calque dans la branche du fond");
        assert!(calque > fond.find("float3 delta = texDelta;").unwrap(), "après la lecture de la texture");
        assert_eq!(SHADER_SOURCE.matches("TextureFond(input").count(), 1, "un seul site d'appel");
        assert!(SHADER_SOURCE.contains("if (input.TexMode > 2.5 && input.TexMode < 3.5) {"), "le glyphe ne prend plus le calque");
        assert_eq!(QuadInstance::new_calque_texture([0.0, 0.0, 10.0, 10.0]).tex_mode, 4.0);
    }

    /// ⭐ L'APLAT TEXTURÉ (voir `QuadInstance::avec_texture`) — DEMANDE EXPLICITE
    /// (2026-10-16) : « que la texture principale soit présente sur les
    /// indicateurs d'application ». Il reste un aplat (sa couleur, sans
    /// l'échantillon d'une image), reçoit la MÊME texture que les fonds, calculée
    /// UNE fois avant les branches, et ne prend pas la branche du fond.
    #[test]
    fn l_aplat_texture_recoit_la_texture_des_fonds() {
        let ps = SHADER_SOURCE.split("float4 PSMain(").nth(1).unwrap();
        let calcul = ps.find("texDelta = (TextureFond(input.Position.xy + DegOrigin) + TexClarte) * TexForce;").expect("le calcul unique");
        assert!(calcul < ps.find("float4 baseColor;").unwrap(), "avant les branches");
        let aplat = ps.split("// Icône texturée").nth(1).unwrap();
        assert!(aplat.contains("lerp(input.Color, texSample, input.TexMode > 4.5 ? 0.0 : input.TexMode)"), "l'aplat garde sa couleur");
        // ⛔ MESURÉ (2026-10-16, « je ne vois pas la texture sur les indicateurs ») :
        // ajoutée telle quelle, la texture avait le MÊME écart absolu que sur le
        // fond, mais un contraste RELATIF 2 à 3 fois plus faible sur un indicateur
        // vif (0,07-0,10 contre 0,19) — invisible. Elle y est donc RELATIVE :
        // vers le blanc quand elle éclaircit, vers le noir quand elle assombrit.
        assert!(aplat.contains("if (input.TexMode > 4.5) baseColor.rgb = AplatTexture(baseColor.rgb, texDelta);"), "l'aplat passe par sa fonction");
        let fonction = SHADER_SOURCE.split("float3 AplatTexture(").nth(1).expect("la fonction de l'aplat");
        assert!(fonction.contains("lerp(rgb, 1.0, ecart)") && fonction.contains("rgb * (1.0 - ecart)"), "relative, dans les deux sens");
        let pastille = QuadInstance::new([0.0, 0.0, 8.0, 8.0], [1.0; 4], 4.0, 1.0).avec_texture();
        assert_eq!(pastille.tex_mode, 5.0);
        let anneau = QuadInstance::new_ring([0.0, 0.0, 8.0, 8.0], [1.0; 4], 4.0, 2.0).avec_texture();
        assert_eq!((anneau.tex_mode, anneau.ring), (5.0, 2.0), "l'anneau reste un anneau");
    }

    /// ⭐ Les ombres des barres SOUS le contenu (demande du 2026-10-19) : leur
    /// calque, déclaré en DERNIER dans le `cbuffer`, se pose sur les fonds de
    /// barre seulement, avant l'alpha.
    #[test]
    fn les_ombres_se_posent_sous_le_contenu() {
        let cbuffer = SHADER_SOURCE.split("float4 TexPerso[").nth(1).unwrap();
        assert!(cbuffer.find("float4 Ombres;").unwrap() < cbuffer.find("};").unwrap(), "dans le cbuffer, après la texture perso");
        assert!(SHADER_SOURCE.contains("Texture2D OmbresTex : register(t2);"));
        let ps = SHADER_SOURCE.split("float4 PSMain(").nth(1).unwrap();
        let pose = ps.find("if (fond && Ombres.x > 0.5) {").expect("sur les fonds seulement, dans PSMain");
        assert!(pose < ps.find("float alpha = shapeAlpha").unwrap(), "avant l'alpha");
    }

    /// ⚠️ L'index d'un réglage de `texture_perso::REGLAGES` est sa place dans
    /// `TexPerso` : le shader doit lire chacun À SA PLACE (par `P`, que
    /// `TextureFond` remplit de `TexPerso`), et déclarer autant de registres que
    /// Rust en envoie.
    #[test]
    fn chaque_reglage_est_lu_a_sa_place_par_le_shader() {
        use crate::gfx::texture_perso::{REGISTRES, REGLAGES};
        assert!(SHADER_SOURCE.contains(&format!("float4 TexPerso[{REGISTRES}];")));
        assert!(SHADER_SOURCE.contains(&format!("float4 nature[{REGISTRES}] = {{")));
        assert!(SHADER_SOURCE.contains(&format!("[unroll] for (int i = 0; i < {REGISTRES}; i++) P[i] = n == 8 ? TexPerso[i] : nature[i];")));
        for (i, r) in REGLAGES.iter().enumerate() {
            let lecture = format!("float pp_{} = P[{}].{};", r.id, i / 4, ["x", "y", "z", "w"][i % 4]);
            assert!(SHADER_SOURCE.contains(&lecture), "absent : {lecture}");
        }
        assert!(SHADER_SOURCE.contains(&format!("float phase = P[{}].w;", REGISTRES - 1)));
    }

    /// ⭐ LES BRUITS DE BASE (voir `texture_perso::BASES`) : chaque index a sa
    /// branche dans `TexBruit`, et ⛔ `TexBruit` n'a qu'UN site d'appel (voir
    /// `TexPersoFond` : chaque copie allonge la compilation).
    #[test]
    fn chaque_bruit_de_base_a_sa_branche() {
        use crate::gfx::texture_perso::BASES;
        let bruit = SHADER_SOURCE.split("float3 TexBruit(").nth(1).unwrap().split("\n}\n").next().unwrap();
        let geometrie = SHADER_SOURCE.split("float TexGeometrie(").nth(1).unwrap().split("\n}\n").next().unwrap();
        let matiere = SHADER_SOURCE.split("float3 TexMatiere(").nth(1).unwrap().split("\n}\n").next().unwrap();
        let motif = SHADER_SOURCE.split("float3 TexMotif(").nth(1).unwrap().split("\n}\n").next().unwrap();
        for index in 0..BASES.len() {
            let present = match index {
                // Valeur : le dernier recours de `TexBruit`.
                0 => bruit.trim_end().ends_with("return TexValeur(p).xxx;"),
                // Les natures : `base - 3` (voir `TexNatureFond`).
                4..=10 => bruit.contains("if (base >= 4 && base <= 10)"),
                // Écailles : la fin de `TexGeometrie`, qui prend 15 à 22.
                22 => bruit.contains("if (base >= 15)") && bruit.find("base >= 23").unwrap() < bruit.find("base >= 15").unwrap(),
                // Les matières : `TexMatiere` (23 et plus), Truchet en dernier recours.
                32 => bruit.contains("if (base >= 23) return TexMatiere(") && bruit.find("base >= 33").unwrap() < bruit.find("base >= 23").unwrap() && matiere.trim_end().ends_with("return (1.0 - smoothstep(0.07, 0.11, d)).xxx;"),
                23..=27 | 30 | 31 => matiere.contains(&format!("if (base == {index})")),
                // Les motifs : `TexMotif` (33 et plus), les losanges en dernier recours.
                42 => index == BASES.len() - 1 && bruit.contains("if (base >= 33) return TexMotif(") && motif.trim_end().ends_with("return lerp(teinte, 0.9, trait * 0.6).xxx;"),
                33..=41 => motif.contains(&format!("if (base == {index})")),
                15..=21 if index != 20 => geometrie.contains(&format!("if (base == {index})")),
                _ => bruit.contains(&format!("base == {index}")),
            };
            assert!(present, "le bruit {index} ({}) n'a pas de branche", BASES[index]);
        }
        assert_eq!(SHADER_SOURCE.matches("TexBruit(ou").count(), 1, "un seul site d'appel");
        assert_eq!(SHADER_SOURCE.matches("TexWorley(p").count(), 1, "les cellules, d'un appel");
        assert_eq!(SHADER_SOURCE.matches("TexPersoFond(ecran").count(), 1);
    }

    /// La texture de l'interface (voir `gfx::texture_interface`) : déclarée
    /// APRÈS les fondus dans le `cbuffer`, dans l'ordre de `constantes()`, et
    /// appliquée au SEUL fond (`TexMode::Backdrop`), avant le tramage.
    #[test]
    fn la_texture_de_l_interface_ne_touche_que_les_fonds() {
        let cbuffer = SHADER_SOURCE.split("float4 FonduBandeDroite;").nth(1).unwrap();
        let ordre = ["float TexNature;", "float TexForce;", "float TexEchelle;", "float TexClarte;", "float3 TexChroma;", "float TexRemplissage;"];
        let positions: Vec<usize> = ordre.iter().map(|c| cbuffer.find(c).unwrap_or_else(|| panic!("{c}"))).collect();
        assert!(positions.windows(2).all(|p| p[0] < p[1]), "ordre du cbuffer");
        assert!(cbuffer.find("};").unwrap() > positions[5], "dans le cbuffer");
        // La clarté décale, la chroma colore — les deux dans le fond.
        assert!(SHADER_SOURCE.contains("texDelta = (TextureFond(input.Position.xy + DegOrigin) + TexClarte) * TexForce;"));
        assert!(SHADER_SOURCE.contains("abs(delta) * TexChroma"));
        // Appliquée dans la branche du fond, avant le tramage.
        let fond = SHADER_SOURCE.split("} else if (input.TexMode > 1.5 && input.TexMode < 4.5) {").nth(1).unwrap();
        let fond = fond.split("// Icône texturée").next().unwrap();
        let texture = fond.find("float3 delta = texDelta;").expect("texture dans la branche du fond");
        assert!(texture < fond.find("DitherHash(input.Position.xy)").unwrap(), "avant le tramage");
        // Une nature par index de `NATURES` (hors « aucune ») ; « Personnalisée »,
        // la dernière, lit la composition de l'applet.
        let natures = crate::gfx::texture_interface::NATURES;
        for index in 1..natures.len() - 1 {
            assert!(SHADER_SOURCE.contains(&format!("if (n == {index})")), "nature {index} absente du shader");
        }
        assert_eq!(natures[natures.len() - 1].0, "perso");
        assert!(SHADER_SOURCE.contains(&format!("n == {} ? TexPerso[i]", natures.len() - 1)));
    }

    /// La structure d'instance et le layout d'entrée doivent rester D'ACCORD :
    /// chaque champ a son élément, et `ring` est le dernier, à l'offset 48 (voir
    /// `create_input_layout`). Un champ ajouté sans son élément ferait lire au
    /// GPU un décalage faux — sans erreur de compilation, juste un rendu faux,
    /// exactement la classe de bug que la doc de `ViewportConstants` décrit.
    #[test]
    fn quad_instance_layout_matches_the_input_layout() {
        assert_eq!(size_of::<QuadInstance>(), 72);
        // Les DÉCALAGES eux-mêmes, et pas seulement la taille. Un champ
        // RÉORDONNÉ laisse la structure à 52 octets et décale tout ce qui suit :
        // `CreateInputLayout` réussit, le GPU lit des octets faux, et rien ne le
        // signale — c'est ce cas-là que ces assertions ferment. Ce sont les
        // valeurs écrites dans `create_input_layout`.
        assert_eq!(std::mem::offset_of!(QuadInstance, rect), 0);
        assert_eq!(std::mem::offset_of!(QuadInstance, color), 16);
        assert_eq!(std::mem::offset_of!(QuadInstance, radius), 32);
        assert_eq!(std::mem::offset_of!(QuadInstance, scale), 36);
        assert_eq!(std::mem::offset_of!(QuadInstance, tex_mode), 40);
        assert_eq!(std::mem::offset_of!(QuadInstance, rotation), 44);
        assert_eq!(std::mem::offset_of!(QuadInstance, ring), 48);
        assert_eq!(std::mem::offset_of!(QuadInstance, bascule), 52);
        assert_eq!(std::mem::offset_of!(QuadInstance, uv), 56);
        // La texture ENTIÈRE par défaut : toutes les instances existantes sont
        // inchangées (voir le champ `uv`).
        assert_eq!(QuadInstance::new_textured([0.0; 4], 0.0, 1.0).uv, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(QuadInstance::new_textured([0.0; 4], 0.0, 1.0).bascule, 0.0);
        // Aucun anneau par défaut : toutes les instances existantes sont
        // inchangées (voir le champ `ring`).
        assert_eq!(QuadInstance::new([0.0; 4], [0.0; 4], 0.0, 1.0).ring, 0.0);
        assert_eq!(QuadInstance::new_textured([0.0; 4], 0.0, 1.0).ring, 0.0);
        assert_eq!(QuadInstance::new_ring([0.0; 4], [0.0; 4], 0.0, 3.0).ring, 3.0);
    }

    /// Le shader est compilé À L'EXÉCUTION (voir `QuadRenderer::new`) : une
    /// erreur de syntaxe HLSL ne se verrait donc qu'au lancement de
    /// l'application, sous la forme d'un échec de création du renderer — et
    /// sans message lisible, l'application étant lancée détachée. Ce test la
    /// ramène au `cargo test`, où elle coûte une dizaine de secondes (un pixel
    /// shader PAR MOTIF du dégradé, voir `pixel_source`). Aucun pilote n'est
    /// requis : `D3DCompile` est un compilateur embarqué (voir `gfx::shader`).
    #[test]
    fn shader_source_compiles_for_both_stages() {
        crate::gfx::shader::compile_shader(SHADER_SOURCE, "VSMain", "vs_5_0").expect("le vertex shader doit compiler");
        for (mode, nom) in crate::gfx::degrade::MODES.iter().enumerate() {
            crate::gfx::shader::compile_shader(pixel_source(mode), "PSMain", "ps_5_0")
                .unwrap_or_else(|e| panic!("le pixel shader du motif {nom} doit compiler : {e}"));
        }
    }

    /// Sans découpage demandé, le rectangle est le viewport ENTIER — c'est
    /// l'invariant qui garantit qu'aucun rendu n'hérite du découpage du
    /// précédent (voir la doc du champ `rasterizer_state`).
    #[test]
    fn scissor_rect_defaults_to_the_whole_viewport() {
        let rect = scissor_rect(None, 800.0, 600.0);
        assert_eq!((rect.left, rect.top, rect.right, rect.bottom), (0, 0, 800, 600));
    }

    /// D3D11 exige un rectangle CONTENU dans la cible de rendu : tout
    /// débordement est ramené dans les bornes, jamais laissé tel quel.
    #[test]
    fn scissor_rect_clamps_to_the_render_target() {
        let rect = scissor_rect(Some([-40.0, -10.0, 2000.0, 2000.0]), 800.0, 600.0);
        assert_eq!((rect.left, rect.top, rect.right, rect.bottom), (0, 0, 800, 600));
    }

    /// Un rectangle dégénéré reste VALIDE (`right >= left`) au lieu de
    /// s'inverser : c'est le cas d'une barre masquée (largeur 0, voir
    /// `app::effective_primary_thicknesses`), qui doit ne rien dessiner sans
    /// pour autant produire un rectangle que D3D11 refuserait.
    #[test]
    fn scissor_rect_never_inverts() {
        let empty = scissor_rect(Some([12.0, 30.0, 0.0, 0.0]), 800.0, 600.0);
        assert_eq!((empty.left, empty.top, empty.right, empty.bottom), (12, 30, 12, 30));

        // Entièrement hors du viewport, à droite et en bas.
        let outside = scissor_rect(Some([900.0, 700.0, 50.0, 50.0]), 800.0, 600.0);
        assert_eq!((outside.left, outside.top, outside.right, outside.bottom), (800, 600, 800, 600));
    }

    /// La bande de découpage des barres latérales du dock (`Dock::lateral_clip`)
    /// est bien arrondie au pixel : le scissor travaille en entiers, et un
    /// `top_thickness` fractionnaire (il suit un curseur) ne doit pas décaler
    /// la coupure d'un pixel selon l'arrondi du jour.
    #[test]
    fn scissor_rect_rounds_fractional_edges() {
        let rect = scissor_rect(Some([0.0, 40.6, 800.0, 559.4]), 800.0, 600.0);
        assert_eq!((rect.left, rect.top, rect.right, rect.bottom), (0, 41, 800, 600));
    }
}

unsafe fn update_instance_buffer(
    ctx: &ID3D11DeviceContext,
    buffer: &ID3D11Buffer,
    instances: &[QuadInstance],
) -> Result<()> {
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    ctx.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))?;
    std::ptr::copy_nonoverlapping(
        instances.as_ptr(),
        mapped.pData as *mut QuadInstance,
        instances.len(),
    );
    ctx.Unmap(buffer, 0);
    Ok(())
}
