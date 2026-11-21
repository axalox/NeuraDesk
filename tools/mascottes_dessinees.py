# Les mascottes DESSINEES pour NeuraDesk (2026-11-14) : six personnages en
# vecteurs, au format de page-mascot — deux planches 3x3 :
#   directions : haut-gauche, haut, haut-droite, gauche, centre, droite,
#                bas-gauche, bas, bas-droite
#   reactions  : cligne, coeur, etincelles, etonne, clin d'oeil, timide,
#                endormi, etourdi, ravi
# Chaque planche est ecrite en SVG (900x900, cases de 300), rendue par Edge
# sans fond, puis reduite a 768 px WebP + vignette 96 px.
#
#   python tools/mascottes_dessinees.py <dossier de travail> <dossier assets/popups/mascottes>
import math
import os
import subprocess
import sys

from PIL import Image

TRAIT = "#3a2a2e"
EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
RENDU = 1536

_ids = [0]


def nouvel_id():
    _ids[0] += 1
    return f"c{_ids[0]}"


def ell(cx, cy, rx, ry, fill, stroke=TRAIT, sw=3.5, extra=""):
    s = f' stroke="{stroke}" stroke-width="{sw}"' if stroke else ""
    return f'<ellipse cx="{cx:.1f}" cy="{cy:.1f}" rx="{rx:.1f}" ry="{ry:.1f}" fill="{fill}"{s} {extra}/>'


def path(d, fill="none", stroke=TRAIT, sw=3.5, extra=""):
    s = f' stroke="{stroke}" stroke-width="{sw}" stroke-linecap="round" stroke-linejoin="round"' if stroke else ""
    return f'<path d="{d}" fill="{fill}"{s} {extra}/>'


def silhouette(formes, fill, sw=7):
    """Plusieurs formes fondues en UNE silhouette : contour epais d'abord, puis
    les remplissages par-dessus — seul le bord exterieur garde son trait."""
    contour = "".join(f.replace("{F}", TRAIT).replace("{S}", f'stroke="{TRAIT}" stroke-width="{sw}" stroke-linejoin="round"') for f in formes)
    plein = "".join(f.replace("{F}", fill).replace("{S}", "") for f in formes)
    return contour + plein


def e_(cx, cy, rx, ry, rot=0):
    r = f' transform="rotate({rot:.1f} {cx:.1f} {cy:.1f})"' if rot else ""
    return f'<ellipse cx="{cx:.1f}" cy="{cy:.1f}" rx="{rx:.1f}" ry="{ry:.1f}" fill="{{F}}" {{S}}{r}/>'


def p_(d):
    return f'<path d="{d}" fill="{{F}}" {{S}}/>'


# ------------------------------------------------------------------ visage


