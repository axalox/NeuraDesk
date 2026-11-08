"""L'icône de l'applet « Caractères » — un « é », dessiné.

PRODUIT `TopbarIcons/caracteres.png` (128×128, monochrome par contrat).

    python tools/rasteriser_caracteres.py

 ⚠️ **MONOCHROME, ET C'EST LE CONTRAT DE LA RACINE DE `TopbarIcons/`** : le shader
n'échantillonne que le canal alpha et repeint dans la teinte du thème (voir
`gfx::texture`, `TexMode::Glyph`). Le blanc n'est donc qu'une commodité de lecture
— ce qui compte est l'ALPHA. C'est la variante COULEUR qui vit dans
`fluent_couleur/` et `fluent_3d/`, produite par `tools/rasteriser_fluent_emoji.py`.

 ⚠️ **POURQUOI UNE LETTRE ET NON UN PICTOGRAMME** : l'applet donne des accents, et
il n'existe pas de glyphe clair pour « é ». Une lettre accentuée se lit
instantanément, à la taille d'une icône de barre, et elle dit exactement ce que
l'applet fait.

 ⚠️ La police est **Segoe UI Gras**, livrée avec Windows, jamais redistribuée —
même règle que Segoe UI Emoji et Segoe Fluent Icons.
"""

import os

from PIL import Image, ImageDraw, ImageFont

ICI = os.path.dirname(os.path.abspath(__file__))
SORTIE = os.path.join(ICI, "..", "TopbarIcons", "caracteres.png")
POLICE = os.path.join(os.environ.get("SystemRoot", r"C:\Windows"), "Fonts", "segoeuib.ttf")

COTE, ENCRE = 128, 116
LETTRE = "é"


def main() -> None:
    if not os.path.isfile(POLICE):
        raise SystemExit(f"police absente : {POLICE}")
    # Même recette que `rasteriser_energy.py` : on rend une première fois pour
    # MESURER la boîte d'encre, puis on re-rend à l'échelle qui l'amène à ENCRE px,
    # et on la centre. Sans ça, chaque glyphe aurait l'échelle de sa propre boîte.
    def rendre(taille: int) -> Image.Image:
        image = Image.new("RGBA", (taille * 3, taille * 3), (0, 0, 0, 0))
        ImageDraw.Draw(image).text((taille, taille), LETTRE, font=ImageFont.truetype(POLICE, taille), fill=(255, 255, 255, 255))
        return image

    boite = rendre(COTE).getbbox()
    if not boite:
        raise SystemExit("AUCUNE ENCRE : la lettre n'a pas été rendue")
    plus_grand = max(boite[2] - boite[0], boite[3] - boite[1])
    morceau = rendre(max(8, COTE * ENCRE // plus_grand)).crop(rendre(max(8, COTE * ENCRE // plus_grand)).getbbox())
    sortie = Image.new("RGBA", (COTE, COTE), (0, 0, 0, 0))
    sortie.paste(morceau, ((COTE - morceau.width) // 2, (COTE - morceau.height) // 2), morceau)
    sortie.save(SORTIE)

    # LE CONTRÔLE QUI COMPTE : l'encre doit occuper ENCRE px de haut ou de large.
    # Une icône trop petite se voit dans la barre, mais on ne saurait pas POURQUOI.
    boite = sortie.getbbox()
    assert boite, "la sortie est vide"
    grande = max(boite[2] - boite[0], boite[3] - boite[1])
    print(f"encre {grande} px sur {COTE} (visé {ENCRE})")
    if not (ENCRE - 2 <= grande <= ENCRE):
        raise SystemExit(f"L'ENCRE N'EST PAS À L'ÉCHELLE : {grande} px au lieu de {ENCRE}")
    print(f"écrit : {os.path.normpath(SORTIE)}")


if __name__ == "__main__":
    main()
