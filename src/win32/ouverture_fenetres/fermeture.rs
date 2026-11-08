//! ⭐ **L'EFFET DE FERMETURE** — DEMANDE EXPLICITE (2026-10-16) : « un effet de
//! fermeture qui fasse se réduire la fenêtre en la déplaçant jusqu'au point d'où
//! elle est apparue, la durée de l'effet doit être paramétrable » ; puis
//! (2026-10-17) : « il faudrait que l'effet de réduction puisse s'appliquer sur
//! n'importe quelle fenêtre pas seulement une ».
//!
//! **LE PROBLÈME : UNE FENÊTRE FERMÉE N'A PLUS D'IMAGE.** Quand Windows annonce
//! qu'elle se cache (`EVENT_OBJECT_HIDE`), il est trop tard pour la capturer. On
//! garde donc, À L'AVANCE, une image de CHAQUE fenêtre visible du bureau : une
//! session de capture par fenêtre, dont la dernière image est recopiée dans une
//! texture à nous (`Instantane`).
//!
//! ⛔ **LES SESSIONS RESTENT OUVERTES** — MESURÉ : chaque session FERMÉE après
//! avoir reçu une image ajoute ~15 Mo au compteur « mémoire GPU dédiée » du
//! processus (celui du Gestionnaire des tâches), qui ne redescend jamais — alors
//! que la mémoire de la carte, elle, ne bouge pas. Photographier les fenêtres par
//! sessions brèves faisait monter ce compteur de 450 Mo par minute.
//!
//! - **Les fenêtres sont relevées tous les `SCRUTATION`**, de haut en bas : au
//!   plus `MAX_SUIVIES`, les plus hautes d'abord.
//! - **Une session ouverte par passage du fil** : en ouvrir une le bloque 100 à
//!   600 ms.
//! - **Copier ne coûte que si la fenêtre change** : WGC ne livre une image que
//!   sur un changement ; au premier plan au plus tous les `PAS_DE_COPIE`,
//!   ailleurs tous les `PAS_DE_COPIE_ARRIERE`, dans la même texture tant que la
//!   taille ne change pas.
//! - **La fenêtre qui vient de s'ouvrir passe en tête de file** (`adopter`).
//! - **Pas pendant une animation** : ouvrir une session bloque le fil, et
//!   saccaderait le vol en cours.
//! - ⛔ **Pas une fenêtre qui couvre l'écran** (jeu, vidéo) : capturer un jeu en
//!   continu lui coûterait.
//! - Une fenêtre réduite n'est pas suivie : sa fermeture ne se voit pas.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11ShaderResourceView, ID3D11Texture2D, D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible};

use super::{candidate, ecran_de, en_f32, est_masquee_par_dwm, icone_de, image_vide, rect_visible, Image};
use crate::gfx::device::GfxDevice;
use crate::win32::wgc::CaptureSession;

/// Tous les combien on relève les fenêtres du bureau.
const SCRUTATION: Duration = Duration::from_secs(1);
/// Au plus ce nombre de fenêtres suivies — les plus hautes.
const MAX_SUIVIES: usize = 24;
/// Pas plus d'une copie dans ce délai pour la fenêtre au premier plan…
const PAS_DE_COPIE: Duration = Duration::from_millis(200);
/// … et dans celui-ci pour les autres.
const PAS_DE_COPIE_ARRIERE: Duration = Duration::from_secs(1);
/// ⛔ MESURÉ : la première image d'une session est souvent NOIRE, et une fenêtre
/// immobile n'en livre plus d'autre — elle n'aurait jamais d'image. Sa session
/// est donc rouverte, au plus ce nombre de fois ; ensuite l'image est prise
/// telle quelle (la fenêtre est peut-être vraiment noire).
const ESSAIS_MAX: u8 = 3;

/// La dernière image d'une fenêtre, dans une texture à nous.
struct Instantane {
    texture: ID3D11Texture2D,
    srv: ID3D11ShaderResourceView,
    rect: [f32; 4],
    ecran: RECT,
    echelle: f32,
}

/// La session sur une fenêtre, et sa dernière image.
struct Suivie {
    session: CaptureSession,
    /// Où elle retourne : l'icône de son application, lue à l'ouverture de la
    /// session (une fenêtre cachée n'a plus d'identité lisible).
    icone: Option<[f32; 4]>,
    /// Une première image vérifiée non vide : les suivantes ne le sont plus.
    verifiee: bool,
    derniere_copie: Option<Instant>,
    image: Option<Instantane>,
}

