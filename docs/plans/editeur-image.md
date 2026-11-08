# Éditeur d'image intégré — plan

**Demandé le 2026-09-24.** Une applet dans la zone **Intégrés** de la barre de droite, qui ouvre un
éditeur d'image **dans une fenêtre à part, redimensionnable** (réponse « 1b »).

Choix de l'utilisateur :

| Question | Réponse |
| --- | --- |
| Où s'ouvre-t-il | **Fenêtre à part**, redimensionnable, agrandissable |
| Entrées | une image de **l'historique du presse-papiers**, un **fichier** du disque, une **capture d'écran ou d'une zone** |
| Outils | **tous** (y compris luminosité/contraste et calques) |
| Sorties | **toutes** : presse-papiers, fichier PNG/JPG, **joindre à une note** |
| Inspiration | Flameshot v14, ShareX |

---

## Ce qu'on reprend de Flameshot et de ShareX

**Flameshot** (v14 = surtout du multi-écran, du DPI et du presse-papiers ; les outils datent d'avant) :
- la capture de zone **« en place »** : l'écran est figé, on trace la sélection, on annote **par-dessus
  l'écran figé**, puis Entrée/Ctrl+C/Ctrl+S — sans passer par une fenêtre d'édition ;
- raccourcis d'une lettre : `P` crayon, `D` ligne, `A` flèche, `R` rectangle, `C` cercle, `M` marqueur,
  `T` texte, `B` pixeliser, `S` sélection ; flèches = déplacer la sélection de 1 px, `Maj`+flèches =
  la redimensionner ;
- ⚠️ leçon v14 : **« Forced PNG clipboard format on Windows »** — le JPG dans le presse-papiers posait
  des problèmes de compatibilité. On écrit **DIBV5 + PNG** (ce que `clipboard::write_image` fait déjà) ;
- ⚠️ leçon v14 : **« Last region on scaled displays »** — la région mémorisée doit tenir compte du DPI.
  Ce dépôt a déjà payé ce piège plusieurs fois (voir la mémoire sur le DPI du panneau de notification).

**ShareX** (éditeur moderne) — outils et raccourcis :
`V` sélection, `R` rectangle, `E` ellipse, `L` ligne, `A` flèche, `F` main levée, `T` texte,
`O` bulle, `N` **étape** (pastilles numérotées 1, 2, 3…), `I` image (coller une image), `J` emoji,
`K` curseur, `H` **surligneur**, `W` **gomme intelligente** (recouvre en mélangeant les couleurs
voisines), `B` flou, `P` pixeliser, `M` **loupe**, `S` **projecteur** (assombrit tout sauf une zone),
`C` recadrer, `U` **découpe** (retirer une bande horizontale/verticale et recoller).
Plus : **effets d'image** globaux (luminosité, contraste, teinte, saturation, niveaux de gris,
inverser, netteté, flou…), **taille du canevas**, capture **1/2/3** = écran n, **Espace** = plein écran.

---

## Architecture

### Ce que le dépôt sait déjà faire
- **Page CEF hors écran** (`win32::cef_popup`) : rendu dans une texture D3D11, relais souris/clavier,
  **`resize_if_needed`** existe déjà.
- **Presse-papiers** : `clipboard::read_image` / `write_image` (DIBV5 + PNG), `encode_png`/`decode_png`.
- **Historique** : `ClipboardEntry` + pleine résolution paresseuse (`cef_clipboard`).
- **Dialogues** : `file_dialog::ouvrir` / `enregistrer_sous`.
- **Notes** : `config::store_note_image` + `Note::push_image`.
- **Capture** : `gfx::duplication` (DXGI Desktop Duplication) et `wgc` (fenêtres).

### Ce qui manque — trois fondations, dans cet ordre de risque

