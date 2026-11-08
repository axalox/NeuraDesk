//! **LA TEXTURE DE L'INTERFACE** — ⭐ DEMANDE EXPLICITE (2026-10-15) : « une option
//! dans les thèmes pour ajouter des effets de textures à l'interface de l'app »,
//! sur le modèle d'un grain `feTurbulence` généré à la volée, sans image.
//!
//! **GÉNÉRÉE DANS LE SHADER, PAS EN SVG** : le fond des barres ET celui des
//! panneaux passent tous par le même chemin, `TexMode::Backdrop` de `PSMain`
//! (voir `gfx::quad_renderer`). Une texture posée là couvre toute l'interface
//! d'un coup, en COORDONNÉES ÉCRAN — une seule matière pour tout, comme le
//! dégradé animé. Un filtre SVG dans les pages CEF n'aurait couvert que le contenu
//! des pages, jamais les barres.
//!
//! **SEULS LES FONDS** : ni les icônes, ni le texte, ni les pastilles (les autres
//! `TexMode`). La texture s'AJOUTE à la couleur (± `force`), après la teinte, le
//! dégradé et les lueurs, et avant le tramage anti-bandes.
//!
//! ⚠️ **CE N'EST PAS UN RELIEF** : aucune lumière, aucun bord éclairé (le relief
//! des barres a été refusé trois fois). Une variation de luminosité à plat.
//!
//! **DÉSACTIVÉE PAR DÉFAUT** (`aucune`), même règle que les autres effets
//! permanents : c'est une décision de l'utilisateur.
//!
//! L'état vit dans des atomiques, relus à chaque rendu par
//! `quad_renderer::update_constant_buffer` : les Réglages agissent à chaud.

use std::sync::atomic::{AtomicU32, Ordering};

/// Les natures de texture : `(id, libellé)`. L'INDEX est celui que le shader lit
/// (`TexNature`, voir `TextureFond`) — ⚠️ ajouter EN FIN, jamais au milieu.
pub const NATURES: [(&str, &str); 9] = [
    ("aucune", "Aucune"),
    ("grain", "Grain"),
    ("papier", "Papier"),
    ("toile", "Toile"),
    ("lin", "Lin"),
    ("trame", "Trame"),
    ("sable", "Sable"),
    ("nuages", "Nuages"),
    // Composée dans l'applet Textures (voir `gfx::texture_perso`).
    ("perso", "Personnalisée"),
];

/// Force par défaut quand on choisit une texture, en fraction (0.4 = 40 %).
pub const DEFAULT_FORCE: f32 = 0.4;
/// Amplitude de luminosité à 100 % de force (± 12 %) — au-delà, la texture
/// salit le texte posé dessus.
pub const AMPLITUDE_MAX: f32 = 0.12;
/// Échelle du motif (1.0 = 100 %).
pub const DEFAULT_ECHELLE: f32 = 1.0;
pub const MIN_ECHELLE: f32 = 0.5;
pub const MAX_ECHELLE: f32 = 4.0;

/// Intensité de la couleur à pleine teinte : la chroma est multipliée par
/// l'amplitude de la texture, ce facteur la rend lisible sans la criarder.
const FORCE_CHROMA: f32 = 1.5;

/// ⭐ **DEUX JEUX DE RÉGLAGES** — DEMANDE EXPLICITE (2026-10-15) : « appliquer des
/// textures au fond des terminaux, mais pas forcément la même texture que pour le
/// reste de l'app ». `INTERFACE` habille les barres et les panneaux (le shader le
/// lit à chaque rendu) ; `TERMINAUX` habille le fond des terminaux (voir
/// `win32::texture_terminaux`, qui en fait une image pour la page). La texture
/// PERSONNALISÉE (`gfx::texture_perso`) reste une seule composition, commune.
pub struct ReglagesTexture {
    nature: AtomicU32,
    force: AtomicU32,
    echelle: AtomicU32,
    // ⭐ DEMANDE EXPLICITE (2026-10-15) : « un slider pour éclaircir ou assombrir la
    // texture et un autre pour régler son hue ». Clarté dans [-1, 1] (0 : la texture
    // éclaircit et assombrit autant), teinte en degrés, 0 = NEUTRE (sans couleur).
    clarte: AtomicU32,
    teinte: AtomicU32,
}