def oeil(c, sp, x, y, sorte, echelle_x):
    o = sp["oeil"]
    rx, ry = o["rx"] * echelle_x, o["ry"]
    lx, ly = c["regard"]
    out = ""
    if sorte in ("ouvert", "grand"):
        k = 1.18 if sorte == "grand" else 1.0
        rx, ry = rx * k, ry * k
        if o.get("blanc"):
            out += ell(x, y, rx, ry, "#ffffff", sw=3)
            pk = 0.42 if sorte == "grand" else 0.6
            px, py = x + lx * rx * 0.3, y + ly * ry * 0.38
            out += ell(px, py, rx * pk, ry * pk * 1.05, o["iris"], stroke=None)
            out += ell(px + rx * 0.18, py - ry * 0.22, rx * 0.17, rx * 0.17, "#ffffff", stroke=None)
        else:
            out += ell(x, y, rx, ry, o["iris"], sw=2.5)
            if o.get("facettes"):
                for fx, fy in ((-0.4, 0.2), (0.0, 0.45), (0.4, 0.2), (-0.2, -0.15), (0.25, -0.1)):
                    out += ell(x + fx * rx, y + fy * ry, rx * 0.16, rx * 0.16, o["facettes"], stroke=None, extra='opacity="0.7"')
            else:
                pk = 0.38 if sorte == "grand" else 0.55
                out += ell(x + lx * 2.5, y + ly * ry * 0.3 + ry * 0.12, rx * pk, ry * pk, o.get("pupille", "#1b1214"), stroke=None)
            out += ell(x - rx * 0.32 + lx * 2, y - ry * 0.38 + ly * 2, rx * 0.34, ry * 0.26, "#ffffff", stroke=None)
            out += ell(x + rx * 0.32 + lx * 2, y + ry * 0.36 + ly * 2, rx * 0.14, ry * 0.11, "#ffffff", stroke=None)
        if o.get("cils"):
            cote = 1 if x > c["FX"] else -1
            out += path(f"M{x + cote * rx * 0.75:.1f},{y - ry * 0.75:.1f} l{cote * 7:.1f},-6", sw=3)
            out += path(f"M{x + cote * rx * 0.98:.1f},{y - ry * 0.3:.1f} l{cote * 8:.1f},-3", sw=3)
    elif sorte == "ferme":  # ‿ paupiere baissee
        out += path(f"M{x - rx:.1f},{y:.1f} Q{x:.1f},{y + ry * 0.8:.1f} {x + rx:.1f},{y:.1f}", sw=4)
    elif sorte == "heureux":  # ∩
        out += path(f"M{x - rx:.1f},{y + ry * 0.35:.1f} Q{x:.1f},{y - ry * 0.9:.1f} {x + rx:.1f},{y + ry * 0.35:.1f}", sw=4.5)
    elif sorte == "etoile":
        r1, r2 = max(rx, ry) * 1.15, max(rx, ry) * 0.5
        pts = []
        for i in range(10):
            a = -math.pi / 2 + i * math.pi / 5
            r = r1 if i % 2 == 0 else r2
            pts.append(f"{x + r * math.cos(a):.1f},{y + r * math.sin(a):.1f}")
        out += f'<polygon points="{" ".join(pts)}" fill="#ffd23f" stroke="{TRAIT}" stroke-width="3" stroke-linejoin="round"/>'
    elif sorte == "spirale":
        d = f"M{x:.1f},{y:.1f}"
        r_max = max(rx, ry) * 1.05
        for i in range(1, 40):
            a = i * 0.45
            r = r_max * i / 39
            d += f" L{x + r * math.cos(a):.1f},{y + r * math.sin(a):.1f}"
        out += path(d, sw=3)
    elif sorte == "bas":  # timide : regarde en bas
        sauve = c["regard"]
        c["regard"] = (0, 1)
        out += oeil(c, sp, x, y + 2, "ouvert", echelle_x)
        c["regard"] = sauve
    return out


def bouche(c, sp, sorte):
    x, y = c["FX"], c["FY"] + sp.get("bouche_y", 36)
    w = 9 * sp.get("bouche_l", 1.0)
    coul = sp.get("bouche_trait", TRAIT)
    if sorte == "sourire":
        return path(f"M{x - w:.1f},{y - 2:.1f} Q{x:.1f},{y + w * 0.9:.1f} {x + w:.1f},{y - 2:.1f}", stroke=coul, sw=3.5)
    if sorte in ("ouverte", "langue"):
        k = 1.5 if sorte == "ouverte" else 1.1
        W = w * k
        d = f"M{x - W:.1f},{y - 3:.1f} Q{x:.1f},{y - 1:.1f} {x + W:.1f},{y - 3:.1f} Q{x + W * 0.8:.1f},{y + W * 1.25:.1f} {x:.1f},{y + W * 1.25:.1f} Q{x - W * 0.8:.1f},{y + W * 1.25:.1f} {x - W:.1f},{y - 3:.1f} Z"
        out = path(d, fill="#9e2b3a", stroke=TRAIT, sw=3)
        out += ell(x, y + W * 0.85, W * 0.5, W * 0.32, "#f07c8c", stroke=None)
        if sorte == "langue":
            out += path(f"M{x + 1:.1f},{y + W * 0.6:.1f} q2,14 10,12 q7,-2 4,-14", fill="#f07c8c", sw=3)
        return out
    if sorte == "o":
        return ell(x, y + 3, w * 0.55, w * 0.7, "#9e2b3a", sw=3)
    if sorte == "petit":
        return path(f"M{x - w * 0.5:.1f},{y:.1f} Q{x:.1f},{y + 4:.1f} {x + w * 0.5:.1f},{y:.1f}", stroke=coul, sw=3.5)
    if sorte == "vague":
        return path(f"M{x - w:.1f},{y + 2:.1f} q{w / 4:.1f},-5 {w / 2:.1f},0 t{w / 2:.1f},0 t{w / 2:.1f},0 t{w / 2:.1f},0", stroke=coul, sw=3.5)
    return ""


