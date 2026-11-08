# La disposition : BARRE → ZONE → ICÔNE

> **Remplace `docs/plans/rangement-des-icones-par-identite.md`**, qui gardait le
> modèle plat (une liste d'icônes, quatre listes de numéros) et ne corrigeait que
> l'orthographe des entrées.

**Demandé le 2026-10-29** : « ce système ne va pas, il faut structurer par barre,
puis par zone, et enfin par icône, telle icône va dans telle zone à tel endroit de
la barre ».

---

## Le besoin, et la mesure qui l'a déclenché

Le portable doit arriver sur une autre machine **avec la disposition de
l'utilisateur**. Mesuré le 2026-10-29 sur une VM Windows 10 (1280×648) :

| Ce qui diffère | Effet dans le modèle actuel |
|---|---|
| Le scanner n'y est pas installé (`NAPS2.Console.exe`) | un applet de MOINS dans la base, tout ce qui suit glisse |
| 6 épingles de barre des tâches contre ~30 | la région des épingles change de taille, tout ce qui suit glisse |
| Le dossier de réglages est vierge | `dossiers.toml` filtré aux séparateurs |

Une icône y change donc **de barre et de zone** : « des icônes de la barre de droite
se retrouvent après l'icône de neuradesk », et les zones « Intégrés » et
« Réglages » se vident.

**La cause n'est pas un bug, c'est le modèle** : une zone n'existe aujourd'hui que
comme une **plage contiguë entre deux séparateurs**, donc toute la disposition
dépend d'une arithmétique d'index. Un applet absent décale les zones, donc les
icônes.

---

## L'ÉTAT VÉRIFIÉ (lu, pas supposé)

| Élément | Où | Ce que c'est |
|---|---|---|
| `Dock.icons` | `ui/dock.rs:1793` | **une** liste plate ; l'index 0 est le logo |
| Constantes du noyau | `app.rs:140-156` | `APP_ICON_INDEX=0`, `CLIPBOARD_VIEWER_ICON_INDEX=1`, `SETTINGS_ICON_INDEX=2`, `PINNED_ICON_BASE=3` |
| Épingles Windows | `Dock::ensure_winpin_capacity` `ui/dock.rs:2759` | **ajoutées à la fin**, exprès, pour ne pas décaler les applets |
| Dossiers, notification, ouvertes | `set_dossiers` `:3166`, `set_tray_count` `:2982`, `set_dynamic_apps` `:3086` | ajoutés après ; la notification **doit** précéder les ouvertes |
| Quatre listes de barre | `set_icon_order` `:2554`, `poser_l_ordre` `:2570` | `Vec<usize>` ; tout index `>= icons.len()` est **écarté en silence** |
| Non persistable | `is_persistable` `:2838` | le miroir de notification et les applications ouvertes ne sont **jamais** écrits |
| Zones | `ui/zones.rs:143` `ZoneKey { bar, rank }` | une zone a **déjà** une identité stable : barre + rang |
| Zones persistées | `zones.toml` | `[bars.B/G/D/H]` (bas, gauche, droite, haut) avec `sizes` et `spacers` ; `[zones.D1..D4]` avec `columns`, `cote`, `label`, `name`, `permanent` |
| Ordre persisté | `config.rs:1250` / `:1264` | `icon_order.txt` : **quatre lignes de nombres** |
| Ouvertes | `app_identity.rs:278` `dynamic_key()` | `exe`, ou `profile:<id>` |

Les identités existent déjà toutes ; ce qui manque est de **ranger les icônes dans
les zones** au lieu de les numéroter dans une liste plate.

---

## LE BUT

**Une icône appartient à une ZONE, à un RANG dans cette zone.** Déplacer une icône
ne touche qu'à sa zone ; une icône absente ailleurs est ignorée dans SA zone sans
déplacer les autres ; une zone vide reste une zone.

---

## LA FORME

### Le modèle

```
barre  → zones (dans l'ordre)          → icônes (dans l'ordre)
```

| Barre | Zones |
|---|---|
| **haut** (`H`) | 3 : gauche, milieu, droite |
| **bas** (`B`) | 1 seule |
| **gauche** (`G`) | autant que voulu, elles se suivent |
| **droite** (`D`) | autant que voulu — chez l'utilisateur : Intégrés, Logiciels, Réglages, Notifications |

**Une zone vide EXISTE** : elle garde sa cote, ses colonnes, son nom, son rang, et
elle se dessine. C'est une décision explicite de l'utilisateur (2026-10-29).

### Les identités d'icône

| Sorte | Forme | Stabilité |
|---|---|---|
| Noyau (logo, dash, Réglages, presse-papiers, thème, alimentation, terminal, son, réseau…) | `noyau:<nom>` | le nom est écrit dans le code, à côté de sa constante d'index |
| Épingle Windows | `epingle:<cible>` | la cible (URI ou exe) |
| Dossier de la barre du bas | `dossier:<chemin>` | la clé de `dossiers.toml` |
| Application ouverte | `ouvert:<dynamic_key>` | `exe` ou `profile:<id>` |
| Miroir de la zone de notification | `notification:miroir` | une seule |

⛔ **Les deux dernières ne sont PAS écrites dans le fichier** — elles changent d'une
session à l'autre (`is_persistable`, `ui/dock.rs:2838`). Leur **zone**, elle, est
écrite : la zone existe avec sa cote et ses colonnes, et se remplit à l'exécution.
C'est ainsi qu'on garde « une zone vide existe » sans persister un index mobile.

### Le fichier

`zones.toml` gagne la hiérarchie, puisqu'il porte **déjà** les barres et les zones :

```toml
[bars.D]
zones = ["Intégrés", "Logiciels", "Réglages", "Notifications"]

[zones.Intégrés]
name = "Intégrés"
cote = 930.0
columns = 2
icones = ["noyau:alimentation", "noyau:terminal", "noyau:son"]

[zones.Logiciels]
cote = 1094.0
columns = 2
icones = ["epingle:C:\\Program Files\\Inkscape\\bin\\inkscape.exe"]

[zones.Notifications]      # remplie à l'exécution
cote = 1600.0
columns = 3
icones = ["notification:miroir"]
```

`icon_order.txt` **disparaît**.

---

## L'ÉTAPE 4, LIVRÉE LE 2026-10-29

**Le problème à résoudre** : remplir `icones` de chaque zone à partir de l'état
actuel — quatre listes d'INDEX (`icon_order.txt`) et des séparateurs dispersés
dans ces listes.

**LES TROIS DÉCISIONS, PRISES PAR L'UTILISATEUR LE 2026-10-29** :

| Question | Réponse |
|---|---|
| Une icône qui précède la première zone de sa barre | **Rattachée à CETTE zone** (donc écrite en tête de sa liste) : jamais orpheline, jamais reversée à gauche. Elle se dessine après le séparateur de la zone. |
| Les applications ouvertes et le miroir de notification | **Jamais écrits** — seule leur zone l'est, et elle se remplit à l'exécution (c'est `Dock::is_persistable`, dit par `ui::zones::identite_d_execution`). |
| Une identité qui ne se résout pas ici | **Écrite quand même**, et **gardée** à chaque sauvegarde : c'est exactement ce que le portable transporte. |

**CE QUI A ÉTÉ FAIT** :

1. `ui::zones::repartir_les_icones` — le cœur pur : parcourt l'ordre vivant de la
   barre, compte les rangs, et remplit la liste de chaque zone déclarée.
   **Un espace ordinaire ne ferme pas la zone ouverte** (seule une cote déclare) :
   c'est la seule règle qui ne perde aucune icône.
2. `config::zones_a_ecrire` **reconstruit** les listes depuis l'ordre vivant, puis
   **garde** les identités que cette machine ne sait pas résoudre
   (`ui::zones::remplir_les_listes`). Sans la reconstruction, un glisser d'icône
   serait défait au lancement suivant — les listes des zones font autorité.
3. `app::run` : la migration tourne **après `appliquer_les_zones`** des attributs
   (c'est elle qui pose les COTES, donc ce qui fait exister les zones), écrit
   `zones.toml` **v3**, et passe par `finaliser_la_migration` quand c'est un
   premier lancement.
4. `ZONES_VERSION` = **3**. Un fichier `version = 2` est lu tel quel puis complété
   une fois — la montée de version n'a pas de branche à elle.

**DEUX DÉFAUTS DE L'ÉTAPE 3, TROUVÉS EN ÉCRIVANT CELLE-CI (corrigés)** :

- ⛔ **La reconstruction ne citait PAS les séparateurs.** Une barre reconstruite
  depuis ses listes perdait tous ses espaces, donc **toutes ses zones** : un
  premier lancement après migration aurait effacé la disposition de la machine.
  `Dock::ordres_des_zones` émet maintenant **chaque rang**, déclaré ou non.
- ⛔ **Une barre sans zone était remplacée par une liste vide**, donc toutes ses
  icônes reversées dans la barre de gauche. Mesuré sur la machine : la barre de
  GAUCHE n'a aucune zone, la droite en a quatre — dès que les listes de la droite
  existent, la gauche se vidait. `Dock::appliquer_les_zones` ne remplace que les
  barres dont une zone est déclarée.

**LES DEUX BARRES QUI ATTENDENT** : `bar_repartie` vaut pour la GAUCHE et la
DROITE seulement. Le HAUT est encore reversé au MILIEU par
`Dock::appliquer_les_zones` (voir la section « la zone du milieu » ci-dessous) :
y écrire une liste déplacerait ses icônes d'une moitié à l'autre. Le BAS est
piloté par l'arrangement (`dossiers.toml`). Ces deux barres gardent donc
`icon_order.txt` — **d'où la décision, le même jour, de continuer à le LIRE pour
elles** (il n'est plus la référence des barres réparties).

**TROIS DÉFAUTS DE PLUS, TROUVÉS AU PREMIER LANCEMENT RÉEL (2026-10-30)** — aucun test
ne pouvait les voir, parce qu'ils vivent dans ce que la sonde ne reproduisait pas :

- ⛔ **LES ÉPINGLES DU DOCK N'AVAIENT AUCUNE IDENTITÉ.** `Dock::identite` nommait le
  noyau, les épingles de la barre des tâches, les dossiers, le miroir et les applications
  ouvertes — **pas** les `config::PinnedApp` (indices `PINNED_ICON_BASE..`). Mesuré : la
  zone « Logiciels » est sortie **VIDE** de `zones.toml` (33 applications), et ses icônes
  seraient devenues des orphelines au lancement suivant. Corrigé :
  `epingle_dock:<cible>`, posée par `app::cle_d_epingle_dock` — une SEULE définition,
  partagée avec la sonde.
- ⛔ **`appliquer_les_zones` TOURNAIT AVANT `apply_topbar_features`**, qui pose la table
  des noms du noyau. Au deuxième lancement, 37 identités (`Intégrés` 26 + `Réglages` 11)
  étaient donc irrésolubles et tombaient dans la barre de **GAUCHE** : droite à **38**
  icônes au lieu de 75. Appel **déplacé après** la pose des noms.
- ⛔ **LA SONDE CACHAIT LE PREMIER** : elle posait des noms synthétiques sur TOUTES les
  icônes, y compris les épingles — elle disait donc `0 sans identité` quand l'application
  en avait 33. Elle pose maintenant les **vraies** clés des épingles (fonction de
  production) et **exige** `sans_identite == 0`.

**LE FILET, ET C'EST LE PLANCHER DE L'ÉTAPE 1** : `repartir_les_icones` compte les icônes
qu'elle ne sait pas nommer (`sans_identite`), et **on n'écrit RIEN quand ce compte n'est
pas nul** — ni la migration, ni la sauvegarde (`remplir_les_listes`). Une icône non
nommable ne peut être citée par aucune zone : l'écrire la retirerait de la disposition.

**LA MESURE, SUR LA CONFIGURATION RÉELLE** (lecture seule, rien n'est écrit) :

```
cargo test --bins ui::zones::tests::the_real_config_gains -- --ignored --nocapture
```

Elle rend : `barres sans zone : [Gauche]`, **70 icônes rangées**, 0 d'exécution,
0 sans identité, et la **reconstruction de la barre de droite identique position
par position** à l'ordre d'avant (`Intégrés` 26 icônes, `Logiciels` 33,
`Réglages` 11, `Notifications` vide — son séparateur est à 80, celui du rang
suivant à 296).

**ET LE LANCEMENT RÉEL, QUI EST LE SEUL JUGE** (2026-10-30) : `zones.toml` écrit en v3
au démarrage (+12 s), puis **relance** — les listes des zones pilotent seules la barre
de droite. Le journal (`[zones] ordres pris dans les listes des zones`) rend
**G 35 icônes et D 75 icônes, identiques position par position** aux deux lignes
d'`icon_order.txt` (comparaison faite ligne à ligne, pas à l'œil).

⛔ **Le test qui doit ÉCHOUER sur l'ancien code** est
`the_measured_disposition_is_repartie_into_its_zones_and_comes_back_identical` :
il répartit, puis exige que `ordres_des_zones` redonne l'ordre d'origine
séparateurs compris.

---

## LES ÉTAPES (chacune se vérifie seule)

1. **NOMMER LE NOYAU.** `Dock::identite(index) -> Option<String>` + la table des noms,
   écrite à côté des constantes d'`app.rs`. **Plancher** : un test exige un nom pour
   CHAQUE index persistable inférieur à `PINNED_ICON_BASE` ; une icône sans nom fait
   échouer le test au lieu de disparaître en silence.
2. **LES ZONES PORTENT LEUR LISTE.** `ui/zones.rs` : chaque `Zone` gagne
   `icones: Vec<String>`. Test pur : lecture/écriture d'un `zones.toml` v3, la
   hiérarchie revient identique.
3. **APPLIQUER.** Le dock construit ses quatre ordres **depuis les zones**, au
   démarrage : pour chaque barre, ses zones dans l'ordre, et dans chaque zone ses
   icônes résolues en positions. Les icônes non citées suivent la règle des
   orphelines (aujourd'hui : reversées à gauche). Aucune page n'apprend les
   identités — le dock reste seul à détenir les positions.
4. **LA MIGRATION.** ✅ *Livrée le 2026-10-29 — voir la section détaillée ci-dessus.*
   À la lecture, un `zones.toml` sans `icones` est converti avec le couple
   `zones.toml` (séparateurs) + l'ordre VIVANT : on parcourt la barre, on coupe
   aux séparateurs, et chaque plage devient la liste de sa zone. `icon_order.txt`
   n'est plus la référence des barres réparties (gauche et droite) ; il le reste
   pour le haut et le bas, qui n'ont pas encore de zones (décision de
   l'utilisateur). Test : la disposition d'avant la migration est identique à
   celle d'après, position par position.
5. **LE PORTABLE.** `zones.toml` (v3) part seul. Mesure de recette : sur une machine
   où manquent un applet et 24 épingles, « Intégrés » et « Réglages » gardent leurs
   icônes, leur ordre et leurs colonnes.

---

## L'ÉTAPE 5 — MESURE PARTIELLE DU PORTABLE (2026-10-30)

**Ce qui a été mesuré, sur cette machine** : `icon_order.txt` écarté, l'application
relancée — c'est la situation de la machine d'arrivée, où ce fichier n'existe pas.

**Résultat, et il est bon pour ce que le plan promet** : la barre de droite est
reconstruite **par les seules listes des zones** — 75 entrées, **5 séparateurs**
aux bons rangs, et les **70 icônes dans l'ordre exact** des listes (26 `Intégrés`,
33 `Logiciels`, 11 `Réglages`, `Notifications` vide). `zones.toml` n'a pas bougé
d'un octet (même empreinte SHA-256 avant/après).

**LA GAUCHE VOYAGE AUSSI — DEPUIS LE 2026-10-30.** Une barre qui ne déclare AUCUNE zone
ne pouvait rien porter : une zone est le seul endroit où le modèle range des identités, et
une barre sans séparateur ne peut pas déclarer de zone (c'est la cote d'un espace qui
déclare). Elle a donc **sa liste à elle**, `[bars.G] icones`, dans la table de barre qui
existait déjà :

```toml
[bars.G]
spacers = 2
icones = ["espace:G1", "noyau:logo", "epingle:exe:explorer.exe", "espace:G2", …]
```

⛔ **LES ESPACES Y SONT CITÉS PAR LEUR RANG** (`espace:G1`) : un espace n'a pas d'autre
identité, et sans ces marqueurs la reconstruction les perdrait TOUS — `spacers` s'éroderait
à la première sauvegarde et les points de la barre disparaîtraient. Le rang EST l'identité
d'un espace dans ce modèle.

**MESURÉ, deux lancements réels** (`icon_order.txt` écarté puis remis, instrumenté : `G`/`D`/`HG`/`HD`/`HM` au journal) :

| barre | avec `icon_order.txt` | sans — et ce qui la décrit |
|---|---|---|
| **Droite** | 75 icônes | **75, identiques** — ses quatre zones `D1..D4` |
| **Gauche** | 35 | **les mêmes 35, même ordre visible** — sa liste `[bars.G] icones` |
| **Haut gauche** | `[168]` | **`[114]`** — un espace, au rang 1 |
| **Haut droite** | 13 icônes | **les 4 mêmes icônes** (`alimentation, son, voicemeeter, reseau`) + 9 espaces recréés |
| **Haut milieu** | `[]` | `[]` |

⇒ **Les trois barres voyagent.** Les index changent (les espaces sont recréés), les
IDENTITÉS et leur ordre non.

### Le haut : trois moitiés, trois listes

Capture de l'utilisateur (2026-10-30) : **gauche** = NeuraDesk, pastilles, média, titre ;
**milieu** = l'horloge, centrée ; **droite** = NUM/MAJ/144 Hz/CPU/RAM/réseau/volume/
alimentation. Les trois régions existaient déjà à l'écran (`Side::TopLeft`/`TopMiddle`/
`TopRight`, le milieu ancré au centre) — ce qui manquait était de les DÉCRIRE. Une seule
liste ne peut pas dire laquelle, donc `[bars.H]` en porte trois :

```toml
[bars.H]
spacers = 10
icones        = ["espace:H1"]                       # la moitié GAUCHE
icones_milieu = []                                  # le MILIEU (l'horloge est un widget)
icones_droite = ["noyau:alimentation", "noyau:son", "noyau:voicemeeter", "noyau:reseau",
                 "espace:H2", "espace:H3", …]       # la moitié DROITE
```

⚠️ **LE MILIEU N'A PAS DE RANG** : `BarLetter::halves()` ne compte que les deux moitiés, et
un séparateur y serait irrésoluble — il n'en a pas non plus (c'est une rangée, centrée).

**ET C'EST UNE CIBLE DE GLISSER** (2026-10-30 : « la zone milieu doit être utilisée »).
`Dock::zone_at` rendait `TopLeft` ou `TopRight` à la moitié de l'écran : **rien ne pouvait
jamais atterrir au milieu**, donc `icones_milieu` restait vide par construction. Il rend
maintenant `TopMiddle` dans la bande de l'horloge ET de sa garniture (note, cloche,
minuteur), élargie d'une icône. Sans horloge connue, le partage à la moitié reprend, intact.

⛔ **ET ELLE ÉTAIT PERSISTÉE SANS ÊTRE DESSINÉE** — mesuré le 2026-10-30 par l'utilisateur :
« en essayant de déplacer l'applet réseau pour tester elle a disparu ». `Side::TopMiddle`
existait, `layout_zone` savait la disposer, `icones_milieu` savait l'écrire — mais
**`Self::ZONES`, la liste des barres dont on calcule les emplacements, ne la contenait
pas**. Aucun rectangle n'était donc jamais calculé : l'icône vivait dans le fichier et
nulle part à l'écran. Le test qui le garde, `an_icon_in_the_middle_gets_a_rectangle`,
échoue sur l'ancien code.

⛔ **ET RIEN NE SE PERD EN CHEMIN** : la dépose remplit `top_middle_order` ; la sauvegarde la
retrouve dans `[bars.H] icones_milieu` (`remplir_les_listes`, qui reconstruit les trois
moitiés) ; une barre du haut SANS séparateur s'écrit quand même, puisque sa liste dit
quelque chose (`Dock::zones` et `zones_to_toml` ne sautent plus une barre à zéro espace).
Et aucune entrée de barre VIDE n'est créée : l'écrivain ne la produirait pas, et
`save_zones` compare l'écrit au relu — la sauvegarde échouerait en silence.

⚠️ **LES ICÔNES DU MILIEU SONT RETIRÉES DES AUTRES LISTES** avant `set_icon_order` : au
démarrage celles d'`icon_order.txt` sont orphelines, donc reversées à gauche, et
`poser_l_ordre` marque le milieu « vu » sans les retirer — elles seraient dessinées deux fois.

**CE QU'EST LE MILIEU, dit par l'utilisateur le 2026-10-30** : « l'horloge et les applets qui
sont autour de l'horloge ». Donc :
- **L'horloge elle-même n'est PAS dans la liste** : c'est un WIDGET de `ui::topbar`, toujours
  centré, identique sur toutes les machines — elle n'a rien à transporter.
- `icones_milieu` porte **les applets autour d'elle**.
- ⛔ **ET ELLES SE POSENT À GAUCHE DE L'HORLOGE**, pas dessus : la rangée était ancrée au
  centre de l'écran, qui est exactement le centre de l'horloge. `Dock::clock_width` (posée à
  chaque frame par `app::run` depuis la texture de l'heure) permet à `layout_zone(TopMiddle)`
  de poser la rangée par son bord DROIT, juste avant ce qui précède déjà l'horloge — l'icône
  de note (`topbar::note_rect`). Sans horloge connue (`0.0`, première frame), le centrage
  d'avant reprend, intact.

⚠️ **CE QUI CHANGE QUAND MÊME, ET QU'IL FAUT SAVOIR** : les places d'épingle RÉSERVÉES
(masquées, sans identité) ne sont pas citées par la liste ; elles redeviennent des
orphelines et `set_icon_order` les reverse à la FIN de la barre. Invisible aujourd'hui —
mais une place qui se réveillerait (épingle posée à chaud) apparaîtrait à la fin de la
gauche plutôt qu'à son emplacement d'origine.

⛔ **UN DÉFAUT DE PLUS, TROUVÉ PAR CETTE MESURE** : sans `icon_order.txt`, les épingles de la
barre des tâches n'étaient **pas nommées du tout** (le nommage vivait dans la seule branche
« ordre enregistré »). Toutes les identités de la gauche sont des `epingle:…` : elle ne se
reconstruisait donc pas, alors que la droite revenait intacte — défaut PARTIEL, donc
trompeur. Corrigé : `poser_cles_d_epingles` est appelé dans les deux branches.

**LA LISTE DE LA BARRE, MÊME RÈGLE QUE CELLE DES ZONES EN SAUVEGARDE** : reconstruite depuis
l'ordre vivant, augmentée des identités inconnues d'ici, et **gardée telle quelle** si une
icône visible n'est pas nommable (`remplir_les_listes`). ⚠️ Un **placeholder masqué** n'est
pas un trou : il est ignoré, sinon les cinq places réservées de la configuration réelle
feraient refuser toute écriture.


- ⛔ **EFFET DE BORD MESURÉ, ET RÉPARÉ À LA MAIN** : un lancement SANS
  `icon_order.txt` **réécrit `winpin_base.txt`** (92 → 80 sur cette machine),
  parce que la base se déduit des icônes présentes. Or c'est elle qui fixe où
  commencent les remplisseurs : l'index 80 a cessé d'être un espace, la barre de
  droite a perdu son séparateur de rang 4 (reparti en orphelin à gauche), et
  `compléter_les_barres` a fabriqué un espace neuf à la fin. **Deux réglages
  retrouvés à l'identique après remise de `winpin_base.txt` = 92** (G 35 / D 75,
  vérifiés position par position). ⇒ Perdre `icon_order.txt` sur une
  installation EXISTANTE déplace les rangs, donc les zones. À traiter avant que
  le portable ne remplace quoi que ce soit.