1. ⛔ **UN CANAL POUR LES GROSSES DONNÉES, DE LA PAGE VERS RUST.** Le seul canal actuel est
   `location.href = "neuradesk://…"` : une **URL**, donc limitée (~2 Mo chez Chromium), et **deux
   navigations d'une même tâche se télescopent** (voir la mémoire « PANNEAU ou MENU »). Une capture
   4K en PNG pèse 5 à 15 Mo. **Proposition** : un `SchemeHandlerFactory` pour `neuradesk-data://`,
   et la page fait `fetch("neuradesk-data://image/…", { method: "POST", body: blob })`. Les liaisons
   existent dans `cef 151.8` (`register_scheme_handler_factory`). **Repli** si ça échoue : envoi en
   morceaux acquittés un par un par Rust. **C'est la première chose à prouver** : sans ce canal, ni
   « copier », ni « enregistrer », ni « joindre à une note » ne fonctionnent.
   *(Sens Rust → page : data URI, comme la pleine résolution du presse-papiers — déjà éprouvé.)*

   ✅ **PROUVÉ le 2026-09-24** (`win32::cef_data`) — pas par un schéma personnalisé mais par un
   **détournement** : `fetch(POST)` vers `http://neuradesk-data.invalid/<route>`, intercepté par
   `RequestHandler::resource_request_handler` → `ResourceHandler`. Sonde
   (`NEURADESK_DEBUG_OPEN=cef-test NEURADESK_DEBUG_SONDE_CANAL=1 NEURADESK_DIAG=1`) :
   **1 Ko, 2 Mo, 20 Mo, 60 Mo — taille et empreinte FNV-1a identiques des deux côtés**, 60 Mo en
   ~160 ms. Aucun pré-vol CORS observé, même en `application/octet-stream`.
   ⛔ **Le piège qui a coûté un tour** : l'`on_before_resource_load` par défaut de la crate rend
   `ReturnValue::default()` = **`RV_CANCEL`** → toute requête détournée est annulée avant `open`, et la
   page ne voit que « Failed to fetch ». Il faut le surcharger pour rendre `CONTINUE`.

2. **UNE FENÊTRE CEF REDIMENSIONNABLE.** Tous les popups actuels sont `WS_POPUP` + `WS_EX_TOOLWINDOW`
   + `WS_EX_TOPMOST`, se ferment à la perte de focus, et sont exclus de la capture. L'éditeur doit
   être l'inverse : `WS_OVERLAPPEDWINDOW`, `WS_EX_APPWINDOW` (barre des tâches + Alt-Tab, voir la
   mémoire « Capture visibility »), pas topmost, **ne se ferme pas** à la perte de focus,
   `WM_SIZE` → `resize_if_needed`, `WM_GETMINMAXINFO` → taille minimale, position et taille
   **mémorisées**. Un champ de `PopupSpec` (`fenetre: bool`) plutôt qu'un second mécanisme.

3. **LA CAPTURE DE ZONE.** Figer l'écran (Desktop Duplication, **par écran, au DPI réel**), puis ouvrir
   la page **sans bordure, sur l'écran visé**, en « mode capture » : l'image figée en fond, sélection
   à la souris, poignées, annotation en place (Flameshot) ; `Entrée` → ouvre l'éditeur complet,
   `Ctrl+C` → copie directe, `Échap` → abandon. **Même page, deux modes** : pas de second éditeur.

### Le moteur d'édition — dans la page, en JavaScript
- **Un modèle d'objets**, pas des pixels : chaque annotation est un objet (type, géométrie, style)
  qu'on peut **resélectionner, déplacer, modifier** tant qu'on n'a pas aplati. Rendu sur `<canvas>`.
