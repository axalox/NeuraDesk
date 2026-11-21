# Styles du renard appliques a une planche couleur rendue (1536 px, 3x3 cases de 512).
# Chaque case est filtree SEULE ; textures et palettes sont fixees une fois par style,
# pour que rien ne bouge d'une case a l'autre hors des yeux.
import sys, numpy as np, cv2
from PIL import Image
C = 512          # case au rendu
F = 256          # case a la sortie (planche 768)
def rng(n): return np.random.default_rng(n)
def grain(seed, sigma, amp):
    g = rng(seed).standard_normal((C, C)).astype(np.float32)
    g = cv2.GaussianBlur(g, (0, 0), sigma)
    return 1 + amp * g / g.std()
def palette(base_rgb, base_a, k, seed=1):
    px = base_rgb[base_a > 200].reshape(-1, 3).astype(np.float32)
    px = px[rng(seed).choice(len(px), min(40000, len(px)), replace=False)]
    _, _, centres = cv2.kmeans(px, k, None, (cv2.TERM_CRITERIA_EPS + cv2.TERM_CRITERIA_MAX_ITER, 50, 0.5), 4, cv2.KMEANS_PP_CENTERS)
    return centres
def quantifie(rgb, centres):
    d = ((rgb[..., None, :].astype(np.float32) - centres[None, None]) ** 2).sum(-1)
    return centres[d.argmin(-1)].astype(np.uint8)
def gris(rgb): return cv2.cvtColor(rgb, cv2.COLOR_RGB2GRAY).astype(np.float32)

class Encre:
    def __init__(s, base_rgb, base_a): s.g = grain(11, 0.8, 0.04)
    def __call__(s, rgb, a):
        g = gris(cv2.bilateralFilter(rgb, 7, 40, 5))
        g = np.clip((g - 40) * 1.3 + 30, 0, 255)
        bords = cv2.adaptiveThreshold(gris(rgb).astype(np.uint8), 255, cv2.ADAPTIVE_THRESH_MEAN_C, cv2.THRESH_BINARY, 9, 6).astype(np.float32) / 255
        g = g * (0.55 + 0.45 * bords) * s.g
        g = np.clip(g, 0, 255)
        return np.dstack([g, g, g]).astype(np.uint8)
class Crayon:
    def __init__(s, base_rgb, base_a):
        n = rng(12).standard_normal((C, C)).astype(np.float32)
        k = np.zeros((9, 9), np.float32); np.fill_diagonal(k, 1 / 9)   # hachures a 45 degres
        s.h = 1 + 0.18 * cv2.filter2D(n, -1, k[::-1]) / 0.33
    def __call__(s, rgb, a):
        g = gris(rgb)
        inv = 255 - cv2.GaussianBlur(g, (0, 0), 3)
        dodge = np.clip(g * 255 / np.maximum(255 - inv + 1, 1), 0, 255)       # trait de crayon
        ton = 255 - (255 - g) * 0.55                                           # ombrage leger
        out = np.clip(255 * (dodge / 255) ** 2.2 * (255 - (255 - g) * 0.8) / 255 * s.h, 0, 255)
        sep = np.dstack([out * 1.0, out * 0.97, out * 0.92])
        return np.clip(sep, 0, 255).astype(np.uint8)
class Riso:
    ENCRES = np.array([[255, 98, 52], [232, 64, 96], [0, 128, 140], [29, 45, 80], [250, 242, 228]], np.float32)
    def __init__(s, base_rgb, base_a):
        s.g = grain(13, 0.6, 0.07)
        yy, xx = np.mgrid[0:C, 0:C]
        s.trame = (np.sin(xx * 0.9) * np.sin(yy * 0.9)) * 0.5 + 0.5            # trame de points fixe
    def __call__(s, rgb, a):
        r = rgb.astype(np.float32)
        # melange de deux encres les plus proches, tramees
        d = ((r[..., None, :] - s.ENCRES[None, None]) ** 2).sum(-1)
        o = np.argsort(d, -1)
        e1, e2 = s.ENCRES[o[..., 0]], s.ENCRES[o[..., 1]]
        d1 = np.take_along_axis(d, o[..., :1], -1)[..., 0]; d2 = np.take_along_axis(d, o[..., 1:2], -1)[..., 0]
        t = np.sqrt(d1) / (np.sqrt(d1) + np.sqrt(d2) + 1e-6)
        choix = (s.trame < t)[..., None]
        out = np.where(choix, e2, e1)
        # decalage de reperage : l'encre sombre glisse de 2 px
        sombre = (gris(rgb) < 70)
        dec = np.roll(np.roll(sombre, 2, 0), 2, 1)
        out[dec] = out[dec] * 0.55 + s.ENCRES[3] * 0.45
        out[sombre] = s.ENCRES[3]
        return np.clip(out * s.g[..., None], 0, 255).astype(np.uint8)
class Papier:
    def __init__(s, base_rgb, base_a):
        s.pal = palette(base_rgb, base_a, 9)
        f = rng(14).standard_normal((C, C)).astype(np.float32)
        s.g = 1 + 0.06 * cv2.GaussianBlur(f, (0, 0), 1.2) / cv2.GaussianBlur(f, (0, 0), 1.2).std() \
                + 0.04 * cv2.GaussianBlur(f, (0, 0), 6) / cv2.GaussianBlur(f, (0, 0), 6).std()
    def __call__(s, rgb, a):
        q = quantifie(cv2.medianBlur(rgb, 5), s.pal)
        q = cv2.medianBlur(q, 5).astype(np.float32)
        return np.clip(q * s.g[..., None], 0, 255).astype(np.uint8)
class Pixel:
    BLOC = 4
    def __init__(s, base_rgb, base_a): s.pal = palette(base_rgb, base_a, 20)
    def sortie(s, rgba512):
        petit = cv2.resize(rgba512, (F // s.BLOC, F // s.BLOC), interpolation=cv2.INTER_AREA)
        rgb, a = petit[..., :3], petit[..., 3]
        rgb = quantifie(rgb, s.pal)
        a = np.where(a > 110, 255, 0).astype(np.uint8)
        p = np.dstack([rgb, a])
        return cv2.resize(p, (F, F), interpolation=cv2.INTER_NEAREST)

STYLES = {'ink': Encre, 'sketch': Crayon, 'riso': Riso, 'paper': Papier, 'pixel': Pixel}

def cases(im):
    a = np.asarray(im.convert('RGBA'))
    return [a[(i // 3) * C:(i // 3 + 1) * C, (i % 3) * C:(i % 3 + 1) * C] for i in range(9)]
def styliser(planches, style):
    """planches : dict sorte -> image 1536 RGBA. Rend dict sorte -> image 768 RGBA."""
    base = cases(planches['directions'])[4]
    filtre = STYLES[style](base[..., :3].copy(), base[..., 3].copy())
    out = {}
    for sorte, im in planches.items():
        feuille = np.zeros((3 * F, 3 * F, 4), np.uint8)
        for i, c in enumerate(cases(im)):
            if style == 'pixel':
                r = filtre.sortie(c)
            else:
                rgb = filtre(c[..., :3].copy(), c[..., 3])
                a = c[..., 3:].astype(np.float32) / 255
                # bord doux : pas de grain sur la frange (halo sur fond sombre)
                r = np.dstack([rgb, c[..., 3]])
                r = cv2.resize(r, (F, F), interpolation=cv2.INTER_AREA)
            feuille[(i // 3) * F:(i // 3 + 1) * F, (i % 3) * F:(i % 3 + 1) * F] = r
        out[sorte] = Image.fromarray(feuille, 'RGBA')
    return out
