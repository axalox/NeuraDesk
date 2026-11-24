# Les super-heros en MASCOTTES (2026-11-24) : « utiliser les bustes des super heros TELS QU'ILS
# SONT pour en faire des mascottes, sans leur donner l'aspect des petites mascottes ».
#
# 1. `bases` : FLUX.2 [klein] 4B (Apache-2.0, ComfyUI local) recadre le PORTRAIT du heros
#    (`heros_NN_00001_.png`) en buste : meme visage, meme costume, meme dessin comics, fond
#    blanc, marges autour (sinon les epaules sont coupees net au bord de la case).
#    TIRAGES_BASE tirages, choix dans BASES_CHOIX apres relecture (`revue-buste-NN.png`).
#    ⛔ Ne PAS repeindre un buste de mascotte « personne » en heros (essaye le 2026-11-23 :
#    refuse, ce n'etaient plus ses heros).
# 2. `tirer` : la chaine des personnes (`mascottes_personnes.tirer`) : 8 regards + 9 reactions,
#    avec un regard vers le haut ADOUCI (la consigne des personnes renversait la tete).
# 3. `refaire` / `retenir` : les cases refusees a la relecture (REPRISES), TIRAGES_REPRISE
#    tirages de plus, revue `reprise-superheros-NN.png`, choix a la main dans CHOIX_REPRISE.
# 4. `ecrire` : les planches dans le style d'origine et les cinq styles du renard, chaque case
#    RECALEE sur le buste (Klein change la taille du buste d'une case a l'autre).
#
#   python tools/mascottes_heros.py bases   <cache> [n...]
#   python tools/mascottes_heros.py tirer   <cache> [n...]
#   python tools/mascottes_heros.py refaire <cache> [n...]
#   python tools/mascottes_heros.py retenir <cache>
#   python tools/mascottes_heros.py ecrire  <assets/popups/mascottes> [n...]
#
# ⛔ Le prefixe des fichiers est `superheros-NN`, PAS `heros-NN` : `heros-NN-vignette.webp`
# est deja la vignette du heros de l'inactivite, et l'application compte les `heros-*`.
import os
import sys

import cv2
import numpy as np
from PIL import Image, ImageDraw

ICI = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, ICI)
import comfy_klein  # noqa: E402
import mascottes_ia as ia  # noqa: E402
import mascottes_personnes as mp  # noqa: E402
import mascottes_styles as styles  # noqa: E402
import mascottes_tetes as tetes  # noqa: E402

DOSSIER = os.path.join(ICI, "mascottes_ia", "super_heros")
PORTRAITS = r"C:\IAs\ComfyUI\ComfyUI\output\super_heros"
TIRAGES_BASE = 4
TIRAGES = 3

# Choix a la main de la base (indice 0..TIRAGES_BASE-1), apres relecture.
BASES_CHOIX: dict = {}
# Choix a la main des regards et reactions : (n, case) -> indice.
CHOIX: dict = {}
# Ce qui tourne avec la tete (casque, diademe, masque) : n -> texte.
# ⛔ Seulement ce que le BUSTE porte : « and its halo » pour le 30, dont le buste n'a pas
# d'aureole, la faisait AJOUTER (refuse a la relecture du 2026-11-24).
ACCESSOIRES: dict = {
    6: " and its golden helmet", 7: " and its tiara", 8: " and its gold mask", 16: " and its visor",
    22: " and its hood and mask", 23: " and its winged helmet", 28: " and its chrome helmet",
}


def nom(n: int) -> str:
    return f"superheros-{n:02d}"


def consigne_base(n: int) -> str:
    """Le buste DU heros, recadre depuis son portrait : meme visage, meme costume,
    meme dessin comics. DEMANDE (2026-11-24) : « utiliser les bustes des super
    heros, pas en faire de nouveaux »."""
    return ("Show exactly this same superhero as a head-and-shoulders bust portrait: zoom in on the head and "
            "shoulders, front view, facing the viewer, looking at the viewer with a calm friendly smile, arms down "
            "out of the frame. Keep exactly the same face, hair, costume, colors, chest emblem and the same comic book "
            "art style and line work. The bust is small and centered, with wide empty white margins on the left, the "
            "right and above the head: the whole head and both rounded shoulders are fully inside the image, the bust "
            "is cut just below the collarbones and fades into white at the bottom edge. Plain pure white background: "
            "remove the sky, the city, the objects held, the light ribbons, the glows and every background effect. "
            "No text.")


