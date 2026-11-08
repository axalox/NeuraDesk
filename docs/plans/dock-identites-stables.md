# Des identités stables dans le dock — pour que l'épinglage soit immédiat

**Demandé le 2026-09-29** : « ne peux-tu pas faire en sorte qu'il y ait un index par
emplacement pour trier les apps ? » — puis, devant le choix, **B** : le refactor.

## Le besoin, et pourquoi il n'est pas cosmétique

Le sous-dock sait épingler un dossier, mais **le raccourci n'apparaît sur la barre
qu'au prochain lancement** : la composition du dock est arrêtée au démarrage, et
insérer une icône à chaud se heurte à la façon dont les PLAGES d'icônes sont gérées.

## L'ÉTAT VÉRIFIÉ (lu, pas supposé)

La barre du bas empile quatre plages, chacune gérée par `base + count` et terminée
par un `truncate` sur SA PROPRE base :

    … épingles Windows … | DOSSIERS + « + » | zone de notification | applications ouvertes

Le code le dit lui-même (`app.rs:9528`) : « **`set_dynamic_apps` se termine par un
`truncate` sur SA base, donc la plage de notification doit vivre SOUS la sienne** ».

⚠️ **Trois conséquences mesurables :**

1. **Insérer décale tout ce qui suit**, et les bases mémorisées côté `app.rs`
   (`dynamic_icon_base`, la base de notification) pointent alors à côté — pendant la
   seconde qui suit, un clic tombe sur la MAUVAISE icône.