def joues(c, sp, fort=False):
    o = sp["oeil"]
    ecart = o["ecart"] + sp.get("joue_dx", 10)
    y = c["FY"] + o.get("y", 0) + sp.get("joue_dy", 24)
    op = 0.75 if fort else 0.45
    out = ""
    for s in (-1, 1):
        x = c["FX"] + s * ecart
        out += ell(x, y, 15 if fort else 12, 8 if fort else 6.5, "#ff7f9e", stroke=None, extra=f'opacity="{op}"')
        if fort:
            for k in (-6, 0, 6):
                out += path(f"M{x + k - 3:.1f},{y + 3:.1f} l5,-6", stroke="#e0507a", sw=2)
    if sp.get("taches_rousseur"):
        for s in (-1, 1):
            for fx, fy in ((-8, -4), (0, -7), (8, -3), (-3, 3), (5, 4)):
                out += ell(c["FX"] + s * ecart + fx, y - 8 + fy, 1.8, 1.8, "#b8643c", stroke=None, extra='opacity="0.8"')
    return out


def visage(c, sp, reaction):
    o = sp["oeil"]
    ecart = o["ecart"] * (1 - 0.1 * abs(c["dx"]))
    y = c["FY"] + o.get("y", 0)
    yeux = {
        None: ("ouvert", "ouvert"),
        0: ("ferme", "ferme"),
        1: ("heureux", "heureux"),
        2: ("etoile", "etoile"),
        3: ("grand", "grand"),
        4: ("ouvert", "heureux"),
        5: ("bas", "bas"),
        6: ("ferme", "ferme"),
        7: ("spirale", "spirale"),
        8: ("heureux", "heureux"),
    }[reaction]
    bouches = {None: "sourire", 0: "sourire", 1: "ouverte", 2: "ouverte", 3: "o", 4: "langue", 5: "petit", 6: "o", 7: "vague", 8: "ouverte"}
    out = joues(c, sp, fort=(reaction == 5))
    for s, sorte in ((-1, yeux[0]), (1, yeux[1])):
        loin = (s == c["dx"])
        out += oeil(c, sp, c["FX"] + s * ecart, y + (4 if reaction == 6 else 0), sorte, 0.82 if loin else 1.0)
    out += bouche(c, sp, bouches[reaction])
    return out


# ------------------------------------------------------------------ decors


def coeur(x, y, r):
    d = f"M{x:.1f},{y + r * 0.9:.1f} C{x - r * 1.6:.1f},{y - r * 0.2:.1f} {x - r * 0.8:.1f},{y - r * 1.3:.1f} {x:.1f},{y - r * 0.45:.1f} C{x + r * 0.8:.1f},{y - r * 1.3:.1f} {x + r * 1.6:.1f},{y - r * 0.2:.1f} {x:.1f},{y + r * 0.9:.1f} Z"
    return path(d, fill="#ff4d6d", sw=3)


def etincelle(x, y, r, coul="#ffd23f"):
    d = f"M{x:.1f},{y - r:.1f} Q{x + r * 0.18:.1f},{y - r * 0.18:.1f} {x + r:.1f},{y:.1f} Q{x + r * 0.18:.1f},{y + r * 0.18:.1f} {x:.1f},{y + r:.1f} Q{x - r * 0.18:.1f},{y + r * 0.18:.1f} {x - r:.1f},{y:.1f} Q{x - r * 0.18:.1f},{y - r * 0.18:.1f} {x:.1f},{y - r:.1f} Z"
    return path(d, fill=coul, sw=2.5)


