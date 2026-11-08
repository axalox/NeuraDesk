"""Rasterise le thème d'icônes « Energy Icons ».

PRODUIT `TopbarIcons/energy/*.png`, un par applet, aux MÊMES noms de fichier que
`TopbarIcons/fluent_couleur/` et `TopbarIcons/fluent_3d/`.

    python tools/rasteriser_energy.py <dossier icons/ de energy-icons>

DEMANDE EXPLICITE (2026-10-24) : « faire un nouveau style d'icônes en utilisant
les icônes de https://energyicons.com », en trait GRAS (choix de l'utilisateur
sur une planche des deux graisses). Source :
https://github.com/Sam-r-passmore/energy-icons — licence MIT (Copyright (c) 2026
Sam Passmore), voir `TopbarIcons/SOURCE.md` et `TopbarIcons/LICENSE-energy-icons.txt`.

⚠️ **MONOCHROMES, COMME LES PNG DE LA RACINE** : encre BLANCHE, forme dans
l'alpha. Le dock les teinte (`TexMode::Glyph`) — ils suivent donc la couleur du
thème, comme « NeuraDesk » et « Fluent ».

⚠️ **MÊME GABARIT QUE LES AUTRES THÈMES** : carré de 128 px, plus grand côté de
l'encre ramené à 116 px et centré (voir `ui::icones::ENCRE`), sinon la barre
serait irrégulière.

⚠️ **SANS PIL** : seul `resvg` (installé par cargo) est requis ; les PNG qu'il
écrit (RGBA 8 bits, non entrelacés) sont relus et réécrits avec `zlib`.
"""

import os
import struct
import subprocess
import sys
import tempfile
import zlib

ICI = os.path.dirname(os.path.abspath(__file__))
SORTIE = os.path.join(ICI, "..", "TopbarIcons", "energy")
COTE, ENCRE = 128, 116
GRAISSE = "48-bold.svg"

# (nom du fichier, icône Energy Icons) — choisies sur une planche de contact et
# validées par l'utilisateur (2026-10-24).
ICONES = [
    ("reglages", "settings"), ("presse_papiers", "clipboard"), ("son", "volume-high"),
    ("reseau", "wifi"), ("alimentation", "power"), ("terminal", "terminal"),
    ("notes", "sticky-note"), ("theme", "contrast"), ("mode_nuit", "moon"),
    ("imprimante", "printer"), ("calendrier", "calendar"), ("editeur_image", "image"),
    ("capture_ecran", "camera"), ("minuteur", "timer"), ("surveillance", "activity"),
    ("windows_update", "refresh-cw"), ("peripheriques", "plug"), ("reglages_ecran", "monitor"),
    ("calculatrice", "calculator"), ("ssh", "server"), ("couleurs", "palette"),
    ("fonctionnalites", "puzzle"), ("notifications", "megaphone"), ("recherche", "search"),
    ("automatisations", "workflow"), ("meteo", "cloud-sun"), ("aide", "help-circle"),
    ("afficher_bureau", "app-window"), ("cloche", "bell"), ("verrouillage", "lock"),
    ("caracteres", "letter-case"), ("kaomoji", "smile"), ("proprietes_systeme", "pc-tower"),
    ("shaders", "rainbow"), ("phonetique", "languages"), ("masque", "eye-off"),
    ("maths", "function"), ("lueurs", "sparkles"), ("moniteur", "cpu"),
    ("narrateur", "file-audio"), ("loupe", "zoom-in"), ("textures", "layers"),
    ("scanner", "scanner"),
    # L'applet « Enregistrer » (2026-11-01) : une caméra vidéo.
    ("enregistrer", "video"),
]