2. `Dock::icons` est un `Vec<Icon>` **positionnel**, et `icon_order` / les ordres par
   barre portent des **indices**. Insérer demande de renuméroter tout ça — c'est la
   famille de défauts déjà payée plusieurs fois dans ce dépôt (voir la mémoire :
   « `insert_spacer` INSÈRE dans `icons` ⇒ tous les index glissent d'un cran »).
3. `Dock::set_dossiers` A déjà une base stable (`dossiers_base`) et un compte
   MUTABLE (`dossiers_count`) — mais son `truncate(base + total)` **couperait** les
   plages qui vivent derrière. C'est ce qui a fait croire à tort qu'il suffisait de
   le rappeler.

## LE BUT

**Qu'aucun appelant ne mémorise plus une POSITION.** Le dock possède les plages et
les résout à la demande ; insérer devient alors une opération interne au dock, qui
renumérote ce qu'il possède — et que personne d'autre n'a à connaître.

## LA FORME

1. **Une plage par usage**, identifiée par une valeur stable (et non par un entier
   calculé) : `WinPins`, `Dossiers`, `Notification`, `Dynamiques`.
2. **`Dock` sait à quelle plage appartient chaque icône** — une allocation par plage,
   jamais un `base` renvoyé à l'appelant.
3. **`Dock::emplacement(plage, i) -> Option<usize>`** remplace tous les
   `base + i` d'`app.rs`. C'est LE point du refactor : après lui, plus personne
   d'autre ne détient d'indice.
4. **`Dock::inserer(plage, pos, icone)`** : insère, renumérote les ordres par barre
   et les zones que le dock possède, et rend de quoi rafraîchir ce qui en dépend.
5. `set_dossiers` devient un cas particulier d'allocation, sans `truncate` sur une
   base calculée à l'extérieur.

## LES ÉTAPES (chacune se vérifie seule)

| # | Étape | Comment on sait qu'elle est bonne |
| --- | --- | --- |
| 1 | `Dock::emplacement` + les plages, SANS rien insérer | le dock s'affiche exactement comme avant, toutes barres — c'est un pur remplacement des `base + i` |
| 2 | Les `base + i` d'`app.rs` disparaissent | `grep "_icon_base"` ne rend plus rien côté appelant |
| 3 | `Dock::inserer` + tests unitaires sur les ordres et les zones | PASS: **FAIT** (`c6f17d2`) — 3 tests écrits AVANT la fonction, rouges d'abord. Il reste à le **BRANCHER** : c'est l'étape 4 |
| 4 | L'épinglage appelle `inserer` et construit la texture du dossier | PASS: **FAIT** (`0a576e9`, `5c99acc`, étape 4d) — les NEUF sites d'appel passent par `DossiersVivants`, la vignette est générée à l'épinglage, et `dossier_slots` a disparu |

⚠️ **Les étapes 1 et 2 ne changent AUCUN comportement** — c'est ce qui rend le
refactor sûr : on peut s'arrêter après chacune, et l'application est entière.

## L'ÉTAT RÉEL DE L'ÉTAPE 2 (mesuré le 2026-09-29, après avoir commencé)

⚠️ **Il y a 70 sites, pas « une vingtaine »** — mon estimation était fausse, et le
compte par plage est ce qui décide de l'ordre :

| Plage | Sites | État |
| --- | --- | --- |
| `Dossiers` | **7** | PASS: **FAIT** — la base n'est plus gardée, les 4 sites de rendu/armement passent par `emplacement` |
| `WinPins` | **23 réels** | PASS: **FAIT** — `cef_settings` + 4 sites directs (`3dc5362`, éprouvé à l'écran) + **rafraîchissement par frame** (`acb1e2c`, couvre les 13 sites de la boucle) + **les 2 bornes** (`95c64f4`). Il reste 18 `grep`, dont 14 couverts par le rafraîchissement et 4 qui DOIVENT rester |

### LES DEUX DÉCISIONS SONT PRISES (`6c9f35f`) — les sites restants s'y ramènent

1. **Borne de plage** → `Dock::plage_fin(Plage)` : le dock sait où finit sa plage (il lui
   fallait `winpin_count`, le pendant de `dossiers_count`). Remplace les 2 sites qui
   calculaient `base + winpins.len()`. ⚠️ `Plage::Dynamiques` rend `None` (pas de compte).
2. **Rang dans une plage** → `Dock::rang_dans(Plage, index)`, **généralisation de
   `dossier_slot` qui faisait déjà exactement cela**. Remplace les 8 sites qui écrivent
   `index - winpin_icon_base` — **et corrige un défaut réel** : au-delà de la plage,
   `index - base` rendait un rang INEXISTANT, que `winpins.get(…)` rejetait par chance.

⚠️ **TROISIÈME DÉCISION (5 sites) — ET ELLE EST PLUS SIMPLE QU'ELLE N'EN A L'AIR.**

Ces sites passent la base **en paramètre** à des fonctions auxiliaires (dont
`profile_parents`, **pure et testée**), **pendant que `dock` est emprunté mutablement**.
La tentation est de leur faire rendre des positions **relatives** — ce qui demande de
reprendre leurs signatures **et leurs tests**.

⛔ **MAIS LE CODE DONNE DÉJÀ LA RAISON DE LEUR COPIER** : « *la copie est faite ICI pour
que l'emprunt de `dock` ne coure pas jusqu'au `show`* ». Ce n'est donc pas une position
**périmée** qu'ils détiennent, c'est une copie faite **pour esquiver un conflit
d'emprunt** — et les signatures n'ont pas besoin de changer.

⇒ **LA VOIE EST : RAFRAÎCHIR CETTE COPIE DEPUIS LE DOCK À CHAQUE FRAME**, là où elle est
affectée aujourd'hui une fois au lancement. Une insertion ne coûterait alors qu'**une
frame d'index périmé** — invisible pour du rendu, et sans toucher aux emprunts.

✅ **ÉVALUÉ, ET FAIT (`acb1e2c`) — DEUX LIGNES AU LIEU DE TREIZE CONVERSIONS.** Mesuré :

    3029: let winpin_icon_base;          <- déclaration
    3152 / 3643: = dock.ensure_winpin_capacity(...)   <- les deux affectations
    4329: 'main: loop {                  <- LA BOUCLE COMMENCE ICI
    6019: … index >= winpin_icon_base    <- premier usage DANS la boucle

Les affectations sont **avant** la boucle, et **les treize sites durs sont dedans**.
Figée au lancement, la base était donc fausse partout d'un coup après une insertion.

⇒ Elle est devenue `mut` et **se rafraîchit en tête de chaque frame** depuis
`plage_base(Plage::WinPins)`. **Les treize sites n'ont pas été convertis** : ils lisent
une valeur remise à jour. Les usages d'AVANT la boucle (les deux bornes, l'écriture du
fichier) gardent la valeur du lancement, qui est la bonne à cet instant.

⚠️ **C'EST LE BUT ATTEINT, PAS LA LETTRE DU PLAN** : l'appelant détient encore une
position, mais **remise à jour** — donc une insertion ne laisse rien de faux au-delà
d'une frame, et une frame d'écart sur du rendu est invisible. Le reste de l'étape 2 se
réduit donc à peu de chose :

- les **2 bornes de plage** → `plage_fin` (décision prise, conversion à faire) ;
- et **c'est tout** : les 8 rangs de clic et les 5 fonctions pures sont DANS la boucle,
  donc déjà couverts par le rafraîchissement.
| `Dynamiques` | 22 | PASS: **RIEN À FAIRE — VÉRIFIÉ** |
| `Notification` | 12 | PASS: **RIEN À FAIRE — VÉRIFIÉ** |

✅ **LES DEUX DERNIÈRES PLAGES N'ONT PAS LE DÉFAUT, ET C'EST MESURÉ.** Toutes leurs
affectations sont **DANS la boucle de frames** et **demandent la base au dock** :

    6253 / 6550: dynamic_icon_base = active_dock.dynamic_icon_base();
    7655:        tray_icon_base    = dock.tray_icon_base();
    7656:        dynamic_icon_base = dock.dynamic_icon_base();
    9656:        tray_icon_base    = Some(dock.set_tray_count(…));
    10219:       dynamic_icon_base = Some(dock.set_dynamic_apps(…));

⇒ **L'ÉTAPE 2 EST DONC TERMINÉE** : `Dossiers` et `WinPins` avaient une base figée au
lancement (corrigées), ces deux-ci la relisaient déjà à chaque frame.

⚠️ **LA LEÇON, ET ELLE VAUT POUR LA SUITE** : j'allais convertir **34 sites** qui n'avaient
pas de problème. **Compter les occurrences d'un symbole ne dit rien de leur état** — il
faut regarder OÙ la valeur est posée. C'est la troisième fois aujourd'hui qu'un compte
m'égare (les « 17 Mo/s », les « un seul lecteur », les « 28 sites »).

⇒ **LE PAS SUIVANT EST L'ÉTAPE 3 (l'insertion sûre)**, et non plus de la conversion : les
quatre bases sont vivantes. Le travail réel y est le **décalage des ORDRES**, qui citent
des index ABSOLUS — et `Dock::decaler_le_bloc_des_epingles` en est le précédent, avec sa
borne et sa démonstration.

