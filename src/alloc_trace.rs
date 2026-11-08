//! **L'ALLOCATEUR INSTRUMENTÉ — nommer QUI alloue les gros blocs.**
//!
//! Écrit le 2026-10-08, après que la sonde `win32::memoire` a établi la forme du
//! problème sans pouvoir le nommer : le processus principal tient **~2,2 Go**
//! stables, dont **27 blocs indépendants de 33 132 544 octets** (853 Mo) et
//! **7 de 16 576 512** (111 Mo). Ni Chromium (le total ne bouge pas avec les
//! panneaux), ni les fenêtres (11 seulement), ni des images (cette taille n'est
//! le produit d'aucune dimension `L×H×4`). Restait à savoir **quelle ligne de
//! code les demande** — et c'est ce que fait ceci.
//!
//! # Pourquoi un `#[global_allocator]` et pas un profileur
//!
//! Aucun profileur n'est installé, `Add-Type` est bloqué par l'antivirus sur
//! cette machine, et `VirtualQueryEx` (voir `win32::memoire`) ne dit que la
//! TAILLE des régions, jamais leur auteur. Un `#[global_allocator]` voit passer
//! **chaque** allocation et peut en prendre la pile d'appel.
//!
//! # ⛔ LES TROIS PIÈGES, ET COMMENT ILS SONT DÉSARMÉS
//!
//! 1. **LA RÉENTRANCE.** Journaliser alloue (le `format!`, la `Backtrace`
//!    elle-même) — donc le traceur s'appellerait lui-même, sans fin. Un
//!    `thread_local!` initialisé `const` (donc sans allocation paresseuse) sert
//!    de garde : on entre, on journalise, on sort.
//! 2. **L'INITIALISATION PARESSEUSE.** Lire la variable d'environnement alloue.
//!    La faire depuis `alloc` — via un `OnceLock::get_or_init` — re-entrerait
//!    dans l'allocateur pendant son initialisation. L'état est donc posé une
//!    fois pour toutes par [`init`], appelée depuis `main` ; tant qu'elle n'a pas
//!    tourné, [`actif`] répond `false` et RIEN n'est tracé.
//! 3. **LE VOLUME.** Une application qui rend à 60 Hz alloue sans arrêt. Le
//!    seuil (8 Mo par défaut) et un plafond d'événements journalisés gardent le
//!    journal lisible — sans eux, on noierait la réponse sous le bruit.
//!
//! # Ce que ça coûte quand c'est éteint
//!
//! **Rien** : quand `NEURADESK_TRACE_ALLOC` n'est pas positionnée, [`actif`]
//! rend `false` et le chemin se réduit à une lecture d'atome avant de sortir.
//!
//! # Usage
//!
//! ```text
//! NEURADESK_DIAG=1 NEURADESK_TRACE_ALLOC=1 neuradesk.exe
//! ```
//!
//! ⚠️ **`NEURADESK_DIAG=1` EST AUSSI NÉCESSAIRE** : la sortie part dans le
//! journal de diagnostic (voir `diag`), parce que l'application est lancée
//! détachée et qu'un `eprintln!` n'irait nulle part.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

/// **L'ALLOCATEUR QUI TRACE** — à poser en `#[global_allocator]` dans `main`.
pub struct Traceur;

thread_local! {
    /// La garde de réentrance — voir le piège n°1 de la doc de module.
    ///
    /// ⚠️ **`const` N'EST PAS COSMÉTIQUE** : une initialisation paresseuse
    /// allouerait au premier accès, c'est-à-dire depuis l'allocateur lui-même.
    static DEDANS: Cell<bool> = const { Cell::new(false) };
}

/// L'interrupteur, posé par [`init`]. `None` = pas encore initialisé = on ne
/// trace pas (voir le piège n°2).
static ACTIF: OnceLock<bool> = OnceLock::new();

/// La taille minimale d'une allocation pour qu'elle soit journalisée.
static SEUIL: AtomicUsize = AtomicUsize::new(8 * 1024 * 1024);

/// Le nombre d'événements encore journalisables — voir le piège n°3.
static RESTANTS: AtomicUsize = AtomicUsize::new(400);

/// Le rang de l'événement, pour que le journal se lise dans l'ordre.
static RANG: AtomicUsize = AtomicUsize::new(0);

/// **À APPELER UNE FOIS, TÔT DANS `main`** — avant toute allocation qu'on veut
/// voir. Lit l'environnement une seule fois et fige l'interrupteur.
pub fn init() {
    let actif = std::env::var_os("NEURADESK_TRACE_ALLOC").is_some();
    let _ = ACTIF.set(actif);
    if actif {
        if let Ok(valeur) = std::env::var("NEURADESK_TRACE_SEUIL") {
            if let Ok(seuil) = valeur.trim().parse::<usize>() {
                SEUIL.store(seuil, Ordering::Relaxed);
            }
        }
    }
}

fn actif() -> bool {
    ACTIF.get().copied().unwrap_or(false)
}

/// Journalise une allocation au-dessus du seuil, **avec sa pile d'appel**.
#[inline]
fn tracer(taille: usize) {
    if taille < SEUIL.load(Ordering::Relaxed) || !actif() {
        return;
    }
    if RESTANTS.load(Ordering::Relaxed) == 0 {
        return;
    }
    DEDANS.with(|garde| {
        if garde.get() {
            return;
        }
        garde.set(true);
        RESTANTS.fetch_sub(1, Ordering::Relaxed);
        let rang = RANG.fetch_add(1, Ordering::Relaxed);
        // La pile est prise AVANT de formater : `format!` alloue, et la
        // réentrance élaguerait la pile si on la prenait après.
        let pile = std::backtrace::Backtrace::force_capture();
        crate::diag!("[alloc] #{rang} — {taille} octets\n{pile}");
        garde.set(false);
    });
}

unsafe impl GlobalAlloc for Traceur {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointeur = unsafe { System.alloc(layout) };
        tracer(layout.size());
        pointeur
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointeur = unsafe { System.alloc_zeroed(layout) };
        tracer(layout.size());
        pointeur
    }

    unsafe fn dealloc(&self, pointeur: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointeur, layout) };
    }

    /// ⚠️ **`realloc` EST LE CHEMIN DES `Vec` QUI GRANDISSENT** — un tampon
    /// atteignant 33 Mo par doublements successifs n'apparaît QUE ici. Ne tracer
    /// que `alloc` manquerait précisément ce qu'on cherche.
    unsafe fn realloc(&self, pointeur: *mut u8, layout: Layout, nouvelle: usize) -> *mut u8 {
        let rendu = unsafe { System.realloc(pointeur, layout, nouvelle) };
        tracer(nouvelle);
        rendu
    }
}
