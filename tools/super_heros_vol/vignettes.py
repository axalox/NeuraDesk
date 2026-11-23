# Vignettes des super-heros pour les Reglages : le heros debout face a nous (image 40 de
# l'atterrissage), recadre sur son contour, dans un carre de 96 px transparent.
#   python vignettes.py <dossier_mascottes>
import glob, os, sys
from PIL import Image

IMAGE, COTE = 40, 96
d = sys.argv[1]
for fiche in sorted(glob.glob(os.path.join(d, "heros-*-atterrissage.txt"))):
    n = os.path.basename(fiche)[6:8]
    f = {k: int(v) for k, v in (l.strip().split("=") for l in open(fiche))}
    L, H = f["largeur"], f["hauteur"]
    p = Image.open(os.path.join(d, f"heros-{n}-atterrissage.webp")).convert("RGBA")
    case = p.crop(((IMAGE % 8) * L, (IMAGE // 8) * H, (IMAGE % 8 + 1) * L, (IMAGE // 8 + 1) * H))
    case = case.crop(case.getchannel("A").point(lambda a: 255 if a > 20 else 0).getbbox())
    case.thumbnail((COTE, COTE), Image.LANCZOS)
    v = Image.new("RGBA", (COTE, COTE), (0, 0, 0, 0))
    v.paste(case, ((COTE - case.width) // 2, (COTE - case.height) // 2))
    v.save(os.path.join(d, f"heros-{n}-vignette.webp"), "WEBP", quality=90, alpha_quality=100, method=6)
print("ok")
