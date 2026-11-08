"""Le relief « aluminium brossé » du style Retro — DEUX bandes, pas une image.

PRODUIT `assets/styles/retro/barre-haut.png` et `barre-bas.png`, et n'écrit rien
d'autre.

    python tools/rasteriser_aluminium.py

 ⚠️ **POURQUOI DEUX BANDES ET NON UNE IMAGE DE BARRE ENTIÈRE.** Les barres du dock
ont des géométries trop différentes pour un seul dessin (72 px de haut sur toute
la largeur en haut et en bas, ~100 px de large sur 2000 de haut sur les côtés). On
dessine donc seulement le HAUT et le BAS de la barre, à leur taille naturelle, et
le milieu reste le fond flouté teinté — qui est une fonctionnalité, pas un vide.

 ⚠️ **ET POURQUOI UNE SEULE ORIENTATION DE GRAIN SUFFIT.** Les bandes ne sont
JAMAIS étirées verticalement : elles sont posées telles quelles au bord. Il n'y a
donc qu'un étirement HORIZONTAL, et un grain constant le long de X y survit — pour
les quatre barres, y compris les latérales. (Y penser avant de dessiner un second
jeu : j'ai failli en faire quatre fichiers pour rien.)

 ⚠️ **LE MÉTAL EST NEUTRE** — gris et alpha seulement, AUCUNE couleur. C'est ce qui
lui permet de se composer sur n'importe quelle teinte de barre réglée par
l'utilisateur. Un métal coloré entrerait en guerre avec l'axe des couleurs, qui
est censé rester indépendant (voir `docs/plans/styles-couleurs-icones.md`).

 ⚠️ **LE GRAIN SE COMPOSE, IL NE SE DESSINE PAS PAR-DESSUS.** `ImageDraw.line`
REMPLACE la ligne entière, alpha compris : dessiné après le profil, il l'effaçait
(le profil relevait 1-12 au lieu de 40-76, et c'est une SONDE CHIFFRÉE qui l'a vu,
pas l'œil). D'où la composition de deux images.

Source du dessin : le « brushed metal » d'Aqua (Mac OS X), que le thème **Aluminum
Alloy** de Max Rudberg a porté sur GNOME, metacity et Windows XP. Son trait
distinctif n'est pas la pente du clair vers le sombre — c'est le CREUX sombre juste
sous l'arête claire, qui fait lire du métal plutôt que du plastique.
"""

import os
import random

from PIL import Image, ImageDraw

ICI = os.path.dirname(os.path.abspath(__file__))
SORTIE = os.path.join(ICI, "..", "assets", "styles", "retro")

LARGEUR, HAUTEUR = 64, 72
# Les deux coupes, mesurées sur le profil : le creux et la crête tiennent dans les
# 34 premières lignes, la retombée dans les 30 dernières. Le milieu — 8 lignes
# presque transparentes — n'est PAS dessiné : c'est là que le fond flouté passe.
FIN_HAUT, DEBUT_BAS = 34, 42

GRAIN = 0.05  # amplitude du brossage, en alpha. Discret : la barre porte des icônes.


def bande(dessin: ImageDraw.ImageDraw, y0: int, y1: int, pic: float, couleur: tuple) -> None:
    """Une bande dont l'alpha monte PUIS retombe — un coussin, pas une pente."""
    for y in range(y0, y1):
        t = (y - y0) / max(1, (y1 - y0 - 1))
        alpha = pic * (1 - abs(2 * t - 1)) ** 0.75
        dessin.line([(0, y), (LARGEUR, y)], fill=couleur + (int(alpha * 255),))