struct Retour {
    image: Instantane,
    arrivee: [f32; 4],
    debut: Instant,
    /// La forme du retour, lue à la création (voir `super::effets`).
    effet: super::effets::Effet,
    /// Une fenêtre de NeuraDesk qui s'efface en place (voir `super::fondu_actif`
    /// et `super::fondu_propre`).
    fondu: bool,
    /// Durée FIGÉE à la création : le fondu propre d'une fenêtre (la sphère des
    /// programmes) n'est pas relu à chaque image, et `fondu` décide déjà de la
    /// forme du retour.
    duree_du_retour: Duration,
}

impl Retour {
    fn duree(&self) -> Duration {
        self.duree_du_retour
    }
}

pub(super) struct Fermetures {
    suivies: HashMap<isize, Suivie>,
    /// Les fenêtres éligibles du dernier relevé, de haut en bas, pas encore suivies.
    a_suivre: Vec<isize>,
    derniere_scrutation: Option<Instant>,
    /// Les sessions rouvertes faute d'une vraie première image (voir `ESSAIS_MAX`).
    essais: HashMap<isize, u8>,
    retours: Vec<Retour>,
    /// Les fenêtres de NeuraDesk animées à l'ouverture (seules les nôtres qu'on suive).
    pub(super) internes: HashSet<isize>,
    /// D'où chaque fenêtre est apparue (le départ de son vol d'ouverture).
    pub(super) origines: HashMap<isize, [f32; 4]>,
}

impl Fermetures {
    pub(super) fn new() -> Self {
        Self {
            suivies: HashMap::new(),
            a_suivre: Vec::new(),
            derniere_scrutation: None,
            essais: HashMap::new(),
            retours: Vec::new(),
            internes: HashSet::new(),
            origines: HashMap::new(),
        }
    }

    pub(super) fn anime(&self) -> bool {
        !self.retours.is_empty()
    }

    /// Les fenêtres dont la disparition doit être signalée au fil (voir
    /// `rappel_masquage`) : celles dont on a une image, triées.
    pub(super) fn surveillees(&self) -> Vec<isize> {
        let mut hwnds: Vec<isize> = self.suivies.iter().filter(|(_, s)| s.image.is_some()).map(|(&h, _)| h).collect();
        hwnds.sort_unstable();
        hwnds
    }