- **Calques** : une pile ordonnée (image de fond + calques d'objets), visibilité, opacité, ordre.
- **Annuler/refaire par INSTANTANÉS** du modèle (décision déjà prise pour les notes : une commande
  inverse fausse rend un état qui n'a jamais existé).
- **Effets globaux** en pixels (`getImageData`), appliqués au calque de fond, eux aussi annulables.
- **Aucun `<select>`** (il ne s'ouvre jamais en CEF hors écran) : des boutons.
- **Tout `on*` passe par `addEventListener` ET le repli `setAttribute` est gardé** (défaut de ce soir).

---

## Lots de livraison

Chaque lot est livrable seul, testé, et essayé par l'utilisateur avant le suivant.

| Lot | Contenu | Preuve attendue |
| --- | --- | --- |
| **0 — Sonde du canal** | `neuradesk-data://` en POST ; une page de test envoie 20 Mo, Rust rend la taille et un hachage | test + trace : octets reçus = octets envoyés |
| **1 — Squelette** | applet (glyphe, index en fin de bloc, interrupteur Réglages, `icon_order`), fenêtre redimensionnable, ouvrir un **fichier** / l'image du **presse-papiers**, zoom/défilement, **copier**, **enregistrer PNG/JPG** | ouvrir → enregistrer → fichier identique au pixel près |
| ↳ *lot 1 mesuré (2026-09-24)* | harnais `NEURADESK_DEBUG_OPEN=editeur-image` + `NEURADESK_DEBUG_EDITEUR_FICHIER` / `_ENREGISTRER` (écrit sans dialogue, compare, journal `NEURADESK_DIAG=1`) | capture opaque 180×1051 : **0 pixel différent**. `NeuraDesk.png` (transparence) : 36 207 différents, **dont 0 opaque** (alpha max 254) — la prémultiplication du canevas, limite connue. Interactions à essayer par l'utilisateur |
| **2 — Annotations** | sélection/déplacement, rectangle, ellipse, ligne, flèche, main levée, texte, surligneur, **étapes numérotées**, couleurs/épaisseur par boutons, annuler/refaire, raccourcis d'une lettre | usage par l'utilisateur |
| ↳ *lot 2 mesuré (2026-09-25)* | modèle dans `editeur_annotations.js` (objets immuables, historique par instantanés JSON) ; levier `NEURADESK_DEBUG_EDITEUR_ANNOTER=1` pose un objet de chaque sorte avant l'enregistrement | sans annotation : **toujours 0 pixel différent** (la recomposition fond + objets ne dégrade rien) ; avec : les 10 objets dans le fichier, **sans le cadre de sélection** (visible à l'écran seulement). Interactions à essayer par l'utilisateur |
| **3 — Masquer** | flou, pixeliser, gomme intelligente, projecteur, loupe | usage |
| ↳ *lot 3 mesuré (2026-09-25)* | les masques relisent **le canevas lui-même** (fond + annotations déjà posées) ; force par l'épaisseur ; la gomme prend la moyenne du **pourtour** sur le fond, fixée à la pose ; les projecteurs partagent **une seule ombre** percée | sans annotation : **0 pixel différent** ; avec les 15 objets : trou du projecteur **égal à l'original au pixel**, ombre = 0,4 × l'original. ⛔ **piège mesuré** : `destination-out` retire selon l'alpha de la SOURCE — percer avec la teinte de l'ombre (α 0,6) laissait 24 % d'ombre dans le trou. Interactions à essayer par l'utilisateur |
| **4 — Géométrie** | recadrer, pivoter, retourner, redimensionner, taille du canevas, découpe (retirée le 2026-09-25) | tests sur les dimensions |
| ↳ *lot 4 mesuré (2026-09-25)* | `editeur_geometrie.js` : chaque opération rend un **nouveau fond** + une **carte** des points — les annotations suivent sans être aplaties ; l'historique garde le fond **par référence** (un Ctrl+Z annule une opération). Levier `NEURADESK_DEBUG_EDITEUR_GEOMETRIE=1\|2` | mode 1 (6 quarts de tour, 4 miroirs, canevas +40×60 puis recadré) : **IDENTIQUE AU PIXEL PRÈS** — pivoter/retourner/recadrer ne lissent pas. Mode 2 + ANNOTER : 2511×621 → 621×2511 → découpe 497 → réduit 373×1883, **15 → 11 objets**. ⛔ **piège mesuré** : un objet tout entier dans la bande découpée se posait sur la couture en filet de **0,3 px** (la bande est arrondie au pixel, pas l'objet) ⇒ un objet que la carte réduit à **moins d'un pixel** disparaît. Interactions à essayer par l'utilisateur |
| ↳ *outils Flameshot manquants (2026-09-25)* | lus dans `flameshot/src/tools/` : **rectangle plein** (X, coins ronds), **inverser** (I, complément RGB, alpha gardé), **pointe de l'étape** (glisser, comme le compteur), **épaisseur fine** (9 niveaux, `[`/`]` et −/+ ; Fin/Moyen/Épais = niveaux 2/4/6, forces de masque inchangées). Écartés : envoi en ligne (service externe), épingler et « ouvrir avec » (des actions, pas des outils) | ANNOTER (17 objets) : zone inversée = **255 − original, écart 0** sur 3 600 px ; capture : barre et objets. Interactions à essayer par l'utilisateur |
| ↳ *panneau, ciseaux, actions Flameshot (2026-09-25)* | **L'éditeur devient un PANNEAU** de la zone libre (`dash_rect`, comme Notes et le calendrier ; `fenetre: false`) — demande « affiche l'éditeur d'image dans un dash plutôt » ; le mécanisme `PopupSpec::fenetre` est gardé sans utilisateur. **La découpe (✂, U) est RETIRÉE** à la demande. Les actions Flameshot restantes : **épingler** (`win32::epingle` : fenêtre `WS_EX_LAYERED` au premier plan, glisser, molette = taille, Ctrl+molette = opacité, double-clic/Échap/milieu = fermer), **ouvrir avec** (PNG temporaire dans `%TEMP%\NeuraDesk\editeur`, purgé après 24 h, puis `SHOpenWithDialog`), **valider** (copier puis fermer, Ctrl+Entrée), **fermer** (✕, Échap sans rien à annuler). Envoi en ligne (Imgur) : en attente d'une décision de l'utilisateur (publication externe) | tests : 550 (épingle : prémultiplication, moyenne en réduction, pixels nets en agrandissement, échelle initiale) ; capture du panneau. Interactions à essayer par l'utilisateur |
| **5 — Entrées** | bouton **« Éditer »** dans l'historique du presse-papiers ; **capture d'écran / de zone** (mode capture en place) | usage, **multi-écran et DPI** |
| **6 — Effets et calques** | luminosité, contraste, teinte, saturation, gris, inverser, netteté ; panneau des calques | usage |
| ↳ *calques, XCF et PSD (2026-09-25)* | demande « gérer les calques comme dans gimp, et il faut pouvoir rouvrir les fichiers ». **Page** : `editeur_calques.js` — un calque = toile immuable + position (il peut déborder) + opacité + visibilité + **16 modes** (les noms de `globalCompositeOperation`, libellés de GIMP) + **ses annotations**, en pixels de l'image ; panneau à droite (œil, vignette, double-clic = renommer, mode par boutons, opacité, nouveau / dupliquer / monter / descendre / fusionner vers le bas / aplatir / supprimer), outil **✥ (D)** et flèches pour déplacer le calque actif. Comme GIMP : annotations, masques, couleurs et gomme ne touchent que le calque **ACTIF** ; la géométrie, **tous** (`transformerDocument` — recadrer et le canevas ne font que décaler, sans rogner). « Ouvrir » remplace, **« Ouvrir en tant que calques » (Ctrl+Alt+O)** ajoute, le presse-papiers devient un nouveau calque. **Rust** : `document_image` (le modèle, le paquet de la page, le JSON vers la page), `format_xcf` (lecture toutes versions/précisions/compressions, groupes aplatis ; écriture v010 zlib), `format_psd` (crate `ag-psd`), et ⛔ **`decodeur_document` : un fichier ouvert est décodé dans un SOUS-PROCESSUS** (le même exe, `--decoder-document`) — le dock est en `panic = "abort"`, un PSD corrompu le tuerait | **mesuré dans GIMP 3.2.6** (`gimp-console-3 -i --batch-interpreter python-fu-eval`) : nos XCF et PSD s'ouvrent avec les 4 calques, noms accentués, décalages, modes, opacité, calque caché ; aplatis, **les deux donnent les mêmes pixels, égaux au mélange sRGB du canevas** (espace de fusion 2 = RVB non linéaire, ce que fait le navigateur). Un XCF écrit par GIMP 3 (v023) se relit. Tests : 593. Rendu de la page et interactions à essayer par l'utilisateur |
| **7 — Sorties restantes** | **joindre à une note** (choix de la note par boutons), bulle, emoji, image collée, curseur | la note porte l'image |

---

## Risques et pièges connus
- **Le canal (lot 0)** : tout le reste en dépend — d'où son rang.
- **Clavier dans une fenêtre CEF hors écran** : le relais existe (`dispatch_input`, correctif AltGr),
  mais les raccourcis d'une lettre ne doivent pas partir quand on tape dans un champ texte.
- **DPI et multi-écran** pour la capture de zone : mesurer par écran, jamais supposer 100 %.
- **Deux instances CEF ne coexistent pas** : la vérification au harnais exige l'app fermée.
- **La capture ne voit que le rendu** : un geste qui ne marche pas est invisible au harnais — chaque
  lot se termine par un essai **de l'utilisateur**.
- **Poids** : garder `notes.html` / `settings.html` comme contre-exemple — l'éditeur se découpe en
  plusieurs fichiers JS (`editeur/outils.js`, `editeur/effets.js`…) plutôt qu'un seul HTML géant.
