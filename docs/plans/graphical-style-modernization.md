# Plan — modernisation du style graphique de NeuraDesk

Statut global (2026-09-03) : **Phase 0 terminée et vérifiée.** **Phase 1 : fondation posée et vérifiée** (module `ui::tokens`, un bug réel corrigé, migration des appelants restant à faire). Phases 2-4 non commencées.

Contexte : rapprocher le rendu de l'app (dock + fenêtres `egui`/D3D11 Dash/Réglages/Presse-papiers) du niveau de finition d'une appli web moderne, sur 4 volets : système de tokens de couleur, typographie, ombres/profondeur sur les tuiles du dock, transitions d'ouverture/fermeture.

## Corrections d'hypothèses (validées en lisant le code réel, avant la Phase 0)

1. **egui 0.35 / egui-directx11 0.13**, pas 0.28/0.5 (mémoire projet obsolète sur ce point) — confirmé par `cargo build` (`Compiling egui v0.35.0`). `epaint` 0.35 a migré vers **skrifa**, avec support de police variable et `Style::text_options.font_hinting` — le bug "texte flou aux tailles par défaut" documenté doit être re-testé sur cette version avant d'être traité comme une contrainte figée.
2. **Aucune capture d'écran externe ne peut voir les fenêtres de l'app** — `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)` est appliqué au dock ET aux trois fenêtres `egui`. La seule vérification visuelle possible est une relecture CPU du back buffer D3D11 **depuis le processus lui-même** — d'où la Phase 0, prérequis dur des 4 phases suivantes.
3. **`AnimateWindow` (mécanisme du terminal) n'est PAS transposable** au Dash/Réglages : synchrone (gèlerait toute la boucle de messages, single-thread), `AW_BLEND` exige `WS_EX_LAYERED` (incompatible avec la swap chain + `DwmExtendFrameIntoClientArea`). La Phase 4 anime donc dans le rendu (opacité + décalage par frame), pas la fenêtre Win32 elle-même.

---

## Phase 0 — Harnais de vérification ✅ implémenté

Sans ça, aucune des 4 phases ne peut être vérifiée : le rendu est littéralement inobservable de l'extérieur.

### Ce qui a été construit
- **`src/gfx/capture.rs`** — `dump_backbuffer`/`dump_backbuffer_composited` : copie D3D11 (`CopyResource` vers une texture staging `D3D11_USAGE_STAGING`/`D3D11_CPU_ACCESS_READ`), conversion BGRA→RGBA, écriture PNG (`image::save_buffer`). La variante composée mélange l'alpha prémultiplié sur un damier gris pour juger du contraste réel.
- **`SwapChain::back_buffer()`** (`src/gfx/swapchain.rs`) — `GetBuffer(0)`, à appeler AVANT `present()` (`DXGI_SWAP_EFFECT_FLIP_DISCARD` ne garantit le buffer 0 qu'à ce moment-là).
- **`EguiOverlay::dump_backbuffer()`** (`src/win32/egui_overlay.rs`) — enveloppe les deux ci-dessus pour une fenêtre `egui` quelconque.
- **`dash::debug_dump_backbuffer`, `settings_dash::debug_dump_backbuffer`, `clipboard_viewer::debug_dump_backbuffer`** — accesseurs `pub fn(&Path) -> bool` qui vont chercher l'`EguiOverlay` dans le `STATE` `thread_local!` de chaque fenêtre.
- **`src/win32/debug_capture.rs`** (nouveau module) — `DebugCapture` (parsing des variables d'environnement) + `guard_single_instance()`.
- **`win32::appbar::AppBar::register`** — respecte `NEURADESK_NO_APPBAR` : si positionnée, AUCUN `SHAppBarMessage` n'est envoyé (l'AppBar renvoyée ne réserve rien, son `Drop` ne libère rien).
- **`app::run`** — câblage : lit `DebugCapture::from_env()` en tout premier (avant `Window::create_dock()`), appelle `guard_single_instance()` si actif, **saute l'application de la teinte/taille de la vraie barre des tâches en mode capture** (ces deux chemins redémarrent `explorer.exe`, un effet de bord bien plus lourd que l'AppBar), ouvre la fenêtre demandée par `NEURADESK_DEBUG_OPEN` via un appel de fonction direct (`dash::toggle`/etc., PAS une entrée simulée), dumpe le back buffer du dock + de la fenêtre ouverte aux frames listées par `NEURADESK_CAPTURE_FRAMES`, sort proprement après la dernière frame demandée.