    /// ⭐ **UN GESTE RETENU PAR LE MODULE INJECTÉ** — DEMANDE EXPLICITE
    /// (2026-11-04, voie B) : `window-hook` retient `WM_CLOSE` (fermer) ou
    /// `SC_MINIMIZE` (réduire) dans l'application et nous prévient AU MOMENT de
    /// la retenue (voir `super::geste_retenu`). On capture donc ici, tout de
    /// suite, et on joue le retour pendant que l'application attend.
    ///
    /// **L'ORDRE COMPTE** : capturer D'ABORD (la fenêtre est encore visible),
    /// vider sa région ENSUITE (elle disparaît sans repeinte, voir
    /// `super::vider_la_region`), animer enfin. Si on vidait la région d'abord,
    /// `PrintWindow` ne rendrait qu'un aplat — mesuré.
    ///
    /// `false` si la fenêtre ne répond pas ou si la capture échoue : l'appelant
    /// laisse alors le geste se faire sans effet, ce qui est toujours
    /// préférable à une fenêtre qu'on garderait cachée pour rien.
    pub(super) fn animer_geste(&mut self, gfx: &GfxDevice, hwnd: HWND, geste: super::Geste, retenue_ms: u64) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsZoomed};
        // ⛔ **L'INTERRUPTEUR DES RÉGLAGES GOUVERNE AUSSI LES GESTES RETENUS** —
        // MESURÉ (2026-11-05) : décoché, « Animer les fenêtres qui se ferment… »
        // n'arrêtait que le chemin SANS injection ; les quatre gestes des
        // applications injectées continuaient à s'animer, un réglage qui ne
        // réglait rien. Le module ne retient plus rien non plus quand la durée
        // vaut 0 (voir `win32::injection::regler_retenue`) : ici on ne fait que
        // refuser un geste arrivé d'une DLL encore chargée avec l'ancienne durée.
        if !super::fermeture_active() {
            crate::diag!("[fermeture] {:#x} geste {geste:?} ignoré : les effets sont éteints", hwnd.0 as isize);
            return false;
        }
        // ⛔ Le lancement ne passe JAMAIS par ici : il suit le pipeline
        // d'ouverture (`Evenement::Lancement`). Cette garde garde les `match
        // geste` ci-dessous exhaustifs sans arm factice.
        if geste == super::Geste::Lancer {
            return false;
        }
        // Agrandir une fenêtre déjà agrandie (ou restaurer une fenêtre qui ne
        // l'est pas) ne change RIEN à l'écran : pas d'effet, et surtout pas de
        // fenêtre cachée pour rien.
        let deja = match geste {
            super::Geste::Agrandir => unsafe { IsZoomed(hwnd) }.as_bool(),
            super::Geste::Restaurer => !unsafe { IsZoomed(hwnd) }.as_bool(),
            _ => false,
        };
        if deja {
            crate::diag!("[fermeture] {:#x} : le geste ne changerait rien — pas d'effet", hwnd.0 as isize);
            return false;
        }
        let (Some(rect), Some(ecran)) = (rect_visible(hwnd), ecran_de(hwnd)) else {
            crate::diag!("[fermeture] {:#x} : rectangle ou écran illisible — pas d'effet", hwnd.0 as isize);
            return false;
        };
        let Some(mut capture) = crate::win32::capture_fenetre::Capture::start(hwnd) else {
            crate::diag!("[fermeture] {:#x} : capture impossible — pas d'effet", hwnd.0 as isize);
            return false;
        };
        if !capture.poll(&gfx.device) {
            crate::diag!("[fermeture] {:#x} : la capture n'a rien rendu — pas d'effet", hwnd.0 as isize);
            return false;
        }
        let Some(srv) = capture.srv().cloned() else {
            crate::diag!("[fermeture] {:#x} : texture de la capture absente — pas d'effet", hwnd.0 as isize);
            return false;
        };
        let Ok(texture) = (unsafe { srv.GetResource() })
            .and_then(|ressource| windows::core::Interface::cast::<ID3D11Texture2D>(&ressource))
        else {
            crate::diag!("[fermeture] {:#x} : la texture n'est pas une Texture2D — pas d'effet", hwnd.0 as isize);
            return false;
        };

        // ⛔ La retenue est le PLAFOND : l'effet doit finir AVANT qu'elle ne
        // soit levée, sinon la fenêtre réapparaîtrait le temps que le message
        // soit traité.
        //
        // ⭐ **SAUF POUR AGRANDIR ET RESTAURER** : la fenêtre ne réapparaît pas
        // dans son ANCIEN rectangle, elle en change. L'image doit donc couvrir
        // la libération du message : elle dure 150 ms DE PLUS, garde sa taille
        // finale (le `t` est borné), et c'est elle qui se retire de la fenêtre
        // déjà agrandie — sans un trou de bureau entre les deux.
        // Ce que le geste fera de la fenêtre : disparaître vers son icône
        // (fermer, réduire), ou aller d'un rectangle à un autre (agrandir,
        // restaurer). ⭐ DEPUIS LE 2026-11-05, CHAQUE GESTE A SA FORME, réglée
        // dans les Réglages ; l'étirement reste le défaut d'agrandir/restaurer.
        // ⛔ Quand un agrandissement prend une AUTRE forme, il en emprunte le
        // sens : la fenêtre disparaît vers son icône, et c'est la vraie qui
        // apparaît agrandie à la libération du message.
        let (arrivee, effet, fondu) = match geste {
            super::Geste::Fermer | super::Geste::Reduire => {
                let icone = icone_de(hwnd);
                let moment = if geste == super::Geste::Fermer { super::Moment::Fermeture } else { super::Moment::Reduction };
                // Sans icône (fenêtre sans application identifiable), elle
                // s'efface en place plutôt que de voler vers un point arbitraire.
                (icone.unwrap_or_else(|| point_central(en_f32(rect))), super::effet_du_moment(moment), icone.is_none())
            }
            super::Geste::Agrandir | super::Geste::Restaurer => {
                let effet = super::effet_du_moment(super::Moment::Agrandissement);
                if effet == super::effets::Effet::Etirer {
                    let cible = if geste == super::Geste::Agrandir {
                        let Some(travail) = super::ecran_de_travail(hwnd) else {
                            crate::diag!("[fermeture] {:#x} : écran de travail illisible — pas d'effet", hwnd.0 as isize);
                            return false;
                        };
                        en_f32(travail)
                    } else {
                        let Some(repos) = super::rect_restaure(hwnd) else {
                            crate::diag!("[fermeture] {:#x} : rectangle de repos illisible — pas d'effet", hwnd.0 as isize);
                            return false;
                        };
                        en_f32(repos)
                    };
                    (cible, effet, false)
                } else {
                    let icone = icone_de(hwnd);
                    (icone.unwrap_or_else(|| point_central(en_f32(rect))), effet, icone.is_none())
                }
            }
            // Écarté en tête de fonction (routé vers le pipeline d'ouverture).
            super::Geste::Lancer => unreachable!("le lancement ne passe pas par animer_geste"),
        };
        // ⛔ Seul l'étirement couvre la libération du message (voir la note
        // ci-dessus) : une forme qui DISPARAÎT doit finir AVANT elle, comme une
        // fermeture.
        let duree_du_retour = if effet == super::effets::Effet::Etirer {
            Duration::from_millis(retenue_ms + 150)
        } else {
            Duration::from_millis(retenue_ms.saturating_sub(60).max(80))
        };
        let echelle = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
        let image = Instantane { texture, srv, rect: en_f32(rect), ecran, echelle };
        self.retours.push(Retour { image, arrivee, debut: Instant::now(), effet, fondu, duree_du_retour });

        // La fenêtre s'efface MAINTENANT ; sa région lui est rendue dès qu'on
        // sait ce qu'elle est devenue.
        super::vider_la_region(hwnd);
        let cle = hwnd.0 as isize;
        let quelle_etape = match geste {
            super::Geste::Fermer => 0,
            super::Geste::Reduire => 1,
            super::Geste::Agrandir => 2,
            super::Geste::Restaurer => 3,
            super::Geste::Lancer => unreachable!("le lancement ne passe pas par animer_geste"),
        };
        std::thread::spawn(move || {
            let hwnd = HWND(cle as _);
            let is_window = || unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)) }.as_bool();
            // ⛔ **LA RÉGION ATTEND L'ÉTAT FINAL.** Rendue à l'heure de l'effet,
            // elle ferait réapparaître la fenêtre dans son ANCIEN rectangle le
            // temps que l'application traite le message — la plus visible des
            // coutures. Sondée jusqu'à 3 s, puis rendue quand même : une
            // application qui refuse le geste ne doit pas rester invisible.
            let atteinte = |etape: i32| match etape {
                0 => true,                                     // fermée : après l'effet
                1 => unsafe { IsIconic(hwnd) }.as_bool(),      // réduite
                2 => unsafe { IsZoomed(hwnd) }.as_bool(),      // agrandie
                _ => !unsafe { IsZoomed(hwnd) }.as_bool() && !unsafe { IsIconic(hwnd) }.as_bool(), // restaurée
            };
            if quelle_etape == 0 {
                // Une boîte « Enregistrer ? » qui aurait survécu : on lui rend
                // sa région après l'effet.
                std::thread::sleep(duree_du_retour + Duration::from_millis(400));
            } else {
                let debut = Instant::now();
                while is_window() && !atteinte(quelle_etape) && debut.elapsed() < Duration::from_secs(3) {
                    std::thread::sleep(Duration::from_millis(15));
                }
            }
            if is_window() {
                super::rendre_la_region(hwnd);
            }
        });
        let quoi = match geste {
            super::Geste::Fermer => "fermeture",
            super::Geste::Reduire => "réduction",
            super::Geste::Agrandir => "agrandissement",
            super::Geste::Restaurer => "restauration",
            super::Geste::Lancer => unreachable!("le lancement ne passe pas par animer_geste"),
        };
        // ⛔ **ON CESSE DE LA SUIVRE** — MESURÉ (2026-11-05) : une fermeture
        // retenue jouait DEUX effets. Le premier est le nôtre ; puis la fenêtre
        // applique enfin `WM_CLOSE`, le crochet signale sa disparition
        // (`rappel_masquage`), et `masquee` rejouait un retour avec la dernière
        // image suivie — 1,5 s après, par-dessus la fin du premier.
        self.oublier(hwnd.0 as isize);
        crate::diag!("[fermeture] {:#x} {quoi} retenue {retenue_ms} ms, forme {effet:?} : capture et retour joués", hwnd.0 as isize);
        true
    }

    /// **UNE FENÊTRE QUI VIENT DE SE POSER PASSE EN TÊTE DE FILE** : sa session
    /// s'ouvre au tick suivant (`suivre_la_suivante`), donc son image est prête
    /// aussi vite qu'avant.
    ///
    /// ⛔ **CE N'EST PLUS LA SESSION DE L'OUVERTURE, ET C'EST VOULU** — MESURÉ
    /// (2026-11-04) : l'ouverture capture désormais par `PrintWindow` (voir
    /// `super::commencer_la_capture`), qui n'a aucune session `wgc` à céder.
    /// L'ancienne reprise évitait surtout de rouvrir une session ; ici elle
    /// s'ouvre une frame plus tard, et l'effet de fermeture ne perd rien.
    pub(super) fn adopter(&mut self, hwnd: HWND) {
        if !self.eligible(hwnd) {
            return;
        }
        let cle = hwnd.0 as isize;
        if !self.a_suivre.contains(&cle) {
            self.a_suivre.insert(0, cle);
        }
    }

    pub(super) fn oublier(&mut self, hwnd: isize) {
        self.internes.remove(&hwnd);
        self.origines.remove(&hwnd);
        self.essais.remove(&hwnd);
        self.suivies.remove(&hwnd);
    }

    /// Une fenêtre s'est cachée : si on a son image, elle repart d'où elle venait.
    pub(super) fn masquee(&mut self, hwnd: isize) {
        let Some(Suivie { image: Some(mut image), icone, .. }) = self.suivies.remove(&hwnd) else { return };
        // Déplacée ou redimensionnée depuis la dernière copie : on part d'où elle est.
        if let Some(rect) = rect_visible(HWND(hwnd as _)) {
            image.rect = en_f32(rect);
        }
        let arrivee = self.origines.get(&hwnd).copied().or(icone).unwrap_or_else(|| point_central(image.rect));
        // Le fondu PROPRE d'une fenêtre (sphère des programmes) la fait fondre
        // même si le réglage GLOBAL est éteint — voir `super::fondu_propre`.
        let fondu = self.internes.contains(&hwnd) && (super::fondu_actif() || super::fondu_propre(hwnd).is_some());
        let duree_du_retour = match (fondu, super::duree_fondu_fermeture_de(hwnd)) {
            (true, Some(ms)) => Duration::from_millis(ms as u64),
            (true, None) => Duration::from_millis(super::duree_fondu_fermeture_ms() as u64),
            (false, _) => super::duree_fermeture(),
        };
        crate::diag!("[fermeture] {hwnd:#x} {}", if fondu { "s'efface en fondu".to_string() } else { format!("retourne vers {arrivee:?}") });
        self.retours.push(Retour {
            image,
            arrivee,
            debut: Instant::now(),
            effet: super::effet_du_moment(super::Moment::Fermeture),
            fondu,
            duree_du_retour,
        });
    }

    /// Une frame : relève les fenêtres, rafraîchit leurs images, avance les retours.
    /// `occupe` : une ouverture est en cours (ne pas bloquer le fil).
    pub(super) fn tick(&mut self, gfx: &GfxDevice, occupe: bool, ouvertes: &dyn Fn(isize) -> bool) {
        let maintenant = Instant::now();
        self.retours.retain(|r| maintenant - r.debut < r.duree());
        // Le fondu de NOS fenêtres a besoin de leur image, même sans l'effet de
        // fermeture des applications (voir `eligible`).
        if !super::fermeture_active() && !super::fondu_actif() && !super::un_fondu_propre_existe() {
            self.suivies.clear();
            self.a_suivre.clear();
            return;
        }
        if self.derniere_scrutation.is_none_or(|d| maintenant - d >= SCRUTATION) {
            self.derniere_scrutation = Some(maintenant);
            self.scruter(ouvertes);
        }
        if !occupe && !self.anime() {
            self.suivre_la_suivante(gfx);
        }
        self.copier(gfx, maintenant);
        // ⛔ D3D11 ne détruit une ressource lâchée qu'au vidage suivant — et ce fil
        // ne présente rien au repos.
        unsafe { gfx.context.Flush() };
    }

    /// Relève les fenêtres éligibles : oublie celles qui ne le sont plus, note
    /// celles qui restent à suivre.
    fn scruter(&mut self, ouvertes: &dyn Fn(isize) -> bool) {
        let mut eligibles: Vec<isize> =
            fenetres_de_haut_en_bas().into_iter().filter(|&h| !ouvertes(h) && self.eligible(HWND(h as _))).collect();
        eligibles.truncate(MAX_SUIVIES);
        self.suivies.retain(|h, _| eligibles.contains(h));
        self.essais.retain(|h, _| eligibles.contains(h));
        self.a_suivre = eligibles.into_iter().filter(|h| !self.suivies.contains_key(h)).collect();
    }

    /// Ouvre la session de la plus haute fenêtre pas encore suivie.
    fn suivre_la_suivante(&mut self, gfx: &GfxDevice) {
        while !self.a_suivre.is_empty() {
            let hwnd = self.a_suivre.remove(0);
            let h = HWND(hwnd as _);
            if self.suivies.contains_key(&hwnd) || !self.eligible(h) {
                continue;
            }
            if let Some(session) = CaptureSession::start(&gfx.device, h) {
                let suivie = Suivie { session, icone: icone_de(h), verifiee: false, derniere_copie: None, image: None };
                self.suivies.insert(hwnd, suivie);
            }
            return;
        }
    }

    /// Sonde chaque session et copie sa dernière image, au plus tous les
    /// `PAS_DE_COPIE` (premier plan) ou `PAS_DE_COPIE_ARRIERE`.
    fn copier(&mut self, gfx: &GfxDevice, maintenant: Instant) {
        let premier = unsafe { GetForegroundWindow() }.0 as isize;
        let mut a_rouvrir = Vec::new();
        for (&hwnd, suivie) in self.suivies.iter_mut() {
            let pas = if hwnd == premier { PAS_DE_COPIE } else { PAS_DE_COPIE_ARRIERE };
            if suivie.derniere_copie.is_some_and(|d| maintenant - d < pas) {
                continue;
            }
            let dernier_essai = self.essais.get(&hwnd).copied().unwrap_or(0) + 1 >= ESSAIS_MAX;
            match copier_une(gfx, HWND(hwnd as _), suivie, dernier_essai) {
                Copie::Faite => suivie.derniere_copie = Some(maintenant),
                Copie::Rien => {}
                Copie::Vide => {
                    *self.essais.entry(hwnd).or_insert(0) += 1;
                    a_rouvrir.push(hwnd);
                }
                // Fenêtre agrandie : le pool a gardé l'ancienne taille.
                Copie::Perimee => a_rouvrir.push(hwnd),
            }
        }
        // Une session neuve les reprendra au relevé suivant.
        for hwnd in a_rouvrir {
            self.suivies.remove(&hwnd);
        }
    }

    /// Les images des fenêtres qui retournent d'où elles venaient. Un effet
    /// peut en demander PLUSIEURS par fenêtre (« éclatement », « génie » —
    /// voir `poses_du_retour`), d'où le `flat_map`.
    pub(super) fn images(&self) -> Vec<Image<'_>> {
        let maintenant = Instant::now();
        self.retours
            .iter()
            .flat_map(|r| {
                let t = (maintenant - r.debut).as_secs_f32() / r.duree().as_secs_f32();
                let rayon = super::RAYON_FENETRE * r.image.echelle;
                poses_du_retour(r.effet, r.fondu, t, r.image.rect, r.arrivee, rayon).into_iter().map(move |partie| Image {
                    srv: &r.image.srv,
                    rect: partie.pose.rect,
                    angle: partie.pose.angle,
                    ecran: r.image.ecran,
                    uv: partie.uv,
                    rayon: partie.rayon,
                    opacite: partie.pose.opacite,
                })
            })
            .collect()
    }

    /// Une fenêtre qu'on peut suivre : visible, ni réduite ni masquée, plus petite
    /// que son écran ; d'une autre application (fenêtre d'application), ou un
    /// panneau de NeuraDesk animé à l'ouverture.
    fn eligible(&self, hwnd: HWND) -> bool {
        if hwnd.0.is_null() || !unsafe { IsWindowVisible(hwnd) }.as_bool() || unsafe { IsIconic(hwnd) }.as_bool() {
            return false;
        }
        // Masquée : d'un autre bureau virtuel, ou une application suspendue.
        if est_masquee_par_dwm(hwnd) {
            return false;
        }
        let (Some(rect), Some(ecran)) = (rect_visible(hwnd), ecran_de(hwnd)) else { return false };
        if crate::win32::window::couvre_l_ecran(rect, ecran) {
            return false;
        }
        // Les nôtres : par leur fondu (global ou PROPRE à la fenêtre), ou par
        // l'effet de fermeture.
        if self.internes.contains(&(hwnd.0 as isize)) {
            return super::fondu_actif()
                || super::fondu_propre(hwnd.0 as isize).is_some()
                || super::fermeture_active();
        }
        if !super::fermeture_active() {
            return false;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        pid != std::process::id() && candidate(hwnd)
    }
}

