# Mise à jour depuis GitHub — conception et procédure de publication

Configuration : **`portable-AAAA-MM-JJ`** (le tag porte la version, jamais le nom
de l'asset), sur le dépôt <https://github.com/axalox/NeuraDesk>.

## Ce que fait l'application

1. **Vérification** — `win32::maj` interroge
   `https://api.github.com/repos/axalox/NeuraDesk/releases/latest` une fois au
   démarrage (après 20 s, au plus une fois toutes les six heures), et à
   l'ouverture de la fenêtre « À propos » (anti-rebond de cinq minutes). Une
   vérification manuelle — bouton de la page, entrée « Rechercher les mises à
   jour » du menu du clic droit — ignore l'anti-rebond.
2. **Proposition** — si le tag distant est plus récent que
   `NEURADESK_PUBLICATION`, la fenêtre « À propos » affiche la version
   disponible et un bouton d'installation ; un toast Windows le signale une
   seule fois par version (le toast est déposé par le fil principal, WinRT
   exigeant l'appartement COM qui y est initialisé).
3. **Téléchargement** — l'asset fixe `NeuraDesk-portable.zip` est écrit en flux
   dans `%LOCALAPPDATA%\NeuraDesk\maj\`, avec progression. Si la release publie
   aussi `NeuraDesk-portable.zip.sha256` (ou `SHA256SUMS`), l'empreinte est
   calculée pendant l'écriture et l'archive est refusée en cas d'écart.
4. **Préparation** — l'archive est dépliée dans un dossier **frère** du dossier
   de l'application : `<dossier>/.<nom>.maj-<horodatage>`. Cela se fait pendant
   que l'application tourne : aucun fichier en cours d'exécution n'est touché, et
   l'installation finale ne dure qu'une seconde ou deux.
5. **Installation** — quand l'utilisateur clique « Redémarrer et installer »,
   l'application se ferme. Un **assistant** (une copie de l'exécutable, lancée
   avec `CREATE_BREAKAWAY_FROM_JOB` pour survivre au Job Object) attend la mort
   du parent, renomme `<dossier>` en `<dossier>.ancien-<horodatage>`, met la
   préparation à sa place, relance l'application, vérifie qu'elle est **vivante
   après dix secondes**, puis supprime l'ancien dossier. Si elle ne démarre pas,
   il remet l'ancienne version en place et la relance.

## Version locale

`build.rs` émet `NEURADESK_PUBLICATION = git describe --tags --abbrev=0`.
⚠️ **Le tag doit exister AVANT le build final, puis `touch build.rs`** : le tag
ne change ni `.git/HEAD` ni `.git/logs/HEAD`, et le script ne se relancerait
pas. Sinon « À propos » affiche le tag précédent, et l'application proposera une
mise à jour vers elle-même.

Deux tags de lignées différentes ne se comparent jamais : seuls les tags
`portable-…` déclenchent quelque chose. `master-portable-…` et
`neurodesk-portable-…` sont ignorés (ce sont d'autres applications).

## Procédure de publication (inchangée, plus une étape facultative)

1. Poser le tag `portable-AAAA-MM-JJ` (suffixe `b` si le jour est déjà publié —
   un tag publié ne se déplace pas, on force la référence seulement si l'asset
   est remplacé).
2. `touch build.rs`, construire le portable, assembler et zipper comme
   d'habitude.
3. Créer la release GitHub avec l'asset **`NeuraDesk-portable.zip`**.
4. **Facultatif mais recommandé** : publier en plus
   `NeuraDesk-portable.zip.sha256` (le hash SHA-256 en hexadécimal, suivi du nom
   du fichier). Sans cet asset, l'application accepte l'archive sur la seule foi
   de HTTPS et de l'origine vérifiée.

## Garanties de sécurité

- HTTPS, et l'URL de l'archive doit commencer par
  `https://github.com/axalox/NeuraDesk/releases/download/` — une réponse
  détournée ne peut pas désigner un autre hôte.
- Refus des entrées de zip absolues, avec `..`, avec lettre de lecteur ou avec
  liens symboliques (évasion de répertoire).
- Plafond de taille décompressée (4 Gio) et vérification que l'archive contient
  `neuradesk.exe` à sa racine.
- Essai de vie de dix secondes et retour en arrière automatique.

## Leviers

| Variable | Effet |
| --- | --- |
| `NEURADESK_NO_MAJ` | coupe entièrement le module (vérification et installation) |
| `NEURADESK_MAJ_DOSSIER` | remplace le dossier de l'application (essais de l'assistant) |
| `NEURADESK_CAPTURE`, `NEURADESK_CAPTURE_FRAMES` | aucune requête ni écriture de 600 Mo pendant un run de capture |

Journal de l'assistant : `%LOCALAPPDATA%\NeuraDesk\maj\maj.log` (écrit même sans
`NEURADESK_DIAG`, parce que l'assistant démarre avant `diag::init`).

## Tests

```bash
cargo test --bin neuradesk maj::tests
```

Sondes réseau `#[ignore]` (elles interrogent réellement GitHub) :

```bash
cargo test --bin neuradesk -- --ignored --nocapture interroge_la_release
cargo test --bin neuradesk -- --ignored --nocapture telecharge_un_petit
```

## Limites connues

- **La première mise à jour vers une version qui contient cette fonctionnalité
  ne peut pas être automatique** : les portables antérieurs ne connaissent pas
  `win32::maj`. Elle se fait à la main (page des releases GitHub).
- L'archive livrée est le portable ; un binaire lancé depuis un dossier non
  inscriptible (Program Files) échoue à la préparation, avec un message explicite.
- **Espace disque** : pendant l'installation, l'ancien et le nouveau dossier
  coexistent (~1,3 Go pour un portable de 626 Mo décompressés) ; le
  téléchargement ajoute ~277 Mo dans `%LOCALAPPDATA%\NeuraDesk\maj\`, supprimés
  dès l'extraction terminée.
- L'assistant est une **copie de l'exécutable** dans `%LOCALAPPDATA%` (Windows
  verrouille l'image d'un exe en cours). Un antivirus peut la signaler ; le
  téléchargement manuel depuis la page des releases reste le recours.
- Aucune fenêtre de progression n'est affichée pendant l'échange des dossiers
  (l'application est fermée) ; la préparation, elle, est visible dans « À
  propos ». L'assistant ne montre une boîte de message qu'en cas d'échec.
