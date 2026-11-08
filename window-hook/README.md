# window-hook — la voie B, étape 1 : OBSERVER

Crate **indépendante** (table `[workspace]` vide, comme `shell-hook/`) : rien
d'elle n'entre dans le binaire de NeuraDesk.

```bash
CARGO_TARGET_DIR=C:/ndwork/wh-target cargo build --release
```

produit :

- `neuradesk-windowhook.dll` (`window_hook.dll`) — le module injecté ;
- `window-hook-inject` — l'injecteur.

## Usage

```bash
window-hook-inject <exe|pid> <chemin de la DLL>
window-hook-inject stop
```

`stop` écrit `C:\ndtarget\window-hook.stop` : la DLL défait ses sous-classes
depuis son propre fil, puis se décharge (`FreeLibraryAndExitThread`). ⛔ Ne
jamais la décharger autrement — une fenêtre qui pointerait sur du code libéré
tuerait l'application à la première souris.

Journal : `C:\ndtarget\window-hook-<pid>.log`.

## ⛔ Ce que cette étape NE fait PAS

**Elle ne retient aucun message.** Elle sous-classe les fenêtres de premier
niveau et journalise ; elle transmet toujours au proc d'origine. Aucune
application n'est modifiée dans son fonctionnement.

## Mesure — 2026-11-04, `charmap.exe` (pid 13932, classe `#32770`)

```
=== veille démarrée — pid 13932 ===
0x6c91498 #32770  sous-classée
0x6c91498 #32770  WM_SYSCOMMAND            SC=0xf020 (visible=true)   ← réduire
0x6c91498 #32770  WM_WINDOWPOSCHANGING      (visible=true)
0x6c91498 #32770  WM_SYSCOMMAND            SC=0xf120 (visible=true)   ← restaurer
0x6c91498 #32770  WM_WINDOWPOSCHANGING      (visible=true)
0x6c91498 #32770  WM_WINDOWPOSCHANGING      (visible=true)
0x6c91498 #32770  WM_CLOSE                  (visible=true)             ← fermer
0x6c91498 #32770  WM_WINDOWPOSCHANGING      (visible=true)
0x6c91498 #32770  WM_DESTROY                (visible=false)
0x6c91498 #32770  WM_NCDESTROY              (visible=false)
```

**Ce que ça prouve, et pourquoi c'est la bonne brique :**

1. l'injection dans une application quelconque passe (`LoadLibraryW` rendu non
   nul), sans antivirus en face sur cette machine ;
2. le sous-classement atteint la fenêtre de l'application **et reçoit les trois
   gestes** qui nous intéressent, AVANT l'opération ;
3. la fermeture reste propre : `WM_DESTROY` puis `WM_NCDESTROY` arrivent
   normalement, le proc d'origine est bien appelé.

**Ce que ça dit pour l'étape 2 :** `WM_SYSCOMMAND` (`SC_MINIMIZE` / `SC_CLOSE`)
et `WM_CLOSE` sont **retenables** — on peut ne pas les transmettre, jouer
l'effet, puis les renvoyer. `WM_SHOWWINDOW` (le lancement) ne se refuse pas :
il faudra cacher la fenêtre juste après, depuis le processus, ce qui est déjà
beaucoup plus rapide que depuis l'extérieur.

## Étape 2 — RETENIR, ET PRÉVENIR NEURADESK

Contrôlée par un fichier, **absent par défaut** (la DLL reste un observateur) :

```
C:\ndtarget\window-hook.hold     → un nombre, la retenue en millisecondes (0–5000)
```

Un seul geste est retenu pour l'instant : `WM_CLOSE`. `WM_SYSCOMMAND SC_MINIMIZE`
est reconnu mais `SC_CLOSE` est **exclu** — la croix envoie `WM_CLOSE`, et retenir
le menu système est le chemin le plus sûr de bloquer une application.

**MESURÉ (2026-11-04, `charmap.exe`, retenue 800 ms)** :

```
0x14212ac #32770  WM_CLOSE                  (visible=true)
0x14212ac RETENU 800 ms
0x14212ac LIBÉRÉ après 800 ms : 0x0010
0x14212ac #32770  WM_CLOSE                  (visible=true) ← LA NÔTRE
0x14212ac #32770  WM_DESTROY                (visible=false)
0x14212ac #32770  WM_NCDESTROY              (visible=false)
```