/// Les réglages communs à toutes les natures, en valeurs (force en fraction,
/// échelle en facteur, clarté dans [-1, 1], teinte en degrés).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rendu {
    pub force: f32,
    pub echelle: f32,
    pub clarte: f32,
    pub teinte: f32,
}

/// La texture des barres et des panneaux.
pub static INTERFACE: ReglagesTexture = ReglagesTexture::new();
/// La texture du fond des terminaux (Terminal et SSH).
pub static TERMINAUX: ReglagesTexture = ReglagesTexture::new();

impl ReglagesTexture {
    /// Désactivée (`aucune`), force et échelle par défaut, clarté et teinte neutres.
    pub const fn new() -> Self {
        Self {
            nature: AtomicU32::new(0),
            force: AtomicU32::new(DEFAULT_FORCE.to_bits()),
            echelle: AtomicU32::new(DEFAULT_ECHELLE.to_bits()),
            clarte: AtomicU32::new(0),
            teinte: AtomicU32::new(0),
        }
    }

    /// L'index de la nature active (0 = aucune).
    pub fn nature(&self) -> usize {
        self.nature.load(Ordering::Relaxed) as usize
    }

    pub fn nature_id(&self) -> &'static str {
        NATURES[self.nature().min(NATURES.len() - 1)].0
    }

    /// Choisit la nature par son ID ; un ID inconnu est IGNORÉ (rend `false`).
    pub fn set_nature_id(&self, id: &str) -> bool {
        match NATURES.iter().position(|(n, _)| *n == id) {
            Some(index) => {
                self.nature.store(index as u32, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    pub fn force(&self) -> f32 {
        f32::from_bits(self.force.load(Ordering::Relaxed))
    }

    /// Force de 0 à 1, bornée ; `NaN` est ignoré (`f32::clamp` le propagerait).
    pub fn set_force(&self, force: f32) {
        if !force.is_nan() {
            self.force.store(force.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        }
    }

    pub fn echelle(&self) -> f32 {
        f32::from_bits(self.echelle.load(Ordering::Relaxed))
    }

    /// Échelle bornée à `[MIN_ECHELLE, MAX_ECHELLE]` ; `NaN` est ignoré.
    pub fn set_echelle(&self, echelle: f32) {
        if !echelle.is_nan() {
            self.echelle.store(echelle.clamp(MIN_ECHELLE, MAX_ECHELLE).to_bits(), Ordering::Relaxed);
        }
    }

    pub fn clarte(&self) -> f32 {
        f32::from_bits(self.clarte.load(Ordering::Relaxed))
    }

    /// Clarté dans `[-1, 1]` : négative, la texture assombrit ; positive, elle
    /// éclaircit. `NaN` est ignoré.
    pub fn set_clarte(&self, clarte: f32) {
        if !clarte.is_nan() {
            self.clarte.store(clarte.clamp(-1.0, 1.0).to_bits(), Ordering::Relaxed);
        }
    }

    pub fn teinte(&self) -> f32 {
        f32::from_bits(self.teinte.load(Ordering::Relaxed))
    }

    /// Teinte en degrés, `[0, 360]` ; 0 = neutre. `NaN` est ignoré.
    pub fn set_teinte(&self, teinte: f32) {
        if !teinte.is_nan() {
            self.teinte.store(teinte.clamp(0.0, 360.0).to_bits(), Ordering::Relaxed);
        }
    }

    /// Force, échelle, clarté et teinte — ce qu'une texture de la bibliothèque
    /// emporte avec son motif (voir `texture_perso::Texture`).
    pub fn rendu(&self) -> Rendu {
        Rendu { force: self.force(), echelle: self.echelle(), clarte: self.clarte(), teinte: self.teinte() }
    }

    /// Pose les quatre, bornés comme par leurs curseurs.
    pub fn poser_rendu(&self, rendu: Rendu) {
        self.set_force(rendu.force);
        self.set_echelle(rendu.echelle);
        self.set_clarte(rendu.clarte);
        self.set_teinte(rendu.teinte);
    }

    /// Les deux registres du `cbuffer` : `TexNature`, `TexForce` (déjà en
    /// amplitude de luminosité), `TexEchelle`, `TexClarte`, puis `TexChroma` et un
    /// remplissage. Force nulle si aucune texture : le shader saute tout le calcul.
    pub(crate) fn constantes(&self) -> [[f32; 4]; 2] {
        let nature = self.nature();
        let force = if nature == 0 { 0.0 } else { self.force() * AMPLITUDE_MAX };
        let [r, g, b] = chroma_de_teinte(self.teinte());
        [[nature as f32, force, self.echelle(), self.clarte()], [r, g, b, 0.0]]
    }

    /// « id,force,échelle,clarté,teinte » — un seul écrivain. Les deux derniers
    /// champs sont venus après : un fichier à trois champs se relit encore.
    pub fn serialiser(&self) -> String {
        format!("{},{},{},{},{}", self.nature_id(), self.force(), self.echelle(), self.clarte(), self.teinte())
    }

    /// Relit `serialiser`. Un champ illisible garde sa valeur courante.
    pub fn charger(&self, contenu: &str) {
        let mut champs = contenu.trim().split(',').map(str::trim);
        if let Some(id) = champs.next() {
            self.set_nature_id(id);
        }
        if let Some(Ok(force)) = champs.next().map(str::parse::<f32>) {
            self.set_force(force);
        }
        if let Some(Ok(echelle)) = champs.next().map(str::parse::<f32>) {
            self.set_echelle(echelle);
        }
        if let Some(Ok(clarte)) = champs.next().map(str::parse::<f32>) {
            self.set_clarte(clarte);
        }
        if let Some(Ok(teinte)) = champs.next().map(str::parse::<f32>) {
            self.set_teinte(teinte);
        }
    }
}

// Les fonctions libres sont celles de l'INTERFACE — le shader, les Réglages et la
// configuration les appelaient avant qu'il y ait deux jeux.
#[cfg(test)]
pub fn nature() -> usize {
    INTERFACE.nature()
}

pub fn nature_id() -> &'static str {
    INTERFACE.nature_id()
}

pub fn set_nature_id(id: &str) -> bool {
    INTERFACE.set_nature_id(id)
}

pub fn force() -> f32 {
    INTERFACE.force()
}

pub fn set_force(force: f32) {
    INTERFACE.set_force(force)
}

pub fn echelle() -> f32 {
    INTERFACE.echelle()
}

pub fn set_echelle(echelle: f32) {
    INTERFACE.set_echelle(echelle)
}

pub fn clarte() -> f32 {
    INTERFACE.clarte()
}

pub fn set_clarte(clarte: f32) {
    INTERFACE.set_clarte(clarte)
}

pub fn teinte() -> f32 {
    INTERFACE.teinte()
}

pub fn set_teinte(teinte: f32) {
    INTERFACE.set_teinte(teinte)
}

/// La COULEUR d'une teinte, privée de sa luminance : la teinte pure (TSV à
/// pleine saturation) moins sa luminance, donc un vecteur qui colore sans
/// éclaircir ni assombrir — la clarté garde ce rôle seule. 0 : neutre.
pub(crate) fn chroma_de_teinte(teinte: f32) -> [f32; 3] {
    if !(teinte > 0.0) {
        return [0.0; 3];
    }
    let h = (teinte % 360.0) / 60.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let [r, g, b]: [f32; 3] = match h as u32 {
        0 => [1.0, x, 0.0],
        1 => [x, 1.0, 0.0],
        2 => [0.0, 1.0, x],
        3 => [0.0, x, 1.0],
        4 => [x, 0.0, 1.0],
        _ => [1.0, 0.0, x],
    };
    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    [r - luma, g - luma, b - luma].map(|c| c * FORCE_CHROMA)
}

/// ⭐ **LE CALQUE de `reglages`**, `largeur × hauteur` pixels, en RGBA à alpha
/// DROIT (voir `QuadInstance::new_calque_texture`) : rendu hors écran par le
/// shader de l'interface — mêmes natures, même échelle, au pixel près —, puis
/// relu. `perso` : la composition de la nature « Personnalisée ». `None` : aucune
/// texture, ou le GPU a refusé.
pub fn calque_rgba(
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    ctx: &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    reglages: &ReglagesTexture,
    perso: &crate::gfx::texture_perso::Composition,
    largeur: u32,
    hauteur: u32,
) -> Option<Vec<u8>> {
    let renderer = crate::gfx::QuadRenderer::new(device).ok()?;
    calque_rgba_a(device, ctx, &renderer, reglages, perso, (largeur, hauteur), (0, 0))
}

/// `calque_rgba` avec un `renderer` déjà prêt (le compiler coûte cher), pour le
/// rectangle d'écran qui commence à `origine` : le motif y est celui que
/// reçoivent les barres à cet endroit (voir `win32::ombre_barre`).
pub fn calque_rgba_a(
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    ctx: &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    renderer: &crate::gfx::QuadRenderer,
    reglages: &ReglagesTexture,
    perso: &crate::gfx::texture_perso::Composition,
    (largeur, hauteur): (u32, u32),
    origine: (i32, i32),
) -> Option<Vec<u8>> {
    use windows::Win32::Graphics::Direct3D11::{
        ID3D11RenderTargetView, ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
    if reglages.nature() == 0 || largeur == 0 || hauteur == 0 {
        return None;
    }
    let desc = D3D11_TEXTURE2D_DESC {
        Width: largeur,
        Height: hauteur,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
        ..Default::default()
    };
    let mut cible: Option<ID3D11Texture2D> = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut cible)) }.ok()?;
    let cible = cible?;
    let mut rtv: Option<ID3D11RenderTargetView> = None;
    unsafe { device.CreateRenderTargetView(&cible, None, Some(&mut rtv)) }.ok()?;
    let rtv = rtv?;
    renderer.poser_texture(reglages.constantes(), perso.constantes());
    let (l, h) = (largeur as f32, hauteur as f32);
    let quad = [crate::gfx::QuadInstance::new_calque_texture([0.0, 0.0, l, h])];
    renderer.render_at(ctx, &rtv, l, h, &[crate::gfx::QuadBatch::new(None, &quad)], origine).ok()?;
    let (_, _, rgba) = crate::gfx::capture::capture_rgba(device, ctx, &cible).ok()?;
    Some(rgba)
}

/// Les constantes de l'INTERFACE, lues par le shader à chaque rendu.
pub(crate) fn constantes() -> [[f32; 4]; 2] {
    INTERFACE.constantes()
}

/// Le fichier de réglage de l'interface (voir `ReglagesTexture::serialiser`).
pub const FICHIER: &str = "texture_interface.txt";
/// Celui des terminaux, même format.
pub const FICHIER_TERMINAUX: &str = "texture_terminaux.txt";

pub fn serialiser() -> String {
    INTERFACE.serialiser()
}

pub fn charger(contenu: &str) {
    INTERFACE.charger(contenu)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les atomiques sont GLOBAUX : un seul test les touche, pour ne pas se
    /// battre avec un autre fil de test.
    #[test]
    fn l_etat_se_borne_se_relit_et_s_eteint_par_defaut() {
        assert_eq!(nature_id(), "aucune", "désactivée par défaut");
        assert_eq!(constantes()[0][1], 0.0, "aucune texture : force nulle au shader");
        assert!(!set_nature_id("inconnue"));
        assert!(set_nature_id("papier"));
        assert_eq!(nature(), 2);
        set_force(3.0);
        assert_eq!(force(), 1.0);
        set_force(f32::NAN);
        assert_eq!(force(), 1.0);
        assert!((constantes()[0][1] - AMPLITUDE_MAX).abs() < 1e-6);
        set_echelle(0.0);
        assert_eq!(echelle(), MIN_ECHELLE);
        set_echelle(99.0);
        assert_eq!(echelle(), MAX_ECHELLE);

        // ⭐ Clarté et teinte (DEMANDE EXPLICITE, 2026-10-15) : bornées, NaN ignoré.
        assert_eq!((clarte(), teinte()), (0.0, 0.0), "neutres par défaut");
        set_clarte(-5.0);
        assert_eq!(clarte(), -1.0);
        set_clarte(f32::NAN);
        assert_eq!(clarte(), -1.0);
        set_teinte(400.0);
        assert_eq!(teinte(), 360.0);
        set_teinte(-3.0);
        assert_eq!(teinte(), 0.0);

        set_force(0.25);
        set_echelle(2.0);
        set_clarte(0.5);
        set_teinte(120.0);
        let texte = serialiser();
        assert_eq!(texte, "papier,0.25,2,0.5,120");
        // On repart d'ailleurs, puis on relit.
        set_nature_id("aucune");
        set_force(DEFAULT_FORCE);
        set_echelle(DEFAULT_ECHELLE);
        set_clarte(0.0);
        set_teinte(0.0);
        charger(&texte);
        assert_eq!((nature_id(), force(), echelle(), clarte(), teinte()), ("papier", 0.25, 2.0, 0.5, 120.0));
        // L'ANCIEN fichier, à trois champs, se relit encore (clarté et teinte gardées).
        charger("toile,0.3,1");
        assert_eq!((nature_id(), force(), echelle(), clarte(), teinte()), ("toile", 0.3, 1.0, 0.5, 120.0));
        // Un fichier abîmé ne casse rien.
        charger("trame, n'importe quoi");
        assert_eq!((nature_id(), force(), echelle()), ("trame", 0.3, 1.0));

        set_nature_id("aucune");
        set_force(DEFAULT_FORCE);
        set_echelle(DEFAULT_ECHELLE);
        set_clarte(0.0);
        set_teinte(0.0);
    }

    /// La teinte devient une CHROMA sans luminance : elle colore la texture sans
    /// l'éclaircir ni l'assombrir (c'est le rôle de la clarté). Teinte 0 : neutre.
    #[test]
    fn la_teinte_colore_sans_changer_la_luminance() {
        assert_eq!(chroma_de_teinte(0.0), [0.0; 3], "0 : neutre");
        for teinte in [1.0, 60.0, 120.0, 200.0, 300.0, 360.0] {
            let c = chroma_de_teinte(teinte);
            let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            assert!(luma.abs() < 1e-5, "{teinte} : luminance {luma}");
            assert!(c.iter().any(|x| x.abs() > 0.1), "{teinte} : pas de couleur");
        }
        // Le rouge pousse le canal rouge, le bleu le canal bleu.
        assert!(chroma_de_teinte(360.0)[0] > 0.0);
        assert!(chroma_de_teinte(240.0)[2] > 0.0);
    }

    /// ⭐ Les terminaux ont LEUR texture : la changer ne touche pas celle de
    /// l'interface, et elle se relit de son propre fichier.
    #[test]
    fn la_texture_des_terminaux_est_independante() {
        let terminaux = ReglagesTexture::new();
        assert_eq!(terminaux.nature_id(), "aucune", "désactivée par défaut");
        assert!(terminaux.set_nature_id("nuages"));
        terminaux.set_force(0.8);
        terminaux.set_teinte(200.0);
        assert_eq!(terminaux.serialiser(), "nuages,0.8,1,0,200");
        let relue = ReglagesTexture::new();
        relue.charger(&terminaux.serialiser());
        assert_eq!(relue.constantes(), terminaux.constantes());
        assert_eq!(relue.constantes()[0][0], 7.0, "l'index de « nuages » pour le shader");
        assert_ne!(FICHIER, FICHIER_TERMINAUX, "chacun son fichier");
    }

    #[test]
    fn les_ids_sont_uniques() {
        let mut ids: Vec<_> = NATURES.iter().map(|(id, _)| *id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), NATURES.len());
        assert_eq!(NATURES[0].0, "aucune", "l'index 0 est « aucune » pour le shader");
    }
}