def profil() -> Image.Image:
    """Le profil en coussin du métal, SANS grain — le grain vient par-dessus."""
    image = Image.new("RGBA", (LARGEUR, HAUTEUR), (0, 0, 0, 0))
    d = ImageDraw.Draw(image)
    d.rectangle([0, 0, LARGEUR, 0], fill=(255, 255, 255, 105))  # l'arête qui accroche
    bande(d, 1, 11, 0.16, (0, 0, 0))                            # LE CREUX — le trait d'Aqua
    bande(d, 11, FIN_HAUT, 0.20, (255, 255, 255))               # la crête du coussin
    bande(d, DEBUT_BAS, 70, 0.30, (0, 0, 0))                    # la retombée du bas
    d.rectangle([0, HAUTEUR - 1, LARGEUR, HAUTEUR - 1], fill=(0, 0, 0, 96))  # le trait de fond
    return image


def brossage(graine: int) -> Image.Image:
    """Le grain, sur SA PROPRE COUCHE — il se composera, il ne remplacera rien."""
    random.seed(graine)
    image = Image.new("RGBA", (LARGEUR, HAUTEUR), (0, 0, 0, 0))
    d = ImageDraw.Draw(image)
    for y in range(1, HAUTEUR - 1):
        g = random.uniform(-1.0, 1.0) * GRAIN
        couleur = (255, 255, 255) if g > 0 else (0, 0, 0)
        d.line([(0, y), (LARGEUR, y)], fill=couleur + (int(abs(g) * 255),))
    return image


def main() -> None:
    os.makedirs(SORTIE, exist_ok=True)
    # UNE graine fixe : la même planche à chaque exécution, donc comparable d'une
    # session à l'autre. C'est ce qui rend un « grain trop fort » réglable sans
    # redessiner.
    metal = Image.alpha_composite(profil(), brossage(20261005))
    metal.crop((0, 0, LARGEUR, FIN_HAUT)).save(os.path.join(SORTIE, "barre-haut.png"))
    metal.crop((0, DEBUT_BAS, LARGEUR, HAUTEUR)).save(os.path.join(SORTIE, "barre-bas.png"))

    # LE CONTRÔLE QUI COMPTE : le profil doit garder son CREUX.
    #
    # ⚠️ LE CREUX EST UN MAXIMUM LOCAL, PAS UN MINIMUM : c'est une BOSSE sombre
    # entre l'arête claire et la crête. La première version de cette sonde prenait
    # `min()` sur la fenêtre du creux — donc le neutre qui l'entoure — et criait au
    # défaut sur une image juste. La bonne lecture est celle d'un VALLEY :
    # `vallee = min()` entre l'arête et la crête, et il doit être PLUS BAS que les
    # deux. C'est cette lecture-là qui distingue un métal d'une simple pente.
    # ⚠️ L'ALPHA SEUL NE DIT RIEN : un noir à 47 et un blanc à 57 s'écrivent tous
    # deux « 47 » et « 57 ». Ce qui décrit le métal, c'est l'effet SIGNÉ — sombre
    # ou clair. On lit donc la luminance du pixel, signée par l'alpha.
    p = metal.load()

    def effet(y: int) -> int:
        r, g, b, a = p[LARGEUR // 2, y]
        lum = (r + g + b) / 3
        return int(a * (1 if lum > 128 else -1))

    colonne = [effet(y) for y in range(HAUTEUR)]
    arete = colonne[0]                  # clair : la plus positive
    creux = min(colonne[2:12])          # sombre : la plus NÉGATIVE de sa fenêtre
    crete = max(colonne[14:FIN_HAUT])   # clair : la plus positive
    retombee = min(colonne[DEBUT_BAS:])  # sombre : la plus négative
    print("effet signé : " + " ".join(f"{a:4d}" for a in colonne[:DEBUT_BAS]))
    print(f"arête {arete:+} · creux {creux:+} · crête {crete:+} · retombée {retombee:+}")
    if not (arete >= 60 and creux <= -20 and crete >= 40 and retombee <= -40):
        raise SystemExit(
            "LE PROFIL A PERDU SA FORME. Il faut, de haut en bas : une arête CLAIRE, "
            "un creux SOMBRE, une crête CLAIRE plus bas, et une retombée SOMBRE. "
            "Sans le creux, ce n'est plus du métal — c'est une pente."
        )
    print(f"2 bandes écrites dans {os.path.normpath(SORTIE)}")


if __name__ == "__main__":
    main()