def decor(c, reaction):
    x, y = c["HX"], c["HY"]
    if reaction == 1:
        return coeur(x + 70, y - 82, 16)
    if reaction == 2:
        return etincelle(x - 78, y - 80, 15) + etincelle(x + 80, y - 86, 18) + etincelle(x + 105, y - 45, 10)
    if reaction == 3:
        return path(f"M{x + 92:.1f},{y - 98:.1f} l-4,26", stroke="#ff8c42", sw=6) + ell(x + 87, y - 62, 3.5, 3.5, "#ff8c42", stroke=None)
    if reaction == 6:
        t = ""
        for i, (dx_, dy_, k) in enumerate(((70, -70, 16), (88, -92, 20), (108, -118, 25))):
            t += f'<text x="{x + dx_:.1f}" y="{y + dy_:.1f}" font-family="Arial Black, Arial" font-weight="900" font-size="{k}" fill="#6b8cff" stroke="{TRAIT}" stroke-width="1.5">z</text>'
        return t
    if reaction == 7:
        return etincelle(x - 50, y - 92, 10) + etincelle(x + 4, y - 104, 9, "#ffffff") + etincelle(x + 56, y - 92, 10)
    return ""


# ------------------------------------------------------------------ corps communs


def buste(fill, col=None, x=150, y0=208, l=62):
    d = f"M{x - l:.1f},292 C{x - l:.1f},{y0 + 20:.1f} {x - l * 0.7:.1f},{y0:.1f} {x:.1f},{y0:.1f} C{x + l * 0.7:.1f},{y0:.1f} {x + l:.1f},{y0 + 20:.1f} {x + l:.1f},292 Z"
    out = path(d, fill=fill, sw=3.5)
    if col:
        out += path(f"M{x - 20:.1f},{y0 + 2:.1f} Q{x:.1f},{y0 + 22:.1f} {x + 20:.1f},{y0 + 2:.1f}", fill=col, sw=3)
    return out


def tete(c, fill, rx=84, ry=78, dy=0):
    return ell(c["HX"], c["HY"] + dy, rx, ry, fill, sw=4)


# ------------------------------------------------------------------ personnages


def mouche(c):
    HX, HY, BX = c["HX"], c["HY"], c["BX"]
    out = ""
    for s in (-1, 1):  # ailes
        cx, cy = BX + s * 72, 196
        out += ell(cx, cy, 58, 30, "#d8efff", stroke="#7da3c9", sw=3, extra=f'opacity="0.85" transform="rotate({s * -32} {cx} {cy})"')
        out += path(f"M{BX + s * 30:.1f},214 Q{cx:.1f},{cy - 6:.1f} {BX + s * 118:.1f},{cy - 30:.1f}", stroke="#9cbbd9", sw=2)
    out += buste("#4a5568", "#8fa0b5")
    out += path("M118,258 L182,258", stroke="#6b7a8f", sw=6) + path("M112,276 L188,276", stroke="#6b7a8f", sw=6)
    for s in (-1, 1):  # antennes
        antenne = f"M{HX + s * 20:.1f},{HY - 60:.1f} Q{HX + s * 26:.1f},{HY - 100:.1f} {HX + s * 44 + c['dx'] * 4:.1f},{HY - 106:.1f}"
        out += path(antenne, sw=8) + path(antenne, stroke="#9aabc2", sw=3.5)
        out += ell(HX + s * 44 + c["dx"] * 4, HY - 106, 8, 8, "#9aabc2", sw=3)
    out += tete(c, "#7a8aa0", 84, 76)
    out += ell(HX, HY - 40, 40, 18, "#93a3b8", stroke=None, extra='opacity="0.6"')
    return out


