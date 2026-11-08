# La barre du bas — s'inspirer de Cairo-Dock

Le socle est **livré et poussé** (`fdf9f77`) : la barre existe, elle occupe le milieu
de l'écran, elle porte des raccourcis de dossiers avec vignettes, une infobulle, et
ses réglages par barre. Ce plan décrit ce qui vient **après**, demandé le
2026-09-28 : « une gestion de la barre du bas avec des fonctionnalités similaires à
Cairo-Dock ainsi que ses fonctionnalités pour les dossiers ».

## Ce que Cairo-Dock a réellement — LU dans sa source, pas supposé

`cairo-dock-plug-ins` compte **74 plugins**. Ceux qui touchent aux dossiers :

| Plugin | Rôle |
| --- | --- |
| `Folders/` | l'applet de dossiers — **un gestionnaire de fichiers ancré au dock** |
| `stack/` | la **pile** : le contenu d'un dossier empilé, qu'on déroule |
| `quick-browser/` | navigateur de fichiers rapide |
| `shortcuts/` | raccourcis |
| `drop-indicator/` | le **retour visuel** pendant un dépôt |
| `shared-files/` | partage de fichiers |

Gestion du dock : `dock-rendering/` (modes de rendu), `icon-effect/`,
`Animated-icons/`, `illusion/`, `motion-blur/`, `switcher/`.

### `Folders/` en détail (six fichiers, ~45 Ko)

`applet-notifications.c` porte **vingt fonctions** — c'est la liste des capacités :

- `_cd_folders_open_folder`
- `_cd_folders_new_file`, `_cd_folders_new_folder` — **créer**
- `_cd_folders_rename_file` + `_on_answer_rename_file` — **renommer**, avec confirmation
- `_cd_folders_move_file` — **déplacer**
- `_cd_folders_delete_file` + `_on_answer_delete_file` — **supprimer**, avec confirmation
- `_cd_folders_show_file_properties` — **propriétés**
- `_cd_folders_sort_by_name` / `_by_date` / `_by_size` / `_by_type` — **quatre tris**
- `cd_folders_on_drop_data` — **accepte un dépôt**
- `cd_folders_free_apps_list`, `_free_app` — le modèle de la liste d'enfants
- `_on_answer_import`

`applet-load-icons.c` (14 Ko) charge les icônes des enfants — c'est le sous-dock.
`iSubdockViewType` décide si le sous-dock reprend l'icône du dossier.

## Ce qui se transpose ici, et ce qui n'a pas de sens

**Se transpose**

- **Le sous-dock / l'éventail** : le dépôt a **déjà** le mécanisme —
  `win32::window_previews` est un panneau ancré à une icône qui s'écarte de sa barre
  et se clampe à l'écran. C'est le point de départ le moins cher.
- **Les icônes des enfants** : `gfx::icon` (`SHGetFileInfoW` + `SHGetImageList`)
  sait déjà extraire l'icône d'un fichier — c'est ce que fait le Dash.
- **Les actions de fichier** : `file_dialog` (ouverts/enregistrer), `appicon::launch`,
  `appicon::open` existent ; il manque `IFileOperation` pour renommer/déplacer/
  supprimer proprement (avec la corbeille et les confirmations de Windows).
- **Le retour visuel** : `HoverAnimator`, la magnification et le tirage existent.

**Ne se transpose PAS**

- **Les modes de rendu 3D** (cube, plan incliné) : le dock est un cadre D3D11 plat, et
  le raymarching a déjà été refusé pour cette raison même (108 pas × 5 formes contre
  17 évaluations/pixel à 144 Hz).
- **Les lanceurs `.dockitem`** : `PinnedApp` + `dock_pins.txt` en sont l'équivalent.
- **Le moteur de thème** : le chantier « thèmes façon KDE » a encore ses lots 6 et 7 —
  ne pas le doubler.

**Le plus cher**

- **Épingler par glisser-déposer d'un dossier sur la barre** : demande un
  **DropTarget OLE**, dont ce dépôt n'a **aucun** aujourd'hui.

## Les quatre lots, dans l'ordre demandé

Chacun est **essayé par l'utilisateur** avant le suivant (règle du dépôt : une capture
prouve qu'une page s'affiche, jamais qu'elle réagit).

### Lot 1 — Le sous-dock

Survoler une vignette de dossier ouvre **ses enfants** en éventail, ancrés à la
vignette. Ouvrir un enfant : un sous-dossier **entre** dedans (change le contenu),
un fichier se lance.

- Point de départ : `win32::window_previews` (ancrage, placement, clamp à l'écran).
- Les icônes viennent de `gfx::icon`.
- ⚠️ Une **seule** fenêtre de recouvrement réutilisée, comme les aperçus — pas une par
  dossier.
- ⚠️ Le survol d'un enfant doit pouvoir **remonter** vers le parent sans refermer :
  c'est le geste qui fait qu'un sous-dock est utilisable ou non.

### Lot 2 — La pile

Le dossier affiché en **pile de vignettes** qu'on déroule au clic. Réutilise la
machinerie du lot 1 ; la différence est la disposition (empilée, non éventail) et le
mode d'ouverture.

### Lot 3 — Le glisser-déposer pour épingler

Déposer un dossier sur la barre l'épingle. Demande un `IDropTarget` sur la fenêtre du
dock — **rien de tel n'existe dans le dépôt** : c'est un ajout Win32 à part entière,
avec ses propres pièges (le fil, les formats, `DragAcceptFiles` ne suffit pas).

### Lot 4 — Le reste, selon l'usage

Candidats, dans l'ordre où Cairo-Dock les a : les **quatre tris** (`_by_name`,
`_by_date`, `_by_size`, `_by_type`), l'icône par **type de fichier**, et les **actions
de fichier** (renommer, déplacer, supprimer, propriétés) — celles-là demandant
`IFileOperation` pour passer par la corbeille et les confirmations de Windows plutôt
que par nos propres dialogues.

## Ce qu'il ne faut PAS oublier

- ⛔ **`app::run` construit deux `DockLayout`** — celle du CLIC et celle du RENDU. Tout
  nouveau champ doit aller dans **les deux**, plus `startup_layout`. C'est ce qui a
  coûté les trois gestes muets de la barre du bas (voir la mémoire du projet).
- ⛔ **Une fonction qui rend `Option<Side>` est un `match` exhaustif déguisé** : ajouter
  une variante d'énumération ne fait rien signaler. Chercher par
  `grep "-> *Option<Side>"`, et par `Option<BarreTirage>`, `Option<BarLetter>`.
- ⛔ **La capture est le bon instrument pour « est-ce dessiné »**, et le mauvais pour
  « est-ce que ça réagit ». Les deux questions ont été confondues pendant cinq
  allers-retours : s'en souvenir avant de relire du code.
