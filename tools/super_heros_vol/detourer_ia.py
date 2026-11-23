# Detourage par IA (BiRefNet, ZhengPeng7/BiRefNet, licence MIT) : pour les heros dont le costume
# contient du vert, ou le fond vert ne peut pas servir. Fond gris uni en entree.
# Meme sortie que detourer.py : PNG RGBA recadres en commun + planche de controle.
#   python detourer_ia.py <dossier_images | image.png> <dossier_sortie>
import glob, os, sys
import numpy as np
import torch
from PIL import Image
from scipy import ndimage
from transformers import AutoModelForImageSegmentation

TAILLE = 1024
torch.set_float32_matmul_precision("high")
_modele = None


def modele():
    global _modele
    if _modele is None:
        m = AutoModelForImageSegmentation.from_pretrained("ZhengPeng7/BiRefNet", trust_remote_code=True)
        _modele = m.to("cuda").eval().half()
    return _modele


@torch.no_grad()
def alpha_de(lot):
    """lot : liste d'images PIL RGB -> liste de masques float 0..1 a la taille d'origine."""
    moy = torch.tensor([0.485, 0.456, 0.406]).view(3, 1, 1)
    ect = torch.tensor([0.229, 0.224, 0.225]).view(3, 1, 1)
    x = torch.stack([(torch.from_numpy(np.asarray(i.resize((TAILLE, TAILLE), Image.BILINEAR), np.float32) / 255).permute(2, 0, 1) - moy) / ect for i in lot])
    sortie = modele()(x.to("cuda").half())[-1].sigmoid().float().cpu()
    return [np.asarray(Image.fromarray((s[0].numpy() * 255).astype(np.uint8)).resize(i.size, Image.BILINEAR), np.float32) / 255
            for s, i in zip(sortie, lot)]


def nettoyer(a):
    a = np.where(a < 0.04, 0, a)
    lab, n = ndimage.label(a > 0.1, structure=np.ones((3, 3)))
    if n > 1:
        tailles = ndimage.sum(np.ones_like(a), lab, range(1, n + 1))
        garder = np.zeros(n + 1, bool)
        garder[1:] = tailles >= 0.015 * tailles.max()
        a = a * ndimage.binary_dilation(garder[lab], iterations=2)
    return a


def defond(rgb, a, fond):
    """Retire la contribution du fond gris dans les bords semi-transparents (C = a*F + (1-a)*B)."""
    f = rgb.astype(np.float32) / 255
    a3 = a[..., None]
    avant = np.where(a3 > 0.02, (f - (1 - a3) * fond) / np.maximum(a3, 0.02), 0)
    return np.clip(avant, 0, 1)


if __name__ == "__main__":
    src, dst = sys.argv[1], sys.argv[2]
    os.makedirs(dst, exist_ok=True)
    fs = [src] if src.lower().endswith(".png") else sorted(glob.glob(os.path.join(src, "*.png")))
    images = [Image.open(f).convert("RGB") for f in fs]
    alphas = []
    for k in range(0, len(images), 4):
        alphas += alpha_de(images[k:k + 4])
    sorties = []
    for im, a in zip(images, alphas):
        rgb = np.asarray(im)
        bords = np.concatenate([rgb[:12, :12].reshape(-1, 3), rgb[:12, -12:].reshape(-1, 3), rgb[-12:, :12].reshape(-1, 3), rgb[-12:, -12:].reshape(-1, 3)])
        fond = np.median(bords, 0).astype(np.float32) / 255
        a = nettoyer(a)
        sorties.append(np.dstack([defond(rgb, a, fond), a]))
    union = np.zeros(sorties[0].shape[:2], bool)
    for o in sorties:
        union |= o[..., 3] > 0.03
    ys, xs = np.where(union)
    m = 4
    y0, y1 = max(ys.min() - m, 0), min(ys.max() + m + 1, union.shape[0])
    x0, x1 = max(xs.min() - m, 0), min(xs.max() + m + 1, union.shape[1])
    for k, o in enumerate(sorties):
        Image.fromarray((o[y0:y1, x0:x1] * 255 + 0.5).astype(np.uint8), "RGBA").save(os.path.join(dst, f"{k:03d}.png"))
    print(f"{len(sorties)} images, cadre {x1-x0}x{y1-y0}, touche le bord: "
          f"{ys.min()==0 or xs.min()==0 or ys.max()==union.shape[0]-1 or xs.max()==union.shape[1]-1}")
    # planche de controle : 1 image sur 8, sur fond sombre puis clair
    sel = sorties[::8]
    w, h = x1 - x0, y1 - y0
    cols = 6
    rows = (len(sel) + cols - 1) // cols
    pl = Image.new("RGB", (w * min(cols, len(sel)), h * rows * 2))
    for half, fond in enumerate([(25, 25, 35), (240, 240, 235)]):
        for i, o in enumerate(sel):
            tuile = Image.new("RGBA", (w, h), fond + (255,))
            tuile.alpha_composite(Image.fromarray((o[y0:y1, x0:x1] * 255 + 0.5).astype(np.uint8), "RGBA"))
            pl.paste(tuile.convert("RGB"), ((i % cols) * w, (half * rows + i // cols) * h))
    pl.thumbnail((2000, 2000))
    pl.save(dst.rstrip("/\\") + "_controle.png")