def bases(cache: str, nums: list) -> None:
    os.makedirs(cache, exist_ok=True)
    os.makedirs(os.path.join(DOSSIER, "bases"), exist_ok=True)
    for n in nums:
        portrait = os.path.join(PORTRAITS, f"heros_{n:02d}_00001_.png")
        chemins = []
        for g in range(TIRAGES_BASE):
            s = os.path.join(cache, f"buste-{n:02d}-{g}.png")
            if not os.path.exists(s):
                comfy_klein.editer(portrait, s, consigne_base(n), 500 + g)
            chemins.append(s)
        k = BASES_CHOIX.get(n, 0)
        Image.open(chemins[k]).convert("RGB").save(os.path.join(DOSSIER, "bases", f"{nom(n)}.png"))
        T = 256
        r = Image.new("RGB", (T * (TIRAGES_BASE + 1), T), "white")
        r.paste(Image.open(portrait).convert("RGB").resize((T * 832 // 1216, T)), (0, 0))
        dr = ImageDraw.Draw(r)
        for i, c in enumerate(chemins):
            r.paste(Image.open(c).convert("RGB").resize((T, T)), ((i + 1) * T, 0))
            dr.text(((i + 1) * T + 4, 4), str(i), fill="blue")
        dr.rectangle(((k + 1) * T, 0, (k + 2) * T - 1, T - 1), outline="red", width=3)
        r.save(os.path.join(cache, f"revue-buste-{n:02d}.png"))
        print(n, "base", flush=True)


def brancher() -> None:
    """La chaine des personnes, sur les dossiers et choix des super-heros."""
    mp.BASES, mp.CADRE, mp.RETENUS = (os.path.join(DOSSIER, x) for x in ("bases", "cadre", "retenus"))
    mp.TIRAGES = TIRAGES
    mp.REFAIRE, mp.YEUX_OUVERTS = set(), set()
    mp.CHOIX = {(nom(n), c): k for (n, c), k in CHOIX.items()}
    # Le regard vers le haut des personnes (« chin high, head far back ») renverse la
    # tete d'un heros realiste : on ne voit plus que son cou. Adouci (2026-11-24).
    mp.REGARDS = dict(mp.REGARDS)
    mp.REGARDS["haut"] = ("The {x} tilts its head up a little and rolls its eyes up to look at the top edge of the "
                          "image; its whole face{acc} stays clearly visible from the front, eyes, nose and mouth.")
    mp.REGARDS["hautgauche"] = ("Three-quarter view: the {x} turns its head{acc} toward the left side of the image and "
                                "tilts it up a little, looking up toward the top-left corner of the image; its face "
                                "stays clearly visible, eyes, nose and mouth.")
    for n in range(1, 31):
        mp.PERSONNES[nom(n)] = (f"Super-héros {n}", ACCESSOIRES.get(n, ""))


# ---------------------------------------------------------------- recalage

def epaules(rgb: np.ndarray):
    """(largeur, centre x, bas, hauteur) des epaules. Largeur : mediane des largeurs de
    la silhouette sur les lignes bas-150..bas-50 (au-dessus du fondu, sous le menton).
    Hauteur : du bas jusqu'a la ligne ou la silhouette retombe sous 75 % de cette
    largeur (le haut des epaules) ; elle ne change pas quand le corps tourne."""
    a = mp.detourer(rgb) > 128
    ys = np.nonzero(a.any(axis=1))[0]
    if len(ys) == 0:
        return None
    bas = int(ys.max())
    larg = np.zeros(a.shape[0])
    centres = []
    for y in range(a.shape[0]):
        xs = np.nonzero(a[y])[0]
        if len(xs):
            larg[y] = xs.max() - xs.min()
            if bas - 150 <= y <= bas - 50:
                centres.append((xs.max() + xs.min()) / 2)
    if not centres:
        return None
    ref = float(np.median(larg[max(bas - 150, 0):bas - 49]))
    y = bas - 150
    while y > 0 and larg[y] >= 0.75 * ref:
        y -= 1
    return ref, float(np.median(centres)), bas, float(bas - y)


def recaler(rgb: np.ndarray, base: np.ndarray) -> np.ndarray:
    """Ramene une case sur le buste de base. Klein change la taille et la place du
    buste d'une case a l'autre (surtout en levant la tete) : refuse a la relecture
    du 2026-11-24. Deux estimations de l'echelle : la largeur des epaules (fausse
    quand le corps tourne : plus etroit, il serait GROSSI de 25 %) et leur hauteur
    au-dessus du bas du buste (fausse quand une meche descend sur l'epaule). On garde
    la plus proche de 1 : on ne retaille que si les deux s'accordent. Le bas du buste
    et le centre des epaules sont ramenes sur ceux de la base.
    ⛔ Essayes d'abord : ORB + RANSAC (peu de points sur ces aplats, buste reduit de
    20 %) et la correlation d'une bande du torse (la chevelure d'une tete baissee la
    couvre : grossi de 20 %)."""
    eb, ec = epaules(base), epaules(rgb)
    if eb is None or ec is None:
        return rgb
    par_largeur, par_hauteur = eb[0] / ec[0], eb[3] / ec[3]
    echelle = min((par_largeur, par_hauteur), key=lambda e: abs(np.log(e)))
    if not 0.8 < echelle < 1.25:
        echelle = 1.0
    M = np.float32([[echelle, 0, eb[1] - echelle * ec[1]], [0, echelle, eb[2] - echelle * ec[2]]])
    h, w = rgb.shape[:2]
    return cv2.warpAffine(rgb, M, (w, h), flags=cv2.INTER_CUBIC, borderValue=(255, 255, 255))


def lire_1024(chemin: str) -> np.ndarray:
    return np.asarray(Image.open(chemin).convert("RGB").resize((1024, 1024), Image.LANCZOS))


def rgba_recale(chemin: str, base: np.ndarray) -> np.ndarray:
    rgb = recaler(lire_1024(chemin), base)
    return np.dstack([rgb, mp.detourer(rgb)]).clip(0, 255).astype(np.uint8)


def planches(n: int) -> dict:
    """`mascottes_personnes.planches`, chaque case recalee sur le buste."""
    x = nom(n)
    base_chemin = mp.cadre(x)
    base = lire_1024(base_chemin)
    o = mp.rgba(base_chemin)

    def case(c):
        return o if c is None else rgba_recale(os.path.join(mp.RETENUS, f"{x}-{c}.webp"), base)

    imgs = {"directions": [case(c) for c in mp.CASES], "reactions": [case(cle) for cle, _ in mp.REACTIONS]}
    commune = tetes.boite_commune(*imgs.values())
    return {sorte: tetes.placer(i, commune) for sorte, i in imgs.items()}


# ---------------------------------------------------------------- reprises

# DEMANDE (2026-11-24) : les cases refusees a la relecture de l'utilisateur.
HG, H, HD, G, BG, B, BD = "hautgauche", "haut", "hautdroite", "gauche", "basgauche", "bas", "basdroite"
REPRISES = {
    1: [HG, H, HD], 3: [HG, H, HD], 4: [HG, HD], 5: [H], 6: [H, HG], 7: [HG, HD], 8: [H], 9: [HG, HD],
    10: [HG, H, HD], 11: [H, HD], 16: [H, BG], 19: [H], 20: [H], 21: [H], 22: ["timide"], 23: ["timide"],
    24: [HG, H, HD], 25: [HD], 26: [G], 27: [HD, HG], 28: [H], 29: [H, "coeur"], 30: ["coeur", BD, B, BG, G, HG],
}
TIRAGES_REPRISE = 8
STABLE = (" Its shoulders, chest and costume stay exactly the same size and in exactly the same place as in the "
          "image; only the head moves, and it keeps exactly the same size.")
PRECISIONS = {
    "coeur": ("The {x} is in love: inside its normal eyes, its pupils become small cute red hearts, it smiles "
              "happily, and three small pink hearts float above its head. Its face and skin keep their normal "
              "colors, nothing glows."),
    "timide": ("The {x} is shy: a light pink blush appears on the bare skin of its cheeks only, its eyes look "
               "down and it has a small bashful smile. Its mask, hood and costume keep exactly their colors."),
}
# (n, case) -> numero du tirage retenu (1..TIRAGES_REPRISE), apres relecture.
CHOIX_REPRISE: dict = {  # releve du 2026-11-24 sur les planches reprise-*.png
    (1, "hautgauche"): 4, (1, "haut"): 4, (1, "hautdroite"): 8,
    (3, "hautgauche"): 4, (3, "haut"): 4, (3, "hautdroite"): 4,
    (4, "hautgauche"): 8, (4, "hautdroite"): 4,
    (5, "haut"): 7,
    (6, "haut"): 4, (6, "hautgauche"): 8,
    (7, "hautgauche"): 5, (7, "hautdroite"): 8,
    (8, "haut"): 4,
    (9, "hautgauche"): 8, (9, "hautdroite"): 8,
    (10, "hautgauche"): 3, (10, "haut"): 5, (10, "hautdroite"): 8,
    (11, "haut"): 6, (11, "hautdroite"): 8,
    (16, "haut"): 3, (16, "basgauche"): 5,
    (19, "haut"): 4, (20, "haut"): 5, (21, "haut"): 6,
    (22, "timide"): 4, (23, "timide"): 7,
    (24, "hautgauche"): 6, (24, "haut"): 7, (24, "hautdroite"): 7,
    (25, "hautdroite"): 7, (26, "gauche"): 5,
    (27, "hautdroite"): 7, (27, "hautgauche"): 3,
    (28, "haut"): 7,
    (29, "haut"): 4, (29, "coeur"): 2,
    (30, "coeur"): 3, (30, "basdroite"): 3, (30, "bas"): 4, (30, "basgauche"): 3,
    (30, "gauche"): 4, (30, "hautgauche"): 7,
}


def consigne_reprise(x: str, c: str) -> str:
    if c in PRECISIONS:
        return f"{PRECISIONS[c].format(x='person')} {mp._FIN_REACTION.format(fond='pure white')}"
    return mp.consignes(x)[c] + STABLE


def refaire(cache: str, n: int) -> None:
    x = nom(n)
    base_chemin = mp.cadre(x)
    base = lire_1024(base_chemin)
    lignes = []
    for c in REPRISES.get(n, []):
        consigne = consigne_reprise(x, c)
        chemins = []
        for g in range(1, TIRAGES_REPRISE + 1):
            s = os.path.join(cache, f"{x}-{c}-r{g}.png")
            if not os.path.exists(s):
                if c in mp.SANS_MARGE:
                    brut = os.path.join(cache, f"{x}-{c}-rsm{g}.png")
                    if not os.path.exists(brut):
                        comfy_klein.editer(os.path.join(mp.BASES, f"{x}.png"), brut, consigne, 700 + g)
                    mp.vers_cadre(Image.open(brut)).save(s)
                else:
                    comfy_klein.editer(base_chemin, s, consigne, 700 + g)
            chemins.append(s)
        lignes.append((c, chemins))
        print(x, c, flush=True)
    T = 220
    r = Image.new("RGB", (T * (TIRAGES_REPRISE + 1), T * max(len(lignes), 1)), "white")
    dr = ImageDraw.Draw(r)
    for j, (c, chemins) in enumerate(lignes):
        r.paste(Image.fromarray(base).resize((T, T)), (0, j * T))
        dr.text((4, j * T + 4), c, fill="red")
        for i, ch in enumerate(chemins):
            r.paste(Image.fromarray(recaler(lire_1024(ch), base)).resize((T, T)), ((i + 1) * T, j * T))
            dr.text(((i + 1) * T + 4, j * T + 4), str(i + 1), fill="blue")
    r.save(os.path.join(cache, f"reprise-{x}.png"))


def retenir(cache: str) -> None:
    for (n, c), g in CHOIX_REPRISE.items():
        Image.open(os.path.join(cache, f"{nom(n)}-{c}-r{g}.png")).convert("RGB").save(
            os.path.join(mp.RETENUS, f"{nom(n)}-{c}.webp"), quality=95, method=6)
    print(len(CHOIX_REPRISE), "cases retenues")


def main() -> None:
    quoi, ou = sys.argv[1], sys.argv[2]
    nums = [int(x) for x in sys.argv[3:]] or list(range(1, 31))
    if quoi == "bases":
        bases(ou, nums)
        return
    brancher()
    if quoi == "retenir":
        retenir(ou)
        return
    for n in nums:
        if quoi == "tirer":
            mp.tirer(nom(n), ou)
        elif quoi == "refaire":
            refaire(ou, n)
        elif quoi == "ecrire":
            rendus = planches(n)
            ia.ecrire(ou, nom(n), rendus)
            for style in styles.STYLES:
                ia.ecrire(ou, f"{nom(n)}-{style}", styles.styliser(rendus, style))
            print(n, "ok", flush=True)


if __name__ == "__main__":
    main()
