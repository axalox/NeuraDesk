# Plan — migration de l'UI (Dash/Réglages/Presse-papiers/popups) vers CEF + HTML/CSS

Statut (2026-09-03) : **Migration terminée (Phases 0 à 7).** egui a été
entièrement retiré du code et du graphe de dépendances — voir le
paragraphe Phase 7 plus bas pour le détail. Historique complet des
phases conservé ci-dessous.

Statut initial (Phase 0/1) : **Phase 0 ET Phase 1 terminées.** Popups migrés vers CEF (HTML/CSS composé avec le flou d'arrière-plan existant, souris transmise à CEF) : Power (`cef_test_window.rs`), Calendar (`cef_calendar.rs`, pas d'IPC), Context (`cef_context.rs`, réutilise l'action `app/quit`), Network (`cef_network.rs`, canal Rust→JS booléen), Sound (`cef_sound.rs`, le plus gros des cinq — listes de périphériques audio dynamiques sérialisées en JSON via `serde_json`, IDs Windows encodés `encodeURIComponent`/décodés `win32::cef_app::percent_decode`, vérifié par capture avec les VRAIS périphériques de la machine, dont plusieurs entrées Voicemeeter/Voicemod). Toute la mécanique fenêtre/rendu commune a été extraite dans `win32::cef_popup` (chaque popup n'est plus qu'un fichier ~90-150 lignes déclarant sa `PopupSpec`) après que Context/Network aient rendu la duplication `cef_test_window.rs`/`cef_calendar.rs` trop coûteuse à continuer. Canal JS→Rust (`neuradesk://<action>`) et canal Rust→JS (`CefBrowserSurface::execute_js`/`Frame::execute_java_script`) tous les deux implémentés et vérifiés — voir "IPC Rust↔JS" plus bas. Sous-process CEF contenus par un Job Object même en cas d'arrêt brutal.

**Phase 2 terminée aussi** : `SpacerSize` (`cef_spacer_size.rs`) et `BarSize` (`cef_bar_size.rs`), tous deux pilotés par un `<input type="range">` natif — l'événement `input` (pas `change`) fait `location.href = "neuradesk://..."` à CHAQUE pas du glissement, MÊME mécanisme JS→Rust que tous les popups précédents (pas de nouveau canal à concevoir, l'inquiétude notée plus haut ne s'est pas matérialisée). Particularité propre à ces deux popups : une seule fenêtre CEF réutilisée pour PLUSIEURS cibles différentes dans le temps (n'importe quel espace/n'importe quelle barre, un seul exemplaire affiché à la fois, comme tous les popups) — `index`/`BarKind` retenus CÔTÉ RUST dans un `Cell` (`CURRENT_INDEX`/`CURRENT_KIND`) fixé à l'ouverture (`toggle`), jamais renvoyés par la page. Vérifié par capture (Rust→JS : la valeur/le libellé poussés à l'ouverture s'affichent correctement) ; le round-trip JS→Rust (glisser réellement le curseur) n'a PAS été testé en direct — contrairement aux popups précédents, `SetSpacerSize`/`SetBarThickness` appellent `config::save_spacer_sizes`/`save_bar_thicknesses`, qui écrivent dans `%APPDATA%\NeuraDesk\config.toml`, LE MÊME fichier que l'app réelle — un test automatisé l'aurait modifié pour de vrai. Le mécanisme JS→Rust est le même que celui déjà prouvé bout en bout par le test A/B de Power ; seul le parsing (`strip_prefix` + `parse::<f32>`) est nouveau, simple à relire.

**Phase 3 (Voicemeeter) terminée aussi — critère de succès validé.** `cef_voicemeeter.rs` + `assets/popups/voicemeeter.html` : c'est LE popup qui a motivé cette migration entière (sa table à 5 colonnes souffrait de plusieurs bugs RÉELS d'`egui_extras::TableBuilder`, documentés en détail dans `win32::popup_menu.rs`). CSS Grid (`.table`, `grid-template-columns`) aligne les colonnes NATIVEMENT — vérifié par capture avec le VRAI Voicemeeter Potato de cette machine (5 strips personnalisés "Spotify"/"Micro"/"ECRAN"/"Casque"/"TV", gains réels à 2 décimales, icône mute correcte sur "TV" [muet], cases d'assignation de bus par strip toutes correctes, bus "Odyssey G70B (NVIDIA..." tronqué proprement, "Canaux masqués (10)" — 16 canaux Potato au total moins les 6 visibles, arithmétique cohérente) : AUCUN des bugs d'alignement de la version egui ne s'est reproduit, exactement l'hypothèse de départ de toute cette migration.

Différence de conception par rapport aux popups précédents : l'état est poussé À CHAQUE FRAME tant que le popup est visible (`win32::cef_voicemeeter::poll`, `win32::cef_popup::is_visible`, nouvelle fonction), pas seulement à l'ouverture — Voicemeeter peut changer depuis lui-même ou une autre application connectée à son API. Risque identifié et traité CÔTÉ JS AVANT de le rencontrer en pratique (pas un bug trouvé après coup) : reconstruire le DOM à ~60 Hz aurait interrompu un glissement de curseur de gain en cours (un `<input type="range">` natif perd la capture souris dès que son nœud DOM est retiré) — `assets/popups/voicemeeter.html` ne reconstruit le DOM QUE si la structure affichée change (canal apparu/disparu/masqué), sinon met à jour les valeurs EN PLACE, en sautant tout curseur qui a actuellement le focus. Aucun clic/glissement réel testé sur cette capture (par prudence — cette machine a un Voicemeeter RÉELLEMENT configuré) : la vérification porte sur le sens Rust→JS (confirmé par capture ci-dessus) et sur la relecture du code du sens JS→Rust (même mécanisme `on_before_browse` déjà prouvé par le test A/B de Power).

**Phase 4 (Visionneuse Presse-papiers) terminée.** `cef_clipboard.rs` +
`assets/popups/clipboard.html` remplacent `win32::clipboard_viewer`
(module egui séparé, PAS un `PopupKind` — cette fenêtre remplit toute la
"zone libre" du dock comme le Dash, voir `app::dash_rect`). Deux ajouts
génériques à `win32::cef_popup`, RÉUTILISABLES par les prochaines phases :
- `resize_if_needed` — premier popup CEF dont la taille peut changer
  PENDANT qu'il est ouvert (`dash_rect` réagit aux curseurs d'épaisseur de
  barre) ; redimensionne le swap chain ET la texture CEF importée
  (`CefBrowserSurface::resize`, jusqu'ici sans appelant depuis Phase 0).
- `toggle_with` (`toggle` généralisé, garde son ancienne signature comme
  cas particulier) — un `on_show` appelé juste avant `force_foreground`,
  SEULEMENT sur la transition vers visible : nécessaire pour capturer
  `GetForegroundWindow()` (voir "Coller", plus bas) avant que
  `force_foreground` ne change justement ce que cet appel renverrait.

Miniatures ET image pleine résolution transmises en `data:image/
png;base64,...` (`ClipboardEntry::thumb_png_bytes`, nouvelle méthode
publique, réutilise `encode_png` déjà existant) — l'image pleine
résolution est chargée PARESSEUSEMENT, seulement à la sélection d'une
entrée (`window.setFullImage`), jamais les 30 entrées d'un coup. Une
particularité RÉSOLUE avant d'être rencontrée en pratique (pas un bug
trouvé après coup) : cette fenêtre ne relit plus `clipboard_history` en
direct à chaque frame comme la version egui (l'état est poussé une fois à
l'ouverture, pas à chaque frame comme Voicemeeter) — un copier ailleurs
PENDANT que la fenêtre est ouverte l'aurait donc laissée périmée sans
`win32::cef_clipboard::refresh_if_visible`, appelée depuis `app::run`
juste après l'insertion d'une nouvelle entrée dans `clipboard_history`
(`Event::ClipboardChanged`), plutôt qu'un sondage par frame.

Vérifié par capture avec le VRAI historique de cette machine (mélange de
texte — extraits de conversation, URL GitHub — et d'images, dont une
capture d'écran d'un outil de palette de couleurs) : miniatures
correctement chargées pour plusieurs images différentes, texte multi-
lignes préservé, panneau de prévisualisation affichant l'entrée
sélectionnée à l'échelle correcte (repli sur la miniature en l'absence de
clic réel — confirme que le mécanisme de repli progressif fonctionne
même sans interaction), fenêtre correctement redimensionnée à la grande
zone `dash_rect`. Aucun clic/glissement réel simulé (Coller/Enregistrer
sous/Effacer non exercés pour de vrai — `Coller` en particulier
enverrait un VRAI Ctrl+V vers la fenêtre précédente, un effet de bord
authentique sur une session live) : le mécanisme de dispatch partage le
`on_before_browse` déjà prouvé par le test A/B de Power, vérifié par
relecture de code pour le reste.

