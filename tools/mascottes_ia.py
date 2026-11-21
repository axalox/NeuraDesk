# Les mascottes dont le CORPS vient d'une IA (2026-11-21) : une image fixe par
# personnage, generee UNE fois par FLUX.1-schnell (Apache-2.0, Space Hugging Face
# black-forest-labs/FLUX.1-schnell) et gardee dans `tools/mascottes_ia/` — la
# regeneration ne depend donc PAS du Space.
#
# Le corps ne bouge jamais : seuls les yeux changent. Les yeux de l'IA sont
# DANS des verres (lunettes, visiere) : on les decoupe, on comble le verre par
# un degrade ajuste dessus, puis
#   - directions : l'iris d'origine glisse dans le verre (9 regards) ;
#   - reactions  : yeux, bouches et decors de `mascottes_dessinees.py`, dans le
#                  meme ordre que les autres mascottes.
# Puis les cinq styles du renard (`mascottes_styles.py`), filtres case par case.
#
#   python tools/mascottes_ia.py <dossier de travail> <assets/popups/mascottes> [personnage…]
#
# Les planches sont en WebP SANS PERTE : avec perte, la compression differait
# d'une case a l'autre (jusqu'a 11 niveaux) et le corps scintillait quand le
# regard changeait.
import base64
import os
import subprocess
import sys

import cv2
import numpy as np
from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mascottes_dessinees as md  # noqa: E402
import mascottes_styles as styles  # noqa: E402
import mascottes_tetes as tetes  # noqa: E402

ICI = os.path.dirname(os.path.abspath(__file__))

# Les mesures de chaque image, en pixels de l'image IA (1024). `iris` : ellipses
# OpenCV ((cx, cy), (largeur, hauteur), angle) ; `verres` : (centre, (rx, ry))
# de l'interieur du verre ou de la visiere ; `bouche` : ellipse a effacer (None :
# pas de bouche, le bec du hibou reste) ; `sain` : la luminance du verre vide
# (claire pour des lunettes, sombre pour la visiere de Neura) ; `trait_yeux` :
# la couleur des yeux dessines (sur une visiere sombre, le trait brun ne se
# verrait pas). La boite de la figure se mesure au detourage.
# Les trois dernieres viennent du serveur ComfyUI LOCAL (`tools/comfy_flux.py`).
_STYLE = (", head and upper body, facing the viewer, die-cut sticker style, thick dark brown outline, flat colors "
          "with soft cel shading, simple clean shapes, isolated on a plain pure white background, centered, no text")
