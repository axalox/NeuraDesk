# Les insectes realistes de `win32::insectes` (5 mouches, 5 moustiques, 5 papillons).
# Mouches : domestique, verte, bleue, a damier, syrphe. Moustiques : commun,
# tigre, anophele, annele, Aedes vexans. Pose et vol = deux tirages FLUX.1-schnell
# de la meme espece (photo macro, vue de dessus).
#
# Sources : photos vues de dessus generees en local (ComfyUI) par FLUX.1-schnell,
# et par FLUX.2 [klein] 4B pour la mouche en vol (ailes deployees depuis la
# mouche posee, meme individu). Detourage : BiRefNet (MIT), complete par le
# « blanc vers transparence » pour garder les pattes et antennes fines, et les
# ailes membraneuses de la mouche et du moustique rendues translucides.
#
#   python tools/insectes_realistes.py <dossier des tirages> <dossier assets/popups/mascottes>
#
# Chaque image sort carree, l'insecte tete en HAUT, son corps au CENTRE (le
# pivot de la rotation dans l'appli), tout le contenu dans le cercle inscrit
# (rien n'est coupe quand il tourne). Les images d'une meme espece partagent la
# meme fraction « longueur du corps / cote » ; les papillons, la meme envergure.
import os
import sys

import numpy as np
from PIL import Image

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "super_heros_vol"))
from detourer_ia import alpha_de, nettoyer  # noqa: E402

# (sortie, tirage, espece, ailes translucides)
CHOIX = [
    ("insecte-mouche-1-pose", "photo-m1-pose", "mouche", True),
    ("insecte-mouche-1-vol", "photo-m1-vol", "mouche", True),
    ("insecte-mouche-2-pose", "photo-m2-pose", "mouche", True),
    ("insecte-mouche-2-vol", "photo-m2-vol", "mouche", True),
    ("insecte-mouche-3-pose", "photo-m3-pose", "mouche", True),
    ("insecte-mouche-3-vol", "photo-m3-vol", "mouche", True),
    ("insecte-mouche-4-pose", "photo-m4-pose", "mouche", True),
    ("insecte-mouche-4-vol", "photo-m4-vol", "mouche", True),
    ("insecte-mouche-5-pose", "photo-m5-pose", "mouche", True),
    ("insecte-mouche-5-vol", "photo-m5-vol", "mouche", True),
    ("insecte-moustique-1-pose", "photo-q1-pose", "moustique", True),
    ("insecte-moustique-1-vol", "photo-q1-vol", "moustique", True),
    ("insecte-moustique-2-pose", "photo-q2-pose", "moustique", True),
    ("insecte-moustique-2-vol", "photo-q2-vol", "moustique", True),
    ("insecte-moustique-3-pose", "photo-q3-pose", "moustique", True),
    ("insecte-moustique-3-vol", "photo-q3-vol", "moustique", True),
    ("insecte-moustique-4-pose", "photo-q4-pose", "moustique", True),
    ("insecte-moustique-4-vol", "photo-q4-vol", "moustique", True),
    ("insecte-moustique-5-pose", "photo-q5-pose", "moustique", True),
    ("insecte-moustique-5-vol", "photo-q5-vol", "moustique", True),
    ("insecte-papillon-1", "papillon-paon-2", "papillon", False),
    ("insecte-papillon-2", "vulcain-b-1", "papillon", False),
    ("insecte-papillon-3", "papillon-machaon-2", "papillon", False),
    ("insecte-papillon-4", "papillon-citron-1", "papillon", False),
    ("insecte-papillon-5", "papillon-azure-4", "papillon", False),
]
COTE = {"mouche": 256, "moustique": 256, "papillon": 320}


