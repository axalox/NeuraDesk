//! Fondu d'apparition et de disparition d'une FENÊTRE — `WS_EX_LAYERED` +
//! `SetLayeredWindowAttributes(LWA_ALPHA)`, la mécanique que
//! `win32::window_previews` emploie depuis sa création, extraite ici pour que
//! les MENUS et les INFOBULLES s'en servent sans la recopier.
//!
//! DEMANDE EXPLICITE : « le fondu des infobulles et des menus ». Deux
//! curseurs SÉPARÉS (`menu_fade_ms`/`tooltip_fade_ms`), sur la décision de
//! l'utilisateur : ce sont deux familles de fenêtres qui n'apparaissent pas
//! dans les mêmes conditions — un menu s'ouvre sous un clic, une infobulle
//! sous un survol continu — et `win32::tooltip` a déjà ses propres réglages,
//! distincts de ceux des menus (voir `tooltip::TOOLTIP_SCALE`, même raison).
//! Le fondu des APERÇUS DE FENÊTRES garde le sien
//! (`window_previews::fade_ms`), inchangé.
//!
//! **L'opacité est celle de la FENÊTRE ENTIÈRE, pas de son contenu** : une
//! seule valeur à animer, aucun travail de rendu par frame, et la dernière
//! image présentée se fond d'elle-même — exactement ce qu'il faut pendant la
//! disparition, où plus rien n'est redessiné. C'est le choix documenté par
//! `window_previews::PreviewPanel::tick`, et il vaut ici pour les mêmes
//! raisons.
//!
//! **POURQUOI UNE FENÊTRE `LAYERED` RESTE CLIQUABLE À L'OPACITÉ ZÉRO** — la
//! question se pose, puisqu'un menu apparaît transparent et doit recevoir les
//! clics et le focus dès sa première frame. `LWA_ALPHA` ne change PAS le test
//! de survol : ce sont `UpdateLayeredWindow` (dont les pixels d'alpha nul sont
//! traversants) et `LWA_COLORKEY` (dont la couleur-clé l'est) qui rendent une
//! fenêtre transparente aux clics, pas l'alpha uniforme. La preuve est dans le
//! dépôt : `window_previews` reçoit ses clics (`CLICKED`, rempli par son
//! wndproc) alors que son fondu d'apparition part lui aussi de zéro.
//!
//! **CE MODULE NE DESSINE RIEN** : il ne fait qu'animer une opacité et la
//! poser. Le rendu, la région arrondie et l'ordre Z restent où ils étaient.

use std::cell::Cell;
use std::time::Instant;

use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::UI::WindowsAndMessaging::{SetLayeredWindowAttributes, LWA_ALPHA};

/// Bornes des DEUX curseurs, communes : c'est la même grandeur des deux côtés,
/// et deux commandes d'une même grandeur ne doivent pas s'arrêter à des
/// endroits différents (même règle que `ui::dock::bar_thickness_bounds`).
///
/// **`0` EST UNE VALEUR LÉGITIME, ET C'EST L'ARRÊT DU FONDU** : à zéro
/// milliseconde la fenêtre apparaît et disparaît d'un coup, c'est-à-dire
/// exactement le comportement qui précédait ce module. C'est le seul moyen de
/// le retrouver sans recompiler, et c'est ce que la borne basse doit permettre.
pub const MIN_FADE_MS: f32 = 0.0;
/// Plafond : au-delà, un menu mettrait plus de temps à apparaître qu'un
/// utilisateur n'en met à cliquer ailleurs, et le fondu de sortie deviendrait
/// plus long que la fermeture qu'il illustre.
pub const MAX_FADE_MS: f32 = 1200.0;

/// Durées par défaut. 150 ms est le régime d'un menu de système d'exploitation
/// (assez pour se lire, trop court pour se faire attendre) ; l'infobulle prend
/// la même, n'ayant aucune raison d'en avoir une autre au départ — les deux
/// curseurs permettent de les éloigner ensuite.
pub const DEFAULT_MENU_FADE_MS: f32 = 150.0;
pub const DEFAULT_TOOLTIP_FADE_MS: f32 = 150.0;