enum Copie {
    Faite,
    /// Rien de neuf.
    Rien,
    /// Une première image noire : la session est à rouvrir (voir `ESSAIS_MAX`).
    Vide,
    /// La session ne correspond plus à la fenêtre.
    Perimee,
}

fn copier_une(gfx: &GfxDevice, hwnd: HWND, suivie: &mut Suivie, dernier_essai: bool) -> Copie {
    if !suivie.session.poll(&gfx.device) {
        return Copie::Rien;
    }
    if !suivie.verifiee {
        if !dernier_essai && image_vide(gfx, suivie.session.srv()) {
            return Copie::Vide;
        }
        suivie.verifiee = true;
    }
    let Some(source) = source_de(&suivie.session) else { return Copie::Perimee };
    let (Some(rect), Some(ecran)) = (rect_visible(hwnd), ecran_de(hwnd)) else { return Copie::Rien };
    let echelle = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
    // Même taille : recopiée dans la même texture, sans en créer une à chaque fois.
    match suivie.image.as_mut().filter(|i| source.meme_taille(&i.texture)) {
        Some(image) => {
            source.copier_dans(gfx, &image.texture);
            image.rect = en_f32(rect);
            image.ecran = ecran;
            image.echelle = echelle;
        }
        None => {
            let Some((texture, srv)) = copier(gfx, &source) else { return Copie::Rien };
            suivie.image = Some(Instantane { texture, srv, rect: en_f32(rect), ecran, echelle });
        }
    }
    Copie::Faite
}