def detourer(rgb, ia, translucide):
    f = rgb.astype(np.float32)
    mini = f.min(axis=2)
    # Le sombre sur fond blanc : pattes et antennes que l'IA perd.
    sombre = np.clip((255 - mini) / 255 * 1.6 - 0.25, 0, 1)
    sombre = np.where(sombre > 0.35, sombre, 0)
    a = np.maximum(ia, sombre)
    if translucide:
        # La membrane des ailes est presque blanche : on la rend translucide,
        # ses nervures (plus sombres) restent plus marquees.
        clair = np.clip((mini - 195) / 20, 0, 1)
        membrane = np.clip((255 - mini) / 255 * 3.0, 0.12, 1)
        a = a * (1 - clair) + np.minimum(a, membrane) * clair
    a = np.where(a < 0.05, 0, a)
    a3 = a[..., None]
    avant = np.where(a3 > 0.02, (f / 255 - (1 - a3)) / np.maximum(a3, 0.02), 0)
    return np.dstack([np.clip(avant, 0, 1), a])


def mesurer(rgba):
    a = rgba[..., 3]
    h, w = a.shape
    ys, xs = np.nonzero(a > 0.5)
    cx = float(np.average(xs, weights=a[ys, xs]))
    bande = np.abs(np.arange(w) - cx) < w * 0.02
    corps = (a > 0.8) & bande[None, :]
    cy_ys = np.nonzero(corps.any(axis=1))[0]
    y0, y1 = cy_ys.min(), cy_ys.max()
    cy = (y0 + y1) / 2
    ys, xs = np.nonzero(a > 0.04)
    rmax = float(np.sqrt((xs - cx) ** 2 + (ys - cy) ** 2).max())
    return cx, cy, float(y1 - y0), rmax


def poser(rgba, cx, cy, cote_src, cote):
    """Le carre `cote_src` centre en (cx, cy), ramene a `cote` pixels."""
    h, w = rgba.shape[:2]
    toile = np.zeros((int(cote_src) + 2, int(cote_src) + 2, 4), np.float32)
    ox, oy = int(round(cx - cote_src / 2)), int(round(cy - cote_src / 2))
    sx0, sy0 = max(ox, 0), max(oy, 0)
    sx1, sy1 = min(ox + toile.shape[1], w), min(oy + toile.shape[0], h)
    toile[sy0 - oy:sy1 - oy, sx0 - ox:sx1 - ox] = rgba[sy0:sy1, sx0:sx1]
    # Redimensionner en premultiplie, puis revenir en droit.
    pre = toile.copy()
    pre[..., :3] *= pre[..., 3:4]
    im = Image.fromarray((pre * 255 + 0.5).astype(np.uint8), "RGBA").resize((cote, cote), Image.LANCZOS)
    p = np.asarray(im, np.float32) / 255
    a = p[..., 3:4]
    p[..., :3] = np.where(a > 0.004, p[..., :3] / np.maximum(a, 0.004), 0)
    return Image.fromarray((np.clip(p, 0, 1) * 255 + 0.5).astype(np.uint8), "RGBA")


def main():
    src, dst = sys.argv[1], sys.argv[2]
    images = [Image.open(os.path.join(src, t + ".png")).convert("RGB") for _, t, _, _ in CHOIX]
    masques = []
    for k in range(0, len(images), 4):
        masques += alpha_de(images[k:k + 4])
    mesures = []
    for (nom, _, espece, transl), im, ia in zip(CHOIX, images, masques):
        rgba = detourer(np.asarray(im), nettoyer(ia), transl)
        cx, cy, corps, rmax = mesurer(rgba)
        mesures.append((nom, espece, rgba, cx, cy, corps, rmax))
    # Par espece : la plus petite fraction corps/cote qui laisse tout entrer.
    fraction = {}
    for nom, espece, _, _, _, corps, rmax in mesures:
        f = corps / (2 * rmax / 0.97)
        fraction[espece] = min(fraction.get(espece, 1.0), f)
    for nom, espece, rgba, cx, cy, corps, rmax in mesures:
        # Les ailes des papillons se touchent au corps et faussent sa mesure :
        # ils sont mis a la meme ENVERGURE (le cercle qui les contient).
        cote_src = 2 * rmax / 0.97 if espece == "papillon" else corps / fraction[espece]
        poser(rgba, cx, cy, cote_src, COTE[espece]).save(os.path.join(dst, nom + ".webp"), lossless=True)
        print(nom, "corps", round(corps), "px")
    for espece, f in fraction.items():
        print(f"{espece} : corps = {f:.4f} du cote")


if __name__ == "__main__":
    main()