**Phase 5 (Dash) terminée — le plus gros morceau de toute la migration.**
`cef_dash.rs` + `assets/popups/dash.html` remplacent `win32::dash`. Même
périmètre que la version egui (voir sa doc de module) — PAS celui de
l'ancienne version WebView2/HTML historique (panneau "Gérer les
catégories" dédié, invites d'installation, lancement en administrateur :
tous déjà hors périmètre de la version egui remplacée, pas seulement de
celle-ci).

Deux choix de conception qui s'écartent du reste de la migration :
- **Recherche/filtrage/regroupement ENTIÈREMENT côté JS** (pas un
  aller-retour IPC par frappe comme les curseurs) — Rust pousse la liste
  COMPLÈTE une fois (`push_state`), JS filtre/trie/groupe lui-même
  (`matchesFilter`/`groupByCategory`/`categoryPriority`, dupliqués côté
  JS de `win32::dash.rs`, qui reste la référence si la logique doit
  changer). Un round-trip IPC par caractère tapé sur ~350 applications
  aurait été plus lent qu'un filtrage JS pur.
- **Extraction d'icônes dupliquée** (pas partagée avec `win32::dash.rs`
  via un module commun) : `win32::dash` reste un chemin de retour intact
  — même politique que les popups précédents, juste appliquée à un
  morceau de code plus gros que d'habitude. Le chargement PAR LOTS
  (`ICON_BATCH_PER_FRAME`) reste nécessaire quel que soit le framework
  d'affichage : un scan Shell (`SHGetFileInfoW`/`IImageList::GetIcon`)
  reste coûteux indépendamment d'egui/CEF — voir la doc de module de
  `win32::dash` pour le bug réel (gel + kill Windows) que cette
  contrainte évite.