⛔ **`WinPins` EST PERSISTÉE SUR LE DISQUE — ENQUÊTÉ, ET CE N'EST PAS UN BLOCAGE.**

⚠️ **CE PARAGRAPHE A D'ABORD AFFIRMÉ DEUX CHOSES FAUSSES** — un « lu par UN SEUL
endroit » et un « supprimer le fichier ». L'enquête les a démenties toutes les deux, et
**c'est le paragraphe que tu lis qui les corrige** : il ne renvoie nulle part ailleurs,
parce qu'un plan qui pointe vers une section qu'il ne contient pas est un piège de plus.
Ce qui est vrai :

`winpin_base.txt` a **DEUX lecteurs** :
- `cef_settings`, qui s'en servait comme **borne** de `decaler_le_bloc_des_epingles`
  (« au-delà, plus rien ne bouge ») — **CONVERTI** : il demande la borne au dock ;
- **`app.rs`, AU DÉMARRAGE**, avant que le dock n'existe, pour ne recréer les espaces
  que jusqu'à cette base. **Celui-là reste, donc le fichier n'est PAS à supprimer** —
  le retirer laissait des espaces fantômes.

⇒ Il reste à convertir les **28 sites d'`app.rs`** ; `save_winpin_base` et
`load_winpin_base` **restent en place**.