**CE QUI RESTE À FAIRE POUR L'ÉTAPE 5** : la recette sur la VM (un applet et
24 épingles en moins) — elle demande la machine. La disposition, elle, voyage
désormais des trois côtés.

---

**LA RECETTE, PREMIÈRE MOITIÉ — MESURÉE LE 2026-10-30, SANS LA VM** :

```
cargo test --bins ui::zones::tests::the_recipe -- --ignored --nocapture
```

`the_recipe_on_a_machine_missing_twenty_four_pins` est un **`run()` sans interface** : les
noms du noyau viennent d'`app::apply_topbar_features` (la VRAIE table), les clés des
épingles d'`app::cle_d_epingle_dock`, et les listes de `zones.toml` de la machine. Le dock
d'arrivée ne nomme que **neuf** épingles au lieu de trente-trois.

| Zone | Avant | Après (24 épingles en moins) |
|---|---|---|
| `D1` Intégrés | 26 icônes | **26, identiques** (identités ET ordre) |
| `D2` Logiciels | 33 icônes | **9** — exactement les épingles présentes, dans leur ordre |
| `D3` Réglages | 11 icônes | **11, identiques** |
| `D4` Notifications | vide | vide |

Et les quatre zones **gardent leur cote et leurs colonnes** (`zone_height`,
`zone_columns` comparés au fichier). C'est la promesse du plan, tenue.