/// Les fenêtres de premier niveau, de la plus haute à la plus basse.
fn fenetres_de_haut_en_bas() -> Vec<isize> {
    unsafe extern "system" fn chaque(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let liste = &mut *(lparam.0 as *mut Vec<isize>);
        liste.push(hwnd.0 as isize);
        windows::core::BOOL(1)
    }
    let mut liste: Vec<isize> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(chaque), LPARAM(&mut liste as *mut _ as isize));
    }
    liste
}

/// L'image d'une fenêtre : le coin haut-gauche `largeur` × `hauteur` de `texture`.
struct Source {
    texture: ID3D11Texture2D,
    largeur: u32,
    hauteur: u32,
}

impl Source {
    /// Recopie l'image dans `cible`, de la même taille.
    fn copier_dans(&self, gfx: &GfxDevice, cible: &ID3D11Texture2D) {
        let zone = D3D11_BOX { left: 0, top: 0, front: 0, right: self.largeur, bottom: self.hauteur, back: 1 };
        unsafe { gfx.context.CopySubresourceRegion(cible, 0, 0, 0, 0, &self.texture, 0, Some(&zone)) };
    }

    /// `texture` a exactement la taille de l'image.
    fn meme_taille(&self, texture: &ID3D11Texture2D) -> bool {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        (desc.Width, desc.Height) == (self.largeur, self.hauteur)
    }
}