⚠️ **Et la conversion d'une plage n'est vérifiable que par l'usage** : aucun test ne
couvre le rendu de `app::run`. Les 798 tests prouvent qu'on n'a rien cassé ailleurs,
pas que la barre s'affiche — c'est l'utilisateur qui tranche.

## ÉTAPE 3 — L'INSERTION SÛRE : CE QUE LES DEUX LECTURES ONT ÉTABLI (2026-09-30)

⚠️ **CE N'EST PLUS DE LA CONVERSION, C'EST LA FONCTIONNALITÉ.** Les quatre bases sont
vivantes (étape 2 terminée) ; ce qui reste à faire, c'est **insérer une icône** dans une
plage — donc **décaler les ORDRES**, qui citent des index **ABSOLUS**.

⛔ **LES DEUX FONCTIONS ONT ÉTÉ LUES EN ENTIER, ET ELLES NE SONT PAS SYMÉTRIQUES** — c'est
ce qui décide la forme de l'étape. Ce qui suit remplace la liste de « précédents » qui se
trouvait ici : ce n'était pas une piste à suivre, c'était un mécanisme à vérifier.

### 1. `Dock::insert_spacer` (`ui/dock.rs:4247`)

⛔ **SON POINT D'INSERTION N'EST PAS UN PARAMÈTRE** :

    let at = self.base_non_persistable().unwrap_or(self.icons.len());

soit `min(tray_base, dynamic_base)` (`base_non_persistable`, `dock.rs:2121`) — donc **la
frontière persistable, AU-DESSUS de la plage des dossiers**. **Il n'existe dans ce dépôt
aucune insertion d'icône à un index choisi.**

| Cible | Règle | Bornée ? |
| --- | --- | --- |
| les QUATRE ordres persistés | `entry >= at` → `+1` | **non** |
| `tray_base` | `if >= at { +1 }` | conditionnelle |
| `dynamic_base` | `if >= at { +1 }` | conditionnelle |

Ce qu'elle **ne touche pas** : `bottom_order`, `winpin_base`, `winpin_count`,
`dossiers_base`, `dossiers_count`.

**POURQUOI C'EST JUSTE AUJOURD'HUI — et il faut le dire, pas l'espérer :** l'ordre
d'allocation est `ensure_winpin_capacity` → `set_dossiers` → `set_tray_count` →
`set_dynamic_apps` (`app.rs:3163`, `3852`, `9656`, `10219`), chacune à `icons.len()` la
PREMIÈRE fois. Donc `winpin_base < dossiers_base < at`, et `bottom_order` est lui aussi
entièrement sous `at`. **Le `if >= at` est ce qui rend la propriété vraie plutôt que
supposée** — c'est la forme à reprendre.

### 2. `Dock::decaler_le_bloc_des_epingles` (`ui/dock.rs:4193`)

Trois lignes, déléguées à `decaler_bloc_epingles_dans` (`dock.rs:1394`) — **fonction pure,
partagée par ses DEUX appelants** (le retrait depuis les Réglages, et l'ajout depuis
`cef_dash::handle_add_to_zone`, qui tient ses quatre listes en locales avant d'écrire le
fichier) :

- ajout : `entry >= index && entry < limite` → `+1` ;
- retrait : `retain(|e| e != index)` **puis** `entry > index && entry < limite` → `-1` —
  la citation part AVANT le décalage, sinon elle désignerait l'icône suivante ;
- **BORNÉE** par `limite` (= `winpin_base`) : rien au-delà ne bouge ;
- **ne touche aucune base**, et **ne touche pas `icons`** : c'est un décalage de CITATIONS ;
- **n'insère rien** — c'est l'appelant qui repose la citation. Le test
  `the_pin_block_shifts_and_nothing_above_it` (`dock.rs:11204`) l'écrit noir sur blanc :
  « la citation de 5, elle, est reposée par l'APPELANT ».

### 3. LES DEUX QUESTIONS, RÉPONDUES

