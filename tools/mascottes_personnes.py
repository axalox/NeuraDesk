# Les personnes AJOUTEES (2026-11-22), dans le style des personnes d'origine
# (page-mascot, MIT) : FLUX.2 [klein] 4B (Apache-2.0, ComfyUI local) a
# repeint le buste d'origine du cuisinier ou de l'infirmiere en une autre
# personne, dans une autre tenue (`bases/`). Puis, sur cette base reduite dans
# une marge (`cadre/` : un chapeau leve ne sort plus de l'image), Klein fait
# les 8 regards et les 9 expressions en images ENTIERES ; un tirage par case
# est retenu (`retenus/`, choix a la main dans `CHOIX`).
#
#   python tools/mascottes_personnes.py tirer <cache> [noms]    # Klein (long)
#   python tools/mascottes_personnes.py ecrire <assets/popups/mascottes> [noms]
#
# Style par defaut seulement (pas les cinq styles du renard) pour l'instant.
import os
import sys

import cv2
import numpy as np
from PIL import Image, ImageDraw

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import comfy_klein  # noqa: E402
import mascottes_ia as ia  # noqa: E402
import mascottes_tetes as tetes  # noqa: E402
from mascottes_aliens import REACTIONS, _FIN_REACTION  # noqa: E402

DOSSIER = os.path.join(os.path.dirname(os.path.abspath(__file__)), "mascottes_ia", "personnes")
BASES, CADRE, RETENUS = (os.path.join(DOSSIER, d) for d in ("bases", "cadre", "retenus"))
ECHELLE, HAUT = 0.8, 205
TIRAGES = 4
FONDU = 90  # le bas du buste s'efface sur ~90 px (1024), comme les bustes d'origine

# nom : (libelle, ce qui tourne avec la tete)
PERSONNES = {
    "pompiere": ("Pompière", " and its firefighter helmet"),
    "policier": ("Policier", " and its police cap"),
    "pilote": ("Pilote", " and its pilot cap"),
    "jardinier": ("Jardinier", " and its straw hat"),
    "peintre": ("Peintre", " and its beret"),
    "cowboy": ("Cowboy", " and its cowboy hat"),
    "detective": ("Détective", " and its hat"),
    "chevalier": ("Chevalier", ""),
    "princesse": ("Princesse", " and its crown"),
    "rockeuse": ("Rockeuse", ""),
    "footballeur": ("Footballeur", ""),
    "plongeuse": ("Plongeuse", " and its diving mask"),
    "facteur": ("Facteur", " and its cap"),
    "boulangere": ("Boulangère", " and its headscarf"),
}

_FIN = ("Its body stays exactly where it is. Same expression, same person, same clothes, same colors, same "
        "proportions, same art style with the same thick dark outline, plain pure white background.")
REGARDS = {
    "gauche": "Three-quarter view: the {x}'s head{acc} is turned toward the left side of the image, and it looks toward "
              "the left edge of the image.",
    "haut": "The {x} raises its chin high and tilts its whole head far back to look straight up at the top edge of the "
            "image; we see its face{acc} clearly from below, with its mouth still visible.",
    "bas": "The {x} tilts its head down and looks down toward the bottom edge of the image with its usual happy smile; "
           "we see a bit more of the top of its head and its face{acc} slightly from above.",
    "hautgauche": "The {x} raises its chin high and tilts its whole head far back, as much as when it looks straight "
                  "up, while also turning its head{acc} toward the left side of the image: it looks up toward the "
                  "top-left corner of the image, and we see its face from below.",
    "basgauche": "Three-quarter view: the {x}'s head{acc} is turned toward the left side of the image and bowed down, "
                 "it looks toward the bottom-left corner of the image.",
}
CASES = ["hautgauche", "haut", "hautdroite", "gauche", None, "droite", "basgauche", "bas", "basdroite"]

# Choix a la main apres relecture (indice 0..TIRAGES-1 ; sinon le plus stable).
CHOIX: dict = {
    ("cowboy", "hautgauche"): 3,
    ("peintre", "hautgauche"): 0, ("peintre", "haut"): 3, ("peintre", "gauche"): 3,
    ("princesse", "hautgauche"): 3, ("princesse", "haut"): 3,
    ("rockeuse", "haut"): 0, ("rockeuse", "basgauche"): 3, ("rockeuse", "etincelles"): 3,
    ("footballeur", "haut"): 0, ("footballeur", "basgauche"): 2, ("footballeur", "basdroite"): 3,
    # le masque de plongee glissait sur le visage
    ("plongeuse", "droite"): 1, ("plongeuse", "basdroite"): 0, ("plongeuse", "etourdi"): 3,
    ("facteur", "etincelles"): 3,
    # refaits (REFAIRE, YEUX_OUVERTS)
    ("facteur", "hautgauche"): 4, ("plongeuse", "hautgauche"): 11, ("chevalier", "hautgauche"): 9,
    ("detective", "hautgauche"): 7, ("detective", "haut"): 11, ("cowboy", "gauche"): 7,
    ("pilote", "hautgauche"): 7, ("pilote", "haut"): 9, ("policier", "haut"): 9,
    ("pompiere", "hautgauche"): 4, ("pompiere", "haut"): 9, ("pompiere", "droite"): 4, ("pompiere", "bas"): 17,
}