**La moitié qui manque** : l'applet absent (le binaire de la VM a une icône de moins, donc
tous les index qui suivent glissent). Ça ne se simule pas ici — il faut la VM.

---

## CE QU'IL NE FAUT PAS FAIRE

- ⛔ **Ne pas persister les applications ouvertes ni le miroir de notification** :
  seule leur ZONE est écrite. `ui/dock.rs:2541` documente ce qui arrive sinon —
  « la zone de notification est vide ».
- ⛔ **Ne pas convertir les identités en indices dans les pages** : le dock détient
  les positions, lui seul.
- ⛔ **Ne pas faire d'une zone une plage contiguë** : c'est exactement le modèle
  qu'on quitte. Une zone doit pouvoir être vide et rester à sa place.
- ⛔ **Ne pas réutiliser le rang d'une zone supprimée** (`ZoneKey { bar, rank }`
  existe pour ça — voir le décalage fantôme documenté).
- ⛔ **Ne pas nommer un noyau par son index** (`noyau:17`) : c'est la faute déguisée.

---

## LA ZONE DU MILIEU DU HAUT — MESURE DU 2026-10-29, ET POURQUOI ELLE ATTEND

**Ce qui existe** : `layout_top_side(order, layout, anchor_x, grow_right, map)`
appelé DEUX fois (`ui/dock.rs:6821` et `:6824`) — une moitié ancrée à
`TOP_ZONE_CORNER_MARGIN` qui pousse vers la droite, une ancrée à
`total_width - TOP_ZONE_CORNER_MARGIN` qui pousse vers la gauche. **Le milieu
n'existe pas** : c'est le vide entre les deux.

