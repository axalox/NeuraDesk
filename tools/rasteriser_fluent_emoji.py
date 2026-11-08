"""Rasterise les thèmes d'icônes « Fluent Emoji » (couleur et 3D).

PRODUIT `TopbarIcons/fluent_couleur/*.png` et `TopbarIcons/fluent_3d/*.png`, un
par applet, aux MÊMES noms de fichier que `TopbarIcons/energy/` (même table de
noms, trois dossiers).

    python tools/rasteriser_fluent_emoji.py

DEMANDE EXPLICITE (2026-10-11) : « Fluent Emoji, en version couleur et aussi la
version 3D ». Source : https://github.com/microsoft/fluentui-emoji — licence MIT
(Copyright (c) Microsoft Corporation), voir `TopbarIcons/SOURCE.md`.

 - « Color » est en SVG : rendu par `resvg` (installé par cargo) à 512 px ;
 - « 3D » est un PNG de 256 px : réduit tel quel.

⚠️ **MÊME GABARIT QUE LES AUTRES JEUX** : carré de 128 px, encre ramenée à 116 px
(voir `rasteriser_energy.py`), sinon la barre serait irrégulière.

⚠️ **PAS DE SCANNER** : Unicode n'a pas d'emoji « scanner », et le fax a déjà
été refusé par l'utilisateur pour « Symboles ». Les deux thèmes reprennent le
scanner DESSINÉ de `TopbarIcons/dessins/scanner.png`.
"""

import os
import shutil
import subprocess
import tempfile
import urllib.parse
import urllib.request

from PIL import Image

ICI = os.path.dirname(os.path.abspath(__file__))
RACINE = os.path.join(ICI, "..", "TopbarIcons")
DEPOT = "https://raw.githubusercontent.com/microsoft/fluentui-emoji/main/assets/"
COTE, ENCRE = 128, 116

# (nom du fichier, dossier de l'emoji dans le dépôt) — les MÊMES idées que
# « Symboles », sauf la météo (le soleil derrière un nuage, plus parlant que le
# nuage seul) et le thème (◐ n'existe pas : le premier quartier de lune, qui
# est sa moitié claire et sa moitié sombre).
EMOJI = [
    ("reglages", "Gear"),
    ("presse_papiers", "Clipboard"),
    ("son", "Speaker high volume"),
    ("reseau", "Globe with meridians"),
    # Recouverte par le bouton power DESSINÉ (voir `dessiner_bouton_power.py`,
    # appelé en fin de `main`) : Fluent Emoji n'a pas de bouton d'alimentation.
    ("alimentation", "Electric plug"),
    ("terminal", "Keyboard"),
    ("notes", "Speech balloon"),
    ("theme", "First quarter moon"),
    ("mode_nuit", "Crescent moon"),
    ("imprimante", "Printer"),
    ("calendrier", "Calendar"),
    ("editeur_image", "Framed picture"),
    ("capture_ecran", "Camera"),
    ("minuteur", "Stopwatch"),
    ("surveillance", "Chart increasing"),
    ("windows_update", "Counterclockwise arrows button"),
    ("peripheriques", "Puzzle piece"),
    ("reglages_ecran", "Desktop computer"),
    ("calculatrice", "Abacus"),
    ("ssh", "Key"),
    ("couleurs", "Artist palette"),
    ("fonctionnalites", "Plus"),
    ("notifications", "Megaphone"),
    ("recherche", "Magnifying glass tilted left"),
    ("automatisations", "High voltage"),
    ("meteo", "Sun behind cloud"),
    ("aide", "Red question mark"),
    ("afficher_bureau", "Window"),
    ("cloche", "Bell"),
    ("verrouillage", "Locked"),
    ("caracteres", "Input latin letters"),
    ("kaomoji", "Grinning face"),
    ("proprietes_systeme", "Control knobs"),
    ("shaders", "Rainbow"),
    ("phonetique", "Mouth"),
    # L'applet « Masquer l'interface » (2026-10-12) : le singe qui se cache les yeux.
    ("masque", "See-no-evil monkey"),
    # L'applet « Symboles mathématiques » (2026-10-12) : l'équerre.
    ("maths", "Triangular ruler"),
    ("lueurs", "Bright button"),
    # L'applet « Enregistrer » (2026-11-01) : 📹, la caméra VIDÉO (📷 est la capture).
    ("enregistrer", "Video camera"),
]


def telecharger(chemin: str) -> bytes:
    url = DEPOT + urllib.parse.quote(chemin)
    req = urllib.request.Request(url, headers={"User-Agent": "neuradesk"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def au_gabarit(image: Image.Image) -> Image.Image:
    """L'encre recadrée, son plus grand côté à ENCRE px, centrée dans COTE px."""
    image = image.convert("RGBA")
    boite = image.getchannel("A").point(lambda a: 255 if a > 8 else 0).getbbox()
    encre = image.crop(boite)
    echelle = ENCRE / max(encre.size)
    taille = (max(1, round(encre.width * echelle)), max(1, round(encre.height * echelle)))
    encre = encre.resize(taille, Image.LANCZOS)
    sortie = Image.new("RGBA", (COTE, COTE), (0, 0, 0, 0))
    sortie.paste(encre, ((COTE - taille[0]) // 2, (COTE - taille[1]) // 2), encre)
    return sortie


def nom_de_fichier(dossier: str) -> str:
    """Le nom de fichier du dépôt : minuscules, espaces en `_`. ⛔ Les TIRETS
    restent : « See-no-evil monkey » donne `see-no-evil_monkey` (404 sinon)."""
    return dossier.lower().replace(" ", "_")


def couleur(dossier: str, tmp: str) -> Image.Image:
    fichier = nom_de_fichier(dossier)
    svg = os.path.join(tmp, fichier + ".svg")
    png = os.path.join(tmp, fichier + ".png")
    open(svg, "wb").write(telecharger(f"{dossier}/Color/{fichier}_color.svg"))
    subprocess.run(["resvg", "-w", "512", svg, png], check=True)
    return Image.open(png)


def trois_d(dossier: str, tmp: str) -> Image.Image:
    fichier = nom_de_fichier(dossier)
    png = os.path.join(tmp, fichier + "_3d.png")
    open(png, "wb").write(telecharger(f"{dossier}/3D/{fichier}_3d.png"))
    return Image.open(png)


def main():
    for jeu, rendre in [("fluent_couleur", couleur), ("fluent_3d", trois_d)]:
        sortie = os.path.join(RACINE, jeu)
        os.makedirs(sortie, exist_ok=True)
        with tempfile.TemporaryDirectory() as tmp:
            for nom, dossier in EMOJI:
                au_gabarit(rendre(dossier, tmp)).save(os.path.join(sortie, nom + ".png"), optimize=True)
                print(jeu, nom)
        shutil.copyfile(os.path.join(RACINE, "dessins", "scanner.png"), os.path.join(sortie, "scanner.png"))
    # Le bouton power DESSINÉ remplace la prise (voir sa doc).
    import dessiner_bouton_power
    dessiner_bouton_power.main()


if __name__ == "__main__":
    main()