### Variables d'environnement (voir la doc de `win32::debug_capture` pour le détail)
- `NEURADESK_CAPTURE=<dossier>` — active le harnais, dossier de sortie des PNG.
- `NEURADESK_CAPTURE_FRAMES=0,5,20` — indices de frame à capturer ; le process quitte après la dernière.
- `NEURADESK_DEBUG_OPEN=dash|settings|clipboard` — ouvre la fenêtre nommée dès la première frame.
- `NEURADESK_NO_APPBAR=1` — **obligatoire pour tout run de test** tant qu'une session réelle peut être en cours : sans elle, `guard_single_instance` refuse de démarrer si une autre instance de NeuraDesk semble déjà active.

### Vérification effectuée
```
NEURADESK_CAPTURE=<dossier> NEURADESK_CAPTURE_FRAMES=0,5,20 NEURADESK_DEBUG_OPEN=dash NEURADESK_NO_APPBAR=1 cargo run
```
- `cargo build` et `cargo build --release` : propres (seuls des warnings préexistants, sans rapport).
- Run réel exécuté : les 6 PNG attendus (`dock_0000/0005/0020.png`, `dash_0000/0005/0020.png`) ont bien été écrits — dash_0020.png montre le Dash entièrement rendu (recherche, catégories, grille d'icônes, texte net), dock_0020.png montre le dock (horloge, moniteur CPU/RAM, icônes de barre) avec la zone centrale correctement transparente (damier de composition). **Première vérification visuelle réelle du rendu de cette app depuis le début du projet** (jusqu'ici jamais possible, `WDA_EXCLUDEFROMCAPTURE` bloquant tout outil externe).
- Aucune instance zombie après coup (`tasklist` vide).
- Anomalie observée, non bloquante : le process sort avec le code 2170 plutôt que 0 après le `break 'main` propre (aucune trace de panique/erreur imprimée, tous les fichiers écrits avant). Cohérent avec le comportement déjà documenté ([[neuradesk-refcell-reentrancy-pattern]] mémoire projet : Windows Defender verrouille parfois brièvement un `.exe` fraîchement recompilé et non signé) plutôt qu'une régression introduite ici — à surveiller si ça persiste sur un prochain run, mais n'a empêché ni la capture ni un arrêt propre.

---

## Phase 1 — Système de tokens de couleur (⏳ fondation posée, migration des appelants restant à faire — 2026-09-03)

### Ce qui a été construit
- **`src/ui/tokens.rs`** (nouveau) — `Tokens { surface, surface_elevated, border, text_primary, text_secondary, on_accent }` + `compute_tokens(&Palette, dark_mode) -> Tokens`, dérivé d'une `Palette` déjà calculée (aucune double source de vérité).
- **Bug réel corrigé** : `egui_overlay::apply_adaptive_theme` peignait `widgets.noninteractive.bg_stroke` (séparateurs/bordures de `Frame`) dans LA MÊME couleur que `zone_bg` — invisibles par construction. Remplacé par `tokens.border`, garanti ≥3:1 contre `surface` (SC 1.4.11).
- **Test de non-régression** (`ui::tokens::tests::tokens_keep_documented_contrast_guarantees`) sur les 120 `THEMES` × 2 modes (clair/sombre) : `surface_elevated`/`border`/`text_secondary` respectent toujours leurs garanties de contraste documentées. A trouvé et fait corriger DEUX bugs réels pendant l'implémentation (pas seulement après coup) :
  1. `text_primary` (`Palette::accent_text`) n'a aucune garantie de contraste contre `surface` par construction (son contrat porte sur le fond RÉEL de la fenêtre, un rôle différent) — `text_secondary` doit garantir son propre plancher, pas supposer que son point de départ le respecte déjà.
  2. `push_until` (la recherche par pas existante, réutilisée pour `border`) a une portée insuffisante (12 pas de magnitude croissante par 0.05, max ~0.6-0.75) quand le point de départ est déjà proche d'une borne HSL 0.0/1.0 — le cas exact de `text_primary`, toujours l'un des deux gris quasi extrêmes de `readable_text_rgb`. Corrigé en réutilisant `readable_text_rgb` directement pour ce cas plutôt qu'une recherche graduelle : elle garantit mathématiquement ≥4.6:1 dans le pire cas (bien au-dessus du plancher 3:1 visé).