et côté NeuraDesk, **au moment de la retenue** :

```
[    82.385] [geste] application 0x14212ac, geste 1, retenue 800 ms
```

### Le protocole

`RegisterWindowMessageW("NeuraDesk.GesteFenetre")`, des DEUX côtés par le même
NOM — jamais un `WM_APP + n` en dur, qui entrerait en collision avec une autre
application. `wParam` = la fenêtre ; `lParam` = `geste | (ms << 8)` ; gestes
`1` fermer, `2` réduire. Rien à sérialiser, donc **aucun `WM_COPYDATA`** : le
fil de l'application n'attend jamais NeuraDesk.

⛔ La fenêtre visée est **`NeuraDeskDockWindow`**, comparée EXACTEMENT : le
processus porte des dizaines de fenêtres `NeuraDesk…` (`CaptureWindow`,
`TooltipWindow`, `Cef…`) et la première trouvée par `EnumWindows` n'a pas le
proc qui traite ce message.

### Ce qui reste à faire

**Fait le 2026-11-04 — les deux gestes sont animés par NeuraDesk.**

`WM_CLOSE`, `SC_MINIMIZE`, **`SC_MAXIMIZE`** et **`SC_RESTORE`** sont retenus (la
DLL les retenait déjà tous les deux ; NeuraDesk ignorait le second). `geste_retenu`
les transmet tous les deux, et c'est `Fermetures::animer_geste` qui capture, vide
la région, joue le retour et rend la région :

| geste | code | quand la région est rendue |
|---|---|---|
| fermer | 1 | `durée + 400 ms` après le début de l'effet, si la fenêtre vit encore (boîte « Enregistrer ? ») |
| réduire | 2 | **dès `IsIconic`**, sondé toutes les 15 ms (plafond 3 s) |
| agrandir | 3 | **dès `IsZoomed`** |
| restaurer | 4 | **dès `!IsZoomed && !IsIconic`** |

⛔ Pour « réduire », rendre la région à l'heure de l'effet ferait réapparaître
la fenêtre EN GRAND pendant les dernières images (les plus petites), le temps
que l'application traite `SC_MINIMIZE` — d'où l'attente d'`IsIconic`.

⭐ **AGRANDIR ET RESTAURER NE SE FERMENT PAS, ILS CHANGENT DE RECTANGLE** :
l'effet dure `retenue + 150 ms` (et non `retenue - 60`), pour que l'image
couvre la libération du message ; elle garde sa taille finale pendant que la
vraie fenêtre prend la sienne, puis se retire — sans trou de bureau.
Le rectangle d'arrivée est `rcWork` du moniteur (agrandir) et
`rcNormalPosition` de `GetWindowPlacement` (restaurer).
⛔ La DLL ne retient PAS le geste s'il ne changerait rien (`SC_MAXIMIZE` sur une
fenêtre déjà agrandie, `SC_RESTORE` sur une fenêtre réduite — sa capture est
vide et son retour vient de la barre des tâches).

⛔ **LE GESTE ANIMÉ RETIRE LA FENÊTRE DE LA LISTE SUIVIE** (`Fermetures::oublier`).
MESURÉ (2026-11-05) : sans ça, une fermeture retenue jouait DEUX effets — le
nôtre, puis, quand l'application applique enfin `WM_CLOSE`, `rappel_masquage`
signalait sa disparition et `masquee` rejouait un retour avec la dernière image
suivie, 1,5 s plus tard.

**MESURÉ (2026-11-04, `charmap.exe`, fenêtre placée à 600×300, retenue 1200 ms,
photos à ~30 ms)** :

- fermeture — `[geste] application 0x2af1624, geste 1, retenue 1200 ms` puis
  `[fermeture] 0x2af1624 fermeture retenue 1200 ms : capture et retour joués`
  (29 ms entre les deux) ; les photos montrent l'image **exactement à la place
  de la fenêtre** (aucun trou), puis son vol vers l'icône jusqu'à ~1,06 s ;