thread_local! {
    /// Durées courantes, en millisecondes — réglables dans les Réglages. Même
    /// mécanisme que `window_previews::FADE_MS` et `ui::dock::hover_scale` :
    /// des cellules lues par `tick` et bornées par leur accesseur, plutôt que
    /// des champs qui obligeraient le panneau Réglages à traverser
    /// `SettingsRefs` (les deux familles de fenêtres vivent dans des
    /// `thread_local!` que ce panneau n'atteint pas).
    static MENU_FADE_MS: Cell<f32> = const { Cell::new(DEFAULT_MENU_FADE_MS) };
    static TOOLTIP_FADE_MS: Cell<f32> = const { Cell::new(DEFAULT_TOOLTIP_FADE_MS) };
}

/// Durée du fondu des MENUS, en millisecondes — voir `MENU_FADE_MS`.
pub fn menu_fade_ms() -> f32 {
    MENU_FADE_MS.with(Cell::get)
}

/// Change la durée du fondu des menus, bornée à [`MIN_FADE_MS`, `MAX_FADE_MS`]
/// — le BORNAGE est ici, comme `window_previews::set_fade_ms`, pour qu'un
/// fichier de configuration corrompu ne puisse pas figer un menu à l'opacité
/// zéro (un menu invisible qui garde le focus serait indélogeable).
pub fn set_menu_fade_ms(ms: f32) {
    MENU_FADE_MS.with(|cell| cell.set(ms.clamp(MIN_FADE_MS, MAX_FADE_MS)));
}

/// Durée du fondu des INFOBULLES, en millisecondes — voir `TOOLTIP_FADE_MS`.
pub fn tooltip_fade_ms() -> f32 {
    TOOLTIP_FADE_MS.with(Cell::get)
}

/// Change la durée du fondu des infobulles, bornée comme celui des menus.
pub fn set_tooltip_fade_ms(ms: f32) {
    TOOLTIP_FADE_MS.with(|cell| cell.set(ms.clamp(MIN_FADE_MS, MAX_FADE_MS)));
}

/// L'opacité animée d'UNE fenêtre. Chaque fenêtre qui fond a la sienne : deux
/// menus ouverts en même temps ne disparaissent pas au même instant.
pub struct WindowFade {
    alpha: f32,
    target: f32,
    /// Dernière valeur POSÉE sur la fenêtre. Une valeur IMPOSSIBLE au départ
    /// (`-1.0`), pour que le premier `push` ait toujours lieu : l'alpha par
    /// défaut d'une fenêtre `LAYERED` neuve est 255 (opaque), et rien ne dit
    /// qu'il correspond à `alpha`.
    applied: f32,
    /// Instant du dernier pas. **Le fondu MESURE LUI-MÊME la durée écoulée**
    /// au lieu de la recevoir : `render_if_visible` est appelé par treize
    /// modules et treize sites d'`app.rs`, lui ajouter un paramètre `dt`
    /// serait une onde de choc pour une donnée que `Instant::now()` donne
    /// ici. `None` = aucun pas encore fait.
    last: Option<Instant>,
}

impl WindowFade {
    /// L'état d'une fenêtre `LAYERED` QUI VIENT D'ÊTRE CRÉÉE : pleinement
    /// opaque, rien de posé. C'est le bon point de départ pour une fenêtre qui
    /// ne fond pas (les quatre panneaux) comme pour une fenêtre qui fond, dont
    /// l'ouverture appellera `restart`.
    pub const fn new() -> Self {
        Self { alpha: 1.0, target: 1.0, applied: -1.0, last: None }
    }

