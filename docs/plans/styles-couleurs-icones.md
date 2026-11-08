# Plan — styles, couleurs et icônes sélectionnables (façon KDE)

DEMANDE EXPLICITE (2026-09-26) : « serait il possible de donner un aspect winui
aux éléments de l'app et de le rendre sélectionnable, je penses intégrer un
système de thèmes comme le fait kde ou gnome sur linux ». Réponses aux trois
questions posées : **les trois axes** (couleurs / style / icônes), et **les
styles utilisateur sont un objectif**.

Remplace, pour l'apparence, `graphical-style-modernization.md` (écrit à l'époque
d'`egui`, avant le passage des panneaux à CEF).

## Le modèle : trois choix indépendants, comme KDE

| Axe | KDE | NeuraDesk |
|---|---|---|
| **Couleurs** | Schéma de couleurs | Les thèmes existants (`cef_settings::apply_theme`, `apply_windows_theme`, mode clair `ui::color::mode_clair`) |
| **Style** | Style d'application (Breeze, Fusion…) | `ui::style` — « NeuraDesk » (l'aspect actuel) et « WinUI » |
| **Icônes** | Thème d'icônes | Les glyphes PNG actuels (`TopbarIcons/`) ou Segoe Fluent Icons |

## Ce qui existe (relevé le 2026-09-26)

- Les pages CEF sont chargées en `file:///` (`cef_popup.rs`, ~1837) : une feuille
  partagée `assets/popups/style.css` peut y être liée comme `menu.css` l'est déjà
  par 9 menus.
- Rust pousse la palette à CHAQUE frame par `execute_js` (`push_window_palette`,
  `push_menu_palette`) : le même canal portera les variables de style.
- **Les formes sont écrites en dur** : 96 `border-radius` sur 17 pages, 12
  valeurs distinctes (6px ×33, 8px ×24, 5px ×12, 50% ×11, 3px ×5…), 24 tailles
  de police, 52 valeurs de `padding`.
- Windows 11 fournit `SegUIVar.ttf` (Segoe UI Variable) et `SegoeIcons.ttf`
  (Segoe Fluent Icons) : utilisées depuis le système, jamais redistribuées.

## Décisions

1. **Un style est un fichier TOML de VALEURS, jamais du CSS ni du JS.** Les deux
   styles intégrés sont des TOML embarqués (`include_str!`) lus par le MÊME
   analyseur que les styles utilisateur — l'analyseur est donc exercé à chaque
   lancement et par les tests. Rust valide chaque valeur (bornes numériques,
   énumérations, nom de police restreint à `[A-Za-z0-9 -]`), puis FORMATE
   lui-même les variables CSS `--nd-*` : aucune chaîne venue d'un fichier
   n'atteint la page telle quelle.
2. **Des jetons par RÔLE, pas par valeur** : `--nd-rayon-petit`,
   `--nd-rayon-controle`, `--nd-rayon-surface`, `--nd-hauteur-controle`,
   `--nd-bordure-alpha`, `--nd-police`… Le style « NeuraDesk » reprend les
   valeurs DOMINANTES actuelles ; les valeurs minoritaires (5px → 6px) bougent
   d'au plus 1 px.
3. **Ce qui n'est pas un nombre est une énumération** (indicateur d'élément
   actif : `point` | `barre` ; bouton : `plein` | `contour` | `discret`),
   traduite en attributs `data-nd-*` sur `<html>` que `style.css` interprète.
   C'est ce qui permet à un style utilisateur de changer la STRUCTURE sans
   écrire de CSS.
4. **Styles utilisateur** : `%APPDATA%\NeuraDesk\styles\*.toml`. Une valeur
   invalide est IGNORÉE (retombée sur « NeuraDesk ») et journalisée par
   `crate::diag!` — jamais un style à moitié appliqué sans trace.
5. **Icônes** : un thème d'icônes associe un NOM (`reglages`, `son`,
   `reseau`…) à une source : un PNG ou un point de code de Segoe Fluent Icons.
   Thèmes utilisateur plus tard : un dossier de PNG aux mêmes noms.
6. **Choix dans Réglages › Apparence** : trois rangées de BOUTONS (pas de
   `<select>`, voir la mémoire du projet).

## Lots

1. **Socle** — `ui::style` (structure, analyseur, validation, deux styles
   intégrés, conversion en variables CSS), persistance du choix, poussée des
   variables avec la palette, `style.css` lié par les 17 pages. **Aucun
   changement visible.** Tests : les deux intégrés se lisent sans erreur, une
   valeur hors bornes est rejetée, un nom de police hostile est refusé.
2. **Conversion des pages** par lots de 3-4 : chaque littéral de forme devient
   un jeton. Vérifié par capture page par page (le style « NeuraDesk » doit
   rester identique à 1 px près).
3. **Style WinUI** + rangée « Style » dans Réglages.
4. **Formes dessinées par Rust** : rayon des barres, surbrillance et indicateur
   des icônes, infobulle (`tooltip.rs`), aperçus (`window_previews.rs`
   `THUMB_RADIUS`), horloge plein écran, police `TOOLTIP_FONT_NAME`.
   **Livré** : icônes du dock → `rayon_icone` (nouvelle clé, 14 / 8),
   vignettes d'aperçu → `rayon_controle`, pastilles de bureaux →
   `rayon_petit`, rayon des menus (donc popups, infobulle, aperçus, horloge
   plein écran) → `rayon_grand`, sauf si le curseur « Arrondi des menus » a
   été bougé depuis le dernier choix de style (choisir un style efface
   `menu_corner_radius.txt`, demande explicite), police de
   l'infobulle, des titres de zone et des titres d'aperçus → `police`. Les
   barres n'ont pas de rayon (collées au bord de l'écran) ; les initiales
   d'icônes gardent Segoe UI (elles relèvent de l'axe icônes).
5. **Axe icônes** : `ui::icones`, thème « Fluent » par Segoe Fluent Icons
   (rendu d'un glyphe de police en texture — à vérifier sur le chemin de texte
   existant), rangée « Icônes » dans Réglages.
   **Livré** : `ui::icones::JeuIcones` porte les 20 textures d'applets (une
   seule variable `icones` dans `app::run`, refaite en début de frame quand
   `generation()` change). Un glyphe est rendu par `gfx::text`, puis son ENCRE
   est recentrée dans un carré de 128 px à 116 px de côté (la taille des PNG,
   mesurée). Police vérifiée par son fichier (`SegoeIcons.ttf`, sinon
   `segmdl2.ttf`, sinon PNG). Choix dans `icones.txt`,
   `NEURADESK_DEBUG_ICONES=<id>` pour capturer sans écrire. Les icônes des
   pages CEF et de la zone de notification ne changent pas.
6. **Axe couleurs** : les thèmes actuels regroupés en « schémas » ; schémas
   utilisateur `%APPDATA%\NeuraDesk\couleurs\*.toml`.
7. **Styles et icônes utilisateur** : lecture des dossiers, validation,
   apparition dans les rangées de Réglages.

## Pièges connus d'avance

- La capture prouve qu'une page S'AFFICHE, jamais qu'elle RÉAGIT : chaque lot
  touchant une page se termine par un essai de l'utilisateur.
- `settings.html` ne défile pas sous le harnais : passer par la colonne de
  catégories (`NEURADESK_DEBUG_CLICK`) et vérifier les horodatages de
  `%APPDATA%\NeuraDesk\` après tout run avec un clic.
- Le nuancier de `settings.html` garde ses blancs (ce n'est pas du texte).