**Premier popup à recevoir un vrai clavier** (voir "IPC Rust↔JS" plus
bas) : le champ de recherche est un `<input>` HTML natif, WM_KEYDOWN/
WM_CHAR relayés génériquement à `win32::cef_popup::dispatch_input`
(bénéficie aussi, gratuitement, à tous les popups avec un curseur —
flèches gauche/droite quand un `<input type="range">` a le focus).

Vérifié par capture avec le VRAI catalogue d'applications de cette
machine (~350 raccourcis, toutes catégories confondues) : icônes
correctement extraites pour des dizaines d'applications réelles (GIMP,
Spotify, VB-Cable, les six variantes Voicemeeter, 7-Zip, Bitdefender,
LibreOffice, Calibre...), groupes de catégories correctement affichés
et triés (Fréquemment utilisés en tête), libellés tronqués avec ellipse,
barre de catégories complète à droite, champ de recherche focalisé.
AUCUN clic réel sur une tuile (lancerait une VRAIE application) ni clic
droit (modifierait pour de vrai l'assignation de catégories persistée) :
vérifiés par relecture de code, même mécanisme `on_before_browse` déjà
prouvé par le test A/B de Power.

**Phase 6 (Panneau Réglages) terminée — dernier gros morceau de la
migration.** `cef_settings.rs` + `assets/popups/settings.html` remplacent
`win32::settings_dash` (~2200 lignes, ~20 sections). Particularité unique
à ce panneau : contrairement à TOUS les popups précédents, il ne possède
QUASIMENT aucun état à lui — il lit/modifie DIRECTEMENT les variables
locales d'`app::run` (`Dock`, épaisseurs de barre, `WindowAccent` × 3,
`Features`...), regroupées dans une struct `SettingsRefs` plutôt qu'une
vingtaine de paramètres positionnels — exactement ce que `win32::
settings_dash::render_if_visible` faisait déjà. Seule exception : "Mes
couleurs"/thèmes personnalisés/"Couleur de base", qui vivaient DÉJÀ dans
l'état propre du panneau egui (pas dans `app::run`), donc pareil ici.

Pipette d'écran (`WH_MOUSE_LL`/`GetPixel`, "reprendre une couleur affichée
n'importe où à l'écran") entièrement dupliquée de `win32::settings_dash` —
ni egui ni CEF n'y jouent le moindre rôle, un mécanisme Win32 pur porté
tel quel. Délibérément PAS de sélecteur de couleur natif (`<input
type="color">`) : aurait réintroduit la classe de bug de focus (fenêtre
top-level séparée) que `win32::settings_dash` a précisément fuie en
migrant loin de l'ancienne version WebView2 — hex + pipette + 48
préréglages + "Mes couleurs" couvrent le même besoin sans ce risque,
jamais vérifiable de façon fiable par ce harnais (interaction de focus).

État poussé à CHAQUE FRAME tant que visible (comme Voicemeeter) — un
thème complet touche huit réglages à la fois, un rafraîchissement total
est plus simple à garder correct qu'un suivi fin. Deux catalogues
STATIQUES (120 thèmes, 48 préréglages Windows) arrivent une seule fois à
l'ouverture (`window.setCatalog`), séparément de l'état récurrent.

