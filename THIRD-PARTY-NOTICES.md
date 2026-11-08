# NeuraDesk — licences des composants tiers

NeuraDesk est distribué sous **GNU General Public License, version 3 ou ultérieure**
(`GPL-3.0-or-later`) : le texte intégral est dans [`LICENSE`](LICENSE).

Ce fichier rassemble les mentions que les licences des composants tiers **exigent de
joindre à toute copie distribuée**. Il accompagne chaque archive publiée, à côté de
`LICENSE`.

> **Source :** le code source correspondant est publié sur
> <https://github.com/axalox/NeuraDesk>, et chaque archive portable correspond à un
> tag de release (`portable-AAAA-MM-JJ`) de ce dépôt (GPLv3, §6(d)).

---

## 1. Icônes

Cinq jeux différents alimentent `TopbarIcons/` et les data-URI de
`assets/popups/style.css`. Tous sont sous licence MIT, et **les PNG/SVG sont
incorporés dans `neuradesk.exe`** (`include_bytes!`) : c'est pourquoi ces mentions
doivent voyager avec le binaire.

| Jeu | Auteur | Où | Licence | Joint sous |
|---|---|---|---|---|
| Iconsax (style Outline) | Vuesax / Erfan Khadivar | `TopbarIcons/*.png`, `TopbarIcons/svg/` | MIT | `LICENSE-iconsax.txt` |
| Keyline Icons | keylineicons.com | `TopbarIcons/*.png`, `TopbarIcons/svg/keyline-icons/` | MIT | `LICENSE-keyline-icons.txt` |
| Fluent Emoji | Microsoft Corporation | `TopbarIcons/fluent_couleur/`, `fluent_3d/` | MIT | `LICENSE-fluentui-emoji.txt` |
| Energy Icons | Sam Passmore (2026) | `TopbarIcons/energy/` | MIT | `LICENSE-energy-icons.txt` |
| Phosphor Icons | Phosphor Icons (2023) | data-URI dans `assets/popups/style.css` | MIT | voir le texte ci-dessous |

### Texte de la licence MIT — commun aux cinq jeux