def lire_png(chemin):
    """(largeur, hauteur, octets RGBA) d'un PNG RGBA 8 bits non entrelacé."""
    with open(chemin, "rb") as f:
        donnees = f.read()
    assert donnees[:8] == b"\x89PNG\r\n\x1a\n", chemin
    i, idat, largeur, hauteur = 8, b"", 0, 0
    while i < len(donnees):
        (longueur,) = struct.unpack(">I", donnees[i:i + 4])
        genre = donnees[i + 4:i + 8]
        corps = donnees[i + 8:i + 8 + longueur]
        if genre == b"IHDR":
            largeur, hauteur, prof, couleur, _, _, entrelace = struct.unpack(">IIBBBBB", corps)
            assert (prof, couleur, entrelace) == (8, 6, 0), "resvg doit ecrire du RGBA 8 bits"
        elif genre == b"IDAT":
            idat += corps
        i += 12 + longueur
    brut = zlib.decompress(idat)
    pas = largeur * 4
    sortie = bytearray(hauteur * pas)
    precedente = bytearray(pas)
    for y in range(hauteur):
        filtre = brut[y * (pas + 1)]
        ligne = bytearray(brut[y * (pas + 1) + 1:(y + 1) * (pas + 1)])
        for x in range(pas):
            a = ligne[x - 4] if x >= 4 else 0
            b = precedente[x]
            c = precedente[x - 4] if x >= 4 else 0
            if filtre == 1:
                ligne[x] = (ligne[x] + a) & 255
            elif filtre == 2:
                ligne[x] = (ligne[x] + b) & 255
            elif filtre == 3:
                ligne[x] = (ligne[x] + (a + b) // 2) & 255
            elif filtre == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                ligne[x] = (ligne[x] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        sortie[y * pas:(y + 1) * pas] = ligne
        precedente = ligne
    return largeur, hauteur, bytes(sortie)


def ecrire_png(chemin, largeur, hauteur, rgba):
    brut = b"".join(b"\x00" + rgba[y * largeur * 4:(y + 1) * largeur * 4] for y in range(hauteur))

    def morceau(genre, corps):
        return struct.pack(">I", len(corps)) + genre + corps + struct.pack(">I", zlib.crc32(genre + corps))

    with open(chemin, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(morceau(b"IHDR", struct.pack(">IIBBBBB", largeur, hauteur, 8, 6, 0, 0, 0)))
        f.write(morceau(b"IDAT", zlib.compress(brut, 9)))
        f.write(morceau(b"IEND", b""))


def boite(largeur, hauteur, rgba):
    xs = [x for y in range(hauteur) for x in range(largeur) if rgba[(y * largeur + x) * 4 + 3] > 8]
    ys = [y for y in range(hauteur) for x in range(largeur) if rgba[(y * largeur + x) * 4 + 3] > 8]
    return min(xs), min(ys), max(xs), max(ys)


def rendre(svg_blanc, taille, dossier):
    png = os.path.join(dossier, "r.png")
    subprocess.run(["resvg", "-w", str(taille), svg_blanc, png], check=True)
    return lire_png(png)


def main():
    source = sys.argv[1]
    os.makedirs(SORTIE, exist_ok=True)
    with tempfile.TemporaryDirectory() as dossier:
        for nom, icone in ICONES:
            with open(os.path.join(source, icone, GRAISSE), encoding="utf-8") as f:
                svg = f.read()
            # ENCRE BLANCHE : les SVG peignent en `currentColor`, noir par défaut.
            svg_blanc = os.path.join(dossier, "blanc.svg")
            with open(svg_blanc, "w", encoding="utf-8") as f:
                f.write(svg.replace("currentColor", "#FFFFFF"))
            # Premier rendu : mesurer l'encre. Second : à la taille qui la met à ENCRE.
            l, h, rgba = rendre(svg_blanc, 480, dossier)
            x0, y0, x1, y1 = boite(l, h, rgba)
            taille = round(480 * ENCRE / max(x1 - x0 + 1, y1 - y0 + 1))
            l, h, rgba = rendre(svg_blanc, taille, dossier)
            x0, y0, x1, y1 = boite(l, h, rgba)
            lw, lh = x1 - x0 + 1, y1 - y0 + 1
            dx, dy = (COTE - lw) // 2, (COTE - lh) // 2
            sortie = bytearray(COTE * COTE * 4)
            for y in range(lh):
                for x in range(lw):
                    tx, ty = x + dx, y + dy
                    if 0 <= tx < COTE and 0 <= ty < COTE:
                        alpha = rgba[((y0 + y) * l + x0 + x) * 4 + 3]
                        sortie[(ty * COTE + tx) * 4:(ty * COTE + tx) * 4 + 4] = bytes((255, 255, 255, alpha))
            ecrire_png(os.path.join(SORTIE, nom + ".png"), COTE, COTE, bytes(sortie))
            print(f"{nom:20} <- {icone:14} encre {lw}x{lh}")


if __name__ == "__main__":
    main()