- réduction — même chaîne avec `geste 2` / `réduction`, la fenêtre devenant
  `IsIconic` ; une seule image (~30–40 ms) sépare la disparition de la fenêtre
  de la première image de l'effet.

⚠️ Le mod Windhawk `windows-animations` anime déjà les fermetures sur cette
machine : les deux jouent en même temps. Pour comparer, le désactiver.

## L'injection AUTOMATIQUE (NeuraDesk)

`src/win32/injection.rs` charge `window_hook.dll` dans les applications qui
ouvrent une fenêtre animable, une fois par processus, depuis un fil à part
(réveillé par un `Condvar` — pas de sondage). Case des Réglages :
**Ouverture des fenêtres > Animer aussi les fenêtres des autres applications**,
`effet_injection.txt`, ⛔ **décochée par défaut**.

- le pid est déposé par le crochet `EVENT_OBJECT_CREATE` existant
  (`ouverture_fenetres::rappel`), filtré par `candidate` ;
- ⛔ exclus : les hôtes de l'interface Windows et ses composants (`explorer`,
  `dwm`, `winlogon`, `prevhost`, `dllhost`, `rundll32`…). **`prevhost.exe` a été
  mesuré dans les cibles** : au démarrage, il crée une fenêtre d'aperçu
  « animable » ;
- ⛔ un échec est DÉFINITIF pour ce pid (processus 32 bits, protégé, élevé) :
  il est noté AVANT l'essai, sinon chaque fenêtre relancerait la tentative ;
- sans `window_hook.dll` à côté de l'exe, la case ne fait rien et le journal
  le dit (une ligne) ;
- ⛔ **ON N'INJECTE PAS LE FICHIER DE L'EXE, MAIS UNE COPIE PAR EMPREINTE**
  (`%LOCALAPPDATA%\NeuraDesk\window-hook\window_hook-<empreinte>.dll`). MESURÉ
  (2026-11-05) : une DLL chargée est VERROUILLÉE — `cargo build` ne peut plus
  remplacer celle de l'exe (« Device or resource busy ») et la copie de
  `build.rs` échouait en silence, ce qui a fait tester un module d'hier. Une
  nouvelle version prend un nouveau nom ; les applications déjà injectées
  gardent l'ancienne, sans conséquence. `build.rs` sait en plus RENOMMAGER une
  DLL verrouillée (le renommage reste permis, lui) avant d'écrire la neuve.

**La retenue** n'est plus lue dans `C:\ndtarget` : NeuraDesk écrit
`%APPDATA%\NeuraDesk\window-hook.hold` = durée de fermeture + 120 ms (150-5000).
`C:\ndtarget\window-hook.hold` reste en repli pour les harnais.

**MESURÉ (2026-11-04)** — `charmap.exe` lancé à la main, AUCUNE commande
d'injection :

```
[ 17.374] [injection] module chargé dans 56740
...
[52170281] 0x4340e96 LIBÉRÉ après 1620 ms : 0x0010
[    29.532] [geste] application 0x4340e96, geste 1, retenue 1620 ms
[    29.551] [fermeture] 0x4340e96 fermeture retenue 1620 ms : capture et retour joués
```

## La forme de l'effet

Le geste retenu joue l'effet choisi dans Réglages > **Ouverture des fenêtres** >
**Forme du retour à la fermeture** (`src/win32/ouverture_fenetres/effets.rs`) :
vers l'icône, extinction (tube cathodique), fondu, chute, zoom, **éclatement**,
**génie**. Le retour vers l'icône est le défaut ; `effet_fermeture_style.txt`
(0-7, le 5 est réservé à `Etirer`) le range.

⛔ Un effet est une fonction pure de `t` : il rend une ou plusieurs `Partie`
(pose, sous-rectangle de texture `uv`, rayon des coins). Les effets d'un seul
quadrilatère en rendent une ; **éclatement** en rend seize tuiles qui
s'écartent en tournant, **génie** seize bandes aspirées vers l'icône. À `t = 0`
les parties se recollent exactement en la fenêtre. Sans rectangle UV
(`QuadInstance::uv`, ajouté aux 72 octets de l'instance) la texture était
dessinée entière et ces deux effets étaient impossibles. Le génie ne COURBE
pas : les bandes restent droites, leur empilement donne le courant.