```
MIT License

Copyright (c) ERFAN KHADIVAR                    (Iconsax)
Copyright (c) Keyline Icons                     (Keyline Icons)
Copyright (c) Microsoft Corporation             (Fluent Emoji)
Copyright (c) 2026 Sam Passmore                 (Energy Icons)
Copyright (c) 2023 Phosphor Icons               (Phosphor Icons)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

⚠️ Les textes intégraux d'origine, non reformatés, sont joints à l'archive sous les
quatre noms du tableau. Phosphor n'a pas de fichier d'origine dans le dépôt : le bloc
ci-dessus **est** sa mention, et il accompagne `assets/popups/style.css` où les SVG
sont inlinés en data-URI.

### Glyphes dessinés

Les icônes listées comme « dessinées » dans `TopbarIcons/SOURCE.md` (`theme.png`,
`night_light.png`, `calendar.png`, `printer.png`, `image_editor.png`, `couleurs.png`,
`kaomoji.png`) sont produites par le code de ce dépôt : elles sont couvertes par la
GPLv3, comme le reste du programme. Les dessins d'appoint des thèmes en couleur vivent
dans `TopbarIcons/dessins/` (`scanner.png`, `alimentation.png`).

### Thème « Symboles » — retiré le 2026-10-28

Ce thème et ses 41 PNG étaient des **rendus de `seguiemj.ttf`**, la police emoji livrée
avec Windows. Des images de glyphes d'une police Microsoft ne se redistribuent pas comme
jeu d'actifs : ils ont été **retirés du programme et du dépôt**, et le thème n'est plus
proposé. Les thèmes en couleur sont désormais les deux « Fluent Emoji » (MIT), ci-dessus.

### Thème « Fluent » — aucune police n'est livrée

Le thème « Fluent » dessine ses icônes avec **Segoe Fluent Icons**, lue **sur le système
de l'utilisateur** au moment du rendu. Aucun fichier de police n'est copié, ni dans le
dépôt, ni dans l'exe, ni dans l'archive : si la police est absente, le thème retombe sur
« NeuraDesk ».

---

## 2. Police

**Nordwand Mono** — <https://github.com/tywr/Nordwand-Mono>, Copyright (c) 2026
Tanguy VIVIER, **SIL Open Font License 1.1** (aucun Reserved Font Name).

Le texte intégral accompagne les quatre fichiers `.woff2` :
`assets/popups/fonts/LICENSE-Nordwand-Mono.txt`. La police reste sous OFL 1.1 et
n'est pas couverte par la GPLv3 du programme ; elle est distribuée avec lui au titre
de l'agrégation permise par l'OFL.

---

## 3. Bibliothèques JavaScript

| Composant | Version | Licence | Mentions |
|---|---|---|---|
| `xterm.js` | 6.0.0 | MIT | en-tête complet dans `assets/popups/xterm.js` |
| `@xterm/addon-fit` | 0.11.0 | MIT | en-tête complet dans `assets/popups/xterm-addon-fit.js` |
| `xterm.css` | 6.0.0 | MIT | en-tête complet dans `assets/popups/xterm.css` |

Copyright (c) 2017-2019 The xterm.js authors · (c) 2014-2016 SourceLair Private
Company · (c) 2012-2013 Christopher Jeffrey. Les trois fichiers portent le texte
intégral de la licence, non modifié.

---

## 4. Composants binaires

### Chromium / CEF

Les fichiers `libcef.dll`, `chrome_elf.dll`, `libEGL.dll`, `libGLESv2.dll`,
`d3dcompiler_47.dll`, `dxcompiler.dll`, `dxil.dll`, `vk_swiftshader*`,
`vulkan-1.dll`, `*.pak`, `icudtl.dat`, `v8_context_snapshot.bin` et `locales/`
proviennent de la distribution **CEF** (BSD-3-Clause, © The Chromium Authors) et
embarquent des composants tiers (ANGLE, ICU, Skia, ffmpeg, SwiftShader…).

Les mentions de ces composants sont dans **`CREDITS.html`**, joint à l'archive
(fichier généré par Chromium ; il contient les textes de licence, dont les 549
mentions BSD).

### libVLC

`vlc/libvlc.dll`, `vlc/libvlccore.dll` et `vlc/plugins/` proviennent de **VLC media
player** (VideoLAN) : `libvlc` et `libvlccore` sont sous **LGPL-2.1 ou
ultérieure**, et une partie des modules de `plugins/` sous **GPL-2.0 ou
ultérieure** — d'où la licence `GPL-3.0-or-later` de NeuraDesk, compatible avec les
deux. Le texte est joint : `vlc/COPYING.txt`. Les binaires ne sont **pas modifiés**.

**Offre de source :** le code source correspondant de VideoLAN est disponible sur
<https://www.videolan.org/vlc/download-sources.html>, pour la version livrée
(voir la version de `libvlc.dll`).

### GStreamer

`gstreamer/bin/` et `gstreamer/lib/gstreamer-1.0/` (applet **Enregistrer**)
proviennent de **GStreamer 1.28** (<https://gstreamer.freedesktop.org>), distribution
officielle MSVC x86_64 : GStreamer, GLib et leurs bibliothèques sont sous **LGPL-2.1
ou ultérieure** ; quelques dépendances (zlib, pcre2, libffi, orc, LAME…) ont leurs
propres licences libres. Tous les textes sont joints : `gstreamer/licenses/`. Les
binaires ne sont **pas modifiés** et sont chargés dynamiquement : on peut les
remplacer par une autre version compatible.

**Offre de source :** le code source correspondant est disponible sur
<https://gstreamer.freedesktop.org/src/> et
<https://gitlab.freedesktop.org/gstreamer/cerbero> (recette de la distribution),
pour la version livrée (`gst-launch-1.0 --version`).

### Runtime Visual C++

`vcruntime140.dll` est le runtime Microsoft Visual C++, redistribué avec
l'application conformément aux termes de Visual Studio.

---

## 5. Sons

Les vingt sons de `son/` sont **synthétisés** par script pour ce projet (WAV, libres
de droits, aucun échantillon tiers). Les sons sous licence tierce utilisés lors des
premières versions (Pixabay, La Sonothèque) **ont été retirés du paquet** — voir le
commit `5fcf4c6`.

---

## 6. Dépendances Rust

NeuraDesk est écrit en Rust et lie statiquement ses dépendances. Chacune déclare une
licence permissive, compatible avec la GPLv3 : MIT, Apache-2.0, BSD-2/3-Clause, ISC,
Zlib, Unicode-3.0, CDLA-Permissive-2.0 ou bzip2-1.0.6. **Aucune dépendance n'est sous
GPL, AGPL, SSPL ou une licence non libre.**

La liste complète (nom, version, licence) et les textes de licence collectés dans les
crates eux-mêmes sont dans **[`THIRD-PARTY-CRATES.txt`](THIRD-PARTY-CRATES.txt)**,
également joint à chaque archive. Ce fichier est **régénéré** par
`python tools/generer_notices.py` ; il ne s'édite pas à la main.