Vérifié par capture avec la VRAIE configuration de cette machine : 7
couleurs personnalisées et 7 thèmes personnalisés réellement enregistrés,
état réel des cases "Fonctionnalités" (plusieurs désactivées), couleur de
barre réelle (#C33C23) synchronisée sur les trois barres à 25% de
transparence, écartement d'icônes (40px), intensité de flou (5), taille
de barre des tâches "Par défaut (Windows)" — tous corrects. Aucune action
simulée (le mode natif de teinte de la barre des tâches redémarre
`explorer.exe`, un effet de bord bien trop disruptif pour un test
automatisé) : le mécanisme de dispatch partage le `on_before_browse` déjà
prouvé par le test A/B de Power, vérifié par relecture de code pour le
reste.

**Phase 7 (retrait complet d'egui) terminée — migration achevée, les 7
phases sont closes.** Avant la suppression proprement dite, un vrai trou
de périmètre a été trouvé en relisant les dépendants de
`win32::popup_menu` : le menu "Presse-papiers rapide" (`PopupKind::
Clipboard`, une icône de la barre DIFFÉRENTE de la visionneuse pleine
page migrée en Phase 4 — miniatures/aperçus une ligne, clic = copier et
fermer, ni prévisualisation ni suppression) n'avait jamais été porté.
Corrigé AVANT le reste de la Phase 7, en isolation (`cef_clipboard_menu.
rs` + `assets/popups/clipboard_menu.html`, popup taille fixe, `cef_popup
::toggle` simple — pas `toggle_with`, rien à capturer côté focus
précédent), vérifié par capture avec le VRAI historique de cette machine
(miniatures 40px et texte tronqué corrects) et commité séparément avant
de toucher aux suppressions.

Suppressions effectives : `src/win32/{dash,settings_dash,clipboard_
viewer,popup_menu,egui_overlay}.rs`, les entrées `egui`/`egui-directx11`/
`egui_extras` de `Cargo.toml` (`grep -c "^name = \"egui" Cargo.lock` → 0,
confirmé absent du graphe de dépendances résolu). `force_foreground`/
`make_transparent_and_uncapturable` — les deux seules fonctions de
`egui_overlay.rs` sans la moindre dépendance egui, utilisées par TOUS
les popups CEF — relogées telles quelles dans `win32::cef_popup`
(récupérées via `git show HEAD:src/win32/egui_overlay.rs` après
suppression du fichier de travail). Trouvé au passage : `create_window`
faisait deux fois le même appel `DwmExtendFrameIntoClientArea` (une fois
dans `make_transparent_and_uncapturable`, une fois en double juste après)
— redondance présente depuis la Phase 0, jamais remarquée avant,
supprimée. `win32::popup_menu::BarKind` (Top/Left/Right) s'est avéré
strictement identique à `win32::menu::Bar`, préexistant — plutôt que de
relocaliser `BarKind`, toutes ses utilisations ont été renommées vers
`Bar`, éliminant le doublon.

Nettoyage explicitement DIFFÉRÉ, pas oublié : la suppression du code
egui a rendu mort du code dans `ui::color` (`Palette`, `compute_palette`,
`adaptive_zones`, `AdaptiveZones`, plusieurs fonctions de contraste) et
`win32::calendar` (`CalendarMonth` et ses fonctions associées) — laissé
en place pour l'instant plutôt que supprimé dans la même passe, `ui::
color` portant des tests unitaires (`base_color_themes_keep_readable_
close_button`, `themes_keep_readable_close_button`) qu'une suppression
hâtive aurait pu abîmer sans bénéfice pour cette phase.

Vérifié par `cargo build`/`cargo test` (5/5)/`cargo build --release`
tous verts sur ce dernier état, puis par une salve de capture de
régression sur 4 popups couvrant les chemins les plus modifiés par
cette phase (Power — chemin `cef_popup` de base ; Dash — la plus grosse
surface touchée par la chirurgie `app.rs` ; BarSize — renommage `BarKind`
→`Bar` ; Presse-papiers rapide — le popup ajouté juste avant) : les
quatre rendent correctement, rien n'a régressé.

**Les 7 phases du plan sont terminées — toute l'interface de NeuraDesk
tourne sur CEF/HTML/CSS, egui a été entièrement retiré du code et du
graphe de dépendances.**

> Ce document a été écrit directement (le premier essai de délégation à l'agent `planner` a échoué trois fois de suite sur une surcharge serveur transitoire, sans rapport avec la tâche) — construit sur la recherche déjà faite dans la conversation : un spike autonome exécuté (`C:\dev\cef-spike`, hors du repo) et une lecture directe du code source de `cef-rs` et des fichiers concernés de ce projet.

## Pourquoi CEF OSR ne reproduit PAS le problème qui a tué WebView2

L'ancienne version de NeuraDesk utilisait WebView2 (Chromium) et a été entièrement réécrite vers D3D11 pour une seule raison, documentée dans l'historique du projet : **WebView2 possède sa propre fenêtre et son propre compositeur** — aucun moyen d'intercepter les pixels avant qu'il les affiche, donc aucune transparence/flou en temps réel possible.

**CEF en mode Off-Screen Rendering (OSR) est structurellement différent** : Chromium ne possède AUCUNE fenêtre. Il rend chaque frame dans une texture que l'application contrôle entièrement — soit un buffer CPU (`on_paint`), soit (mode accéléré) une **texture D3D11 partagée** directement importable dans le device D3D11 de l'application. C'est l'application qui compose cette texture avec son propre pipeline (flou compris), exactement comme elle le fait déjà pour le fond flouté des fenêtres `egui` actuelles.

## Validé par un spike réel (pas une hypothèse)

Spike autonome, hors du repo (`C:\dev\cef-spike`), CEF 151.8.0 via le crate `cef`/`cef-dll-sys` ([tauri-apps/cef-rs](https://github.com/tauri-apps/cef-rs), publié sur crates.io, activement maintenu) :

- **Transparence OSR réellement exploitable** — vérifié en lisant l'octet alpha BRUT du buffer `on_paint` (pas une inspection visuelle) : zone hors contenu HTML → alpha=0 ; carré CSS `rgba(255,0,0,0.5)` → alpha=128 (prémultiplié BGRA, cohérent). D'anciens rapports de bugs (CEF ancien/cefpython, trouvés d'abord par recherche web) ne s'appliquent plus à cette version.
- **Le chemin d'intégration D3D11 ne nécessite AUCUNE dépendance supplémentaire** (pas de wgpu) : en lisant `cef-rs/cef/src/bindings/x86_64_pc_windows_msvc.rs`, `AcceleratedPaintInfo::shared_texture_handle` est un `HANDLE` Win32 BRUT (`windows_sys::...::HANDLE`), pas un wrapper. Il est importable directement dans le `GfxDevice.device` déjà existant de ce projet (`src/gfx/device.rs`) via `ID3D11Device1::OpenSharedResource1::<ID3D11Texture2D>` (ou `ID3D11Device5`, à confirmer laquelle expose la méthode dans `windows` 0.62 — les deux existent dans la crate). L'aide "pratique" `cef::osr_texture_import::SharedTextureHandle` de cef-rs importe DANS wgpu — À NE PAS UTILISER, elle ajouterait un second backend de rendu parallèle à D3D11 pour rien.
- **Point d'insertion dans le pipeline de rendu existant déjà identifié** (`src/win32/egui_overlay.rs::EguiOverlay::frame`, lignes ~792-863) : la frame dessine déjà DEUX quads successifs dans le même `rtv` — 1) le fond flouté (`QuadInstance::new_backdrop`, `TexMode::Backdrop`) via `self.quad_renderer`, 2) le contenu (aujourd'hui `egui_renderer.render`). **`src/gfx/quad_renderer.rs` a déjà un mode générique de quad texturé avec alpha RÉELLE de la texture** (`QuadInstance::new_textured`, `TexMode::Texture`, "seule son alpha réelle vient de l'image", ligne 84-88) — c'est exactement le mode à utiliser pour dessiner la texture CEF importée, à la place de `egui_renderer.render`. Zéro changement de shader nécessaire.
- **Pièges rencontrés et résolus** (sinon crash natif SILENCIEUX — `STATUS_BREAKPOINT`/0x80000003, aucune exception normale, aucun rapport Windows Error Reporting, zéro sortie console) :
  1. `cef::api_hash(sys::CEF_API_VERSION_LAST, 0)` DOIT être le TOUT PREMIER appel à l'API CEF dans `main()`, avant même `Args::new()`.
  2. La distribution CEF téléchargée est compilée AVEC le bac à sable Chromium — sans `--no-sandbox` injecté via `on_before_command_line_processing` (le hook officiel de l'App handler — PAS `Args::as_cmd_line()`, qui n'a aucun effet), même crash silencieux.
- **Build** : `cef-dll-sys`'s `build.rs` compile la couche C++ `libcef_dll_wrapper` localement via **CMake + Ninja + MSVC** à chaque build propre (~1 min, mis en cache ensuite) — CMake et Ninja ont dû être installés sur cette machine (`winget install Kitware.CMake`/`Ninja-build.Ninja`), absents avant.
- **Runtime à distribuer** à côté de l'exe : `libcef.dll`, `chrome_elf.dll`, `d3dcompiler_47.dll`, `dxcompiler.dll`, `dxil.dll`, `icudtl.dat`, `chrome_100/200_percent.pak`, `resources.pak`, `libEGL.dll`, `libGLESv2.dll`, `vk_swiftshader.dll`, `vulkan-1.dll`, dossier `locales/` (~150 fichiers `.pak`) — plusieurs centaines de Mo, contre zéro pour WebView2 (qui réutilisait Edge déjà installé sur Windows).
- **Architecture multi-process INHÉRENTE à CEF** : chaque lancement re-exécute le MÊME exe avec des arguments `--type=...` différents pour les sous-process (rendu, GPU, réseau, utilitaire) — une dizaine de sous-process constatés au démarrage du spike, même pour une seule page statique.

## Ce qui reste HORS PÉRIMÈTRE (contraintes dures, ne changent pas)

- **Le dock lui-même** (barres, icônes, drag&drop, animation de survol — `src/gfx/quad_renderer.rs`, `src/ui/dock.rs`) reste en rendu D3D11 custom tel quel. Cette migration ne concerne QUE les fenêtres actuellement en egui.
- **Le flou d'arrière-plan GPU** (`src/gfx/backdrop.rs`, `src/gfx/duplication.rs`, `src/gfx/blur.rs`, Desktop Duplication + dual Kawase) — les fenêtres CEF composent AVEC ce flou déjà existant (même mécanisme `capture_blurred_batch`/`QuadInstance::new_backdrop`), pas de flou dupliqué côté CEF.
- **Le harnais de vérification visuelle** (`src/gfx/capture.rs`, `src/win32/debug_capture.rs`, variables `NEURADESK_CAPTURE*`) — TOUJOURS le seul moyen de vérifier visuellement le rendu (fenêtres exclues de toute capture d'écran externe, `WDA_EXCLUDEFROMCAPTURE`). À étendre pour capturer aussi le contenu CEF, pas seulement le dock/les fenêtres egui restantes.
- **Contrainte de sécurité de session déjà établie** : ne jamais simuler de clic/input réel pour tester — vérification par capture de back buffer + appels de fonction directs (`dash::toggle` etc., pattern déjà en place, à reproduire pour CEF : `browser.host().send_mouse_click_event(...)` appelé directement plutôt qu'un `SendInput` Win32).

---

## Phase 0 — Preuve de bout en bout sur UN SEUL popup (obligatoire avant tout le reste)

### État réel (2026-09-03)
**Le cœur technique est prouvé** : `gfx/cef_texture.rs` + `win32/cef_app.rs` + `win32/cef_test_window.rs` (fenêtre de TEST, pas encore un vrai `PopupKind`) affichent réellement `assets/popups/power.html` — capturé, vérifié visuellement (texte "Veille"/"Redémarrer"/"Arrêter"/"Déconnexion"/"Quitter NeuraDesk" lisible). Bug réel rencontré et corrigé : `WindowInfo::shared_texture_enabled` non activé au premier essai → CEF envoyait `on_paint` (jamais implémenté) au lieu d'`on_accelerated_paint`, aucun contenu n'apparaissait SANS LA MOINDRE ERREUR (fond flouté seul, silencieusement).

**Reste avant de considérer la Phase 0 100% terminée** :
1. ✅ Fond flouté visible SOUS le contenu CEF — bug réel trouvé et corrigé (`QuadRenderer::render` efface la cible à chaque appel ; deux appels séparés faisaient disparaître le fond). Un seul appel avec les deux lots, vérifié par capture.
2. ✅ Souris transmise à CEF (`CefBrowserSurface::on_mouse_move`/`on_mouse_left_button`/`set_focus`, câblés dans `cef_test_wndproc`) — vérifié par un appel direct (`debug_hover`, pas `SendInput`) : le survol CSS du bouton "Veille" apparaît. Clavier PAS encore câblé (page de test sans champ de texte, pas nécessaire pour l'instant).
3. ✅ Un vrai clic sur l'icône Alimentation ouvre maintenant `win32::cef_test_window` (renommage du module toujours "cef_test_window" pour l'instant — le nom ne reflète plus vraiment son rôle, à renommer lors d'une prochaine passe de nettoyage). `PopupKind::Power`/`render_power` (`win32::popup_menu.rs`) restent dans le code, INUTILISÉS pour cette icône — retirés seulement une fois la Phase 1 (popups restants) terminée, pas avant.
4. ✅ Nettoyage des sous-process CEF à la fermeture — cause identifiée (`--no-sandbox` désactive la gestion de conteneurs de processus interne à Chromium) et corrigée indépendamment de CEF via un Job Object Windows (`win32::cef_app::contain_child_processes`, `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) : vérifié par test explicite (lancement + `taskkill /F` du parent + attente ~10s → plus aucun sous-process orphelin).
5. ✅ `cargo build --release` testé avec CEF — passe proprement.

**Limitation résolue depuis** : cliquer un bouton du popup déclenche maintenant réellement une action côté Rust — voir "IPC Rust↔JS" dans "Risques transverses" plus bas pour le mécanisme (`neuradesk://<action>`, `RequestHandler::on_before_browse`). Tous les boutons de `power.html` (Verrouiller/Veille/Redémarrer/Arrêter/Déconnexion/Quitter) sont câblés vers `win32::power::execute`/`should_close`. Vérifié SANS jamais déclencher une action destructive réelle (verrouillage/redémarrage/arrêt) pendant la vérification automatisée : comparaison A/B au harnais de capture, un run cliquant "Quitter NeuraDesk" (via `cef_test_window::debug_click`, appel de fonction direct, jamais `SendInput`) à la frame 40 s'arrête après la capture de la frame 30 et ne capture jamais les frames 45/60, alors qu'un run témoin identique sans clic les capture toutes — preuve que le chemin complet onclick→URL interceptée→`drain_actions`→`should_close = true` fonctionne, sans avoir à observer un effet Win32 réel pour les actions destructives.

**Candidat recommandé : `PopupKind::Power`** (`src/win32/popup_menu.rs::render_power`), pas `Calendar` — plus simple (une liste de boutons, pas de grille avec pastille "aujourd'hui" à reproduire en CSS), petite taille (220×380), aucune dépendance à un état système complexe (contrairement à Voicemeeter/Network). Calendar reste un bon 2e candidat pour la Phase 1 (grille CSS, un peu plus de mise en page réelle à valider).

### Étapes
1. **`src/gfx/cef_texture.rs`** (nouveau) — `pub fn open_shared_texture(device: &ID3D11Device1, handle: HANDLE) -> Result<(ID3D11Texture2D, ID3D11ShaderResourceView)>`, wrapper direct autour d'`OpenSharedResource1`/`CreateShaderResourceView`. Note : `GfxDevice.device` est aujourd'hui un `ID3D11Device` (voir `src/gfx/device.rs`) — vérifier s'il faut `.cast::<ID3D11Device1>()` (méthode `windows-rs` standard sur les interfaces COM) ou si `GfxDevice::new` doit demander `ID3D11Device1` dès la création.
2. **`src/win32/cef_overlay.rs`** (nouveau, à côté d'`egui_overlay.rs`, pas à la place — les deux coexistent pendant toute la migration) — mécanique CEF commune : subprocess dispatch (`main()` modifié, voir étape 5), `App`/`Client`/`RenderHandler` minimal calqué sur le spike (`view_rect` = taille de la fenêtre popup, `on_accelerated_paint` → importe la texture via l'étape 1 et la stocke pour le rendu, PAS `on_paint`/buffer CPU — le mode accéléré est ce qui évite un aller-retour CPU→GPU à chaque frame), traduction souris/clavier vers `browser.host().send_mouse_click_event`/`send_mouse_move_event`/`send_key_event` (Win32 `WM_MOUSEMOVE`/`WM_LBUTTONDOWN`/`WM_CHAR`/`WM_KEYDOWN` déjà traduits pour egui dans `egui_overlay.rs::on_mouse_move` etc. — même point d'entrée, juste redirigé vers CEF au lieu d'`egui::RawInput`).
3. **`src/win32/popup_menu.rs`** — nouvelle variante `PopupKind::PowerWeb` (temporaire, ou un flag sur `PopupKind::Power` le temps de la Phase 0) : au lieu d'appeler `render_power(ui)`, crée/réutilise une fenêtre `CefOverlay` chargeant un fichier HTML local (`assets/popups/power.html`, nouveau dossier) reproduisant le contenu de `render_power` en CSS. Rendu par frame : dessiner le quad de fond flouté (`QuadInstance::new_backdrop`, INCHANGÉ) PUIS un quad `QuadInstance::new_textured` lié à la SRV importée de CEF (remplace `egui_renderer.render` pour cette fenêtre précise).
4. **`assets/popups/power.html`** (nouveau) — reproduit `render_power` : liste de boutons (Veille/Redémarrer/Arrêter/Déconnexion + séparateur + Quitter NeuraDesk), CSS minimal (pas de framework — vanilla CSS suffit pour ce popup). `<body>` SANS `background` (transparence par défaut, voir la doc de `cef-rs::WindowInfo::set_as_windowless`).
5. **`src/main.rs`** — dispatch subprocess CEF (`api_hash` PUIS `Args::new()` PUIS `execute_process`, voir les pièges ci-dessus) AVANT toute autre initialisation — le process actuel crée déjà la fenêtre du dock très tôt dans `app::run()` ; l'init CEF (`cef::initialize`) doit se faire une fois, dans le process navigateur uniquement, probablement juste après (à valider par essai : CEF a besoin d'une boucle de messages Win32 compatible avec celle déjà en place dans `Window::pump_messages`/`do_message_loop_work` — CEF fournit `do_message_loop_work()` en pompe externe, à intercaler dans la boucle `'main: loop` existante d'`app::run`, PAS une boucle séparée).
6. **`src/win32/debug_capture.rs`** — étendre `NEURADESK_DEBUG_OPEN=power-web` (ou équivalent) pour ouvrir cette variante et capturer son back buffer, même mécanisme que les autres (`EguiOverlay::dump_backbuffer` a un équivalent direct : dumper le back buffer du swap chain de la fenêtre `CefOverlay`, indépendant du fait que le contenu vienne d'egui ou de CEF — `gfx::capture::dump_backbuffer_composited` ne connaît que le swap chain, pas ce qui l'a peint).

### Pièges spécifiques à surveiller
- **Réentrance/thread** : CEF s'attend à tourner sur le thread UI (celui qui a appelé `cef::initialize`) — vérifier que ce thread est le MÊME que la boucle de messages Win32 existante (`app::run`, un seul thread aujourd'hui) ; sinon tous les appels `browser.host()...` doivent être postés sur le bon thread (CEF a ses propres primitives pour ça, `cef::post_task`).
- **Focus clavier** : contrairement à egui (géré par ce projet via ses propres `WM_KEYDOWN`/`WM_CHAR` traduits), CEF OSR attend explicitement `SetFocus`/`send_focus_event` en plus des événements clavier — sans ça, le texte tapé peut ne jamais atteindre la page.
- **Redimensionnement** : `view_rect` doit changer ET `host.was_resized()` doit être appelé si jamais la taille de la fenêtre change dynamiquement (a priori rare pour ces popups à taille fixe, mais `PopupKind::Voicemeeter`/`Calendar` redimensionnent leur fenêtre au contenu réel — à garder en tête pour les phases suivantes, pas bloquant pour Power).
- **Cycle de vie du sous-process CEF si `neuradesk.exe` est tué brutalement** — la mémoire du projet documente déjà des `neuradesk.exe` zombies après un kill du process principal (bénin, un seul process) ; avec CEF, un kill brutal du process principal risque de laisser des sous-process CEF orphelins (rendu/GPU) — à vérifier explicitement (ouvrir Power-web, `taskkill /F` le process principal, `tasklist` ensuite) avant de considérer cette phase terminée.