# Cases refaites apres relecture (2026-11-22) : bouche disparue ou oeil ferme
# en levant la tete, regard trop timide. TIRAGES_PLUS tirages de plus, avec
# une consigne qui exige les deux yeux ouverts et la bouche visible.
REFAIRE = {
    ("facteur", "hautgauche"), ("plongeuse", "hautgauche"), ("chevalier", "hautgauche"),
    ("detective", "hautgauche"), ("detective", "haut"), ("cowboy", "gauche"),
    ("pilote", "hautgauche"), ("pilote", "haut"), ("policier", "haut"),
    ("pompiere", "hautgauche"), ("pompiere", "haut"), ("pompiere", "droite"), ("pompiere", "bas"),
}
TIRAGES_PLUS = 8
NETTETE = ("Both of its eyes stay wide open and clearly visible, and its small smiling mouth stays clearly visible "
           "on its face.")


# Regard vers le bas : les paupieres tombaient (yeux mi-clos) ; encore des
# tirages, avec les yeux ouverts exiges.
YEUX_OUVERTS = {("pompiere", "bas")}
OUVERTS = ("Its eyes are wide open, never closed or half-closed: we clearly see its big round pupils looking down "
           "toward the bottom edge of the image.")


def nb_tirages(nom: str, d: str) -> int:
    n = TIRAGES + (TIRAGES_PLUS if (nom, d) in REFAIRE else 0)
    return n + (TIRAGES_PLUS if (nom, d) in YEUX_OUVERTS else 0)


def consigne_du_tirage(c: str, g: int) -> str:
    if g <= TIRAGES:
        return c
    if g <= TIRAGES + TIRAGES_PLUS:
        return f"{c} {NETTETE}"
    return f"{c} {OUVERTS}"


def consignes(nom: str) -> dict:
    acc = PERSONNES[nom][1]
    c = {d: f"{p.format(x='person', acc=acc)} {_FIN}" for d, p in REGARDS.items()}
    for d in ("gauche", "hautgauche", "basgauche"):
        c[d.replace("gauche", "droite")] = c[d].replace("left", "right")
    for cle, phrase in REACTIONS:
        c[cle] = f"{phrase.format(x='person')} {_FIN_REACTION.format(fond='pure white')}"
    return c


# Cotes et bas : depuis la base SANS marge (avec marge, Klein grossissait la
# tete tournee de ~30 %), puis ramenes dans le cadre.
SANS_MARGE = {"gauche", "droite", "basgauche", "bas", "basdroite"}


