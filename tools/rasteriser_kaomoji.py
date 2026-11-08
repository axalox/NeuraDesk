"""L'icône de l'applet « Kaomojis » — un visage `^_^`, dessiné.

PRODUIT `TopbarIcons/kaomoji.png` (128×128, monochrome par contrat).

    python tools/rasteriser_kaomoji.py

 ⚠️ **MONOCHROME, ET C'EST LE CONTRAT DE LA RACINE DE `TopbarIcons/`** : le shader
n'échantillonne que le canal alpha et repeint dans la teinte du thème (voir
`gfx::texture`, `TexMode::Glyph`). Le blanc n'est donc qu'une commodité de lecture
— ce qui compte est l'ALPHA. La variante COULEUR vit dans `fluent_couleur/` et
`fluent_3d/`, produite par `tools/rasteriser_fluent_emoji.py`.

 ⚠️ **POURQUOI UN VISAGE ET NON UNE EMOJI** : l'applet donne des kaomojis, c'est-à-dire
des visages faits de PONCTUATION. `^_^` est le plus court d'entre eux, et il dit
exactement ce que la fenêtre contient — là où une emoji dirait « émoticône », qui est
l'autre onglet du panneau de Windows. La variante couleur, elle, prend une emoji
(voir `rasteriser_fluent_emoji.py`) : c'est le seul dessin lisible en couleur à 128 px.

 ⚠️ La police est **Segoe UI Gras**, livrée avec Windows, jamais redistribuée —
même règle que Segoe UI Emoji et Segoe Fluent Icons.
"""

import os

from PIL import Image, ImageDraw, ImageFont

ICI = os.path.dirname(os.path.abspath(__file__))
SORTIE = os.path.join(ICI, "..", "TopbarIcons", "kaomoji.png")
POLICE = os.path.join(os.environ.get("SystemRoot", r"C:\Windows"), "Fonts", "segoeuib.ttf")

COTE, ENCRE = 128, 116
VISAGE = "^_^"


def main() -> None:
    if not os.path.isfile(POLICE):
        raise SystemExit(f"police absente : {POLICE}")

    # Même recette que `rasteriser_caracteres.py` : on rend une première fois pour
    # MESURER la boîte d'encre, puis on re-rend à l'échelle qui l'amène à ENCRE px,
    # et on la centre. Sans ça, chaque glyphe aurait l'échelle de sa propre boîte.
    def rendre(taille: int) -> Image.Image:
        image = Image.new("RGBA", (taille * 4, taille * 4), (0, 0, 0, 0))
        ImageDraw.Draw(image).text(
            (taille, taille), VISAGE, font=ImageFont.truetype(POLICE, taille), fill=(255, 255, 255, 255)
        )
        return image

    boite = rendre(COTE).getbbox()
    if not boite:
        raise SystemExit("AUCUNE ENCRE : le visage n'a pas été rendu")
    plus_grand = max(boite[2] - boite[0], boite[3] - boite[1])
    echelle = max(8, COTE * ENCRE // plus_grand)
    morceau = rendre(echelle).crop(rendre(echelle).getbbox())
    sortie = Image.new("RGBA", (COTE, COTE), (0, 0, 0, 0))
    sortie.paste(morceau, ((COTE - morceau.width) // 2, (COTE - morceau.height) // 2), morceau)
    sortie.save(SORTIE)

    # LE CONTRÔLE QUI COMPTE : l'encre doit occuper ENCRE px de haut ou de large.
    boite = sortie.getbbox()
    assert boite, "la sortie est vide"
    grande = max(boite[2] - boite[0], boite[3] - boite[1])
    print(f"encre {grande} px sur {COTE} (visé {ENCRE})")
    if not (ENCRE - 2 <= grande <= ENCRE):
        raise SystemExit(f"L'ENCRE N'EST PAS À L'ÉCHELLE : {grande} px au lieu de {ENCRE}")
    print(f"écrit : {os.path.normpath(SORTIE)}")


if __name__ == "__main__":
    main()