1. **« Les quatre ordres, ou seulement celui de sa barre ? »** → **les quatre ordres
   persistés**. Mais **PAS `bottom_order`** : la barre du bas a sa **cinquième** liste, et
   **aucune insertion ne la décale**.
2. **« Décale-t-il les plages qui suivent ? »** → **oui, les deux BASES**, chacune
   conditionnellement. Le reste était bien supposé par les appelants : `set_dynamic_apps`
   `truncate` sur sa base (`dock.rs:2313`), `set_tray_count` reconstruit par
   `retain`/`extend` (`dock.rs:2245`).

### 4. CE QU'IL A FALLU ÉCRIRE — ET LE CHEMIN À NE PAS PRENDRE

> **FAIT** (`c6f17d2`) — `Dock::inserer(plage, rang, icone) -> Option<usize>`, écrit
> **après trois tests rouges** (5 erreurs `E0599`, `no method named inserer`) qui
> décident sa forme. Ce qui suit est la conception telle qu'elle a été raisonnée AVANT
> d'écrire, et elle a tenu — sauf une assertion de test, fausse, corrigée avant
> l'implémentation : `plage_fin(Dossiers)` est **l'index du bouton « + »**, pas un
> nombre d'icônes (compter les icônes donnait un cran de trop).

**`insert_spacer` n'est PAS réutilisable telle quelle**, et le manque n'est pas « lire
`insert_spacer` » : c'est qu'il n'existe pas d'**insertion à index arbitraire**. Mais ses
deux moitiés existent déjà, séparément :

- le décalage des quatre ordres — **borné** dans `decaler_bloc_epingles_dans`, **non borné**
  dans `insert_spacer` ;
- le décalage des bases — les deux `if >= at` d'`insert_spacer`.

Il manque donc exactement trois choses :

1. **le point d'insertion en paramètre** ;
2. **la borne haute décidée, pas subie** — l'insertion d'un dossier DOIT décaler
   `Notification` et `Dynamiques`, donc **pas de borne comme les épingles** : c'est la
   liste entière qui suit ;
3. **`bottom_order`**, que personne ne couvre — la plage des dossiers y vit, et le bouton
   « + » aussi.

⛔ **ET UN CHEMIN À NE PAS PRENDRE : rejouer `set_dossiers(count + 1)`.** Elle fait
`icons.truncate(base + total)` (`dock.rs:2392`) — elle **DÉTRUIRAIT** les plages
notification et dynamiques, qui vivent au-dessus. Et elles **ne se replaceraient pas** :
`set_tray_count` et `set_dynamic_apps` **gardent leur base** (`get_or_insert`,
`dock.rs:2203` ; `unwrap_or`, `dock.rs:2305`), donc elles ne se re-poseraient pas au bon
endroit. ⇒ L'insertion passe par `icons.insert(at, …)` **puis** décalage, comme
`insert_spacer` — et **jamais** par un second `set_dossiers`.

**LES TESTS SUR LESQUELS S'APPUYER** : `both_bases_follow_every_insertion`
(`dock.rs:11061`), `creating_a_spacer_shifts_the_orders_with_the_icons` (`11028`),
`a_new_spacer_lands_right_after_the_element_it_follows` (`11097`),
`the_pin_block_shifts_and_nothing_above_it` (`11145`).

⚠️ **ET LA LEÇON DU 2026-09-29 TIENT TOUJOURS** : cette étape ne se fait pas en fin de
journée. Insérer dans `icons` décale des index ABSOLUS — c'est **littéralement** la classe
du plantage `0xc0000409` du 2026-09-29, la famille de défauts que ce dépôt a le plus payée.

## CE QU'IL NE FAUT PAS FAIRE

- ⛔ **Ne pas déplacer les bornes à la main** (`dynamic_icon_base += 1` et autres
  rattrapages) : c'est exactement le défaut d'index que ce plan supprime, et il a
  déjà été payé.
- ⛔ **Ne pas garder un `base + i` « pour aller vite »** pendant la transition : un
  seul survivant suffit à ramener le bug, et il ne se verrait qu'au moment d'une
  insertion.
