# Origine de ces icônes

Glyphes de la barre du haut/dock (réglages, presse-papiers, son, réseau,
terminal, notes, afficher le bureau) — remplacent les glyphes Segoe MDL2 Assets
d'origine par des icônes [Iconsax](https://iconsax.io/) (par Vuesax),
style **Outline**.

- SVG source : `svg/*.svg`, récupérés depuis
  [rendinjast/iconsax-react](https://github.com/rendinjast/iconsax-react)
  (licence MIT, voir `svg/LICENSE`).
- `*.png` : rastérisation 128×128 des SVG ci-dessus via `resvg`, en
  conservant le remplissage d'origine (`#292D32`) — sans importance ici,
  seul le canal alpha est échantillonné par le shader (voir
  `gfx::quad_renderer`, `TexMode::Glyph`), la couleur affichée vient de la
  teinte appliquée au quad.

| Fichier      | Icône Iconsax (style Outline)     |
|--------------|-------------------------------------|
| settings.png | `Settings/setting-2.svg`            |
| clipboard.png| `Content-Edit/clipboard-text.svg`   |
| sound.png    | `Video, Audio, Image/volume-high.svg` |
| network.png  | `Essetional/wifi.svg`               |
| terminal.png | `Programing/code.svg`               |
| notes.png    | `Content-Edit/note-text.svg`        |
| monitor.png  | `Computers, Devices, Electronics/monitor.svg` |

Iconsax n'a pas d'icône dédiée "fenêtre de terminal" (set orienté UI
produit, pas OS/dev) — `terminal.png` est une approximation volontaire
(symbole `>_`) plutôt qu'un équivalent littéral. `power.png` était dans le
même cas (un éclair dans un cercle) : voir plus bas, elle vient désormais de
Keyline Icons.

Même remarque pour `monitor.png` ("afficher le bureau" / Show Desktop) : le
bouton de Windows est un simple rectangle sans iconographie, Iconsax n'a pas
d'icône « bureau » — un écran d'ordinateur (`Computers, Devices,
Electronics/monitor`) est l'approximation retenue, la plus lisible en petit
dans une barre de 80 px.

## `theme.png` — la SEULE icône qui n'est PAS une Iconsax, et pourquoi

`theme.png` est le bouton « basculer le thème clair/sombre de Windows » (voir
`win32::theme::toggle_dark_mode`). **Ce n'est pas une rastérisation : le glyphe
est DESSINÉ**, par `C:\ndtarget\theme-icon.py` (hors dépôt, comme les scripts
qui ont produit les autres PNG — mais il n'y avait ici aucun SVG à traiter).

Raison : **aucun rastériseur SVG n'était disponible** au moment de l'ajouter —
ni Inkscape, ni ImageMagick, ni `cairosvg`, et `resvg` n'est pas une dépendance
du projet. Plutôt que d'installer un outil pour une seule icône, le disque coupé
a été tracé avec Pillow.

**Ce n'est pas une dégradation.** Seul le **canal alpha** est échantillonné (voir
le paragraphe ci-dessus) : un glyphe dessiné est fonctionnellement identique à un
glyphe rastérisé. Les métriques, elles, ont été **mesurées** sur les neuf autres
et non choisies — canevas 128×128, trait de 8 px, cadre utile de 116×116
(marges de 6 px) — pour que l'icône se lise dans le même jeu.

Forme : un cercle au trait coupé par un diamètre vertical, une moitié pleine
(le glyphe « contraste » universel). Choisi par l'utilisateur entre trois
propositions ; il dit **basculer** plutôt qu'un thème précis, ce qui correspond
au geste — et c'est aussi pourquoi l'icône ne change pas d'apparence selon le
thème courant.


## `night_light.png` — l'éclairage nocturne (dessiné, comme `theme.png`)

DEMANDE EXPLICITE : « une applet pour commuter le mode nuit de Windows » — l'éclairage
nocturne (le filtre bleu), pas le thème sombre, qui a déjà sa bascule.

**Dessiné, et pas rastérisé** : aucun glyphe Iconsax n'évoque l'éclairage nocturne, et aucun
rastériseur SVG n'est disponible sur cette machine (même situation que `theme.png`, dont ce
fichier porte déjà la justification). Seul le **canal alpha** est échantillonné au rendu : un
glyphe dessiné est fonctionnellement identique à un glyphe rastérisé.

Forme : **un croissant**, obtenu en retirant d'un disque plein un second disque de même taille
décalé vers le haut-droite (compositing `SourceCopy` avec un pinceau transparent, donc une
EFFACURE et non un mélange). Il dit l'éclairage nocturne sans ambiguïté, et il est
volontairement **distinct de `theme.png`** — un cercle au trait coupé — pour que les deux
bascules système voisines ne se confondent pas.

Métriques **mesurées sur les autres**, pas choisies : canevas 128×128, cadre utile de 116×116
(marges de 6 px). Le bord CONVEXE du croissant touche la marge gauche, et ses pointes les
marges haute et basse — c'est ce qui lui donne la même présence que ses voisines, un croissant
étant par nature plus étroit qu'un disque.


## `calendar.png` — dessinée, et son dessin est MESURÉ

DEMANDE EXPLICITE (2026-09-23) : « une applet calendrier dans la zone Intégrés de la barre
de droite qui affichent les fêtes du jour et les dates d'anniversaires ».

**Dessinée, pas rastérisée** — même situation que `theme.png`, `night_light.png` et
`printer.png`. Générateur : la sonde `dessine_les_glyphes_du_calendrier`
(`src/gfx/texture.rs`), qui écrit le fichier.

- `calendar.png` — deux anneaux, une bande d'en-tête pleine, et une grille de 3 × 3 TROUS
  pour les jours. Les trous sont ce qui fait lire « calendrier » plutôt que « bloc ».