### Critères de vérification (Phase 0 terminée)
- [x] `cargo build`/`cargo build --release` passent avec la nouvelle dépendance `cef` ajoutée à `Cargo.toml`.
- [x] Capture (`NEURADESK_CAPTURE`, harnais existant) du popup Power-web : boutons visibles, texte lisible, fond flouté du dock visible à travers (transparence confirmée EN CONTEXTE réel, pas seulement dans le spike isolé).
- [ ] Capture à deux niveaux de transparence différents (curseur "Transparence" du panneau Réglages) : le flou CEF suit bien le réglage existant (le tint est appliqué par le MÊME quad `new_backdrop` que les fenêtres egui, donc automatique — à confirmer par capture, pas supposer).
- [x] Clic sur un bouton (via `cef_test_window::debug_click`, appel de fonction DIRECT sur `browser.host()` — PAS de `SendInput`) déclenche l'action réelle correspondante — vérifié pour "Quitter NeuraDesk" par comparaison A/B au harnais de capture (voir plus haut), les autres boutons partagent le même chemin `on_before_browse`/`drain_actions`/match.
- [x] `taskkill /F` du process principal pendant que Power-web est ouvert : aucun sous-process CEF ne reste (`tasklist` après, comparé à avant ouverture, ~10s de délai observé).
- [ ] Temps entre le clic sur l'icône Power et le premier rendu visible du popup mesuré (capture avec plusieurs frames numérotées, comme le harnais le fait déjà) — comparé au temps egui actuel, pour quantifier le coût du multi-process CEF sur la réactivité perçue.