- ⛔ **Ne pas toucher à l'ordre des plages dans la barre** : c'est un autre sujet, et
  le mélanger rendrait l'attribution des régressions impossible.

## LA SECONDE DEMANDE : GÉRER LES DOSSIERS ÉPINGLÉS DEPUIS LE DASH

**Demandé le 2026-09-29**, en même temps que l'épinglage immédiat — et les deux
s'enchaînent : gérer un dossier épinglé (le renommer, changer sa vignette, le retirer)
n'a d'intérêt que si l'effet est **visible tout de suite**.

**Aujourd'hui**, ces gestes vivent dans le panneau **Réglages**
(`cef_settings`, bras `dossier-*`) — et ils y **rechargent l'application**
(`dossiers_ecrits` → `request_reload`). C'est cohérent avec la composition figée, et
c'est exactement ce que cette demande veut faire disparaître.

**CE QU'IL Y AVAIT À AJOUTER — ET C'EST FAIT**, à l'étape 4d (2026-09-30). Les cinq
gestes du panneau Réglages (`dossier-ajouter`, `-retirer`, `-renommer`,
`-renommer-effacer`, `-vignette`) passent maintenant par le **même point d'étranglement**
que ceux de la barre, `app::DossiersVivants` :

| Geste | Ce qu'il demande |
| --- | --- |
| renommer, effacer le nom | `GesteDossier::Remplacement` — le NOMBRE ne change pas. ⚠️ Ce n'est pas purement décoratif : le repli d'un dossier sans vignette porte l'**INITIALE** du nom quand la police d'icônes manque, donc renommer peut changer ce qui est DESSINÉ |
| changer la vignette | `GesteDossier::Remplacement` — les deux tableaux de textures changent **en place**, le dock n'est pas touché |
| ajouter, retirer | `GesteDossier::Ajout` / `Retrait` — insertion et retrait, comme la barre |

⚠️ **LA VIGNETTE ÉTAIT LE POINT DUR, ET ELLE EST RÉSOLUE** : elle était décodée en texture
D3D11 au lancement (l'ancien `dossier_textures`, `let` **immuable**) et liée une par lot au
rendu. Les deux tableaux sont désormais `mut`, possédés par `EmplacementsDossiers`, et
modifiés par ses deux méthodes — donc ils ne peuvent plus diverger d'un cran.

**LE RANG, ET POURQUOI IL A FALLU RÉÉCRIRE LES CINQ BRAS** : `GesteDossier` porte un rang,
calculé AVANT la mutation de la liste — après un `remove` il n'existe plus, après un `push`
pas encore. Les formes d'origine ne le donnaient pas (`retain`, `iter_mut().find()`), et
c'est exactement ce qui les empêchait de suivre.

⇒ **L'ORDRE ÉTAIT** : étape 2 (les quatre plages) → étape 3 (l'insertion sûre) → étape 4
(l'épinglage immédiat) → **puis** ces gestes de gestion. **Il est parcouru en entier.**

⚠️ **Les quatre plages et pas seulement `WinPins`** : insérer un dossier décale tout ce
qui le suit (`Notification`, `Dynamiques`), donc leurs bases mémorisées seraient
fausses — c'est précisément le défaut que ce refactor existe pour supprimer.

## CE QUI RESTE EN ATTENTE APRÈS

**Le refactor est terminé.** Ce qui reste tient au reste du dépôt, pas à lui :

- les **lots 2, 3 et 4** de `docs/plans/barre-du-bas.md` (la pile, le glisser-déposer OLE,
  les actions de fichier) ;
- ⚠️ **`Dock::inserer` ET `retirer` N'ONT QU'UN APPELANT, LA PLAGE `Dossiers`** : les trois
  autres variantes de `Plage` ne sont pas encore construites par du code de production
  (voir l'`#[allow(dead_code)]` de l'énumération, qui porte le détail). Le jour où les
  épingles Windows ou le miroir s'inséreront à chaud, la mécanique sera déjà là, testée.