/// L'image d'une session. ⛔ MESURÉ : le contenu d'une fenêtre MAXIMISÉE est
/// plus petit que la texture du pool (ses bords débordent de l'écran) — exiger
/// l'égalité rouvrait sa session à chaque passage. `None` seulement si le contenu
/// ne tient plus dans la texture (fenêtre agrandie depuis l'ouverture).
fn source_de(session: &CaptureSession) -> Option<Source> {
    let (largeur, hauteur) = session.size();
    unsafe {
        let texture: ID3D11Texture2D = windows::core::Interface::cast(&session.srv()?.GetResource().ok()?).ok()?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);
        (largeur <= desc.Width && hauteur <= desc.Height).then_some(Source { texture, largeur, hauteur })
    }
}

/// Copie l'image dans une texture à nous, à sa taille.
fn copier(gfx: &GfxDevice, source: &Source) -> Option<(ID3D11Texture2D, ID3D11ShaderResourceView)> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { source.texture.GetDesc(&mut desc) };
    let copie_desc = D3D11_TEXTURE2D_DESC {
        Width: source.largeur,
        Height: source.hauteur,
        MipLevels: 1,
        ArraySize: 1,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
        ..desc
    };
    unsafe {
        let mut copie = None;
        gfx.device.CreateTexture2D(&copie_desc, None, Some(&mut copie)).ok()?;
        let copie = copie?;
        source.copier_dans(gfx, &copie);
        let mut srv = None;
        gfx.device.CreateShaderResourceView(&copie, None, Some(&mut srv)).ok()?;
        Some((copie, srv?))
    }
}