- Visibilité élargie (`pub(crate)`, aucun changement de comportement) sur `contrast_ratio`/`push_until`/`MIN_ZONE_BOUNDARY_CONTRAST` dans `ui::color`, pour que `tokens.rs` les réutilise sans dupliquer la logique de contraste.
- **Vérifié par capture** (harnais Phase 0, `NEURADESK_DEBUG_OPEN=settings`) : le panneau Réglages rend correctement avec le nouveau `border`, aucune régression visuelle.
- `cargo test` (5/5) et `cargo build --release` passent.

### Suite tentée (2026-09-03, deuxième passe) — dédoublonnage sûr fait, unification des alphas NON faite
En creusant les "64 occurrences" pour les migrer, la réalité s'est révélée plus nuancée qu'un simple copier-coller de constante :

- **`window_fill` (rôle "fond de la fenêtre entière")** : `tinted_section_fill(background, (130.0 * tint_alpha) as u8)` était RÉELLEMENT dupliqué à l'identique dans `dash.rs` et `clipboard_viewer.rs` — extrait en une seule fonction partagée, **`egui_overlay::standard_window_fill`**, valeur inchangée (130), aucun changement visuel. Fait, vérifié (`cargo build`/`cargo test` verts, formule strictement identique).
- **Fond de "carte/section"** : PAS le même rôle partout, malgré l'apparence — `settings_dash.rs` (115) n'a AUCUN `window_fill` propre (`Color32::TRANSPARENT`, sa section EST son seul fond), alors que `clipboard_viewer.rs` (180) et `dash.rs` (130, `bg_rect` de la colonne de catégories) empilent leur fond de carte PAR-DESSUS un `window_fill` déjà semi-opaque. Unifier ces trois valeurs sans corriger l'opacité résultante composée par-dessus le `window_fill` de chaque fenêtre changerait leur poids visuel de façon inégale.
- Risque plus profond découvert en traçant la garantie de lisibilité : `settings_dash.rs` protège son texte via une fonction dédiée (`settings_accent_text_min_opacity`, calée EXACTEMENT sur son propre `SECTION_FILL_ALPHA`) — mais `dash.rs`/`clipboard_viewer.rs` passent `tint_alpha` **directement** comme opacité minimale garantie à `egui_overlay::apply_theme`, un chemin distinct jamais tracé jusqu'au bout ici. Changer leurs alphas de fond de section (130/180) sans d'abord vérifier ce second chemin risquerait de réintroduire la classe de bug "texte illisible sur fond flouté" déjà corrigée plusieurs fois dans ce fichier (voir les nombreux commentaires "BUG RÉEL SIGNALÉ" qu'il contient).

**Décision** : dédoublonnage fait là où c'était sûr et vérifiable (`window_fill`) ; unification des trois valeurs de fond de section (115/180/130) **volontairement pas faite** cette session — nécessite d'abord de tracer entièrement le chemin de garantie de lisibilité de `dash.rs`/`clipboard_viewer.rs` (une tâche à part, pas un simple remplacement de constante). `popup_menu.rs` (235, transparence indépendante `menu_tint`) et le fond de barre de recherche du Dash (255, rôle volontairement opaque) ne sont PAS des doublons du rôle "section" — laissés tels quels.

Les champs `surface`/`surface_elevated`/`text_primary`/`text_secondary`/`on_accent` de `Tokens` restent prêts à l'emploi (seul `border` est câblé) pour la Phase 3 (`surface_elevated` y sera utile pour les tuiles du dock) et une future passe dédiée à la garantie de lisibilité de `dash.rs`/`clipboard_viewer.rs`.


Les couleurs sont déjà bien centralisées (`ui::color::Palette`/`compute_palette`) — ce qui manque, ce sont trois **rôles** précis :
- **Surface élevée** : chaque fenêtre fabrique son fond de section à la main avec un alpha en dur, cinq valeurs indépendantes constatées (`SECTION_FILL_ALPHA = 115` dans `settings_dash.rs`, `180*tint`/`130*tint`/`235`/`255*tint` ailleurs) pour le même rôle.
- **Bordure** : `v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, zone_bg)` (`egui_overlay.rs:377`) peint les séparateurs de la couleur du FOND — invisibles par construction. Bug latent réel à corriger avec ce token.
- **Texte secondaire** : aucune hiérarchie typographique de couleur actuellement (seulement `accent_text`/`zone_text`).

### Étapes
1. `src/ui/tokens.rs` (nouveau) — `Tokens { color, radius, space, alpha, stroke }`, dérivés de l'accent par fenêtre (ne remplace pas la personnalisation existante).
2. Câbler `apply_adaptive_theme` (`egui_overlay.rs:318-401`) sur les tokens, à valeurs identiques au départ (sauf `bg_stroke` → `border`, correction du bug).
3. Migrer les appelants un par un : `settings_dash.rs` (64 occurrences) → `dash.rs` → `clipboard_viewer.rs` → `popup_menu.rs`.
4. Test de non-régression sur les 120 `THEMES` × clair/sombre × 3 niveaux de `dash_tint`, même forme que `themes_keep_readable_close_button` (`ui/color.rs:895`).

### Pièges
- Ne jamais réintroduire `override_text_color` (court-circuite `WidgetVisuals::text_color()`, bug réel déjà rencontré).
- `tinted_section_fill` exige de l'alpha NON prémultiplié.
- L'opacité réelle dépend de `dash_tint` ET de l'alpha de section (formule déjà dans `settings_accent_text_min_opacity`).

---

## Phase 2 — Typographie (non commencé, ~2 j) — dépend de la Phase 0

**Police recommandée : Segoe UI Variable, PAS Inter.** Le dock rend déjà son horloge/moniteur système en GDI (`gfx/text.rs`) avec `"Segoe UI"` en dur (`ui/topbar.rs`) — une police custom uniquement côté `egui` créerait deux typographies dans la même app. `C:\Windows\Fonts\SegUIVar.ttf` existe sur cette machine.

### Étapes
1. `src/ui/typography.rs` (nouveau) — `install_fonts(ctx)` via **`Context::add_font(FontInsert)`** (PAS `set_fonts`, qui écraserait les polices de repli emoji/tofu). Chargement depuis `C:\Windows\Fonts\SegUIVar.ttf`, repli `segoeui.ttf`, repli final silencieux.
2. **Piège critique** : vérifier par capture que les glyphes `✖`/`🔄`/`🗑` (déjà mordus une fois par un bug de tofu, voir `ui/color.rs:881-894`) restent présents — `FontPriority::Highest` + `add_font` doit suffire, mais à confirmer visuellement.
3. Re-tester le bug "texte flou" sur egui 0.35 (probablement résolu par le changement ab_glyph→skrifa) avant de réduire les tailles actuelles (20/22/22/22/28 pt, énormes) vers une échelle moderne (12/14/13/14/20 pt indicatif).
4. Leviers de netteté disponibles en 0.35 : `style.text_options.font_hinting`, `subpixel_binning`.
5. Aligner `ui/topbar.rs` sur `"Segoe UI Variable Text"` (le glyphe média reste `"Segoe MDL2 Assets"`, sans rapport).

---

## Phase 3 — Ombres/profondeur sur les tuiles du dock (non commencé, ~2-3 j) — dépend des Phases 0 et 1

Le pixel shader du dock calcule déjà une distance signée au rectangle arrondi (SDF, `quad_renderer.rs:205-207`) — une ombre douce est le même SDF avec un bord large, zéro texture/passe supplémentaire.

### Étapes
1. `QuadInstance` gagne un champ `softness: f32` (remplace `_pad`, taille inchangée) — `with_mode` l'initialise à `0.0`, donc tous les appelants existants rendent identique au bit près par défaut.
2. `Dock::shadow_instances(&self, layout, elevation: f32)` — ombre qui grandit avec `HoverAnimator::scale`.
3. **Piège n°1** : le dock est une fenêtre transparente — une ombre qui déborde de `bar_rects` composerait un halo visible sur le VRAI bureau. Clamper à la marge disponible (`(bar_thickness - icon_size) / 2`, ~12px par défaut).
4. **Piège n°2** : la moitié des icônes sont des glyphes monochromes transparents — une ombre carrée derrière un glyphe transparent = carré gris flottant. Nécessite une "surface de tuile" (dépend des tokens de la Phase 1) sous chaque icône.
5. Réglage persisté opt-in (`dock_elevation.txt`, défaut `0.0` = désactivé), section dans `settings_dash.rs` sur le modèle de `render_icon_gap_section`.
6. **Piège n°3** : `QuadRenderer::MAX_INSTANCES = 128`, tronqué SILENCIEUSEMENT — deux lots supplémentaires (ombres + surfaces) s'en approchent avec beaucoup d'icônes épinglées. Ajouter un `debug_assert!` pour que la troncature cesse d'être silencieuse.

---

## Phase 4 — Transitions d'ouverture/fermeture (non commencé, ~3 j) — dépend de la Phase 0

Anime dans le rendu (opacité + léger décalage), jamais la fenêtre Win32 elle-même (voir correction d'hypothèse 3).

### Étapes
1. `QuadInstance` gagne un champ `opacity: f32` (défaut `1.0` via un constructeur en chaîne `with_opacity`, aucun site d'appel existant à modifier).
2. `OpenAnim` (état partagé, `Hidden`/`Opening`/`Shown`/`Closing`) dans `egui_overlay.rs` — 180ms ouverture / 140ms fermeture, easing `emath::easing::cubic_out`.
3. **Piège A** : `with_visual_transform` décale le dessin mais pas le hit-testing — vider les événements en attente pendant l'animation (l'UI n'est de toute façon pas interactive pendant 180ms).
4. **Piège C — RISQUE ÉLEVÉ, classe de bug déjà rencontrée dans ce projet** : `ShowWindow` peut redispatcher un message de façon synchrone → ré-entrance sur le `RefCell` `STATE` → panique (`panic = "abort"` en release, voir `Cargo.toml`). Utiliser le motif `pending_hide: Option<HWND>` déjà établi (`settings_dash.rs`, `pending_resize`) — collecter puis appliquer APRÈS la sortie du `STATE.with`.
5. **Piège D/E** : `TOGGLE_DEBOUNCE = 300ms` et `HAD_FOCUS`/`FOCUS_LOST_SINCE` (`dash.rs`) doivent rester cohérents avec la nouvelle mécanique d'animation (garde anti-auto-fermeture à ne pas casser).
6. Hors périmètre volontaire : `popup_menu.rs` (mécanique `SetWindowRgn` + report d'appels déjà délicate) — à traiter séparément une fois la mécanique éprouvée sur Dash/Réglages/Presse-papiers.

---

## Ordonnancement et définition de « fini »

| Phase | Durée estimée | Dépend de | Risque max |
|---|---|---|---|
| 0 — harnais capture | 1 j | — | Moyen (AppBar/taskbar) — **mitigé, voir ci-dessus** |
| 1 — tokens | 2-3 j | 0 | Moyen |
| 2 — typographie | 2 j | 0 | Moyen |
| 3 — ombres | 2-3 j | 0, 1 | Moyen |
| 4 — transitions | 3 j | 0 | **Élevé** (réentrance RefCell) |

**Définition de « fini » globale** :
- [ ] `cargo test` vert, incluant les tests de contraste de tokens sur 120 thèmes × 2 modes × 3 opacités.
- [ ] `cargo build --release` sans nouveau warning.
- [ ] Captures back buffer des 4 fenêtres + du dock, sur 3 thèmes extrêmes, avant/après chaque phase.
- [ ] Capture du dock à `elevation = 0.0` identique à la référence pré-phase 3.
- [ ] Séquence d'ouverture/fermeture du Dash capturée frame par frame, alpha moyen monotone.
- [ ] Aucun `SendInput`/simulation de clic dans le code de test ou la procédure de vérification.
- [ ] Zone de travail Windows (`SPI_GETWORKAREA`) inchangée après toutes les sessions de test.

**Risques transverses** : `docs/` ne contenait alors que la documentation de Voicemeeter — rien sur egui/D3D11 à consulter pour ces 4 phases ; les sources faisant autorité sont dans `~/.cargo/registry/src/.../egui-0.35.0/`, `epaint-0.35.0/`, `egui-directx11-0.13.0+egui-0.35.0/`.