PERSOS = {
    "nd-matou": dict(
        image="matou.webp", graine=11, source="Space Hugging Face FLUX.1-schnell",
        invite="cute kawaii ginger tabby cat mascot character wearing big round glasses" + _STYLE,
        iris=[((398.5, 541.3), (104, 122), 177), ((610.7, 534.0), (105, 130), 177)],
        verres=[((385, 534), (93, 102)), ((631, 528), (88, 104))],
        bouche=((506, 631), (96, 30)),
        tete=dict(x="cat", acc=" and glasses", corps="Its body", cou=(640, 672),
                  graines=dict(gauche=3, haut=4, bas=7, hautgauche=4, basgauche=3)),
    ),
    "nd-pieuvre": dict(
        image="pieuvre.png", graine=102, source="ComfyUI local, flux1-schnell-fp8",
        invite="cute kawaii pink octopus mascot character wearing big round glasses with large clear lenses, big eyes "
               "behind the lenses, short curly tentacles at the bottom, compact body" + _STYLE,
        iris=[((409.0, 426.1), (91, 98), 20), ((605.2, 426.1), (91, 97), 163)],
        verres=[((387, 409), (81, 82)), ((624, 409), (81, 82))],
        bouche=((509, 534), (132, 58)),
        tete=dict(x="octopus", acc=" and glasses", corps="Its body with its tentacles", cou=(560, 605),
                  cote=(228, 520), graines=dict(gauche=3, haut=4, bas=7, hautgauche=3, basgauche=3)),
    ),
    "nd-hibou": dict(
        image="hibou.png", graine=204, source="ComfyUI local, flux1-schnell-fp8",
        invite="cute kawaii blue-grey owl mechanic mascot character wearing big round brass aviator goggles with clear "
               "glass lenses over its big eyes, little orange beak, holding a small wrench" + _STYLE,
        iris=[((374.3, 385.3), (131, 147), 43), ((643.1, 386.2), (132, 148), 134)],
        verres=[((356, 365), (106, 116)), ((656, 366), (105, 116))],
        bouche=None,
        tete=dict(x="owl", acc=" and goggles", corps="Its body with its wings, its wrench and its feet", cou=(520, 560),
                  graines=dict(gauche=3, haut=3, bas=None, hautgauche=3, basgauche=3)),
    ),
    "nd-neura": dict(
        image="neura.png", graine=304, source="ComfyUI local, flux1-schnell-fp8",
        invite="cute kawaii small round white robot mascot character, its whole face is one dark glossy rounded screen "
               "visor showing two big round glowing cyan eyes, small antenna with a coral ball tip, small arms" + _STYLE,
        iris=[((410.8, 488.8), (110, 110), 0), ((605.3, 486.5), (110, 110), 0)],
        verres=[((507, 474), (231, 159)), ((507, 474), (231, 159))],
        bouche=((510, 681), (84, 28)),
        sain=(5, 110), trait_yeux="#3ff6ff", pas=(30, 26),
        tete=dict(x="robot", acc=" and its screen visor", corps="Its body with its arms and feet", cou=(690, 725),
                  graines=dict(gauche=3, haut=3, bas=7, hautgauche=3, basgauche=3)),
    ),
}
HAUTEUR = 286            # hauteur maximale de la figure dans la case (300)
LARGEUR = 292            # largeur maximale (la pieuvre est plus large que haute)


def remplir_lisse(img, a_remplir, sain, degre2_croise=True):
    """Comble `a_remplir` par un polynome de degre 2 ajuste sur les pixels `sain`."""
    H, L = a_remplir.shape
    yy, xx = np.mgrid[0:H, 0:L]

    def termes(m):
        t = [np.ones(m.sum()), xx[m], yy[m], xx[m] ** 2, yy[m] ** 2]
        if degre2_croise:
            t.append(xx[m] * yy[m])
        return np.stack(t, 1).astype(float)

    X, Xr = termes(sain), termes(a_remplir)
    out = img.copy()
    for k in range(3):
        coef, *_ = np.linalg.lstsq(X, img[..., k][sain].astype(float), rcond=None)
        out[..., k][a_remplir] = np.clip(Xr @ coef, 0, 255).astype(np.uint8)
    # adoucir la couture
    noyau = np.ones((5, 5), np.uint8)
    m = a_remplir.astype(np.uint8) * 255
    couture = cv2.dilate(m, noyau) & ~cv2.erode(m, noyau)
    flou = cv2.GaussianBlur(out, (0, 0), 1.8)
    out[couture > 0] = flou[couture > 0]
    return out


def ellipse(H, L, e, marge):
    m = np.zeros((H, L), np.uint8)
    (cx, cy), (w, h), a = e
    cv2.ellipse(m, ((cx, cy), (w + 2 * marge, h + 2 * marge), a), 255, -1, cv2.LINE_AA)
    return m


def detourer(rgb):
    """Le fond blanc, rempli depuis le bord (le blanc de la poitrine et des
    reflets reste), puis le bord rogne d'un pixel et adouci : pas de halo
    blanc sur un fond d'ecran sombre."""
    H, L = rgb.shape[:2]
    fond = np.zeros((H + 2, L + 2), np.uint8)
    drapeaux = 8 | cv2.FLOODFILL_MASK_ONLY | (255 << 8)
    bord = [(x, y) for x in range(L) for y in (0, H - 1)] + [(x, y) for y in range(H) for x in (0, L - 1)]
    for x, y in bord:
        if rgb[y, x].min() > 235 and fond[y + 1, x + 1] == 0:
            cv2.floodFill(rgb.copy(), fond, (x, y), (0, 0, 0), (12,) * 3, (12,) * 3, drapeaux)
    plein = (fond[1:-1, 1:-1] == 0).astype(np.uint8) * 255
    plein = cv2.erode(plein, np.ones((3, 3), np.uint8))
    return cv2.GaussianBlur(plein, (0, 0), 0.8)