/// Sans origine ni icône, la fenêtre se réduit vers son centre.
fn point_central(rect: [f32; 4]) -> [f32; 4] {
    super::depart_au_centre(rect)
}

/// **LA GÉOMÉTRIE DES IMAGES D'UN RETOUR** — pure, donc testable sans texture
/// ni périphérique D3D.
///
/// ⛔ **UNE FENÊTRE EN FONDU S'EFFACE EN PLACE, NI PLUS NI MOINS** : rectangle
/// figé, angle nul, UNE seule image, quel que soit le style réglé. Le style
/// (vol, éclatement, génie…) ne s'applique qu'aux fenêtres d'application ;
/// sans ce cas, un panneau de NeuraDesk fermé avec « éclatement » volerait en
/// morceaux vers un point au lieu de disparaître sur place — régression
/// trouvée en revue le 2026-11-05 (la version d'avant la bibliothèque d'effets
/// figeait déjà ce rectangle).
fn poses_du_retour(
    effet: super::effets::Effet,
    fondu: bool,
    t: f32,
    rect: [f32; 4],
    arrivee: [f32; 4],
    rayon: f32,
) -> Vec<super::effets::Partie> {
    if fondu {
        let pose = super::effets::Pose { rect, angle: 0.0, opacite: 1.0 - super::opacite_du_fondu(t) };
        return vec![super::effets::Partie { pose, uv: super::UV_PLEINE, rayon }];
    }
    effet.parties(t, rect, arrivee, rayon)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICONE: [f32; 4] = [2500.0, 400.0, 48.0, 48.0];
    const FENETRE: [f32; 4] = [300.0, 200.0, 1200.0, 800.0];

    /// Elle rapetisse sans jamais regrandir. (Le retour lui-même a déménagé
    /// dans `effets`, qui porte déjà son test de départ et d'arrivée.)
    #[test]
    fn la_fenetre_rapetisse_en_retournant() {
        let mut avant = super::super::effets::retour(FENETRE, ICONE, 0.0);
        for pas in 1..=20 {
            let apres = super::super::effets::retour(FENETRE, ICONE, pas as f32 / 20.0);
            assert!(apres[2] <= avant[2] && apres[3] <= avant[3]);
            avant = apres;
        }
    }

    /// ⛔ **UNE FENÊTRE EN FONDU S'EFFACE EN PLACE, QUEL QUE SOIT LE STYLE.**
    /// La régression du 2026-11-05 : le style réglé s'appliquait aussi aux
    /// fenêtres de NeuraDesk, qui partaient donc en morceaux vers un point au
    /// lieu de fondre sur place. Les deux moitiés comptent — le fondu doit
    /// IGNORER le style, et l'absence de fondu doit l'APPLIQUER.
    #[test]
    fn le_fondu_s_efface_en_place_quel_que_soit_le_style() {
        use super::super::effets::Effet;
        for effet in [Effet::Retour, Effet::Eclater, Effet::Genie] {
            let images = poses_du_retour(effet, true, 0.4, FENETRE, ICONE, 8.0);
            assert_eq!(images.len(), 1, "{effet:?} : un fondu se dessine en UNE image");
            assert_eq!(images[0].pose.rect, FENETRE, "{effet:?} : le rectangle ne doit pas bouger");
            assert_eq!(images[0].pose.angle, 0.0, "{effet:?} : ni tourner");
            assert_eq!(images[0].uv, super::super::UV_PLEINE, "{effet:?} : toute la texture");
            assert!(
                images[0].pose.opacite > 0.0 && images[0].pose.opacite < 1.0,
                "{effet:?} : opacité {} n'est pas un fondu entamé",
                images[0].pose.opacite
            );
        }
        // Sans fondu, c'est bien le style qui décide — et il découpe.
        // ⭐ L'éclatement est une POUSSIÈRE FINE depuis le 2026-11-05 (il était
        // seize tuiles) : bien plus de pièces, jamais une seule.
        assert!(
            poses_du_retour(Effet::Eclater, false, 0.4, FENETRE, ICONE, 8.0).len() > 16,
            "l'éclatement doit se découper en poussière"
        );
        assert_eq!(poses_du_retour(Effet::Retour, false, 0.4, FENETRE, ICONE, 8.0).len(), 1);
    }
}