def grenouille(c):
    HX, HY, FX, FY = c["HX"], c["HY"], c["FX"], c["FY"]
    out = buste("#5fb653", y0=176, l=64)
    out += ell(150, 256, 40, 32, "#e3f5c8", stroke=None)
    for s in (-1, 1):  # pattes avant
        out += ell(150 + s * 46, 286, 18, 9, "#7ccf6a", sw=3)
    formes = [e_(HX, HY + 12, 98, 70)] + [e_(FX + s * 44, FY - 46, 31, 29) for s in (-1, 1)]
    out += silhouette(formes, "#7ccf6a")
    out += ell(HX, HY + 50, 70, 26, "#a7e08f", stroke=None, extra='opacity="0.55"')
    for x, y, r in ((-30, -14, 7), (34, -20, 5), (52, 2, 6), (-56, 4, 4.5)):
        out += ell(HX + x + c["dx"] * 6, HY + y, r, r * 0.85, "#5aa84c", stroke=None)
    return out


def girafe(c):
    HX, HY, FX, FY = c["HX"], c["HY"], c["FX"], c["FY"]
    out = path("M116,196 L110,292 L190,292 L184,196 Z", fill="#f5c75a", sw=3.5)  # cou
    for x, y, rx, ry in ((132, 236, 12, 9), (164, 222, 10, 8), (160, 262, 13, 10), (126, 276, 9, 7), (178, 284, 8, 6)):
        out += ell(x, y, rx, ry, "#c27a34", stroke=None)
    for s in (-1, 1):  # ossicones
        bx = HX + s * 26 - c["dx"] * 3
        out += path(f"M{bx:.1f},{HY - 50:.1f} L{bx + s * 5:.1f},{HY - 98:.1f}", stroke=TRAIT, sw=15)
        out += path(f"M{bx:.1f},{HY - 50:.1f} L{bx + s * 5:.1f},{HY - 98:.1f}", stroke="#f5c75a", sw=8.5)
        out += ell(bx + s * 5, HY - 102, 11, 10, "#8a4f22", sw=3)
    for s in (-1, 1):  # oreilles
        ex, ey = HX + s * 84 - c["dx"] * 4, HY - 30
        out += ell(ex, ey, 30, 14, "#f5c75a", sw=3.5, extra=f'transform="rotate({s * 25} {ex:.1f} {ey:.1f})"')
        out += ell(ex, ey, 18, 7, "#f3a9a0", stroke=None, extra=f'transform="rotate({s * 25} {ex:.1f} {ey:.1f})"')
    cid = nouvel_id()
    out += f'<clipPath id="{cid}"><ellipse cx="{HX:.1f}" cy="{HY - 4:.1f}" rx="80" ry="74"/></clipPath>'
    out += tete(c, "#f5c75a", 80, 74, -4)
    taches = ""
    for x, y, rx, ry in ((-58, -30, 18, 14), (54, -44, 16, 12), (-18, -62, 13, 10), (66, 6, 12, 15), (-70, 16, 10, 14)):
        taches += ell(HX + x + c["dx"] * 8, HY + y, rx, ry, "#c27a34", stroke=None)
    out += f'<g clip-path="url(#{cid})">{taches}</g>'
    out += ell(FX, FY + 40, 50, 30, "#fbe3ad", sw=3.5)
    for s in (-1, 1):
        out += ell(FX + s * 15, FY + 32, 4, 5.5, "#8a4f22", stroke=None)
    return out