def preparer(p, travail):
    """Ecrit dans `travail` : la base sans yeux, la base sans yeux ni bouche,
    et un sprite par iris. Rend les boites des iris."""
    rgb = np.asarray(Image.open(os.path.join(ICI, "mascottes_ia", p["image"])).convert("RGB")).copy()
    H, L = rgb.shape[:2]
    alpha = detourer(rgb)
    lum = rgb.mean(axis=2)
    yy, xx = np.mgrid[0:H, 0:L]
    trou = np.zeros((H, L), np.uint8)
    boites = []
    for i, e in enumerate(p["iris"]):
        trou |= ellipse(H, L, e, 11)
        m = cv2.GaussianBlur(ellipse(H, L, e, 3), (0, 0), 1.0)
        (cx, cy), (w, h), _ = e
        b = (int(cx - w / 2 - 12), int(cy - h / 2 - 12), int(cx + w / 2 + 12), int(cy + h / 2 + 12))
        Image.fromarray(np.dstack([rgb, m])[b[1]:b[3], b[0]:b[2]]).save(os.path.join(travail, f"iris{i}.png"))
        boites.append(b)
    sans_yeux = rgb
    for c, (rx, ry) in p["verres"]:
        dedans = ((xx - c[0]) / (rx - 4)) ** 2 + ((yy - c[1]) / (ry - 4)) ** 2 <= 1
        bas, haut = p.get("sain", (120, 235))
        sain = dedans & (trou == 0) & (lum > bas) & (lum < haut)
        sans_yeux = remplir_lisse(sans_yeux, dedans & (trou > 0), sain)
    ys, xs = np.nonzero(alpha > 128)
    boite = (int(xs.min()), int(ys.min()), int(xs.max()), int(ys.max()))
    if p["bouche"] is None:
        for nom in ("base_yeux_vides", "base_sans_bouche"):
            Image.fromarray(np.dstack([sans_yeux, alpha])).save(os.path.join(travail, f"{nom}.png"))
        return boites, boite
    (bx, by), (bw, bh) = p["bouche"]
    bouche = np.zeros((H, L), np.uint8)
    cv2.ellipse(bouche, ((bx, by), (bw, bh), 0), 255, -1)
    anneau = np.zeros((H, L), np.uint8)
    cv2.ellipse(anneau, ((bx, by + 3), (bw * 1.55, bh * 2), 0), 255, -1)
    lum2 = sans_yeux.mean(axis=2)
    sain = (anneau > 0) & (bouche == 0) & (lum2 > 170) & (yy > by - bh / 2 - 4)
    sans_bouche = remplir_lisse(sans_yeux, bouche > 0, sain, degre2_croise=False)
    for nom, im in (("base_yeux_vides", sans_yeux), ("base_sans_bouche", sans_bouche)):
        Image.fromarray(np.dstack([im, alpha])).save(os.path.join(travail, f"{nom}.png"))
    return boites, boite