    /// La fenêtre APPARAÎT : le fondu repart de zéro.
    ///
    /// **Distinct de `fade_in`, et la distinction est le fond du sujet** :
    /// `restart` s'adresse à une fenêtre qui était CACHÉE (l'opacité doit
    /// repartir de zéro, sinon la première ouverture d'un popup — dont la
    /// fenêtre naît opaque — ne fondrait pas du tout), tandis que `fade_in`
    /// s'adresse à une fenêtre DÉJÀ LÀ qu'on veut ramener au premier plan.
    /// Rouvrir un menu pendant son fondu de sortie est ce second cas : partir
    /// de zéro y produirait un clignotement (l'opacité tomberait d'un coup
    /// avant de remonter).
    pub fn restart(&mut self) {
        self.alpha = 0.0;
        self.target = 1.0;
        self.last = None;
    }

    /// Ramène la fenêtre à pleine opacité en repartant de l'opacité COURANTE
    /// — voir `restart` pour ce qui la distingue.
    pub fn fade_in(&mut self) {
        self.target = 1.0;
    }

    /// Engage la disparition. **Ne cache PAS la fenêtre** : c'est `tick` qui
    /// la cachera pour de bon, quand l'opacité aura atteint zéro
    /// (voir `is_hidden`). Idempotent, et c'est nécessaire : `app::run` appelle
    /// `hide` à CHAQUE frame où il n'y a rien à afficher.
    pub fn fade_out(&mut self) {
        self.target = 0.0;
    }

    /// L'opacité courante, 0.0 à 1.0.
    #[cfg(test)]
    pub fn alpha(&self) -> f32 {
        self.alpha
    }

    /// Vrai si la fenêtre est EN COURS DE DISPARITION : le fondu de sortie est
    /// engagé, l'opacité n'est pas encore nulle. **C'est l'état que
    /// `cef_popup::is_visible` ne sait pas distinguer** — la fenêtre Win32
    /// existe et est visible pendant tout le fondu — et c'est pourquoi
    /// `cef_popup::is_closing` existe à côté.
    pub fn is_fading_out(&self) -> bool {
        self.target == 0.0 && self.alpha > 0.0
    }

    /// Vrai si la fenêtre a FINI de disparaître : c'est le signal pour la
    /// cacher pour de bon.
    pub fn is_hidden(&self) -> bool {
        self.target == 0.0 && self.alpha == 0.0
    }