def dragon(c):
    HX, HY, BX, FX, FY = c["HX"], c["HY"], c["BX"], c["FX"], c["FY"]
    out = ""
    for s in (-1, 1):  # ailes
        b = BX + s * 40
        d = f"M{b:.1f},226 L{b + s * 82:.1f},150 L{b + s * 92:.1f},214 Q{b + s * 70:.1f},208 {b + s * 66:.1f},232 Q{b + s * 46:.1f},222 {b + s * 36:.1f},248 Z"
        out += path(d, fill="#c4b2f5", sw=3.5)
        out += path(f"M{b:.1f},226 L{b + s * 82:.1f},150", stroke="#6c4fb8", sw=5)
    out += buste("#9b7be0")
    out += path("M128,214 Q150,206 172,214 L168,292 L132,292 Z", fill="#f6dc8f", sw=3)
    for y in (234, 254, 274):
        out += path(f"M131,{y} Q150,{y + 5} 169,{y}", stroke="#d9b45e", sw=2.5)
    for s in (-1, 1):  # cornes
        bx = HX + s * 40 - c["dx"] * 3
        d = f"M{bx - s * 12:.1f},{HY - 56:.1f} Q{bx + s * 10:.1f},{HY - 100:.1f} {bx + s * 38:.1f},{HY - 108:.1f} Q{bx + s * 22:.1f},{HY - 84:.1f} {bx + s * 12:.1f},{HY - 58:.1f} Z"
        out += path(d, fill="#fff1c9", sw=3.5)
    for i, k in enumerate((-1, 0, 1)):  # crete
        x = HX + k * 22 - c["dx"] * 4
        h = 24 if k == 0 else 18
        out += path(f"M{x - 11:.1f},{HY - 70:.1f} L{x:.1f},{HY - 76 - h:.1f} L{x + 11:.1f},{HY - 70:.1f} Z", fill="#6c4fb8", sw=3)
    out += tete(c, "#9b7be0", 86, 78)
    out += ell(FX, FY + 40, 46, 27, "#b9a1f0", sw=3.5)
    for s in (-1, 1):
        out += ell(FX + s * 14, FY + 30, 4, 3, "#4a3290", stroke=None)
    return out


def cheveux_avant(c, couleur, sombre, style):
    HX, HY, dx = c["HX"], c["HY"], c["dx"]
    m = dx * 9
    v = c["dy"] * 24  # le bord de la frange suit le regard
    if style == "meche":  # frange de cote
        d = (f"M{HX - 84:.1f},{HY + 12:.1f} C{HX - 92:.1f},{HY - 70:.1f} {HX - 40:.1f},{HY - 98:.1f} {HX + 4:.1f},{HY - 96:.1f} "
             f"C{HX + 54:.1f},{HY - 96:.1f} {HX + 94:.1f},{HY - 64:.1f} {HX + 84:.1f},{HY + 14:.1f} "
             f"C{HX + 74:.1f},{HY - 22:.1f} {HX + 54 + m:.1f},{HY - 40 + v:.1f} {HX + 24 + m:.1f},{HY - 46 + v:.1f} "
             f"C{HX + 4 + m:.1f},{HY - 20 + v:.1f} {HX - 36 + m:.1f},{HY - 22 + v:.1f} {HX - 56 + m:.1f},{HY - 30 + v:.1f} "
             f"C{HX - 66:.1f},{HY - 20:.1f} {HX - 72:.1f},{HY - 6:.1f} {HX - 84:.1f},{HY + 12:.1f} Z")
        out = path(d, fill=couleur, sw=4)
        out += path(f"M{HX - 30:.1f},{HY - 84:.1f} Q{HX + 10:.1f},{HY - 70:.1f} {HX + 34 + m:.1f},{HY - 52:.1f}", stroke=sombre, sw=3)
        out += path(f"M{HX + 46:.1f},{HY - 80:.1f} Q{HX + 70:.1f},{HY - 50:.1f} {HX + 74:.1f},{HY - 14:.1f}", stroke=sombre, sw=3)
        return out
    # frange droite a pointes
    pts = [f"M{HX - 84:.1f},{HY + 4:.1f}", f"C{HX - 90:.1f},{HY - 72:.1f} {HX - 40:.1f},{HY - 98:.1f} {HX:.1f},{HY - 98:.1f}",
           f"C{HX + 40:.1f},{HY - 98:.1f} {HX + 90:.1f},{HY - 72:.1f} {HX + 84:.1f},{HY + 4:.1f}"]
    bas = HY - 30
    xs = [HX + 70, HX + 50, HX + 30, HX + 10, HX - 10, HX - 30, HX - 50, HX - 70]
    d = " ".join(pts)
    for i, x in enumerate(xs):
        y = bas + v + (10 if i % 2 == 0 else -4)
        d += f" L{x + m:.1f},{y:.1f}"
    d += " Z"
    out = path(d, fill=couleur, sw=4)
    out += path(f"M{HX - 40:.1f},{HY - 80:.1f} Q{HX - 10:.1f},{HY - 90:.1f} {HX + 24:.1f},{HY - 84:.1f}", stroke="#ffffff", sw=4, extra='opacity="0.55"')
    return out


