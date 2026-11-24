# Les super-heros a l'encre et en pixels (2026-11-23), avec les filtres des mascottes
# (`tools/mascottes_styles.py` : memes reglages, memes graines) adaptes a une case
# rectangulaire. Les planches gardent leurs DIMENSIONS : le decoupage et la fiche
# `heros-NN-atterrissage.txt` restent valables.
#
#   python tools/super_heros_vol/styles_heros.py <dossier_mascottes> [n...]
#
# Ecrit heros-NN-<style>.webp (traversee) et heros-NN-<style>-{approche,atterrissage,marche}.webp.
# Pour un heros, UNE palette (pixels) et UN grain (encre) servent a toutes ses planches :
# les raccords de l'atterrissage tombent au pixel comme en couleur.
# Pixels : blocs de BLOC px ancres au coin de chaque case, palette de 20 couleurs, alpha
# tout ou rien, WebP SANS PERTE. ⛔ BLOC doit rester egal a `BLOC_PIXELS` de
# src/win32/super_heros.rs (l'application relit la grille au centre des blocs).
import glob
import os
import sys

import cv2
import numpy as np
from PIL import Image

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import mascottes_styles as ms  # noqa: E402

COLONNES, LIGNES = 8, 10
BLOC = 5
ECHELLE_ENCRE = 2  # le filtre a l'encre tourne a 2x (comme les mascottes, rendues a 512 puis 256)
CLIPS = ("approche", "atterrissage", "marche")


def grain(forme: tuple, graine: int, sigma: float, amp: float) -> np.ndarray:
    g = np.random.default_rng(graine).standard_normal(forme).astype(np.float32)
    g = cv2.GaussianBlur(g, (0, 0), sigma)
    return 1 + amp * g / g.std()


def encre(rgb: np.ndarray, g: np.ndarray) -> np.ndarray:
    """`mascottes_styles.Encre`, sur une case de n'importe quelle forme."""
    gr = ms.gris(cv2.bilateralFilter(rgb, 7, 40, 5))
    gr = np.clip((gr - 40) * 1.3 + 30, 0, 255)
    bords = cv2.adaptiveThreshold(ms.gris(rgb).astype(np.uint8), 255, cv2.ADAPTIVE_THRESH_MEAN_C,
                                  cv2.THRESH_BINARY, 9, 6).astype(np.float32) / 255
    gr = np.clip(gr * (0.55 + 0.45 * bords) * g, 0, 255)
    return np.dstack([gr, gr, gr]).astype(np.uint8)


def case_encre(c: np.ndarray, g: np.ndarray) -> np.ndarray:
    h, w = c.shape[:2]
    grand = cv2.resize(c, (w * ECHELLE_ENCRE, h * ECHELLE_ENCRE), interpolation=cv2.INTER_CUBIC)
    rgb = encre(np.ascontiguousarray(grand[..., :3]), g)
    rgb = cv2.resize(rgb, (w, h), interpolation=cv2.INTER_AREA)
    return np.dstack([rgb, c[..., 3]])


def case_pixel(c: np.ndarray, pal: np.ndarray) -> np.ndarray:
    h, w = c.shape[:2]
    gh, gw = -(-h // BLOC), -(-w // BLOC)
    plein = np.zeros((gh * BLOC, gw * BLOC, 4), np.uint8)
    plein[:h, :w] = c
    petit = cv2.resize(plein, (gw, gh), interpolation=cv2.INTER_AREA)
    rgb = ms.quantifie(petit[..., :3], pal)
    a = np.where(petit[..., 3] > 110, 255, 0).astype(np.uint8)
    rgb[a == 0] = 0
    p = np.dstack([rgb, a])
    return cv2.resize(p, (gw * BLOC, gh * BLOC), interpolation=cv2.INTER_NEAREST)[:h, :w]


def cases(planche: np.ndarray, cw: int, ch: int, n: int):
    for i in range(n):
        x, y = (i % COLONNES) * cw, (i // COLONNES) * ch
        yield i, x, y, planche[y:y + ch, x:x + cw]


def palette_du_heros(planches: list) -> np.ndarray:
    """20 couleurs tirees de toutes les planches du heros (une case sur 8)."""
    px = []
    for p, cw, ch, n in planches:
        for i, _, _, c in cases(p, cw, ch, n):
            if i % 8 == 0:
                px.append(c[c[..., 3] > 200][:, :3])
    tout = np.concatenate(px)
    a = np.zeros((len(tout), 1), np.uint8) + 255
    return ms.palette(tout.reshape(-1, 1, 3), a, 20)


def styliser(p: np.ndarray, cw: int, ch: int, n: int, style: str, outil) -> np.ndarray:
    out = np.zeros_like(p)
    for _, x, y, c in cases(p, cw, ch, n):
        out[y:y + ch, x:x + cw] = case_encre(c, outil) if style == "ink" else case_pixel(c, outil)
    return out


def ecrire(chemin: str, rgba: np.ndarray, style: str) -> None:
    im = Image.fromarray(rgba, "RGBA")
    if style == "pixel":
        im.save(chemin, "WEBP", lossless=True, quality=100, method=6)
    else:
        im.save(chemin, "WEBP", quality=88, alpha_quality=100, method=6)


def heros(d: str, n: str) -> None:
    planches = {}
    trav = os.path.join(d, f"heros-{n}.webp")
    if os.path.exists(trav):
        p = np.asarray(Image.open(trav).convert("RGBA"))
        planches[""] = (p, p.shape[1] // COLONNES, p.shape[0] // LIGNES, COLONNES * LIGNES)
    fiche = os.path.join(d, f"heros-{n}-atterrissage.txt")
    if os.path.exists(fiche):
        f = {k: int(v) for k, v in (ligne.strip().split("=") for ligne in open(fiche) if "=" in ligne)}
        for clip in CLIPS:
            p = np.asarray(Image.open(os.path.join(d, f"heros-{n}-{clip}.webp")).convert("RGBA"))
            planches["-" + clip] = (p, f["largeur"], f["hauteur"], f[clip])
    pal = palette_du_heros(list(planches.values()))
    for style in ("ink", "pixel"):
        for suffixe, (p, cw, ch, k) in planches.items():
            outil = pal if style == "pixel" else grain((ch * ECHELLE_ENCRE, cw * ECHELLE_ENCRE), 11, 0.8, 0.04)
            ecrire(os.path.join(d, f"heros-{n}-{style}{suffixe}.webp"), styliser(p, cw, ch, k, style, outil), style)
    print(n, "ok", flush=True)


def main() -> None:
    d = sys.argv[1]
    nums = sys.argv[2:] or sorted({os.path.basename(f)[6:8] for f in glob.glob(os.path.join(d, "heros-[0-9][0-9]*.webp"))})
    for n in nums:
        heros(d, f"{int(n):02d}")


if __name__ == "__main__":
    main()