def regard_seul(p, travail, boites, dx, dy):
    """Le visage de face, l'iris decale de (dx, dy) crans dans son verre : la
    meme chose que les cases de `planches`, mais en pixels (RGBA 1024)."""
    base = Image.open(os.path.join(travail, "base_yeux_vides.png")).convert("RGBA")
    r = p["verres"][0][1]
    kx, ky = p.get("pas", (0.27 * r[0], 0.24 * r[1]))
    for i, b in enumerate(boites):
        calque = Image.new("RGBA", base.size)
        calque.alpha_composite(Image.open(os.path.join(travail, f"iris{i}.png")).convert("RGBA"),
                               (round(b[0] + dx * kx), round(b[1] + dy * ky)))
        (cx, cy), (rx, ry) = p["verres"][i]
        verre = np.zeros((base.height, base.width), np.uint8)
        cv2.ellipse(verre, (cx, cy), (rx - 3, ry - 3), 0, 0, 360, 255, -1, cv2.LINE_AA)
        a = np.asarray(calque).copy()
        a[..., 3] = (a[..., 3].astype(np.uint16) * verre // 255).astype(np.uint8)
        base.alpha_composite(Image.fromarray(a))
    return np.asarray(base)


def b64(chemin):
    with open(chemin, "rb") as f:
        return base64.b64encode(f.read()).decode()


REACT = {0: ("ferme", "ferme", None), 1: ("heureux", "heureux", "ouverte"), 2: ("etoile", "etoile", "ouverte"),
         3: ("grand", "grand", "o"), 4: ("ouvert", "heureux", "langue"), 5: ("bas", "bas", "petit"),
         6: ("ferme", "ferme", "o"), 7: ("spirale", "spirale", "vague"), 8: ("heureux", "heureux", "ouverte")}


def planches(p, travail, boites, boite):
    x0, y0, x1, y1 = boite
    S = min(HAUTEUR / (y1 - y0), LARGEUR / (x1 - x0))
    BX = (x0 + x1) / 2

    def u(x):
        return 150 + (x - BX) * S

    def v(y):  # les pieds touchent le bas de la case
        return 297 - (y1 - y) * S

    defs = (f'<defs><image id="yv" href="data:image/png;base64,{b64(os.path.join(travail, "base_yeux_vides.png"))}" width="1024" height="1024"/>'
            f'<image id="sb" href="data:image/png;base64,{b64(os.path.join(travail, "base_sans_bouche.png"))}" width="1024" height="1024"/>'
            + "".join(f'<image id="i{i}" href="data:image/png;base64,{b64(os.path.join(travail, f"iris{i}.png"))}" '
                      f'width="{b[2] - b[0]}" height="{b[3] - b[1]}"/>' for i, b in enumerate(boites))
            + "".join(f'<clipPath id="l{i}"><ellipse cx="{c[0]}" cy="{c[1]}" rx="{r[0] - 3}" ry="{r[1] - 3}"/></clipPath>'
                      for i, (c, r) in enumerate(p["verres"]))
            + '<clipPath id="case"><rect width="300" height="300"/></clipPath>'
            + "</defs>")
    repere = f'transform="translate({u(0):.3f} {v(0):.3f}) scale({S:.5f})"'
    r = p["verres"][0][1]
    kx, ky = p.get("pas", (0.27 * r[0], 0.24 * r[1]))

    def iris(i, dx, dy, k=1.0):
        bx0, by0, bx1, by1 = boites[i]
        cx, cy = (bx0 + bx1) / 2, (by0 + by1) / 2
        t = f"translate({cx + dx * kx:.1f} {cy + dy * ky:.1f}) scale({k}) translate({-cx:.1f} {-cy:.1f})"
        return f'<g clip-path="url(#l{i})"><use href="#i{i}" x="{bx0}" y="{by0}" transform="{t}"/></g>'

    yeux = [(u(e[0][0]), v(e[0][1])) for e in p["iris"]]
    oeil_y = sum(e[0][1] for e in p["iris"]) / 2
    milieu = sum(e[0][0] for e in p["iris"]) / 2
    (iw, ih) = p["iris"][0][1]
    ecart = (yeux[1][0] - yeux[0][0]) / 2
    bouche_x, bouche_y = p["bouche"][0] if p["bouche"] else (milieu, oeil_y)
    sp = dict(oeil=dict(rx=iw / 2 * S * 0.92, ry=ih / 2 * S * 0.82, ecart=ecart, y=0), bouche_y=v(bouche_y) - v(oeil_y),
              bouche_l=1.15, joue_dx=6, joue_dy=40)
    trait = p.get("trait_yeux")

    def case(direction=None, reaction=None):
        if direction is not None:
            dx, dy = direction % 3 - 1, direction // 3 - 1
            return f'<g {repere}><use href="#yv"/>{iris(0, dx, dy)}{iris(1, dx, dy)}</g>'
        g, d, bouche = REACT[reaction]
        # les « z » d'endormi montent plus haut que les autres decors : sans ce
        # cran, le dernier debordait dans la case du dessus.
        c = dict(dx=0, dy=0, HX=150, HY=142 if reaction == 6 else 128, FX=u(bouche_x), FY=v(oeil_y), regard=(0, 0))
        px, vec = "", (md.joues(c, sp, fort=True) if reaction == 5 else "")
        for i, sorte in enumerate((g, d)):
            if sorte in ("ouvert", "grand", "bas"):
                px += iris(i, 0, 1 if sorte == "bas" else 0, 1.12 if sorte == "grand" else 1.0)
            else:
                o = md.oeil(c, sp, yeux[i][0], yeux[i][1] + (4 if reaction == 6 else 0), sorte, 1.0)
                vec += o.replace(md.TRAIT, trait) if trait else o
        if bouche and p["bouche"]:
            vec += md.bouche(c, sp, bouche)
        fond = "yv" if bouche is None or not p["bouche"] else "sb"
        return f'<g {repere}><use href="#{fond}"/>{px}</g>{vec}{md.decor(c, reaction)}'

    rendus = {}
    for sorte in ("directions", "reactions"):
        corps = "".join(f'<g transform="translate({(i % 3) * 300} {(i // 3) * 300})" clip-path="url(#case)">'
                        + (case(direction=i) if sorte == "directions" else case(reaction=i)) + "</g>" for i in range(9))
        html = os.path.join(travail, f"{sorte}.html")
        png = os.path.join(travail, f"{sorte}.png")
        with open(html, "w", encoding="utf-8") as f:
            f.write(f'<html><body style="margin:0;background:transparent"><svg xmlns="http://www.w3.org/2000/svg" '
                    f'width="{md.RENDU}" height="{md.RENDU}" viewBox="0 0 900 900">{defs}{corps}</svg></body></html>')
        subprocess.run([md.EDGE, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--default-background-color=00000000",
                        f"--screenshot={png}", f"--window-size={md.RENDU},{md.RENDU}", "file:///" + html.replace(os.sep, "/")],
                       check=True, capture_output=True)
        rendus[sorte] = Image.open(png).convert("RGBA")
    return rendus


def ecrire(dest, nom, feuilles):
    for sorte, im in feuilles.items():
        if im.width != 768:
            im = im.resize((768, 768), Image.LANCZOS)
        im.save(os.path.join(dest, f"{nom}-{sorte}.webp"), "WEBP", lossless=True, quality=100, method=6)
        if sorte == "directions":
            im.crop((256, 256, 512, 512)).resize((96, 96), Image.LANCZOS).save(
                os.path.join(dest, f"{nom}-vignette.webp"), "WEBP", lossless=True, quality=100, method=6)


def main():
    travail, dest = sys.argv[1], sys.argv[2]
    for nom in sys.argv[3:] or list(PERSOS):
        t = os.path.join(travail, nom)
        os.makedirs(t, exist_ok=True)
        boites, boite = preparer(PERSOS[nom], t)
        rendus = planches(PERSOS[nom], t, boites, boite)
        if "graines" in PERSOS[nom].get("tete", {}):  # la tete tourne (`mascottes_tetes.py`)
            rendus["directions"] = tetes.planche(PERSOS[nom], nom, boite, detourer,
                                                 lambda dx, dy: regard_seul(PERSOS[nom], t, boites, dx, dy))
        ecrire(dest, nom, rendus)
        for style in styles.STYLES:
            ecrire(dest, f"{nom}-{style}", styles.styliser(rendus, style))
        print(nom, "ok")


if __name__ == "__main__":
    main()