def rousse(c):
    HX, HY, BX, dx = c["HX"], c["HY"], c["BX"], c["dx"]
    d = (f"M{BX - 92:.1f},{HY - 10:.1f} C{BX - 110:.1f},{HY - 100:.1f} {BX + 110:.1f},{HY - 100:.1f} {BX + 92:.1f},{HY - 10:.1f} "
         f"C{BX + 100:.1f},{HY + 60:.1f} {BX + 120:.1f},{HY + 110:.1f} {BX + 96:.1f},{HY + 140:.1f} "
         f"L{BX - 96:.1f},{HY + 140:.1f} C{BX - 120:.1f},{HY + 110:.1f} {BX - 100:.1f},{HY + 60:.1f} {BX - 92:.1f},{HY - 10:.1f} Z")
    out = path(d, fill="#c4522a", sw=4)
    out += path(f"M138,190 L138,214 L162,214 L162,190 Z", fill="#f7d4bc", sw=3)
    out += buste("#2f8f83", "#f7d4bc", y0=212, l=66)
    for s in (-1, 1):  # oreilles
        out += ell(HX + s * 78, HY + 10, 12, 16, "#f7d4bc", sw=3.5)
    out += tete(c, "#f7d4bc", 78, 80)
    out += cheveux_avant(c, "#c4522a", "#9a3a1c", "meche")
    for s in (-1, 1):  # meches devant les epaules
        x = HX + s * 74
        out += path(f"M{x:.1f},{HY + 8:.1f} C{x + s * 16:.1f},{HY + 60:.1f} {x + s * 4:.1f},{HY + 100:.1f} {x + s * 20:.1f},{HY + 140:.1f} "
                    f"L{x + s * 34:.1f},{HY + 136:.1f} C{x + s * 30:.1f},{HY + 90:.1f} {x + s * 34:.1f},{HY + 40:.1f} {x + s * 12:.1f},{HY - 10:.1f} Z",
                    fill="#c4522a", sw=3.5)
    return out


def rose(c):
    HX, HY, BX = c["HX"], c["HY"], c["BX"]
    out = ""
    for s in (-1, 1):  # couettes
        x = BX + s * 76
        out += path(f"M{x:.1f},{HY - 50:.1f} C{x + s * 60:.1f},{HY - 40:.1f} {x + s * 64:.1f},{HY + 60:.1f} {x + s * 44:.1f},{HY + 130:.1f} "
                    f"C{x + s * 30:.1f},{HY + 96:.1f} {x + s * 18:.1f},{HY + 40:.1f} {x - s * 4:.1f},{HY - 16:.1f} Z", fill="#f59ac0", sw=4)
        out += path(f"M{x + s * 30:.1f},{HY - 20:.1f} C{x + s * 44:.1f},{HY + 20:.1f} {x + s * 44:.1f},{HY + 60:.1f} {x + s * 40:.1f},{HY + 100:.1f}", stroke="#e0679b", sw=3)
    out += path(f"M{BX - 86:.1f},{HY:.1f} C{BX - 100:.1f},{HY - 100:.1f} {BX + 100:.1f},{HY - 100:.1f} {BX + 86:.1f},{HY:.1f} L{BX + 80:.1f},{HY + 44:.1f} L{BX - 80:.1f},{HY + 44:.1f} Z", fill="#f59ac0", sw=4)
    out += path(f"M140,190 L140,214 L160,214 L160,190 Z", fill="#fde3d3", sw=3)
    out += buste("#8e7cf0", "#fde3d3", y0=212, l=60)
    out += path("M138,234 l12,8 l12,-8 l-12,-6 Z", fill="#ffd166", sw=2.5)
    for s in (-1, 1):
        out += ell(HX + s * 76, HY + 10, 11, 15, "#fde3d3", sw=3.5)
    out += tete(c, "#fde3d3", 76, 78)
    out += cheveux_avant(c, "#f59ac0", "#e0679b", "frange")
    for s in (-1, 1):  # elastiques des couettes
        out += ell(BX + s * 84, HY - 46, 9, 9, "#ffd166", sw=3)
    out += etincelle(HX + 58 + c["dx"] * 6, HY - 52, 11, "#ffe066")
    return out