    /// Pose l'opacité COURANTE sur la fenêtre, sans avancer le fondu.
    ///
    /// **Sert à l'ouverture**, et ce n'est pas un détail : `restart` remet
    /// l'opacité à zéro, mais la fenêtre n'est `LAYERED` que depuis
    /// `CreateWindowExW` et porte encore son alpha par défaut (255). Sans ce
    /// `push` immédiat, elle se composerait OPAQUE pendant la frame qui précède
    /// le premier `tick` — un éclair de menu entier avant son fondu.
    pub fn push(&mut self, hwnd: HWND) {
        self.applied = self.alpha;
        let alpha = (self.alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        unsafe {
            // Inerte sur une fenêtre sans `WS_EX_LAYERED` (elle rend une
            // erreur, que rien ne journalise) : c'est pourquoi les fenêtres qui
            // ne fondent pas ne reçoivent jamais cet appel.
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
        }
    }

    /// Avance le fondu d'un pas et pose l'opacité si elle a changé — **À
    /// APPELER À CHAQUE FRAME tant que la fenêtre est affichée**, y compris
    /// pendant la disparition : c'est ce pas-là qui la termine, et donc ce qui
    /// finit par la cacher.
    pub fn tick(&mut self, hwnd: HWND, fade_ms: f32) {
        let now = Instant::now();
        let dt = match self.last.replace(now) {
            Some(previous) => now.duration_since(previous).as_secs_f32(),
            // PREMIER pas : la durée écoulée depuis la frame précédente est
            // inconnue, on ne bouge donc pas. L'opacité de départ a déjà été
            // posée par `push` à l'ouverture (voir sa doc).
            None => 0.0,
        };
        self.advance(dt, fade_ms);
        if self.alpha != self.applied {
            self.push(hwnd);
        }
    }

    /// Le pas PUR — aucun appel Win32, donc testable sans fenêtre (voir
    /// `tick`, qui l'enveloppe).
    ///
    /// Le pas vaut « ce qu'il faut parcourir en `dt` secondes pour faire le
    /// trajet complet en `fade_ms` », et le déplacement est BORNÉ par lui :
    /// c'est ce qui empêche un `dt` long (une frame en retard) de dépasser la
    /// cible et de faire clignoter la fenêtre.
    pub fn advance(&mut self, dt: f32, fade_ms: f32) {
        if self.alpha == self.target {
            return;
        }
        // `fade_ms` nul = pas de fondu : un pas infini fait tomber l'opacité
        // exactement sur sa cible, du premier coup (voir `MIN_FADE_MS`).
        let step = if fade_ms <= 0.0 { 1.0 } else { (dt / (fade_ms / 1000.0)).max(0.0) };
        self.alpha += (self.target - self.alpha).clamp(-step, step);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_duration_lands_on_the_target_in_one_step() {
        let mut fade = WindowFade::new();
        fade.restart();
        fade.fade_in();
        fade.advance(0.0, 0.0);
        assert_eq!(fade.alpha(), 1.0, "sans durée, l'apparition est immédiate");
        fade.fade_out();
        fade.advance(0.0, 0.0);
        assert_eq!(fade.alpha(), 0.0);
        // ...et c'est à ce moment-là, et pas avant, que la fenêtre peut être
        // cachée pour de bon.
        assert!(fade.is_hidden());
    }

    #[test]
    fn the_fade_lands_exactly_on_its_target_without_overshooting() {
        let mut fade = WindowFade::new();
        fade.restart();
        fade.fade_in();
        // Un `dt` PLUS LONG que la durée entière : le déplacement est borné par
        // le pas, donc l'opacité s'arrête pile sur la cible.
        fade.advance(10.0, 150.0);
        assert_eq!(fade.alpha(), 1.0, "un pas plus long que le trajet ne dépasse pas la cible");
    }

    #[test]
    fn the_fade_takes_the_announced_duration() {
        let mut fade = WindowFade::new();
        fade.restart();
        fade.fade_in();
        // La moitié de la durée doit donner la moitié du trajet.
        fade.advance(0.075, 150.0);
        assert!((fade.alpha() - 0.5).abs() < 0.001, "à mi-durée, à mi-chemin : {}", fade.alpha());
        fade.advance(0.075, 150.0);
        assert_eq!(fade.alpha(), 1.0);
    }

    #[test]
    fn a_first_step_never_moves_the_alpha() {
        // `tick` sur une durée inconnue (premier pas) ne doit pas faire
        // apparaître la fenêtre à moitié : c'est `push` qui pose l'opacité de
        // départ, et le fondu commence au pas suivant.
        let mut fade = WindowFade::new();
        fade.restart();
        fade.fade_in();
        fade.advance(0.0, 150.0);
        assert_eq!(fade.alpha(), 0.0, "le premier pas, de durée nulle, ne bouge pas");
    }

    #[test]
    fn reopening_during_the_disappearance_resumes_from_the_current_opacity() {
        // Le cas que `restart` et `fade_in` départagent : rouvrir un menu qui
        // s'en va ne doit PAS le faire clignoter (retomber à zéro avant de
        // remonter).
        let mut fade = WindowFade::new();
        fade.restart();
        fade.fade_in();
        fade.advance(0.075, 150.0);
        fade.fade_out();
        fade.advance(0.0375, 150.0);
        let midway = fade.alpha();
        assert!(fade.is_fading_out(), "le fondu de sortie est bien engagé");
        assert!(midway > 0.0 && midway < 0.5);
        fade.fade_in();
        assert!(!fade.is_fading_out(), "rouvrir annule la disparition");
        fade.advance(0.0, 150.0);
        assert_eq!(fade.alpha(), midway, "la réouverture repart de l'opacité courante");
    }

    #[test]
    fn a_window_that_never_fades_is_neither_closing_nor_hidden() {
        // Les quatre PANNEAUX gardent un `WindowFade` à l'état neuf : s'il
        // passait pour « caché », `render_if_visible` les masquerait aussitôt.
        let fade = WindowFade::new();
        assert!(!fade.is_hidden());
        assert!(!fade.is_fading_out());
    }
}