⚠️ **LA SONDE IMPRIME LE GLYPHE EN ASCII.** Un dessin qu'on ne peut pas regarder ne se
vérifie pas dans un PNG — et « ça devrait ressembler à un calendrier » n'est pas une mesure.
L'ASCII, lui, se lit dans la sortie du test.

Métriques relevées sur les voisines, comme pour les trois autres : canevas 128×128, cadre
utile de 116×116.

## `bell.png` — RASTÉRISÉE depuis Keyline Icons (MIT), et pourquoi elle a remplacé un dessin

**DEMANDE EXPLICITE (2026-09-23)** : « une icône de cloche clignotante dans un cycle de
couleurs placée à droite de l'horloge dans les barres du haut ». Puis, le même jour :
« peux-tu utiliser une icône de ce site https://keylineicons.com/ ? ».

### La provenance, exacte

- Jeu : **Keyline Icons** (<https://keylineicons.com/>), **MIT** — « free under the MIT
  License, in commercial work as much as personal ». Le paquet npm `@keyline-icons/react`
  déclare `license: MIT` et pointe sur <https://github.com/keyline-icons/keyline-icons>.
- Icône **`bell`**, style **`stroke`** — celui dont les trois autres (two-tone, duotone,
  fill) dérivent. 24×24, `stroke-width 2`, `fill: none`.
- SVG récupéré par l'API Iconify : `https://api.iconify.design/keyline-icons/bell.svg`.
  **Conservé OCTETS POUR OCTETS** dans `svg/keyline-icons/bell.svg`, avec sa licence dans
  `svg/keyline-icons/LICENSE` — un dossier à part, et non la racine de `svg/`, parce que
  celle-ci porte la licence des Iconsax (Erfan Khadivar), qui ne couvre pas ce fichier.
- Rastérisation : `resvg --width 128 --height 128 svg/keyline-icons/bell.svg bell.png`
  (resvg 0.48.1, `~/.cargo/bin/resvg`).

⚠️ **`SOURCE.md` A AFFIRMÉ PENDANT DES SEMAINES QU'AUCUN RASTÉRISEUR SVG N'ÉTAIT
DISPONIBLE SUR CETTE MACHINE** — c'était vrai quand `theme.png`, `night_light.png`,
`calendar.png` et `printer.png` ont été dessinées, et ça ne l'est plus : **`resvg` est
installé**, et c'est l'outil qui avait produit les neuf Iconsax. Ces quatre-là restent
dessinées ; leur reprise depuis un jeu SVG est désormais possible, mais ce n'est pas
demandé.

### Pourquoi elle remplace la cloche dessinée — trois mesures

1. **Creux intérieur** (fraction de pixels vides À L'INTÉRIEUR de la boîte d'encre : un
   contour est creux, une silhouette est pleine) :

   | | creux |
   |---|---|
   | Iconsax (network, settings, clipboard, monitor, sound, terminal, notes ; power avant 2026-09-25) | 66–72 % |
   | **cloche Keyline** | **65 %** |
   | cloche dessinée (retirée) | 58 % |
   | calendar · printer (dessinées) | 22–24 % |

   Les neuf Iconsax du dépôt sont des **contours**. La cloche Keyline est un contour ;
   la dessinée était entre les deux, et les deux autres glyphes dessinés sont des
   silhouettes pleines.

2. **Étendue** : encre 86×108 px sur 128 à 1:1, contre 92×108 pour la dessinée — **hauteur
   identique**, largeur un peu moindre. Dans la bande des voisines (93×117, 104×112).
3. **L'ASCII du canal alpha** (la méthode de `calendar.png`, appliquée ici) montre une
   vraie cloche : dôme, deux flancs, rebord horizontal, battant DÉTACHÉ — là où la dessinée
   était un cône plein.

### ⛔ LE PIÈGE QUE CE REMPLACEMENT A FERMÉ

`bell.png` était écrite par la sonde `#[ignore]` `dessine_les_glyphes_du_calendrier`.
**La laisser dans sa boucle d'écriture aurait écrasé la cloche rastérisée par l'ancienne
dessinée**, en silence, au prochain `cargo test -- --ignored`. La cloche a donc été retirée
de cette sonde (fonction `cloche` supprimée, avec elle le helper `disque` devenu sans
usage), et sa doc porte le pourquoi.


## `image_editor.png` — dessinée

L'applet de l'éditeur d'image (`win32::cef_image_editor`). Même méthode que
`printer.png` et `calendar.png` : formes pleines, suréchantillonnage 4 × 4, détails en
TROUS. Un cadre au trait de 10 px, un soleil en haut à droite et deux montagnes posées
sur le bas du cadre — la vignette « image » habituelle. Encre 116 × 96 px sur 128.

Générateur : la sonde `#[ignore]` `dessine_le_glyphe_de_l_editeur_d_image`
(`src/gfx/texture.rs`), qui imprime aussi l'ASCII du canal alpha.

## `power.png` — le symbole d'alimentation STANDARD, depuis Keyline Icons (MIT)

**DEMANDE EXPLICITE (2026-09-25)** : « une icône plus standard pour l'applet
alimentation ». L'ancienne était l'Iconsax `Essetional/flash-circle` — un
éclair dans un cercle, faute de bouton d'alimentation dans ce jeu. La nouvelle
est le symbole CEI 60417-5009 (cercle ouvert en haut, trait vertical) que
Windows, macOS et tous les appareils emploient.

- Icône **`power`**, style **`stroke`**, du même jeu que `bell.png` : Keyline
  Icons (<https://keylineicons.com/>), **MIT** — licence dans
  `svg/keyline-icons/LICENSE`. 24×24, `stroke-width 2`.
- SVG récupéré par l'API Iconify (`https://api.iconify.design/keyline-icons/power.svg`),
  conservé octets pour octets dans `svg/keyline-icons/power.svg`.
- Rastérisation : `resvg --width 128 --height 128 svg/keyline-icons/power.svg power.png`.
  Encre 118×118 (marges de 5 px), trait de ~11 px — celui de la cloche, un
  peu plus épais que les Iconsax (~9 px).
- L'ancien `svg/power.svg` (Iconsax) a été retiré : plus rien n'en dérive.

## `screen_capture.png` — les quatre coins d'une sélection, depuis Keyline Icons (MIT)

**DEMANDE EXPLICITE (2026-09-25)** : « une applet de capture d'écran comme le
fait flameshot mais juste la partie capture » (voir `src/win32/capture_ecran.rs`).
Keyline n'a ni `screenshot` ni `crop` ; `scan` — quatre coins arrondis qui
cadrent une zone — est le pictogramme habituel d'une capture de région.

- Icône **`scan`**, style **`stroke`**, même jeu et même licence que `bell.png`
  et `power.png` (`svg/keyline-icons/LICENSE`). 24×24, `stroke-width 2`.
- SVG récupéré par l'API Iconify (`https://api.iconify.design/keyline-icons/scan.svg`),
  conservé octets pour octets dans `svg/keyline-icons/scan.svg`.
- Rastérisation : `resvg --width 128 --height 128 svg/keyline-icons/scan.svg screen_capture.png`.

## `enregistrer.png` — une caméra vidéo, dans les quatre thèmes

DEMANDE EXPLICITE (2026-11-01) : « ne peux-tu pas mettre l'icône d'une caméra ? » pour
l'applet Enregistrer (voir `src/win32/enregistreur.rs`). Une caméra VIDÉO, pas un
appareil photo : la capture d'écran montre déjà 📷 dans Fluent Emoji et Energy.

- **Racine** : Keyline Icons **`video`** (MIT, `svg/keyline-icons/LICENSE`), SVG récupéré
  par l'API Iconify (`https://api.iconify.design/keyline-icons/video.svg`), conservé dans
  `svg/keyline-icons/video.svg`. Rastérisation :
  `resvg --width 128 --height 128 svg/keyline-icons/video.svg enregistrer.png`.
- **`fluent_couleur/`, `fluent_3d/`** : l'emoji 📹 « Video camera », entrée de
  `tools/rasteriser_fluent_emoji.py`.
- **`energy/`** : l'icône `video`, entrée de `tools/rasteriser_energy.py`.
- **Segoe Fluent** : le glyphe `E714` (« Video »), lu sur le système.

## `minuteur.png` — un chronomètre, depuis Keyline Icons (MIT)

**DEMANDE EXPLICITE (2026-09-25)** : « une applet qui permette de mettre des
comptes à rebours et des alarmes » (voir `src/win32/minuteur.rs`). Keyline n'a
ni `stopwatch` ni `alarm` (404 sur l'API) ; `timer` — un cadran, son bouton et
son aiguille — dit « compte à rebours » mieux que `alarm-clock`.

- Icône **`timer`**, style **`stroke`**, même jeu et même licence que les
  précédentes (`svg/keyline-icons/LICENSE`). 24×24, `stroke-width 2`.
- SVG récupéré par l'API Iconify (`https://api.iconify.design/keyline-icons/timer.svg`),
  conservé octets pour octets dans `svg/keyline-icons/timer.svg`.
- Rastérisation : `resvg --width 128 --height 128 svg/keyline-icons/timer.svg minuteur.png`.

## `surveillance.png` — un serveur, depuis Keyline Icons (MIT)

**DEMANDE EXPLICITE (2026-09-26)** : « une applet pour un dash de surveillance
de serveurs par ping, permettant de suivre l'évolution d'adresses ip ou
d'adresses web avec un historique » (voir `src/win32/surveillance.rs`). Keyline
n'a ni `servers`, ni `heartbeat`, ni `pulse` (404 sur l'API) ; `server` — deux
baies empilées et leurs voyants — dit « serveur » sans ambiguïté.

- Icône **`server`**, style **`stroke`**, même jeu et même licence que les
  précédentes (`svg/keyline-icons/LICENSE`). 24×24, `stroke-width 2`.
- SVG récupéré par l'API Iconify (`https://api.iconify.design/keyline-icons/server.svg`),
  conservé octets pour octets dans `svg/keyline-icons/server.svg`.
- Rastérisation : `resvg --width 128 --height 128 svg/keyline-icons/server.svg surveillance.png`.

## `windows_update.png` — deux flèches en cercle, depuis Keyline Icons (MIT)

**DEMANDE EXPLICITE (2026-09-26)** : « dans la zone réglages ajouter une applet
pour accéder à windows update dans les paramètres de windows » (voir
`src/win32/windows_update.rs`). Keyline n'a ni `refresh`, ni `sync`, ni `update`
(404 sur l'API) ; `refresh-cw` — deux flèches qui tournent — est le symbole
même de Windows Update.

- Icône **`refresh-cw`**, style **`stroke`**, même jeu et même licence que les
  précédentes (`svg/keyline-icons/LICENSE`). 24×24, `stroke-width 2`.
- SVG récupéré par l'API Iconify (`https://api.iconify.design/keyline-icons/refresh-cw.svg`),
  conservé octets pour octets dans `svg/keyline-icons/refresh-cw.svg`.
- Rastérisation : `resvg --width 128 --height 128 svg/keyline-icons/refresh-cw.svg windows_update.png`.

## `gestionnaire_peripheriques.png` — une puce, depuis Keyline Icons (MIT)

**DEMANDE EXPLICITE (2026-09-26)** : « une nouvelle applet dans la zone réglages
pour ouvrir le gestionnaire de périphériques de windows » (voir
`src/win32/gestionnaire_peripheriques.rs`). La puce (`cpu`) est le symbole le
plus direct du matériel ; `hard-drive`, `usb` ou `monitor` n'en désignent qu'une
sorte.

- Icône **`cpu`**, style **`stroke`**, même jeu et même licence que les
  précédentes (`svg/keyline-icons/LICENSE`). 24×24, `stroke-width 2`.
- SVG récupéré par l'API Iconify (`https://api.iconify.design/keyline-icons/cpu.svg`),
  conservé octets pour octets dans `svg/keyline-icons/cpu.svg`.
- Rastérisation : `resvg --width 128 --height 128 svg/keyline-icons/cpu.svg gestionnaire_peripheriques.png`.

## `reglages_ecran.png` — un moniteur, depuis Keyline Icons (MIT)

**DEMANDE EXPLICITE (2026-09-26)** : « une applet dans la zone réglages pour
ouvrir les réglages écran dans les paramètres windows » (voir
`src/win32/reglages_ecran.rs`).

- Icône **`monitor`**, style **`stroke`**, même jeu et même licence que les
  précédentes (`svg/keyline-icons/LICENSE`). 24×24, `stroke-width 2`.
- SVG récupéré par l'API Iconify (`https://api.iconify.design/keyline-icons/monitor.svg`),
  conservé octets pour octets dans `svg/keyline-icons/monitor.svg`.
- Rastérisation : `resvg --width 128 --height 128 svg/keyline-icons/monitor.svg reglages_ecran.png`.

## `fonctionnalites_facultatives.png` — une grille avec un plus, depuis Keyline Icons (MIT)

**DEMANDE EXPLICITE (2026-10-03)** : « une applet dans la zone réglages de la
barre de droite pour ouvrir : Fonctionnalités facultatives Windows » (voir
`src/win32/fonctionnalites_facultatives.rs`).

- Icône **`grid-2x2-plus`**, style **`stroke`**, même jeu et même licence que les
  précédentes (`svg/keyline-icons/LICENSE`). 24×24, `stroke-width 2`. La grille
  « + » est l'image même de la page des Paramètres : des fonctionnalités qu'on
  AJOUTE.
- SVG récupéré par l'API Iconify (`https://api.iconify.design/keyline-icons/grid-2x2-plus.svg`),
  conservé octets pour octets dans `svg/keyline-icons/grid-2x2-plus.svg`.
- Rastérisation : `resvg --width 128 --height 128 svg/keyline-icons/grid-2x2-plus.svg fonctionnalites_facultatives.png`.

## `notifications.png` — une bulle avec un point, depuis Keyline Icons (MIT)

**DEMANDE EXPLICITE (2026-10-04)** : « une applet pour afficher les
notifications » (voir `src/win32/centre_notifications.rs`).

- Icône **`message-square-dot`**, style **`stroke`**, même jeu et même licence
  que les précédentes (`svg/keyline-icons/LICENSE`). 24×24, `stroke-width 2`.
  PAS une cloche : `bell.png` est déjà la CLOCHE des rappels de la barre du haut,
  et deux cloches se confondraient.
- SVG récupéré par l'API Iconify (`https://api.iconify.design/keyline-icons/message-square-dot.svg`),
  conservé octets pour octets dans `svg/keyline-icons/message-square-dot.svg`.
- Rastérisation : `resvg --width 128 --height 128 svg/keyline-icons/message-square-dot.svg notifications.png`.

## `recherche.png` — un fichier et une loupe, depuis Keyline Icons (MIT)

**DEMANDE EXPLICITE (2026-10-04)** : « une applet de recherche qui n'apparaît que
si Everything est installé, dans la zone Intégrés » (voir
`src/win32/cef_recherche.rs`).

- Icône **`file-search`**, style **`stroke`**, même jeu et même licence que les
  précédentes (`svg/keyline-icons/LICENSE`). 24×24, `stroke-width 2`. Le FICHIER
  sous la loupe dit ce qu'on cherche — une loupe seule se lirait comme la
  recherche du Dash.
- SVG récupéré par l'API Iconify (`https://api.iconify.design/keyline-icons/file-search.svg`),
  conservé octets pour octets dans `svg/keyline-icons/file-search.svg`.
- Rastérisation : `resvg --width 128 --height 128 svg/keyline-icons/file-search.svg recherche.png`.

## `automatisations.png` — un chemin à embranchement, depuis Keyline Icons (MIT)

**DEMANDE EXPLICITE (2026-10-04)** : « des automatisations simples du type
*quand… alors…* » (voir `src/win32/cef_automatisations.rs`).

- Icône **`route`**, style **`stroke`**, même jeu et même licence que les
  précédentes (`svg/keyline-icons/LICENSE`). 24×24, `stroke-width 2`. Un chemin
  qui part d'un point et repart ailleurs dit « d'ici vers là », c'est-à-dire
  *quand* → *alors*.
- ⚠️ **LE JEU KEYLINE N'A AUCUN ÉCLAIR** : `bolt`, `zap` et `lightning` y sont
  absents (vérifié sur la collection entière, 2026-10-04) — il n'existe qu'une
  famille `X-zap` (cloche, horloge, dossier…), aucune forme nue. L'éclair n'est
  donc que le glyphe FLUENT de secours, et les deux sources disent la même idée
  par deux images différentes.
- SVG récupéré par l'API Iconify (`https://api.iconify.design/keyline-icons/route.svg`),
  conservé octets pour octets dans `svg/keyline-icons/route.svg`.
- Rastérisation : `resvg --width 128 --height 128 svg/keyline-icons/route.svg automatisations.png`.
- Encre mesurée : boîte 108×108 sur 128, 18,3 % de la surface — moins dense que
  `notifications.png` (25,9 %) parce que le trait est fin, mais plus que
  `couleurs.png` (10,5 %, 81×81), déjà en service. La BOÎTE est pleine, ce qui
  est ce qui décide de la taille apparente dans la barre.

## `calculatrice.png` — une calculatrice, depuis Keyline Icons (MIT)

**DEMANDE EXPLICITE (2026-09-26)** : « une applet de calculatrice proposant les
fonctions principales avec un historique » (voir `src/win32/calculatrice.rs`).

- Icône **`calculator`**, style **`stroke`**, même jeu et même licence que les
  précédentes (`svg/keyline-icons/LICENSE`). 24×24, `stroke-width 2`.
- SVG récupéré par l'API Iconify (`https://api.iconify.design/keyline-icons/calculator.svg`),
  conservé octets pour octets dans `svg/keyline-icons/calculator.svg`.
- Rastérisation : `resvg --width 128 --height 128 svg/keyline-icons/calculator.svg calculatrice.png`.

## `ssh.png` — une clé, depuis Keyline Icons (MIT)

**DEMANDE EXPLICITE (2026-09-26)** : « une applet de gestion de serveurs par
ssh et sftp avec un système d'onglets » (voir `src/win32/cef_ssh.rs`).

- Icône **`key-round`**, style **`stroke`**, même jeu et même licence que les
  précédentes (`svg/keyline-icons/LICENSE`). 24×24, `stroke-width 2`.
- ⚠️ **PAS `server`** : c'est déjà l'icône de la surveillance (le PNG sortait
  identique à l'octet près), et `square-terminal` se confondait avec l'applet
  terminal. La clé dit « SSH » sans ressembler à aucune voisine.
- SVG récupéré par l'API Iconify (`https://api.iconify.design/keyline-icons/key-round.svg`),
  conservé octets pour octets dans `svg/keyline-icons/key-round.svg`.
- Rastérisation : `resvg --width 128 --height 128 svg/keyline-icons/key-round.svg ssh.png`.

## `couleurs.png` — une pipette, DESSINÉE

**DEMANDE EXPLICITE (2026-09-27)** : « sur l'applet color picker dans la zone
Intégrés de la barre de droite » (voir `src/win32/cef_couleurs.rs`).

Dessinée, comme `printer.png` / `calendar.png` / `image_editor.png` : formes
pleines, suréchantillonnage 4 × 4. Le générateur est la sonde
`dessine_le_glyphe_de_la_pipette` (`src/gfx/texture.rs`), et c'est **son ASCII
qui a servi à la corriger deux fois** — un dessin qu'on ne regarde pas ne se
vérifie pas :

1. un corps de demi-largeur 7 contre un bulbe de rayon 15 lisait comme une
   **hache** — un corps fin (5) et un bulbe large (rayon 16,5) sont ce qui fait
   lire une pipette ;
2. des transitions de largeur en **marches** laissaient un **trou** dans la
   silhouette (`###### ##`) et une déchirure de 4,7 px au raccord du bulbe, parce
   que le long d'une ligne horizontale `s` et `w` varient tous les deux : les
   transitions sont désormais **linéaires**, et l'évasement va jusque DANS le
   cercle.

Silhouette : pointe conique nulle à l'extrémité, corps diagonal, évasement
continu, bulbe circulaire. Encre ≈ 80 × 31 px sur 128 (elle est très oblique —
c'est ce qui la distingue de `screen_capture.png` et de `image_editor.png`,
posées dans la même zone du dock).

## `meteo.png` — un soleil derrière un nuage, depuis Keyline Icons (MIT)

**DEMANDE EXPLICITE (2026-10-04)** : « une nouvelle applet dans la barre de droite dans la
zone Intégrés, pour afficher la météo […] dans un dash ».

- Icône **`cloud-sun`**, style **`stroke`**, du même jeu que `bell.png` et `power.png` :
  Keyline Icons (<https://keylineicons.com/>), **MIT** — licence dans
  `svg/keyline-icons/LICENSE`. 24×24, `stroke-width 2`, `fill: none`.
- SVG récupéré par l'API Iconify (`https://api.iconify.design/keyline-icons/cloud-sun.svg`),
  conservé **octets pour octets** (587 octets) dans `svg/keyline-icons/cloud-sun.svg`.
- Rastérisation : `resvg --width 128 --height 128 svg/keyline-icons/cloud-sun.svg meteo.png`.

**Vérifiée comme les autres** — creux intérieur **73,1 %**, dans la bande des contours du
dépôt (Iconsax 66–72 %, cloche Keyline 65 %) : c'est bien un CONTOUR, pas une silhouette.
Boîte d'encre 118 × 118 sur 128.

### ⚠️ Le jeu Keyline n'a PAS de vocabulaire pour trois familles de temps

Constaté en listant les 9 864 noms du jeu : **aucun** glyphe pour le brouillard, la bruine
et l'orage (`cloud-fog`, `cloud-drizzle`, `cloud-lightning` rendent 404 ; « fog », « storm »,
« thunder », « bolt » ne rendent rien). Ce qui existe : `sun-medium`, `sun`, `sun-dim`,
`moon`, `cloud-sun`, `cloud-moon`, `cloud`, `clouds`, `cloud-rain`, `snowflake`, `wind`.

La page du dash en tire ce qu'elle peut, et **c'est le LIBELLÉ qui porte la précision** —
brouillard → `clouds`, bruine → `cloud-rain`, orage → `cloud-alert` (un nuage avec un point
d'exclamation : la seule marque de sévérité du jeu). Les tracés sont **inlinés** dans
`assets/popups/meteo.html` plutôt que chargés en fichiers : le panneau n'a ainsi aucune
requête à faire, et les icônes suivent la teinte de la page (`stroke="currentColor"`).

## `aide.png` — une bulle portant un « ? », depuis Keyline Icons (MIT)

**DEMANDE EXPLICITE (2026-10-07)** : « une applet d'aide dans la zone Intégrés qui affiche
un dash avec les explications sur l'utilisation de l'app ».

- **Icône `message-question`**, style **`stroke`** — une BULLE avec un « ? », et non un
  point d'interrogation nu : c'est ce qui dit « les explications » plutôt que « une
  question ».
- **SVG récupéré par l'API Iconify** : `https://api.iconify.design/keyline-icons/message-question.svg`,
  conservé dans `svg/keyline-icons/message-question.svg` — le dossier qui porte la licence
  Keyline, à part de `svg/` (celle des Iconsax ne couvre pas ce fichier).
- **Rastérisation** : `resvg --width 128 --height 128 svg/keyline-icons/message-question.svg aide.png`
  (resvg 0.48.1, `~/.cargo/bin/resvg`) — la recette exacte de `bell.png`.

⚠️ **`help`, `question-mark` ET `info-circle` N'EXISTENT PAS dans ce jeu** — l'API a rendu
**404** pour les trois (9 octets de réponse). `message-question` est le seul nom qui
réponde, et c'est aussi le plus juste.

⚠️ **La variante COULEUR de `aide.png` est une AUTRE famille** — l'emoji ❓,
rendu par les thèmes **Fluent Emoji** (`fluent_couleur/`, `fluent_3d/`), qui remplacent
le thème « Symboles » retiré ci-dessous.

## `proprietes_systeme.png` — trois curseurs, depuis Keyline Icons (MIT)

**DEMANDE EXPLICITE (2026-10-08)** : « j'aimerais une nouvelle applet dans Intégrés pour
ouvrir sysdm.cpl ».

- **Icône `sliders-horizontal`**, style **`stroke`** — trois curseurs, chacun à une position
  différente. C'est le vocabulaire des PROPRIÉTÉS (des réglages qu'on ajuste), et il est
  **délibérément distinct de l'engrenage de `settings.png`** : deux icônes voisines dans la
  même barre ne doivent pas se lire pareil.
- **SVG récupéré par l'API Iconify** : `https://api.iconify.design/keyline-icons/sliders-horizontal.svg`,
  conservé dans `svg/keyline-icons/sliders-horizontal.svg` — le dossier qui porte la licence
  Keyline, à part de `svg/` (celle des Iconsax ne couvre pas ce fichier).
- **Rastérisation** : `resvg --width 128 --height 128 svg/keyline-icons/sliders-horizontal.svg proprietes_systeme.png`
  (resvg 0.48.1, `~/.cargo/bin/resvg`) — la recette exacte de `bell.png`.
- **Encre mesurée : 108 px**, et ce n'est PAS un défaut — `bell.png`, `notifications.png`,
  `screen_capture.png`, `windows_update.png` et `automatisations.png` sont à **108 px**
  pour la même raison (un dessin large et plat dans un carré de 128). Les 118 px des icônes
  verticales ne sont pas une cible à atteindre.

⚠️ **La variante COULEUR de `proprietes_systeme.png` est une AUTRE famille** — l'emoji 🎛,
rendu par les thèmes **Fluent Emoji** (`fluent_couleur/`, `fluent_3d/`), qui remplacent
le thème « Symboles » retiré ci-dessous.

## `shaders.png` — des chevrons de code, depuis Keyline Icons (MIT)

**DEMANDE EXPLICITE (2026-10-10)** : « une applet pour disposer d'un éditeur de shader qui
permette de les utiliser dans les dégradés animés de l'app » (voir `src/win32/cef_shaders.rs`).

- **Icône `code`**, style **`stroke`** — les chevrons `< >` : l'applet est un ÉDITEUR DE
  CODE. Même jeu et même licence que les précédentes (`svg/keyline-icons/LICENSE`). 24×24,
  `stroke-width 2`. PAS un clavier : c'est le symbole du Terminal.
- SVG récupéré par l'API Iconify (`https://api.iconify.design/keyline-icons/code.svg`),
  conservé octets pour octets dans `svg/keyline-icons/code.svg`.
- Rastérisation : `resvg --width 128 --height 128 svg/keyline-icons/code.svg shaders.png`.

⚠️ **La variante COULEUR de `shaders.png` est une AUTRE famille** — l'emoji 🌈,
rendu par les thèmes **Fluent Emoji** (`fluent_couleur/`, `fluent_3d/`), qui remplacent
le thème « Symboles » retiré ci-dessous.

## `phonetique.png` — un schwa `ə`, DESSINÉ

**DEMANDE EXPLICITE (2026-10-11)** : « une applet qui ouvre une fenêtre pour les symboles
de phonétique en deux onglets » (voir `src/win32/cef_phonetique.rs`).

- Le glyphe `ə` (U+0259) de **Segoe UI Gras**, rendu par `tools/rasteriser_phonetique.py`
  (même recette que `kaomoji.png`) : encre ramenée à 116 px dans 128. Le schwa est LE
  signe de l'alphabet phonétique, et il n'existe dans aucune écriture courante.

⚠️ **La variante COULEUR de `phonetique.png` est une AUTRE famille** — l'emoji 👄,
rendu par les thèmes **Fluent Emoji** (`fluent_couleur/`, `fluent_3d/`), qui remplacent
le thème « Symboles » retiré ci-dessous.

## `maths.png` — une somme `∑`, DESSINÉE

**DEMANDE EXPLICITE (2026-10-12)** : « une nouvelle applet dans la zone intégrés pour
ouvrir une fenêtre permettant d'insérer les caractères mathématiques » (voir
`src/win32/cef_maths.rs`).

- Le glyphe `∑` (U+2211) de **Segoe UI Gras**, rendu par `tools/rasteriser_maths.py`
  (même recette que `phonetique.png`) : encre ramenée à 116 px dans 128. La somme ne se
  confond avec aucune lettre, et la calculatrice a déjà les opérateurs du clavier.

⚠️ **La variante COULEUR de `maths.png` est une AUTRE famille** — l'emoji 📐,
rendu par les thèmes **Fluent Emoji** (`fluent_couleur/`, `fluent_3d/`), qui remplacent
le thème « Symboles » retiré ci-dessous.

## ⛔ `symboles/` — RETIRÉ LE 2026-10-28, ET POURQUOI

Ce dossier portait **41 PNG rendus depuis `seguiemj.ttf`** (« Segoe UI Emoji »), la police
emoji livrée avec Windows, plus deux dessins de ce dépôt. Le thème d'icônes « Symboles »
n'existe plus, et **le dossier non plus**.

**La raison n'est pas esthétique.** Le dépôt devient public sous GPLv3 : y publier des
images de glyphes d'une police Microsoft revient à redistribuer ce que la licence de la
police ne cède pas. Contrairement à Segoe Fluent Icons — qu'on LIT sur le système au
runtime, sans jamais la copier — ces PNG étaient des **copies** embarquées dans l'exe par
`include_bytes!`, donc distribuées avec le programme.

Ce qui a changé :

- les deux thèmes **Fluent Emoji** (MIT, © Microsoft) restent les thèmes en couleur, et
  couvrent les mêmes 43 applets : `fluent_couleur/` et `fluent_3d/` ;
- les **deux dessins** qui vivaient ici ont été déplacés dans **`dessins/`** —
  `scanner.png` (aucun emoji « scanner » dans Unicode) et `alimentation.png` (le bouton
  power, voir plus bas). Ils sont de ce dépôt, sans licence tierce, et les deux thèmes
  Fluent en dérivent ;
- `tools/rasteriser_symboles.py` a été supprimé : il n'y a plus rien à régénérer depuis
  une police système. L'historique git garde la recette si elle redevenait légitime.

⚠️ **CE QUI AVAIT ÉTÉ DOCUMENTÉ ICI RESTE VRAI ET UTILE**, et ne doit pas être réappris à
la dure : ces PNG portaient leur **propre couleur** (`TexMode::Texture`, jamais teintés),
ils étaient les seuls du dépôt dans ce cas, et le compte « 37 » annoncé ici était **en
retard de six** sur les 43 fichiers réellement présents (loupe, moniteur, narrateur et
textures n'avaient jamais été ajoutés à la liste). Même leçon que partout ailleurs : un
compte tenu à la main ne se compare à rien.

## `alimentation.png` des deux thèmes en couleur — un bouton power DESSINÉ
DEMANDE EXPLICITE (2026-10-15) : « l'icône de l'applet alimentation devrait être un bouton
power dans tous les styles d'icônes ». Les thèmes NeuraDesk (`power.png`) et Fluent (le
glyphe `E7E8`, *PowerButton*) l'étaient déjà ; `fluent_couleur/` et `fluent_3d/`
montraient la PRISE 🔌.

**Aucun emoji ne convient** : `⏻ U+23FB` rend un rectangle vide (voir ci-dessus), et Fluent
Emoji n'a pas de bouton d'alimentation. `tools/dessiner_bouton_power.py` dessine donc le
symbole de `svg/keyline-icons/power.svg` (Keyline Icons, MIT — le même que `power.png`) en
blanc sur un disque rouge, dans le rendu de chaque jeu : aplats pour `fluent_couleur/`,
disque modelé avec reflet pour `fluent_3d/`. Même gabarit (128 px, encre 116).

⚠️ `rasteriser_fluent_emoji.py` écrit encore la prise, puis **appelle ce script en fin
de course** : le régénérer ne la remet pas.

## Thèmes « Fluent Emoji » et « Fluent Emoji 3D » (`fluent_couleur/`, `fluent_3d/`)

DEMANDE EXPLICITE (2026-10-11). Les emoji **Fluent Emoji** de Microsoft —
https://github.com/microsoft/fluentui-emoji — sous **licence MIT**, Copyright (c)
Microsoft Corporation. Le texte de la licence est dans `LICENSE-fluentui-emoji.txt`, et
**doit accompagner toute copie distribuée** : c'est la seule obligation de la MIT.

- `fluent_couleur/` vient des SVG « Color », rendus par `resvg` ;
- `fluent_3d/` vient des PNG « 3D » (256 px), réduits.

Produits par `tools/rasteriser_fluent_emoji.py`, qui porte la table des 37 emoji choisis.
Les mêmes idées que `energy/`, sauf la météo (🌥 le soleil derrière un nuage) et le thème
(🌓 le premier quartier de lune : ◐ n'existe pas dans Fluent Emoji). **Le scanner est un
DESSIN de ce dépôt** (`dessins/scanner.png`) : Unicode n'a pas d'emoji « scanner », et le
fax a été refusé.

## Thème « Energy Icons » (`energy/`)

DEMANDE EXPLICITE (2026-10-24) : « faire un nouveau style d'icônes en utilisant les icônes
de https://energyicons.com ». Les icônes **Energy Icons** de Sam Passmore —
https://github.com/Sam-r-passmore/energy-icons — sous **licence MIT**, Copyright (c) 2026
Sam Passmore. Le texte de la licence est dans `LICENSE-energy-icons.txt`, et **doit
accompagner toute copie distribuée**.

- graisse **GRASSE** (`48-bold.svg`), choisie par l'utilisateur sur une planche des deux ;
- **MONOCHROMES** : encre blanche, forme dans l'alpha — la barre les teinte, comme les PNG
  de la racine (et non tels quels comme `fluent_couleur/`) ;
- même gabarit : 128 px, encre 116, centrée.

Produits par `tools/rasteriser_energy.py` (seul `resvg` est requis), qui porte la table des
43 correspondances validées par l'utilisateur sur une planche de contact.

## Les icônes des jeux 2048 et « Cinq ou plus » (icônes GNOME d'origine)

DEMANDE (2026-11-07) : « ne peux-tu pas reprendre les icônes d'origine quand c'est
possible ? » — pour les applets `jeu2048` et `cinq_ou_plus` (des forks web des jeux
GNOME, voir `win32::cef_jeu2048` / `cef_cinq_ou_plus`). Les icônes sont reprises des
projets GNOME d'origine :

- **2048** — https://gitlab.gnome.org/GNOME/gnome-2048 (branche `master`), projet sous
  **GPL-3.0-or-later** : `data/icons/org.gnome.TwentyFortyEight-symbolic.svg` (symbolique
  monochrome) et `data/icons/org.gnome.TwentyFortyEight.svg` (couleur).
- **Cinq ou plus / Five or More** — https://gitlab.gnome.org/GNOME/five-or-more (branche
  `main`), projet sous **GPL-2.0-or-later** :
  `data/icons/hicolor/symbolic/org.gnome.five-or-more-symbolic.svg` (symbolique) et
  `data/icons/hicolor/512x512/org.gnome.five-or-more.png` (couleur).

Le **symbolique** alimente les thèmes monochromes (racine NeuraDesk et `energy/`), l'icône
**couleur** alimente `fluent_couleur/` et `fluent_3d/`. Mise au gabarit (128 px, encre 116
centrée) ; le symbolique est repeint en blanc (forme dans l'alpha), la barre le teinte.
Le glyphe de police Segoe Fluent reste un pictogramme générique (aucun équivalent). Les
licences des projets amont s'appliquent à ces fichiers.

### Lights Off (ajouté le 2026-11-08)

DEMANDE (2026-11-08) : « une applet dans Intégrés pour le jeu Lights Off de gnome »
(`win32::cef_lightsoff`). Même démarche que ci-dessus :

- **Lights Off** — https://gitlab.gnome.org/GNOME/lightsoff (branche `main`), projet sous
  **GPL-2.0-or-later** : `data/icons/hicolor/symbolic/apps/org.gnome.LightsOff-symbolic.svg`
  (symbolique → `lightsoff.png` et `energy/`) et
  `data/icons/hicolor/scalable/apps/org.gnome.LightsOff.svg` (couleur → `fluent_couleur/`
  et `fluent_3d/`). Rastérisés par `resvg` à 1024 px puis mis au gabarit (128 px, encre 116
  centrée, symbolique repeint en blanc). Glyphe Segoe Fluent : `E7FA` (générique, distinct).

### Reversi (ajouté le 2026-11-10)

DEMANDE (2026-11-10) : « une applet pour le jeu lagno de kde » — « lagno » est **iagno**,
le Reversi de GNOME (KDE a KReversi, même jeu) : `win32::cef_reversi`. Même démarche :

- **iagno** — https://gitlab.gnome.org/GNOME/iagno (branche `main`), projet sous
  **GPL-3.0-or-later** : `data/icons/hicolor/symbolic/org.gnome.Reversi-symbolic.svg`
  (symbolique → `reversi.png` et `energy/`) et
  `data/icons/hicolor/scalable/org.gnome.Reversi.svg` (couleur → `fluent_couleur/` et
  `fluent_3d/`). Rastérisés par `resvg` à 1024 px puis mis au gabarit (128 px, encre 116
  centrée, symbolique repeint en blanc). Glyphe Segoe Fluent : `E7F9` (générique, distinct).
  ⚠️ **LE LISSAGE D'« au_gabarit » RAMÈNE LE RGB À ZÉRO** dans les pixels d'alpha nul
  (chemin prémultiplié de Pillow) : reconstruire l'encre blanche par l'alpha APRÈS le
  gabarit, sinon le test `les_icones_energy_sont_blanches_et_au_gabarit` le refuse.

### Bovo (ajouté le 2026-11-10)

DEMANDE (2026-11-10) : « une applet pour le jeu bovo de kde » — **Bovo** (Gomoku, cinq
pierres alignées sur 15×15), le jeu de KDE : `win32::cef_bovo`. L'icône vient du thème
**Breeze** (KDE) :

- **Breeze** — https://invent.kde.org/frameworks/breeze-icons (LGPL-3.0-or-later) :
  `icons/apps/48/bovo.svg` (vérifié à l'octet près, 3883 octets). Version COULEUR → `fluent_couleur/`
  et `fluent_3d/` ; `resvg` à 1024 px puis `rasteriser_fluent_emoji.au_gabarit`.
  Aucune variante symbolique n'étant fournie, l'encre des thèmes MONOCHROMES (`bovo.png`
  et `energy/`) est tirée de la **LUMINOSITÉ** de la même icône : le fond clair devient
  transparent, le trait reste, puis l'encre est peinte en blanc et la barre la teinte.
- Glyphe Segoe Fluent : `E7F7` (générique, distinct).
  ⚠️ **LE MASQUE DE LUMINOSITÉ EST BINAIRE**, pas un alpha continu : un dégradé laisse une
  frange d'alpha 1–3 que le `bbox` du test compte, et l'encre mesurée paraît décalée de 4 px.
  Mettre les miettes à zéro (`α < 8`), puis RECENTRER sur la boîte réelle — et reconstruire
  le blanc par l'alpha APRÈS `au_gabarit`, comme pour Reversi.