PERSOS = {
    "nd-mouche": (mouche, dict(oeil=dict(rx=29, ry=32, ecart=40, y=-8, iris="#b8323e", facettes="#e8707a"), bouche_y=40, joue_dx=6, joue_dy=34)),
    "nd-grenouille": (grenouille, dict(oeil=dict(rx=19, ry=20, ecart=44, y=-46, iris="#2b2a2a", blanc=True), bouche_y=26, bouche_l=2.6, joue_dx=22, joue_dy=56)),
    "nd-girafe": (girafe, dict(oeil=dict(rx=12, ry=15, ecart=34, y=-10, iris="#3b2414", cils=True), bouche_y=58, joue_dx=14, joue_dy=26)),
    "nd-dragon": (dragon, dict(oeil=dict(rx=14, ry=17, ecart=36, y=-12, iris="#f0b429", pupille="#3a2208"), bouche_y=52, bouche_l=1.3, joue_dx=14, joue_dy=26)),
    "nd-rousse": (rousse, dict(oeil=dict(rx=13, ry=16, ecart=32, y=4, iris="#3f8a55", pupille="#16301e", cils=True), bouche_y=40, taches_rousseur=True)),
    "nd-cheveux-roses": (rose, dict(oeil=dict(rx=15, ry=19, ecart=32, y=6, iris="#8a5ad0", pupille="#3a1f5e", cils=True), bouche_y=40)),
}


def case(nom, direction=None, reaction=None):
    fn, sp = PERSOS[nom]
    dx, dy = (0, 0) if direction is None else (direction % 3 - 1, direction // 3 - 1)
    c = dict(dx=dx, dy=dy, HX=150 + dx * 5, HY=118 + dy * 6, BX=150 - dx * 6)
    # Le visage glisse plus en hauteur qu'en largeur : la tete est moins large
    # que haute a l'ecran, et un regard en l'air a 12 px ne se voyait pas
    # (signale le 2026-11-14).
    c["FX"], c["FY"] = c["HX"] + dx * 20, c["HY"] + dy * 22
    c["regard"] = (dx, dy)
    return fn(c) + visage(c, sp, reaction) + decor(c, reaction)


def planche(nom, sorte):
    corps = ""
    for i in range(9):
        x, y = (i % 3) * 300, (i // 3) * 300
        contenu = case(nom, direction=i) if sorte == "directions" else case(nom, reaction=i)
        corps += f'<g transform="translate({x} {y})">{contenu}</g>'
    return (f'<html><body style="margin:0;background:transparent"><svg xmlns="http://www.w3.org/2000/svg" '
            f'width="{RENDU}" height="{RENDU}" viewBox="0 0 900 900">{corps}</svg></body></html>')


def main():
    travail, dest = sys.argv[1], sys.argv[2]
    seuls = sys.argv[3:] or list(PERSOS)
    os.makedirs(travail, exist_ok=True)
    for nom in seuls:
        for sorte in ("directions", "reactions"):
            html = os.path.join(travail, f"{nom}-{sorte}.html")
            png = os.path.join(travail, f"{nom}-{sorte}.png")
            with open(html, "w", encoding="utf-8") as f:
                f.write(planche(nom, sorte))
            subprocess.run([EDGE, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--default-background-color=00000000",
                            f"--screenshot={png}", f"--window-size={RENDU},{RENDU}", "file:///" + html.replace("\\", "/")],
                           check=True, capture_output=True)
            im = Image.open(png).convert("RGBA")
            im.resize((768, 768), Image.LANCZOS).save(os.path.join(dest, f"{nom}-{sorte}.webp"), "WEBP", quality=90, method=6)
            if sorte == "directions":
                k = im.width // 3
                im.crop((k, k, 2 * k, 2 * k)).resize((96, 96), Image.LANCZOS).save(
                    os.path.join(dest, f"{nom}-vignette.webp"), "WEBP", quality=88, method=6)
        print(nom, "ok")


if __name__ == "__main__":
    main()
