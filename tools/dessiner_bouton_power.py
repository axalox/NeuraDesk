"""Dessine le BOUTON POWER en couleur des deux thèmes d'icônes en couleur.

DEMANDE EXPLICITE (2026-10-15) : « dans la barre du haut l'icône de l'applet
alimentation devrait être un bouton power dans tous les styles d'icônes ».

⚠️ **AUCUN EMOJI NE CONVIENT** : `⏻ U+23FB` rend un rectangle vide dans Segoe UI
Emoji (voir `TopbarIcons/SOURCE.md`), et Fluent Emoji n'a pas de bouton
d'alimentation — d'où la prise 🔌 qui le remplaçait. Le symbole vient donc du
même SVG que `power.png` (Keyline Icons, MIT : `svg/keyline-icons/power.svg`),
posé en blanc sur un disque rouge, dans le rendu de chaque jeu :

- `fluent_couleur/` : des aplats, comme les SVG « Color » de Fluent Emoji ;
- `fluent_3d/` : un disque modelé (reflet en haut à gauche, ombre en bas), comme
  les rendus « 3D ».

Même gabarit que les rastériseurs : carré de 128 px, encre de 116 px.
Appelé à la FIN de `rasteriser_fluent_emoji.py`, pour que le régénérer ne remette
pas la prise. Seul : `python tools/dessiner_bouton_power.py`.

⛔ **LE THÈME « SYMBOLES » A ÉTÉ RETIRÉ (2026-10-28)** — ses PNG venaient de
Segoe UI Emoji, que ce dépôt ne redistribue plus. Son rendu (`rendu_symboles`,
son dégradé et son liseré) est parti avec lui ; il n'y a plus que les deux
thèmes Fluent Emoji.
"""
import os
import subprocess
import tempfile

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ICI = os.path.dirname(os.path.abspath(__file__))
RACINE = os.path.join(ICI, "..", "TopbarIcons")
SVG = os.path.join(RACINE, "svg", "keyline-icons", "power.svg")
COTE, ENCRE = 128, 116
# Travail en 4× : le disque et ses dégradés sont lissés à la réduction.
GRAND = ENCRE * 4


def symbole(taille: int, epaisseur: float) -> Image.Image:
    """Le symbole d'alimentation en BLANC, `taille` px de côté (resvg)."""
    svg = open(SVG, encoding="utf-8").read()
    svg = svg.replace('stroke="currentColor"', 'stroke="#ffffff"').replace('stroke-width="2"', f'stroke-width="{epaisseur}"')
    with tempfile.TemporaryDirectory() as tmp:
        entree, sortie = os.path.join(tmp, "p.svg"), os.path.join(tmp, "p.png")
        open(entree, "w", encoding="utf-8").write(svg)
        subprocess.run(["resvg", "-w", str(taille), "-h", str(taille), entree, sortie], check=True)
        return Image.open(sortie).convert("RGBA")


def disque(masque_rayon: float) -> Image.Image:
    """Le masque d'un disque centré de `masque_rayon` × GRAND."""
    m = Image.new("L", (GRAND, GRAND), 0)
    r = GRAND * masque_rayon
    c = GRAND / 2
    ImageDraw.Draw(m).ellipse((c - r, c - r, c + r, c + r), fill=255)
    return m


def poser_symbole(base: Image.Image, ombre: bool) -> Image.Image:
    s = symbole(round(GRAND * 0.52), 2.6)
    pos = ((GRAND - s.width) // 2, (GRAND - s.height) // 2)
    if ombre:
        o = Image.new("RGBA", s.size, (90, 10, 10, 0))
        o.putalpha(s.getchannel("A").point(lambda a: a * 0.45))
        o = o.filter(ImageFilter.GaussianBlur(GRAND * 0.012))
        base.alpha_composite(o, (pos[0], pos[1] + round(GRAND * 0.015)))
    base.alpha_composite(s, pos)
    return base


def rendu_fluent_couleur() -> Image.Image:
    base = Image.new("RGBA", (GRAND, GRAND), (0, 0, 0, 0))
    base.paste(Image.new("RGBA", (GRAND, GRAND), (200, 40, 45, 255)), (0, 0), disque(0.5))
    base.paste(Image.new("RGBA", (GRAND, GRAND), (242, 72, 70, 255)), (0, 0), disque(0.44))
    return poser_symbole(base, ombre=False)


def rendu_fluent_3d() -> Image.Image:
    base = Image.new("RGBA", (GRAND, GRAND), (0, 0, 0, 0))
    # Le corps : un dégradé RADIAL décentré vers le haut à gauche (la lumière).
    corps = Image.new("RGBA", (GRAND, GRAND))
    px = corps.load()
    lx, ly = GRAND * 0.36, GRAND * 0.3
    for y in range(GRAND):
        for x in range(GRAND):
            d = min(1.0, ((x - lx) ** 2 + (y - ly) ** 2) ** 0.5 / (GRAND * 0.85))
            px[x, y] = tuple(round(a + (b - a) * d) for a, b in zip((255, 128, 118), (150, 22, 30))) + (255,)
    base.paste(corps, (0, 0), disque(0.5))
    # Le reflet : une ellipse claire, floutée, en haut.
    reflet = Image.new("L", (GRAND, GRAND), 0)
    ImageDraw.Draw(reflet).ellipse((GRAND * 0.22, GRAND * 0.08, GRAND * 0.7, GRAND * 0.36), fill=120)
    reflet = ImageChops.multiply(reflet.filter(ImageFilter.GaussianBlur(GRAND * 0.03)), disque(0.48))
    blanc = Image.new("RGBA", (GRAND, GRAND), (255, 255, 255, 0))
    blanc.putalpha(reflet)
    base.alpha_composite(blanc)
    return poser_symbole(base, ombre=True)


def au_gabarit(image: Image.Image) -> Image.Image:
    """Ramenée à ENCRE px et centrée dans COTE px (même gabarit que les autres)."""
    encre = image.crop(image.getchannel("A").getbbox()).resize((ENCRE, ENCRE), Image.LANCZOS)
    sortie = Image.new("RGBA", (COTE, COTE), (0, 0, 0, 0))
    sortie.paste(encre, ((COTE - ENCRE) // 2, (COTE - ENCRE) // 2), encre)
    return sortie


def main() -> None:
    for jeu, rendre in [("fluent_couleur", rendu_fluent_couleur), ("fluent_3d", rendu_fluent_3d)]:
        chemin = os.path.join(RACINE, jeu, "alimentation.png")
        au_gabarit(rendre()).save(chemin, optimize=True)
        print(f"  bouton power -> {os.path.normpath(chemin)}")


if __name__ == "__main__":
    main()
