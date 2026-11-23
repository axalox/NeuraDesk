# Detourage du fond vert : matte doux par "exces de vert", suppression du debordement vert,
# recadrage commun a toutes les images. Sortie : PNG RGBA par image + planche de controle.
#   python detourer.py <dossier_images> <dossier_sortie>
import glob, os, sys
import numpy as np
from PIL import Image
from scipy import ndimage

BAS, HAUT = 0.10, 0.30   # exces de vert (normalise) : <= BAS opaque, >= HAUT transparent


def detourer(rgb):
    f = rgb.astype(np.float32) / 255.0
    r, g, b = f[..., 0], f[..., 1], f[..., 2]
    exces = g - np.maximum(r, b)                       # fond vert ~0.9, rouge/or ~<0
    # reference mesuree sur les coins (le vert derive apres le VAE)
    coins = np.concatenate([exces[:12, :12].ravel(), exces[:12, -12:].ravel(), exces[-12:, :12].ravel(), exces[-12:, -12:].ravel()])
    ref = float(np.median(coins))
    t = np.clip((exces / max(ref, 1e-3) - BAS) / (HAUT - BAS), 0, 1)
    alpha = 1.0 - t
    g2 = np.minimum(g, np.maximum(r, b))              # debordement vert -> neutre (or intact : G<=R)
    out = np.stack([r, g2, b, alpha], -1)
    out[alpha < 0.02] = 0
    # miettes : on ne garde que les composantes connexes d'au moins 1,5 % de la plus grande
    lab, n = ndimage.label(alpha > 0.1, structure=np.ones((3, 3)))
    if n > 1:
        tailles = ndimage.sum(np.ones_like(alpha), lab, range(1, n + 1))
        garder = np.zeros(n + 1, bool)
        garder[1:] = tailles >= 0.015 * tailles.max()
        proche = ndimage.binary_dilation(garder[lab], iterations=2)
        out[~proche] = 0
    return (out * 255 + 0.5).astype(np.uint8), ref


if __name__ == "__main__":
    src, dst = sys.argv[1], sys.argv[2]
    os.makedirs(dst, exist_ok=True)
    fs = sorted(glob.glob(os.path.join(src, "*.png")))
    imgs, refs = [], []
    for f in fs:
        a, ref = detourer(np.array(Image.open(f).convert("RGB")))
        imgs.append(a); refs.append(ref)
    union = np.zeros(imgs[0].shape[:2], bool)
    for a in imgs:
        union |= a[..., 3] > 8
    ys, xs = np.where(union)
    m = 4
    y0, y1 = max(ys.min() - m, 0), min(ys.max() + m + 1, union.shape[0])
    x0, x1 = max(xs.min() - m, 0), min(xs.max() + m + 1, union.shape[1])
    for k, a in enumerate(imgs):
        Image.fromarray(a[y0:y1, x0:x1], "RGBA").save(os.path.join(dst, f"{k:03d}.png"))
    print(f"{len(imgs)} images, cadre {x1-x0}x{y1-y0}, ref vert {min(refs):.2f}..{max(refs):.2f}, "
          f"touche le bord: {ys.min()==0 or xs.min()==0 or ys.max()==union.shape[0]-1 or xs.max()==union.shape[1]-1}")
    # planche : 1 image sur 8 sur fond sombre puis clair
    sel = imgs[::8]
    w, h = x1 - x0, y1 - y0
    cols = 6
    rows = (len(sel) + cols - 1) // cols
    pl = Image.new("RGB", (w * cols, h * rows * 2))
    for half, fond in enumerate([(25, 25, 35), (240, 240, 235)]):
        for i, a in enumerate(sel):
            tuile = Image.new("RGBA", (w, h), fond + (255,))
            tuile.alpha_composite(Image.fromarray(a[y0:y1, x0:x1], "RGBA"))
            pl.paste(tuile.convert("RGB"), ((i % cols) * w, (half * rows + i // cols) * h))
    pl.thumbnail((2000, 2000))
    pl.save(dst.rstrip("/\\") + "_controle.png")
