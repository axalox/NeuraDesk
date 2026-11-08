# tools/ — les outils de données de la table des fêtes

Deux scripts Node, **hors chaîne de compilation** : ils ne tournent jamais au
build, la table qu'ils produisent est **versionnée** (`src/win32/fetes_france.tsv`)
et embarquée dans le binaire par `include_str!`. Ils existent pour qu'une
régénération soit **reproductible et auditable**, pas pour être exécutés.

## `reconstruire_fetes.js` — re-dérive la table

```bash
node tools/reconstruire_fetes.js [sheet2.xml] [fetes.csv] [sortie.tsv] [reference.json]
```

Défauts : l'archive xlsx extraite et le CSV de l'utilisateur, tous deux **hors du
dépôt** (chemins propres à cette machine) ; la sortie est
`src/win32/fetes_france.tsv` ; la référence est
`tools/calendrier_2026.reference.json`.

⚠️ **Ce dernier est VERSIONNÉ, et il le faut** : sans lui le script retomberait
*silencieusement* sur le xlsx seul (105 ancres de moins) et produirait une table
**différente** — précisément la dérive d'octobre qui a été corrigée.

⭐ **DEUX SOURCES DE DATES servent d'ancres** : le xlsx (237 entrées) et
calendrier.com (105 ancres de plus, qu'il est seul à connaître). Elles
**s'accordent 227 fois sur 228** ; le seul désaccord est **Catherine**
(xlsx 24/11, référence 25/11) et **c'est la référence qui a raison** — la
tradition française la met au 25, et l'ordre du CSV le soutient.

Effet mesuré du second jeu d'ancres : **340 → 346 accords** sur les 365 jours
de la référence, **6 écarts corrigés, aucun nouveau**. Sans lui, octobre dérive
d'un jour sur `12-14` et `23-25` : l'interpolation placait la « collision » de
façon uniforme entre deux ancres, donc au début de l'intervalle, au lieu de là
où le CSV a réellement un doublon.

Le script **reproduit la table versionnée octet pour octet**.

⚠️ **LE MODÈLE, ET IL A COÛTÉ CHER À TROUVER** : le CSV est un calendrier
**un-saint-par-jour**. Dans chaque mois les saints sont dans le **bon ordre
chronologique** (mesure : **2 inversions sur 238** entrées que le xlsx sait
dater), mais les **jours ne sont que la position dans la liste**. Une **intruse**
— un saint qui vit aussi dans son vrai mois (Grégoire au 3/1 alors qu'il est du
3/9) — consomme un créneau et **décale tout ce qui suit de +1**. Les dates du
xlsx servent d'**ancres**, ce qui rend le réalignement déterministe.

⛔ **Le CSV est en `JJ/MM` et le `.tsv` en `MM⇥JJ`** — inverser les deux a produit
deux fois une analyse entièrement fausse.

⚠️ `R(i) = i − jour(i)` **n'est pas monotone** : elle **monte** quand une intruse
a mangé un créneau, elle **descend** quand un jour réel ne porte aucun saint.
Ne jamais « corriger » une ancre qui recule.

## `extraire_reference_calendrier.js` — le contrôle par une tiers autorité

```bash
node tools/extraire_reference_calendrier.js [calendrier.pdf] [table.tsv]
```

Lit le PDF `calendrier.com` (l'année entière sur une page), en extrait les
365 jours, et les compare à la table.

⚠️ **Le PDF stocke son texte en chaînes HEXADÉCIMALES** avec une police
sous-ensemble : chaque glyphe est un CID de 2 octets, décodé par la table
`/ToUnicode` de **sa** police — il y en a trois (titres, corps, numéros de
semaine), et les fusionner donne du charabia. Les positions viennent de
`1 0 0 -1 x y Tm` puis de `n 0 Td` relatifs.

⚠️ **Deux pièges de découpage**, tous deux mesurés :
- les colonnes n'ont pas la même largeur (les titres sont centrés sur leur mot) —
  découper par tranches de `x` fait **fuir la colonne suivante dans la
  précédente** ; on découpe sur le motif **(lettre de jour)(chiffres)** ;
- sur la 2ᵉ ligne de chaque semaine, **le numéro de jour et le nom FUSIONNENT**
  (« `11Paulin` » = le 11 janvier collé à « Paulin »), l'écart étant plus petit
  que l'interlettre.

Résultat au 2026-09-24 : **340 accords sur 365**, un seul jour de la table
absent de la référence. Sur les 25 écarts, une dizaine sont des **abréviations**
de la référence (`Cath.deSuède`, `J.-M.Vianney`, `Thé.del'E.-Jésus`) qui
désignent le même saint — ne pas les prendre pour des fautes.

⭐ C'est cette référence qui a **confirmé le seul déplacement volontaire** de la
re-dérivation : Catherine au **25/11**, et non au 24 comme le dit le xlsx.

⚠️ **Ne pas « aligner » la table sur cette référence** : elle est plus pauvre que
la nôtre (voir l'en-tête de `fetes_france.tsv`). Un écart n'est pas une faute.

## `importer_saints.js` — fusionne un calendrier des saints (2026-11-04)

```bash
node tools/importer_saints.js tools/saints.json [src/win32/fetes_france.tsv] [--essai]
```

`tools/saints.json` (VERSIONNÉ, fourni par l'utilisateur le 2026-11-04) donne
**plusieurs saints par jour, déjà datés** : `{ "mois": { "jour": [noms…] } }`.
Ses lignes **s'AJOUTENT** à la table ; **aucune n'est retirée**.

⛔ **POURQUOI UNE FUSION ET PAS UN REMPLACEMENT** — mesuré avant d'écrire le
script : remplacer la table par ce fichier ferait perdre **327 noms sur 288
jours**, et **cinq des huit alarmes** de `rappels_fetes.toml` (Davy 20/09,
Aurora 04/10, Sandrine 02/04, Pierre 29/06, Julien 02/08) : le fichier ne
connaît pas ces saints-là, et les alarmes se cherchent par TRIPLET
(mois, jour, nom) — un nom qui disparaît est une alarme qui ne sonne plus, en
silence.

Ce que l'import corrige, dans le fichier source :

| défaut mesuré | traitement |
| --- | --- |
| `6/31` (« Ste-Pétronille ») | **écarté** — le fichier la porte AUSSI au 5/31 |
| 9 accents abîmés (`Benoà®t`, `Joà«l`, `Noà«l`…) | réparés (`à®`→`î`, `à«`→`ë`) ; s'il en restait, le nom serait écarté et dit |
| 15 noms à espaces doubles | repliés |
| `St-`/`Ste-` | retirés des noms IMPORTÉS — la table écrit des noms nus, la catégorie dit déjà « saint » |

Et il ne double pas : la comparaison se fait sur un nom **replié** (sans
accents, parenthèses, ponctuation ni préfixe) et par **préfixe de mots** —
« Basile » couvre « Basile le Grand », « Matthieu » couvre « Matthieu,
apôtre », « Mélaine » couvre « Melaine de Rennes ». Sur les 923 entrées du
fichier, **204 rencontrent un saint déjà connu** du jour et ne sont pas
ajoutées.

**Idempotent** : relancé, il ne trouve rien à ajouter et **n'écrit pas**. Ordre
de la chaîne :

```bash
node tools/reconstruire_fetes.js …        # re-dérive la base (si besoin)
node tools/importer_saints.js tools/saints.json
```

État au 2026-11-04 : **1275 formes stockées, 1142 affichées** (133 variantes,
inchangées), **au plus 9 fêtes sur un jour** (25/05), et **aucune alarme
perdue**.
