# Range un clip detoure (81 PNG RGBA, la 81e = la 1re) en planche WebP 8x10 pour l'application.
#   python planche.py <dossier_rgba> <sortie.webp> [hauteur_max]
import glob, os, sys
from PIL import Image

COLONNES, LIGNES = 8, 10


def planche(src, sortie, hauteur_max=320):
    fs = sorted(glob.glob(os.path.join(src, "*.png")))[: COLONNES * LIGNES]
    assert len(fs) == COLONNES * LIGNES, f"{src} : {len(fs)} images"
    ims = [Image.open(f).convert("RGBA") for f in fs]
    l, h = ims[0].size
    if h > hauteur_max:
        l, h = round(l * hauteur_max / h), hauteur_max
        ims = [i.resize((l, h), Image.LANCZOS) for i in ims]
    p = Image.new("RGBA", (l * COLONNES, h * LIGNES), (0, 0, 0, 0))
    for k, im in enumerate(ims):
        p.paste(im, ((k % COLONNES) * l, (k // COLONNES) * h))
    p.save(sortie, "WEBP", quality=88, method=6, alpha_quality=100)
    return p.size, os.path.getsize(sortie)


if __name__ == "__main__":
    taille, octets = planche(sys.argv[1], sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else 320)
    print(f"{sys.argv[2]} : {taille[0]}x{taille[1]}, {octets / 1e6:.1f} Mo")