def vers_cadre(im: Image.Image) -> Image.Image:
    c = round(1024 * ECHELLE)
    feuille = Image.new("RGB", (1024, 1024), "white")
    feuille.paste(im.convert("RGB").resize((c, c), Image.LANCZOS), ((1024 - c) // 2, HAUT))
    return feuille


def cadre(nom: str) -> str:
    os.makedirs(CADRE, exist_ok=True)
    sortie = os.path.join(CADRE, f"{nom}.png")
    if not os.path.exists(sortie):
        src = Image.open(os.path.join(BASES, f"{nom}.png")).convert("RGB").resize((1024, 1024), Image.LANCZOS)
        vers_cadre(src).save(sortie)
    return sortie


def detourer(rgb: np.ndarray) -> np.ndarray:
    """Le masque (0..255) d'un buste sur fond blanc. Le bas du buste est
    peint en fondu vers le blanc : l'inondation depuis le bord du bas y
    entrait et mangeait un tablier ou une blouse blanche. On n'inonde donc
    que par le haut et les cotes, au-dessus du fondu, et le fondu est refait
    en alpha (la largeur du buste juste au-dessus, attenuee vers le bas)."""
    h = rgb.shape[0]
    lignes = np.nonzero((rgb.min(axis=2) < 225).sum(axis=1) > 20)[0]
    bas = int(lignes.max()) if len(lignes) else h - 1
    y0 = max(bas - FONDU, 1)
    fond = rgb.min(axis=2) >= 238
    fond[y0:] = False
    n, lab = cv2.connectedComponents(fond.astype(np.uint8), connectivity=4)
    bord = np.unique(np.concatenate([lab[0], lab[:y0, 0], lab[:y0, -1]]))
    fond = np.isin(lab, bord[bord > 0])
    plein = (~fond).astype(np.uint8)
    n, lab, st, _ = cv2.connectedComponentsWithStats(plein, connectivity=8)
    garde = np.zeros(n, bool)
    garde[1:] = st[1:, cv2.CC_STAT_AREA] > 30  # les coeurs, Z et etoiles flottants restent
    plein = cv2.erode(garde[lab].astype(np.uint8) * 255, np.ones((3, 3), np.uint8)).astype(np.float32)
    t = np.clip((np.arange(h) - y0) / max(bas - y0, 1), 0, 1)
    rampe = 1 - t * t * (3 - 2 * t)
    plein[y0:] = plein[y0 - 1][None, :] * rampe[y0:, None]
    return cv2.GaussianBlur(plein, (0, 0), 0.8)


def rgba(chemin: str) -> np.ndarray:
    rgb = np.asarray(Image.open(chemin).convert("RGB").resize((1024, 1024), Image.LANCZOS))
    return np.dstack([rgb, detourer(rgb)]).clip(0, 255).astype(np.uint8)


def _stabilite(img: np.ndarray, ref: np.ndarray) -> float:
    """L'ecart du bas du buste a la base (le corps doit rester en place),
    plus l'ecart de surface dessinee (une tete levee GROSSISSAIT)."""
    corps = float(np.abs(img[700:950].astype(np.float32) - ref[700:950]).mean())
    surface = abs(float((img.min(axis=2) < 225).mean()) - float((ref.min(axis=2) < 225).mean()))
    return corps + 400 * surface


def tirer(nom: str, cache: str) -> None:
    os.makedirs(cache, exist_ok=True)
    os.makedirs(RETENUS, exist_ok=True)
    base = cadre(nom)
    ref = np.asarray(Image.open(base).convert("RGB"), np.float32)
    c = consignes(nom)
    revue = []
    for d in [k for k in CASES if k] + [k for k, _ in REACTIONS]:
        chemins = []
        n = nb_tirages(nom, d)
        for g in range(1, n + 1):
            consigne = consigne_du_tirage(c[d], g)
            if d in SANS_MARGE:
                brut = os.path.join(cache, f"{nom}-{d}-sm-{g}.png")
                if not os.path.exists(brut):
                    comfy_klein.editer(os.path.join(BASES, f"{nom}.png"), brut, consigne, g)
                s = os.path.join(cache, f"{nom}-{d}-smc-{g}.png")
                if not os.path.exists(s):
                    vers_cadre(Image.open(brut)).save(s)
            else:
                s = os.path.join(cache, f"{nom}-{d}-{g}.png")
                if not os.path.exists(s):
                    comfy_klein.editer(base, s, consigne, g)
            chemins.append(s)
        imgs = [np.asarray(Image.open(s).convert("RGB").resize((1024, 1024))) for s in chemins]
        k = CHOIX.get((nom, d), min(range(n), key=lambda i: _stabilite(imgs[i], ref)))
        Image.open(chemins[k]).convert("RGB").save(os.path.join(RETENUS, f"{nom}-{d}.webp"), quality=95, method=6)
        revue.append((d, imgs, k))
        print(nom, d, "tirage", k, flush=True)
    T = 200
    r = Image.new("RGB", (T * (max(len(i) for _, i, _ in revue) + 1), T * len(revue)), "white")
    dr = ImageDraw.Draw(r)
    for j, (d, imgs, k) in enumerate(revue):
        r.paste(Image.fromarray(ref.astype(np.uint8)).resize((T, T)), (0, j * T))
        dr.text((4, j * T + 4), d, fill="red")
        for i, im in enumerate(imgs):
            r.paste(Image.fromarray(im).resize((T, T)), ((i + 1) * T, j * T))
            dr.text(((i + 1) * T + 4, j * T + 4), str(i), fill="blue")
        dr.rectangle(((k + 1) * T, j * T, (k + 2) * T - 1, (j + 1) * T - 1), outline="red", width=3)
    r.save(os.path.join(cache, f"revue-{nom}.png"))


def planches(nom: str) -> dict:
    o = rgba(cadre(nom))

    def case(c):
        return o if c is None else rgba(os.path.join(RETENUS, f"{nom}-{c}.webp"))

    imgs = {"directions": [case(c) for c in CASES], "reactions": [case(cle) for cle, _ in REACTIONS]}
    commune = tetes.boite_commune(*imgs.values())
    return {sorte: tetes.placer(i, commune) for sorte, i in imgs.items()}


def main() -> None:
    quoi, ou = sys.argv[1], sys.argv[2]
    for nom in sys.argv[3:] or list(PERSONNES):
        if quoi == "tirer":
            tirer(nom, ou)
        elif quoi == "ecrire":
            ia.ecrire(ou, f"personne-{nom}", planches(nom))
            print(nom, "ok", flush=True)
    print("FINI")


if __name__ == "__main__":
    main()
