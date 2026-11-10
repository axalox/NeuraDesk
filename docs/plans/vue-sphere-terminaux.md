# Vue sphère des terminaux et des sessions SSH/SFTP

Demande du 2026-11-01. Deux modes pour TOUS les terminaux (panneau Terminal) et
TOUTES les connexions SSH/SFTP (panneau Serveurs SSH) :

- **mode sphère** : chaque terminal / connexion est une vignette sur la face
  intérieure d'une sphère, caméra au centre ; on tourne au glisser, à la molette
  ou au clavier ; un **clic molette sur une vignette** l'ouvre en mode normal ;
- **mode normal** : l'onglet dans son panneau, tel qu'aujourd'hui (pas de plein
  écran au sens propre — décision de l'utilisateur).

Décisions de l'utilisateur : UNE sphère commune aux deux panneaux ; le clic
molette en mode normal ramène la sphère ; rien d'autre à ajouter.

## Architecture

- `assets/popups/sphere.html` + `win32::cef_sphere` : un PANNEAU de plus, posé à la
  place exacte du panneau qu'il remplace (même rectangle que le Terminal / SSH).
- `assets/popups/sphere_source.js` : chargé par `terminal.html` et `ssh.html`.
  Fabrique les vignettes (titre, hôte, dossier, dernières lignes ou entrées SFTP),
  capte le clic molette, et pose l'indicateur « Terminal · nom » dans la barre.
- Les vignettes voyagent par le canal POST (`terminal/vignettes`,
  `ssh/vignettes`, muets au journal : le contenu d'un terminal peut être secret),
  puis une action `vignettes` ; Rust les rassemble et les pousse à la sphère.
  Tant que la sphère est visible, Rust les redemande chaque seconde.
- **L'état n'est jamais touché** : la sphère ne fait que cacher le panneau ; les
  sessions xterm / SFTP vivent dans leur page, et la sortie des shells est déjà
  relayée panneau caché.
- Les demandes qui ont besoin du `GfxDevice` (ouvrir la sphère, rouvrir un
  panneau sur un onglet) sont posées en file et servies par `app::run` à la
  frame suivante (`cef_sphere::servir`).

## Le clic molette

Capté en phase de capture dans les deux pages, SAUF :
- sur un onglet (il ferme déjà le groupe) ;
- dans un champ de saisie ;
- dans un terminal dont le programme suit la souris
  (`term.modes.mouseTrackingMode !== "none"` : htop, mc, vim `mouse=a`…).

Raccourci réglable (fichier `sphere_raccourci.txt`) : `molette` (défaut),
`ctrl-molette`, `aucun`. Le réglage est dans la sphère elle-même ; l'indicateur
de la barre ouvre la sphère au clic quel que soit le raccourci.

Échap : en sphère, revient au dernier onglet actif ; en mode normal il n'est
JAMAIS intercepté (vim, less…).
