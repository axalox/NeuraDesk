# La TETE QUI TOURNE des mascottes IA (2026-11-21) : la planche des 9 regards
# n'est plus faite en deplacant l'iris dans le verre, mais avec 5 tetes
# tournees par Qwen-Image-Edit-2509 (serveur ComfyUI local, Apache-2.0) :
# gauche, haut, bas, haut-gauche, bas-gauche. Les trois regards de droite sont
# les memes tetes en MIROIR autour de l'axe du visage.
#
# Le corps ne doit PAS bouger d'une case a l'autre (sinon la mascotte tremble
# quand le regard change) : chaque tete modifiee est recalee sur le corps
# d'origine (ECC sur l'alpha, sous le cou), puis recollee au-dessus d'une bande
# de fondu au niveau du cou ; sous le cou, et sur les cotes (les pointes de
# tentacules qui remontent), c'est toujours l'image d'origine.
#
#   python tools/mascottes_tetes.py candidats <travail> <personnage> [graine…]
#       -> genere les 5 tetes pour chaque graine dans <travail> (planche de choix)
#   Les tetes RETENUES sont copiees dans `tools/mascottes_ia/<personnage>-<dir>.png`
#   et `mascottes_ia.py` les assemble : la regeneration ne depend PAS du serveur.
import os
import sys

import cv2
import numpy as np
from PIL import Image

ICI = os.path.dirname(os.path.abspath(__file__))
DIRS = ("gauche", "haut", "bas", "hautgauche", "basgauche")

_FIN = ("{corps} stays exactly where it is. Same expression, same character, same colors, same proportions, "
        "same thick outline, plain white background.")
CONSIGNES = {
    "gauche": "Three-quarter view: the {x}'s head{acc} is turned toward the left side of the image, and it looks toward "
              "the left edge of the image.",
    "haut": "The {x} tilts its head back and looks up toward the top edge of the image; we see its face{acc} from "
            "slightly below.",
    # « bows its head... chin lowered » tournait la tete mais rendait la mine
    # boudeuse ou fachee : le sourire est demande explicitement.
    "bas": "The {x} tilts its head down and looks down toward the bottom edge of the image with its usual happy "
           "smile; we see a bit more of the top of its head and its face{acc} slightly from above.",
    "hautgauche": "Three-quarter view: the {x}'s head{acc} is turned toward the left side of the image and tilted up, "
                  "it looks toward the top-left corner of the image.",
    "basgauche": "Three-quarter view: the {x}'s head{acc} is turned toward the left side of the image and bowed down, "
                 "it looks toward the bottom-left corner of the image.",
}
# Case de la planche (0..8, de haut-gauche a bas-droite) -> (tete, miroir).
CASES = [("hautgauche", False), ("haut", False), ("hautgauche", True),
         ("gauche", False), (None, False), ("gauche", True),
         ("basgauche", False), ("bas", False), ("basgauche", True)]


def consigne(t: dict, direction: str) -> str:
    return CONSIGNES[direction].format(x=t["x"], acc=t.get("acc", "")) + " " + _FIN.format(corps=t["corps"])


def candidats(travail: str, nom: str, graines: list[int]) -> None:
    import mascottes_ia as mi
    from comfy_qwen_edit import modifier
    t = mi.PERSOS[nom]["tete"]
    entree = os.path.join(ICI, "mascottes_ia", mi.PERSOS[nom]["image"])
    os.makedirs(travail, exist_ok=True)
    for d in DIRS:
        for g in graines:
            sortie = os.path.join(travail, f"{nom}-{d}-{g}.png")
            if not os.path.exists(sortie):
                modifier(entree, sortie, g, consigne(t, d))
    cote = 256
    planche = Image.new("RGB", (cote * (len(graines) + 1), cote * len(DIRS)), "white")
    for j, d in enumerate(DIRS):
        planche.paste(Image.open(entree).convert("RGB").resize((cote, cote)), (0, cote * j))
        for i, g in enumerate(graines):
            im = Image.open(os.path.join(travail, f"{nom}-{d}-{g}.png")).convert("RGB").resize((cote, cote))
            planche.paste(im, (cote * (i + 1), cote * j))
    planche.save(os.path.join(travail, f"{nom}-candidats.png"))


def _lire(chemin, detourer):
    rgb = np.asarray(Image.open(chemin).convert("RGB").resize((1024, 1024), Image.LANCZOS)).copy()
    return rgb, detourer(rgb).astype(np.float32) / 255