---

## Phases suivantes (aperçu, à détailler une fois la Phase 0 validée)

| Phase | Contenu | Dépend de |
|---|---|---|
| 1 | ~~`Calendar`~~ ✅ + ~~`Context`~~ ✅ + ~~`Network`~~ ✅ + ~~`Sound`~~ ✅ — **Phase 1 terminée** | 0 |
| 2 | ~~`SpacerSize`/`BarSize`~~ ✅ (curseurs `<input type=range>`, événement `input` — même canal `neuradesk://` que les popups précédents) | 0 |
| 3 | ~~`Voicemeeter`~~ ✅ (CSS Grid a réellement résolu les bugs de colonnes de TableBuilder, validé par capture avec un Voicemeeter Potato réel) | 1, 2 |
| 4 | ~~Visionneuse Presse-papiers~~ ✅ (liste + aperçu image, `<img src="data:...">`, chargement paresseux de la pleine résolution) | 1 |
| 5 | ~~Dash~~ ✅ (recherche + grille d'icônes + catégories, filtrage côté JS, premier popup avec un vrai clavier) | 1-4 |
| 6 | ~~Panneau Réglages~~ ✅ (~20 sections, pipette d'écran Win32 dupliquée telle quelle, pas de `<input type="color">`) | 5 |
| 7 | ~~Retrait complet d'`egui`/`egui-directx11`/`egui_extras`~~ ✅ (+ menu "Presse-papiers rapide" migré au passage, gap trouvé en cours de route ; `BarKind`/`Bar` consolidés ; `ui::color`/`win32::calendar` mort mais délibérément non nettoyé) | 1-6 |

Chaque phase suit le même schéma que la Phase 0 : fichier HTML/CSS par popup/fenêtre, extension du harnais de capture, vérification sans input simulé.

## Risques transverses

- **IPC Rust↔JS — implémenté DANS LES DEUX SENS** :
  - **JS→Rust** : `assets/popups/*.html` fait `location.href = "neuradesk://<action>"` (ex. `power/lock`, `app/quit`, `network/toggle-wifi`) ; côté Rust, `win32::cef_app::NeuraDeskRequestHandler::on_before_browse` intercepte AVANT toute navigation réelle, extrait `<action>` de l'URL (`CefString::from(&request.url()).to_string()`), le pousse dans `RenderState.actions: Vec<String>` (même `Rc<RefCell<...>>` que la texture importée — pas de synchronisation cross-thread nécessaire, CEF tourne sur le thread UI via la pompe externe), et retourne `true` pour annuler la navigation. `win32::cef_popup::drain_actions()` vide cette file à chaque frame (`app::run`, juste après `render_if_visible`), avec un `match` texte → `win32::power::execute`/`should_close = true`/`win32::network::toggle_wifi`/`toggle_bluetooth`.
  - **Rust→JS** : `CefBrowserSurface::execute_js(code)` (`win32::cef_app.rs`) appelle `Frame::execute_java_script` sur la frame principale du navigateur CEF — utilisé par `win32::cef_network::push_state()` pour pousser l'état Wi-Fi/Bluetooth (`win32::network::wifi_state`/`bluetooth_state`, PAS calculable en JS pur, contrairement au calendrier) via `window.setState({wifi, bluetooth})` (voir `assets/popups/network.html`), à l'ouverture du popup ET après chaque bascule effective — sans ce mécanisme la page resterait figée sur l'état lu au tout premier chargement. Vérifié par capture : l'état réel de la machine ("Wi-Fi : indisponible sur cet appareil", "Bluetooth désactivé") apparaît bien dans la page, pas un texte statique. Même mécanisme réutilisé tel quel par Sound/Voicemeeter/Dash (listes dynamiques, JSON via `serde_json`) — aucune reconception nécessaire phase après phase.
  - **Clavier** (ajouté en Phase 5, pour le champ de recherche du Dash) : `win32::cef_app::CefBrowserSurface::on_key_down`/`on_key_up`/`on_char` appellent `host.send_key_event` (`cef::KeyEvent`, `windows_key_code` = code de touche virtuelle Win32 brut, CEF utilise directement les mêmes valeurs `VK_*` sur Windows) — relayés génériquement par `win32::cef_popup::dispatch_input` (`WM_KEYDOWN`/`WM_SYSKEYDOWN`/`WM_KEYUP`/`WM_SYSKEYUP`/`WM_CHAR`/`WM_SYSCHAR`), donc disponible GRATUITEMENT à tous les popups, pas seulement au Dash (un `<input type="range">` focalisé réagit désormais aussi aux flèches, comportement natif du navigateur, aucun code supplémentaire nécessaire).
- **Taille de l'installation** : passer de ~10-20 Mo (exe seul) à plusieurs centaines de Mo (runtime CEF) — impact sur la distribution si jamais partagée au-delà de cette machine.
- **Temps de démarrage** : le multi-process CEF ajoute un coût non nul à l'ouverture d'un popup (mesuré comme critère Phase 0 ci-dessus) — si perceptible, envisager de garder CEF "chaud" (processus de rendu déjà lancé, fenêtre juste masquée) plutôt que de le créer à chaque ouverture, comme le fait déjà `hide_if_visible`/`ShowWindow(SW_HIDE)` pour les fenêtres egui actuelles.
- **`panic = "abort"` en profil release** (`Cargo.toml`) — une panique côté Rust dans un callback CEF (appelé depuis du code C++ natif) a un comportement d'unwinding across-FFI-boundary non défini même en profil non-abort ; en profil abort, tout panic dans un callback `RenderHandler`/`Client` tue immédiatement le process SANS le filet `catch_unwind` que ce projet utilise déjà pour ses wndproc Win32 (voir la mémoire du projet sur les bugs de réentrance déjà rencontrés) — à documenter comme contrainte : tout callback CEF doit être écrit avec la même discipline "jamais paniquer" que les wndproc existants.

## Definition of "fini" (Phase 0 uniquement — les phases suivantes auront la leur)

- [ ] Tous les critères de vérification de la Phase 0 ci-dessus cochés.
- [ ] `cargo test` (suite existante) toujours verte — aucune régression sur `ui::tokens`/`ui::color`.
- [ ] Documentation de ce fichier mise à jour avec le retour d'expérience réel (temps de démarrage mesuré, choix IPC pressenti pour la suite) avant de démarrer la Phase 1.