**L'OBSTACLE, MESURÉ** : il n'y a pas de `Side::TopMiddle`. `Side` a cinq
variantes (`Left`, `Right`, `TopLeft`, `TopRight`, `Bottom`), et `Side::TopRight`
apparaît **50 fois dans trois fichiers** (`ui/dock.rs` 36, `app.rs`, `ui/zones.rs`).
Le dessin d'une icône reçoit son `Side` — marges, écartement, survol — donc une
icône du milieu n'aurait rien à recevoir.

**DEUX FORMES, ET LAQUELLE PRENDRE** :

1. **Une variante `Side::TopMiddle`** (recommandée) : ~50 sites à toucher,
   mécaniques mais dispersés ; le milieu a SES marges et SON écartement, comme
   une barre à part entière. ⚠️ C'est le seul morceau du chantier qui change la
   GÉOMÉTRIE VISIBLE — à faire avec la possibilité de regarder la barre.
2. **Réutiliser `TopLeft` pour le milieu** : coût nul, mais le milieu hériterait
   des règles de la moitié gauche (écartement, survol, ancrage). ⛔ C'est le faux
   silence que ce plan interdit ailleurs.

**CE QUE LA ZONE DU MILIEU DOIT TENIR**, quand elle sera faite :
- **ancrée au CENTRE** (`total_width / 2`), et non à un bord ;
- les deux moitiés **ne déborderont pas dessus** — aujourd'hui rien ne les borne,
  elles se rencontrent au milieu et se chevaucheraient ;
- `zones.toml` la désigne par `Haut` **rang 2** (rang 1 = gauche, rang 3 = droite),
  ce qui ne demande aucun champ nouveau : le rang EST l'ordre.

---

## CE QUI RESTE OUVERT

- **Les trois zones du HAUT, et la barre du BAS** : aucune liste n'est répartie
  pour elles tant que le haut est reversé au milieu et que le bas suit
  `dossiers.toml`. Leurs barres gardent `icon_order.txt`. Côté dessin, le haut est
  découpé en gauche/milieu/droite par `layout_top_side` et la barre du bas
  « occupe le milieu » (`win32/window.rs:3139`) : il faudra vérifier que cette
  découpe se décrit bien comme trois zones, et non comme deux côtés plus un fond.
- **Un dossier dont le chemin change** perd son identité (`dossier:<chemin>`).
- **Deux profils du même navigateur** : `profile:<id>` le couvre, jamais vérifié sur
  une vraie paire.
