"""Régénère le bloc des ICÔNES de `assets/popups/style.css` depuis Phosphor.

Phosphor Icons (MIT, https://phosphoricons.com) : paquet npm `@phosphor-icons/core`.
Usage :
    npm pack @phosphor-icons/core@2.1.1 --ignore-scripts   (dans un dossier vide)
    tar xzf phosphor-icons-core-2.1.1.tgz
    python -I tools/icones_phosphor.py <dossier>/package/assets

Chaque icône devient une classe `ico-<nom>` qui pose `--ico` ; la règle `.ico`
peint `currentColor` à travers ce masque, donc l'icône suit la couleur du texte
du bouton, donc le thème.
"""
import pathlib
import sys
import urllib.parse

ICONES = [
    "regular/x", "regular/push-pin", "fill/push-pin-fill", "regular/gear",
    "regular/plus", "regular/arrows-clockwise", "regular/copy",
    "regular/arrow-square-out", "regular/arrow-up", "regular/arrow-down",
    "regular/arrow-line-down", "regular/stack-simple", "regular/trash",
    "regular/arrow-counter-clockwise", "regular/arrow-clockwise",
    "regular/flip-horizontal", "regular/flip-vertical", "fill/play-fill",
    "fill/pause-fill", "regular/caret-left", "regular/caret-right",
    "regular/music-note", "regular/pencil-simple", "regular/eye",
    "regular/arrow-u-up-left", "regular/arrow-u-up-right",
    "regular/clipboard-text", "regular/folder-open", "regular/archive",
    "regular/dice-five", "regular/image", "regular/bell", "regular/bell-slash",
    "regular/shield", "regular/speaker-high", "regular/speaker-slash",
    "regular/download-simple", "regular/upload-simple", "regular/alarm",
    "regular/monitor", "regular/selection", "regular/webcam",
    "regular/microphone", "regular/cursor-click", "regular/video-camera",
    "regular/sphere", "regular/globe", "regular/terminal-window",
    "regular/folder", "regular/file",
    # Icônes des CATÉGORIES de Réglages (colonne de gauche, façon panneau de
    # configuration GNOME/Unity) — choisies par `iconeCategorie` dans settings.html.
    "regular/palette", "regular/ruler", "regular/drop", "regular/square",
    "regular/squares-four", "regular/clock", "regular/calendar-blank",
    "regular/info", "regular/mouse", "regular/app-window", "regular/wifi-high",
    "regular/keyboard", "regular/cloud-sun", "regular/lock", "regular/list",
    "regular/swatches", "regular/paint-brush", "regular/sliders-horizontal",
]

DEBUT = "/* ── ICÔNES (généré par tools/icones_phosphor.py — ne pas éditer à la main) ── */"
FIN = "/* ── FIN DES ICÔNES ── */"

REGLE = """/* Phosphor Icons, MIT (c) 2023 Phosphor Icons — https://phosphoricons.com
   `<span class="ico ico-x"></span>` dans un bouton : l'icône prend la couleur
   du texte. Un bouton qui n'a QUE l'icône garde un `title` : c'est son nom. */
.ico {
  display: inline-block;
  flex: none;
  width: 1.15em;
  height: 1.15em;
  vertical-align: -0.2em;
  background-color: currentColor;
  -webkit-mask: var(--ico) center / contain no-repeat;
  mask: var(--ico) center / contain no-repeat;
  pointer-events: none;
}"""


def data_uri(svg: str) -> str:
    svg = svg.replace(' fill="currentColor"', "").replace('"', "'").strip()
    return "data:image/svg+xml," + urllib.parse.quote(svg, safe=" /:=',.-")


def main() -> None:
    assets = pathlib.Path(sys.argv[1])
    lignes = [DEBUT, REGLE]
    for chemin in ICONES:
        nom = chemin.split("/")[1]
        svg = (assets / f"{chemin}.svg").read_text(encoding="utf-8")
        lignes.append(f'.ico-{nom} {{ --ico: url("{data_uri(svg)}"); }}')
    lignes.append(FIN)
    bloc = "\n".join(lignes)

    css = pathlib.Path(__file__).resolve().parent.parent / "assets/popups/style.css"
    texte = css.read_text(encoding="utf-8")
    if DEBUT in texte:
        avant, reste = texte.split(DEBUT, 1)
        texte = avant + bloc + reste.split(FIN, 1)[1]
    else:
        texte = texte.rstrip("\n") + "\n\n" + bloc + "\n"
    css.write_text(texte, encoding="utf-8", newline="")
    print(f"{len(ICONES)} icônes")


if __name__ == "__main__":
    main()