def _recaler(rgb, a, a_ref, cou0, cou1):
    """Recale la tete modifiee sur l'original par ECC, en TRANSLATION seule,
    sur l'alpha d'une bande autour du cou : c'est la que la tete est recollee.
    (Sur tout le corps, l'ECC se trompait : Qwen tourne souvent le corps entier
    avec la tete, et la silhouette du dessous ne correspond plus.) Un recalage
    de plus de 60 px est rejete : il serait faux."""
    masque = np.zeros(a.shape, np.uint8)
    masque[max(cou0 - 80, 0):cou1 + 60] = 255
    m = np.eye(2, 3, dtype=np.float32)
    try:
        _, m = cv2.findTransformECC(a_ref, a, m, cv2.MOTION_TRANSLATION,
                                    (cv2.TERM_CRITERIA_EPS | cv2.TERM_CRITERIA_COUNT, 200, 1e-6), masque, 5)
    except cv2.error:
        return rgb, a
    if np.hypot(m[0, 2], m[1, 2]) > 60:
        return rgb, a
    drap = cv2.INTER_LINEAR | cv2.WARP_INVERSE_MAP
    rgb = cv2.warpAffine(rgb, m, (1024, 1024), flags=drap, borderValue=(255, 255, 255))
    a = cv2.warpAffine(a, m, (1024, 1024), flags=drap, borderValue=0)
    return rgb, a


def planche(p: dict, nom: str, boite, detourer, regard) -> Image.Image:
    """La planche des 9 regards, 1536 px (cases de 512), meme cadrage que les
    planches rendues par Edge (`mascottes_ia.planches`). Une direction sans
    tete retenue (graine `None` : aucun essai n'etait reussi) garde le visage
    de face, seul l'iris bouge : `regard(dx, dy)` rend cette image (RGBA
    1024), cadree ICI comme les autres pour que le corps ne saute pas."""
    t = p["tete"]
    o, oa = _lire(os.path.join(ICI, "mascottes_ia", p["image"]), detourer)
    o = o.astype(np.float32)
    cou0, cou1 = t["cou"]
    axe = sum(e[0][0] for e in p["iris"]) / 2
    y = np.arange(1024)[:, None].astype(np.float32)
    x = np.arange(1024)[None, :].astype(np.float32)
    corps = np.clip((y - cou0) / (cou1 - cou0), 0, 1)
    if "cote" in t:  # les membres qui remontent sur les cotes restent ceux d'origine
        dx, y_cote = t["cote"]
        corps = np.maximum(corps, np.clip((np.abs(x - axe) - dx) / 20, 0, 1) * np.clip((y - y_cote) / 20, 0, 1))

    # Le gabarit commence 60 px AU-DESSUS du cou : les epaules tournees du
    # hibou depassaient deja la, en fantome a cote des ailes.
    epaules = np.clip((y - (cou0 - 60)) / 30, 0, 1)
    oa_large = cv2.dilate(oa, np.ones((9, 9), np.uint8))

    def tete(direction, miroir):
        rgb, a = _lire(os.path.join(ICI, "mascottes_ia", f"{nom}-{direction}.png"), detourer)
        rgb, a = _recaler(rgb, a, oa, cou0, cou1)
        rgb = rgb.astype(np.float32)
        if miroir:  # symetrie autour de l'axe du visage
            mx = np.float32([[-1, 0, 2 * axe], [0, 1, 0]])
            rgb = cv2.warpAffine(rgb, mx, (1024, 1024), borderValue=(255, 255, 255))
            a = cv2.warpAffine(a, mx, (1024, 1024), borderValue=0)
        # Pres du cou, la tete modifiee ne deborde pas de la silhouette
        # d'origine : Qwen tourne souvent les epaules avec la tete, et leur bord
        # laissait un fantome a demi transparent a cote du corps.
        wa, wb = a * (1 - corps) * (1 - epaules * (1 - oa_large)), oa * corps
        aa = wa + wb
        c = (rgb * wa[..., None] + o * wb[..., None]) / np.maximum(aa, 1e-6)[..., None]
        return np.dstack([np.clip(c, 0, 255), np.clip(aa, 0, 1) * 255]).astype(np.uint8)

    x0, y0, x1, y1 = boite
    s = min(286 / (y1 - y0), 292 / (x1 - x0)) * 512 / 300
    cote = round(1024 * s)
    feuille = Image.new("RGBA", (1536, 1536))
    dx, dy = round(256 - (x0 + x1) / 2 * s), round(297 * 512 / 300 - y1 * s)
    for i, (d, m) in enumerate(CASES):
        if d is None:
            img = np.dstack([o, oa * 255]).astype(np.uint8)
        elif t["graines"].get(d) is None:
            img = regard(i % 3 - 1, i // 3 - 1)
        else:
            img = tete(d, m)
        im = Image.fromarray(img).resize((cote, cote), Image.LANCZOS)
        # Le decalage est arrondi UNE fois, puis ajoute a l'origine de la case :
        # `int()` d'une coordonnee negative tronquait vers zero, et la premiere
        # ligne et la premiere colonne etaient decalees d'un pixel (le corps
        # sautait quand le regard passait de gauche a droite).
        # Chaque case est coupee a ses bords : une tete levee depassait dans le
        # bas de la case du dessus.
        case = Image.new("RGBA", (512, 512))
        case.alpha_composite(im, (dx, dy)) if dx >= 0 and dy >= 0 else case.paste(im, (dx, dy), im)
        feuille.alpha_composite(case, ((i % 3) * 512, (i // 3) * 512))
    return feuille


if __name__ == "__main__":
    sys.path.insert(0, ICI)
    if sys.argv[1] == "candidats":
        candidats(sys.argv[2], sys.argv[3], [int(g) for g in sys.argv[4:]] or [3, 4])
