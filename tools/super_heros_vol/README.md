# Super-héros de l'inactivité — fabrication des planches

Tout tourne sur le ComfyUI local (http://127.0.0.1:8188), avec le Python de ComfyUI.

1. `portraits.py` : les 10 portraits (FLUX.2 Klein 4B, texte → image).
2. `depart.py <n> <tirages>` : l'image de départ, le héros en vol de trois quarts sur fond vert uni
   (Klein, retouche guidée par le portrait). Choisir à l'œil : l'emblème doit être le sien.
3. `serie.py [n…]` : Wan 2.2 I2V 14B FP8 + LoRA lightx2v (4 pas), MÊME image au début et à la fin
   (`WanFirstLastFrameToVideo`) → une pirouette de 81 images qui boucle ; puis `detourer.py`.
   ~1 min 40 par clip sur une RTX 5080 16 Go.
4. `planche.py <rgba> assets/popups/mascottes/heros-NN.webp` : les 80 premières images en planche
   8 colonnes × 10 lignes (la 81ᵉ = la 1ʳᵉ), hauteur 320 px.

Détourage : alpha doux par « excès de vert » (G − max(R, B)) rapporté au vert mesuré dans les coins
(il dérive après le VAE), débordement vert neutralisé par G = min(G, max(R, B)) (l'or reste intact),
miettes isolées retirées.

Modèles : FLUX.2 Klein 4B (Apache-2.0), Wan 2.2 I2V A14B (Apache-2.0), LoRA lightx2v Wan2.2-Lightning (Apache-2.0), BiRefNet (MIT) — vérifiés sur Hugging Face le 2026-11-23, crédités dans `THIRD-PARTY-NOTICES.md`.

## Héros 11 à 30 : aux couleurs de l'écran d'accueil (néons)

Leurs costumes contiennent du vert : le fond vert est impossible. Même chaîne, avec :

1. `portraits_neon.py` : les 20 portraits néon.
2. `depart_gris.py <n> <tirages>` : départ sur **fond gris uni**, sans traînées de lumière.
   `DEPART_EXTRA="The chest emblem is …"` impose un emblème : Klein dérive sinon vers un écusson
   en losange avec un « S » (refait pour 16, 17, 18, 23, 29).
3. `serie_neon.py [n…]` : Wan comme plus haut, puis `detourer_ia.py` — **BiRefNet**
   (`ZhengPeng7/BiRefNet`, MIT, via `transformers`, il faut `timm`) et retrait du gris dans
   les bords semi-transparents. ~2 min 10 par héros.
4. `revue.py <sortie.png> n…` : une image sur 8 de chaque clip, pour relire à l'œil.

⛔ Lancer la série AU PREMIER PLAN par lots de 4 (< 10 min) : en arrière-plan, Claude Code a
tué la tâche faute de mémoire (ComfyUI garde ~50 Go de modèles en RAM).

## L'atterrissage (les 30 héros)

Il arrive de loin en haut à droite, se pose en bas au centre, repart à pied (`super_heros/atterrissage.rs`).
Trois clips SUR PLACE par héros, fond gris, 640×640 :

1. `poses_lot.sh n…` (appelle `pose.py <n> A|W <tirages>`) : deux poses par Klein depuis le portrait,
   A = en vol face à nous, W = marche de profil vers la droite, 3 tirages chacune ;
   `revue_poses.py <sortie.png> n…` pour choisir à l'œil (`POSES` dans `serie_atterrissage.py`).
2. `serie_atterrissage.py [n…]` : Wan, `VOL_L/VOL_H/VOL_IMAGES` ; l'approche A→A (33 images,
   boucle), l'atterrissage A→W (81 images, `VOL_FIN` = image de fin différente), la marche W→W
   (33 images, boucle). Reprend où il s'est arrêté. ~4 min 30 par héros.
3. `sequence.py <n> assets/popups/mascottes` : BiRefNet sur les trois clips avec UN cadre et UNE
   échelle communs (les raccords tombent au pixel), héros debout à 280 px ; écrit
   `heros-NN-approche.webp`, `heros-NN-atterrissage.webp`, `heros-NN-marche.webp` et la fiche
   `heros-NN-atterrissage.txt` (nombres d'images, case, ligne des pieds, foulée mesurée sur le pied posé).
   ⛔ Pas pendant une génération Wan : BiRefNet et Wan se disputent la carte graphique.
4. `debut_marche.py assets/popups/mascottes` : ajoute `marche_des` aux fiches — l'image où le héros
   se met à marcher SUR PLACE dans l'atterrissage (Wan cloue la fin du clip sur la marche) ;
   l'application le fait avancer dès là. `--sec` : affiche sans écrire.
5. `vignettes.py assets/popups/mascottes` : les vignettes 96 px de la grille des Réglages.

## Styles « à l'encre » et « en pixels » (2026-11-23)

`styles_heros.py assets/popups/mascottes [n…]` : les filtres des mascottes (`tools/mascottes_styles.py`)
sur chaque case, planches AUX MÊMES DIMENSIONS (la fiche d'atterrissage reste valable) :
`heros-NN-ink[-clip].webp`, `heros-NN-pixel[-clip].webp`. Une palette et un grain par héros pour ses
quatre planches (raccords au pixel). Pixels : blocs de 5 px ancrés au coin de chaque case, WebP sans
perte ; ⛔ `BLOC` = `BLOC_PIXELS` de `src/win32/super_heros.rs` — l'application relit la grille au
centre des blocs et l'agrandit au plus proche voisin. ~2 min par héros, sans carte graphique.
Réglage : `heros_style` (origine / encre / pixels) dans `mascotte.txt`.

## Les super-héros en mascottes (2026-11-24)

`tools/mascottes_heros.py` : le buste DU héros, recadré par Klein depuis son portrait (même visage,
même costume, même dessin comics, fond blanc avec marges), puis 8 regards + 9 réactions par la chaîne
des personnes, puis les six styles. Fichiers `superheros-NN[-style]-{directions,reactions,vignette}.webp`.
⛔ Pas de version « petite mascotte » : refusée par l'utilisateur.
