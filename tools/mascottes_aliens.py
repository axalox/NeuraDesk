# Les EXTRATERRESTRES (2026-11-21) : 42 mascottes, dix especes en 3 a 7
# variantes. Le corps vient de FLUX.1-schnell (`tools/comfy_flux.py`, serveur
# ComfyUI local), garde sous `tools/mascottes_ia/aliens/alien-<nom>.webp`.
#
# Contrairement aux mascottes a lunettes (`mascottes_ia.py`), il n'y a pas de
# verre ou effacer l'oeil pour redessiner l'iris : les 9 regards ET les 9
# reactions sont des retouches de Qwen-Image-Edit-2509
# (`tools/comfy_qwen_edit.py`), recollees au cou sur le corps de base par
# `mascottes_tetes.assembler` — le corps ne bouge pas d'une case a l'autre.
# Les regards de droite sont ceux de gauche en miroir.
#
#   python tools/mascottes_aliens.py retouches [nom...]     (serveur ComfyUI)
#       -> tools/mascottes_ia/aliens/retouches/<nom>-<case>.webp
#   python tools/mascottes_aliens.py planches <dest> [nom...]   (sans serveur)
#       -> les planches dans les six styles
#
# Une retouche ratee se refait avec une autre graine : l'inscrire dans
# GRAINES, effacer son fichier, relancer `retouches`.
import os
import sys

import cv2
import numpy as np
from PIL import Image

ICI = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, ICI)
import mascottes_ia as ia  # noqa: E402
import mascottes_styles as styles  # noqa: E402
import mascottes_tetes as tetes  # noqa: E402

DOSSIER = os.path.join(ICI, "mascottes_ia", "aliens")
RETOUCHES = os.path.join(DOSSIER, "retouches")
MEGAPIXELS = 0.6

# Espece -> (libelle FR, libelle EN, ce que Qwen doit tourner : « the alien's
# head and its helmet »...).
ESPECES = {
    "retro": ("Petit homme vert", "Little green man", ""),
    "cyclope": ("Cyclope violet", "Purple cyclops", ""),
    "cristal": ("Extraterrestre de cristal", "Crystal alien", " and its forehead gem"),
    "poilu": ("Extraterrestre poilu", "Furry alien", " and its headphones"),
    "savant": ("Savant gris", "Grey scientist", ""),
    "champignon": ("Extraterrestre champignon", "Mushroom alien", " and its mushroom cap"),
    "axolotl": ("Axolotl de l'espace", "Space axolotl", " inside its glass helmet"),
    "meduse": ("Méduse de l'espace", "Space jellyfish", " and its jellyfish bell"),
    "lave": ("Golem de lave", "Lava golem", " and its miner's helmet"),
    "etoile": ("Extraterrestre étoilé", "Starry alien", ""),
}

# Mesures en pixels de l'image de base (1024) : `cou` (y0, y1) la bande ou la
# tete retouchee rejoint le corps de base ; `garde` des rectangles ou la base
# reste (un outil tenu a cote de la tete) ; `trous` le centre des fonds blancs
# ENFERMES dans le dessin (entre un bras et le corps, dans la boucle d'un fil) :
# le detourage par le bord ne les atteint pas.
ALIENS = {
    "axolotl-1": dict(cou=(600, 650)),
    "axolotl-2": dict(cou=(600, 650)),
    "axolotl-3": dict(cou=(640, 690)),
    "champignon-1": dict(cou=(550, 600)),
    "champignon-2": dict(cou=(560, 610)),
    "champignon-3": dict(cou=(470, 520), trous=[(504, 866)]),
    "cristal-1": dict(cou=(555, 600)),
    "cristal-2": dict(cou=(575, 620)),
    "cristal-3": dict(cou=(570, 625)),
    "cyclope-1": dict(cou=(510, 555)),
    "cyclope-2": dict(cou=(525, 565)),
    "cyclope-3": dict(cou=(495, 540)),
    "etoile-1": dict(cou=(450, 500), trous=[(467, 723), (503, 808)]),
    "etoile-2": dict(cou=(370, 420)),
    "etoile-3": dict(cou=(310, 350)),
    "etoile-4": dict(cou=(450, 490), trous=[(321, 509), (719, 457)]),
    "etoile-5": dict(cou=(450, 490)),
    "etoile-6": dict(cou=(450, 500)),
    "etoile-7": dict(cou=(500, 560), trous=[(365, 667)]),
    "lave-1": dict(cou=(470, 520), garde=[(0, 250, 200, 760)], trous=[(365, 518)]),
    "lave-2": dict(cou=(520, 560)),
    "lave-3": dict(cou=(500, 540), garde=[(0, 280, 320, 760)]),
    "lave-4": dict(cou=(550, 590), garde=[(0, 400, 330, 1024)]),
    "lave-5": dict(cou=(400, 440)),
    "lave-6": dict(cou=(500, 545), trous=[(364, 640), (717, 847)]),
    "lave-7": dict(cou=(580, 620), garde=[(0, 430, 330, 1024)], trous=[(309, 635)]),
    "meduse-1": dict(cou=(510, 550)),
    "meduse-2": dict(cou=(545, 585)),
    "meduse-3": dict(cou=(560, 600)),
    "meduse-4": dict(cou=(585, 625)),
    "meduse-5": dict(cou=(585, 625)),
    "meduse-6": dict(cou=(575, 615)),
    "meduse-7": dict(cou=(585, 620)),
    "poilu-1": dict(cou=(530, 580), trous=[(317, 554)]),
    "poilu-2": dict(cou=(540, 580)),
    "poilu-3": dict(cou=(530, 580), trous=[(326, 561), (308, 865)]),
    "retro-1": dict(cou=(510, 560)),
    "retro-2": dict(cou=(570, 615)),
    "retro-3": dict(cou=(515, 555)),
    "savant-1": dict(cou=(530, 575)),
    "savant-2": dict(cou=(430, 480)),
    "savant-3": dict(cou=(510, 560)),
}

# Retires de l'app le 2026-11-22 (a la demande) : leurs images de base restent
# ici, mais ils ne sont plus generes ni livres.
RETIRES = {"etoile-4", "meduse-6", *(f"lave-{i}" for i in range(1, 8)), *(f"savant-{i}" for i in range(1, 4))}
# 2e tri (meme jour) : l'oeil unique des cyclopes et les meduses sans cou ne
# tiennent pas d'une image a l'autre ; l'etoile 1 est de profil.
RETIRES |= {"cyclope-1", "cyclope-2", "cyclope-3", "etoile-1", "meduse-1", "meduse-2", "meduse-3", "meduse-7", "retro-1"}
LIVRES = [n for n in ALIENS if n not in RETIRES]

# Les graines retenues quand la premiere retouche etait ratee : (nom, case).
GRAINES: dict = {
    ("lave-6", "bas"): 3, ("lave-6", "basgauche"): 2,  # la mine etait fachee
    # la fourrure disparaissait (visage lisse, dessin plat)
    ("poilu-2", "cligne"): 2, ("poilu-2", "etonne"): 2, ("poilu-2", "ravi"): 2,
}

# Rappels ajoutes a chaque consigne : Qwen donnait deux yeux au cyclope, et
# un contour de couleur (bleu, jaune, rose) a la tete des savants, qui n'en ont
# pas : sur la planche, il s'arretait net au cou.
RAPPEL = "It keeps exactly the same number of eyes as in the original image."
RAPPELS = {"cyclope": "It is a cyclops: it has only ONE single big eye in the middle of its face, never two eyes.",
           "savant": "Do not add any outline or colored contour line around its head or body: its edges stay "
                     "exactly as soft and thin as in the original image."}

_FIN_REACTION = ("Its body and everything it holds stay exactly where they are. Same head position facing the viewer, "
                 "same character, same colors, same art style, same outline, plain {fond} background.")
# Dans l'ordre de la planche des reactions (voir `mascotte.rs`).
REACTIONS = [
    ("cligne", "The {x} blinks: both of its eyes are fully closed, with a calm happy smile."),
    ("coeur", "The {x} is in love: its eyes have become two shiny red hearts, it smiles happily, and three small "
              "pink hearts float around its head."),
    ("etincelles", "The {x} is amazed and delighted: its eyes sparkle with little stars, it smiles with its mouth "
                   "open, and small yellow sparkles twinkle around its head."),
    ("etonne", "The {x} is surprised: its eyes are wide open and its mouth forms a small round 'oh'."),
    ("clin", "The {x} winks: the eye on the left side of the image is closed in a wink, the other eye stays open, "
             "with a playful smile."),
    ("timide", "The {x} is shy: it blushes with bright pink cheeks, its eyes look down and it has a small bashful "
               "smile."),
    ("endormi", "The {x} is asleep: both of its eyes are closed, its face is peaceful, and three small blue letters Z "
                "float above its head."),
    ("etourdi", "The {x} is dizzy: its eyes are spirals, it has a wobbly smile, and small yellow stars circle "
                "above its head."),
    ("ravi", "The {x} is overjoyed: its eyes are closed in happy upward arcs, it has a very big open smile and "
             "bright pink cheeks."),
]


# Le cyclope n'a qu'un oeil : « ses yeux deviennent des coeurs » lui en
# redonnait deux.
REACTIONS_CYCLOPE = {
    "cligne": "The {x} blinks: its single eye is fully closed, with a calm happy smile.",
    "coeur": "The {x} is in love: its single eye has become one big shiny red heart, it smiles happily, and three "
             "small pink hearts float around its head.",
    "clin": "The {x} winks playfully: its single eye is closed in a cheeky wink, with a playful grin and its tongue "
            "slightly out.",
}


def source(nom: str) -> str:
    return os.path.join(DOSSIER, f"alien-{nom}.webp")


def espece(nom: str) -> str:
    return nom.rsplit("-", 1)[0]


def fond_vert(rgb: np.ndarray) -> bool:
    b = np.concatenate([rgb[0], rgb[-1], rgb[:, 0], rgb[:, -1]]).astype(int)
    r, v, bl = np.median(b, axis=0)
    return v > 120 and v > r + 60 and v > bl + 60


BLANC_MIN = 150
# La part de la hauteur de la tete dont la couture du cou descend pour les
# regards vers le bas (voir `planches`).
DESCENTE_BAS = 0.22


def detourer(rgb: np.ndarray, trous=(), blancs: bool = False, petits: bool = False,
             ombre: bool = False) -> np.ndarray:
    """Le masque du personnage (0..255). Fond VERT (les savants : leur blouse
    blanche touchait le fond blanc, Qwen les a reposes sur du vert) : par la
    distance a la couleur du fond. Fond blanc : les pixels quasi blancs
    CONNEXES au bord, a seuil FIXE — l'inondation flottante de
    `mascottes_ia.detourer` glissait le long des degrades doux (aquarelle,
    3D, fourrure) et avalait le personnage. Les petites taches isolees (une
    signature) partent ; les trous interieurs restent pleins, sauf les
    `trous` nommes et, avec `blancs` (un personnage sans aucun blanc : les
    golems de lave), tout blanc pur enferme de plus de BLANC_MIN px — Qwen
    laisse du fond pris entre la pioche, la tete et le corps."""
    if fond_vert(rgb):
        b = np.concatenate([rgb[0], rgb[-1], rgb[:, 0], rgb[:, -1]]).astype(np.float32)
        ref = np.median(b, axis=0)
        dist = np.linalg.norm(rgb.astype(np.float32) - ref, axis=2)
        fond = dist < 60
    else:
        fond = rgb.min(axis=2) >= 238
        if ombre:  # l'ombre grise sous les pieds (rendus 3D de Klein), dans le bas de l'image seulement
            f = rgb.astype(np.int16)
            gris = (f.min(axis=2) >= 175) & (f.max(axis=2) - f.min(axis=2) < 20)
            gris[: rgb.shape[0] * 3 // 4] = False
            fond |= gris
    n, lab = cv2.connectedComponents(fond.astype(np.uint8), connectivity=4)
    bord = np.unique(np.concatenate([lab[0], lab[-1], lab[:, 0], lab[:, -1]]))
    fond = np.isin(lab, bord[bord > 0]) & fond
    if trous or blancs:
        blanc = (rgb.min(axis=2) >= 245) & ~fond
        n, lab, st, _ = cv2.connectedComponentsWithStats(blanc.astype(np.uint8), connectivity=4)
        for i in range(1, n):
            c = (st[i, 0] + st[i, 2] / 2, st[i, 1] + st[i, 3] / 2)
            if (blancs and st[i, cv2.CC_STAT_AREA] > BLANC_MIN) or                     any(abs(c[0] - x) <= 3 and abs(c[1] - y) <= 3 for x, y in trous):
                fond |= lab == i
    plein = (~fond).astype(np.uint8)
    n, lab, st, _ = cv2.connectedComponentsWithStats(plein, connectivity=8)
    if n > 1:
        grand = 1 + np.argmax(st[1:, cv2.CC_STAT_AREA])
        garde = np.zeros(n, bool)
        # `petits` : les etoiles, coeurs et etincelles qui flottent autour de la
        # tete d'une expression sont de petites taches isolees, a garder.
        garde[1:] = st[1:, cv2.CC_STAT_AREA] > (30 if petits else 0.01 * st[grand, cv2.CC_STAT_AREA])
        plein = garde[lab].astype(np.uint8)
    plein = cv2.erode(plein * 255, np.ones((3, 3), np.uint8))
    return cv2.GaussianBlur(plein, (0, 0), 0.8)


def _lire_rgb(chemin: str) -> np.ndarray:
    return np.asarray(Image.open(chemin).convert("RGB").resize((1024, 1024), Image.LANCZOS)).copy()


def _despill(rgb: np.ndarray) -> np.ndarray:
    """Sur fond vert, le bord gardait un liseré vert : le vert est ramene au
    plus fort des deux autres canaux."""
    r, v, b = (rgb[..., k].astype(np.int16) for k in range(3))
    return np.dstack([r, np.minimum(v, np.maximum(r, b)), b]).astype(np.uint8)


def tete_de(nom: str) -> dict:
    a = ALIENS[nom]
    fr, en, acc = ESPECES[espece(nom)]
    rgb = _lire_rgb(source(nom))
    return dict(cou=a["cou"], garde=a.get("garde", ()), x="alien", acc=acc,
                corps="Its body and everything it holds", fond="green" if fond_vert(rgb) else "white",
                recaler_miroir=True)


def consignes(nom: str) -> dict:
    """{case: consigne} : les 5 tetes tournees (`mascottes_tetes.CONSIGNES`)
    et les 9 reactions."""
    t = tete_de(nom)
    rappel = " ".join(filter(None, (RAPPEL, RAPPELS.get(espece(nom)))))
    c = {d: tetes.consigne(t, d) + " " + rappel for d in tetes.DIRS}
    for cle, phrase in REACTIONS:
        if espece(nom) == "cyclope":
            phrase = REACTIONS_CYCLOPE.get(cle, phrase)
        c[cle] = phrase.format(x=t["x"]) + " " + _FIN_REACTION.format(fond=t["fond"]) + " " + rappel
    return c


def retouches(noms: list) -> None:
    from comfy_qwen_edit import modifier
    import time
    os.makedirs(RETOUCHES, exist_ok=True)
    for nom in noms:
        for case, phrase in consignes(nom).items():
            sortie = os.path.join(RETOUCHES, f"{nom}-{case}.webp")
            if os.path.exists(sortie):
                continue
            t0 = time.time()
            png = sortie[:-5] + ".png"
            modifier(source(nom), png, GRAINES.get((nom, case), 1), phrase, megapixels=MEGAPIXELS)
            # Les retouches ne donnent que la tete, petite dans la case : du
            # WebP avec perte suffit (le corps vient toujours de la base).
            Image.open(png).convert("RGB").save(sortie, quality=90, method=6)
            os.remove(png)
            print(nom, case, f"{time.time() - t0:.1f} s", flush=True)


def _sans_contour(rgb: np.ndarray, a: np.ndarray) -> np.ndarray:
    """Efface le contour de couleur que Qwen ajoute autour de la tete grise des
    savants (meme quand on le lui interdit) : les pixels SATURES qui touchent a
    la fois le fond et la peau grise. Les coeurs, les Z et les etoiles qui
    flottent autour de la tete ne touchent pas la peau : ils restent."""
    f = rgb.astype(np.float32)
    mx, mn = f.max(axis=2), f.min(axis=2)
    sat = (mx - mn) / np.maximum(mx, 1)
    fond = (a < 128).astype(np.uint8)
    gris = ((sat < 0.15) & (a >= 128)).astype(np.uint8)
    # Le trait fait ~10 px (image 1024) : son milieu est a ~5 px du fond et
    # de la peau, et un pixel colle au fond est a 10 px de la peau.
    pres_fond = cv2.dilate(fond, np.ones((25, 25), np.uint8)) > 0
    pres_gris = cv2.dilate(gris, np.ones((25, 25), np.uint8)) > 0
    trait = (sat > 0.3) & (mx > 50) & pres_fond & pres_gris & (a >= 128)
    trait = cv2.dilate(trait.astype(np.uint8), np.ones((3, 3), np.uint8)) > 0
    return np.where(trait, 0, a).astype(a.dtype)


def _detourer_net(trous, blancs=False, contour=False, petits=False, ombre=False):
    """Pour `mascottes_tetes.assembler` : (rgb sans liseré vert, alpha). Les
    `trous` (positions de la base) ne s'appliquent qu'a elle : ailleurs, les
    centres ne tombent sur rien."""
    def d(rgb):
        net = _despill(rgb) if fond_vert(rgb) else rgb
        a = detourer(rgb, trous, blancs, petits, ombre)
        return net, (_sans_contour(net, a) if contour else a)
    return d


# Les 8 regards tournes par FLUX.2 Klein 4B (2026-11-22, Apache-2.0) : images
# ENTIERES (le corps reste quasi immobile chez Klein), une par direction,
# choisies parmi 6 tirages et ramenees aux couleurs de la base. Plus de
# collage tete/corps ni de miroir : les trois regards de droite sont tournes
# pour de vrai.
KLEIN = os.path.join(DOSSIER, "klein")
KLEIN_CASES = ["hautgauche", "haut", "hautdroite", "gauche", None, "droite", "basgauche", "bas", "basdroite"]


def _image_entiere(chemin: str, detourer) -> np.ndarray:
    rgb, a = tetes._lire(chemin, detourer)
    return np.dstack([rgb, a * 255]).astype(np.uint8)


# 2e passe (2026-11-22) : base REDUITE a 80 % dans une marge (`CADRE`), pour
# que la tete levee, les coeurs ou les Z ne sortent plus de l'image ; les 8
# regards ET les 9 expressions sont des images entieres de Klein, sur ce cadre.
KLEIN3 = os.path.join(DOSSIER, "klein3")
CADRE = os.path.join(DOSSIER, "cadre")
ECHELLE_CADRE, HAUT_CADRE = 0.8, 205


def _planches_klein3(nom: str) -> dict:
    marge = (1024 - round(1024 * ECHELLE_CADRE)) // 2
    trous = [(round(x * ECHELLE_CADRE + marge), round(y * ECHELLE_CADRE + HAUT_CADRE))
             for x, y in ALIENS[nom].get("trous", ())]
    ombre = espece(nom) == "poilu"
    d = _detourer_net(trous, ombre=ombre)
    o = _image_entiere(os.path.join(CADRE, f"{nom}.webp"), d)
    dr = _detourer_net(trous, petits=True, ombre=ombre)

    def case(c, det):
        return o if c is None else _image_entiere(os.path.join(KLEIN3, f"{nom}-{c}.webp"), det)

    imgs = {"directions": [case(c, d) for c in KLEIN_CASES], "reactions": [case(cle, dr) for cle, _ in REACTIONS]}
    commune = tetes.boite_commune(*imgs.values())
    return {sorte: tetes.placer(i, commune) for sorte, i in imgs.items()}


def planches(nom: str) -> dict:
    """{sorte: 1536 RGBA} pour les directions et les reactions."""
    if os.path.exists(os.path.join(KLEIN3, f"{nom}-ravi.webp")):
        return _planches_klein3(nom)
    t = tete_de(nom)
    rgb = _lire_rgb(source(nom))
    trous = ALIENS[nom].get("trous", ())
    blancs = espece(nom) == "lave"
    contour = espece(nom) == "savant"
    a = detourer(rgb, trous, blancs)
    # L'axe du miroir est le centre de la TETE (au-dessus du cou), pas celui de
    # la boite de la figure : une cape, une tasse ou un tentacule d'un seul cote decentrait
    # la boite, et la tete miroir se recollait a cote du corps. Le reste de
    # l'ecart est rattrape par un second recalage apres le miroir.
    axe = ALIENS[nom].get("axe") or float(np.nonzero(a[:t["cou"][0]] > 128)[1].mean())
    base = source(nom)
    r = os.path.join(RETOUCHES, nom + "-{}.webp")
    # Les tetes penchees vers le bas : la couture du cou descend d'une part de
    # la hauteur de la tete (le visage y glisse vers le bas ; le haut du crane,
    # lui, ne baisse pas — mesure : Qwen montre le dessus de la tete).
    cou0 = t["cou"][0]
    hauteur_tete = cou0 - int(np.nonzero(a[:cou0].max(axis=1) > 128)[0].min())
    descente = round(DESCENTE_BAS * hauteur_tete)
    d = _detourer_net(trous, blancs, contour)
    if os.path.exists(os.path.join(KLEIN, f"{nom}-gauche.webp")):
        directions = [None if c is None else _image_entiere(os.path.join(KLEIN, f"{nom}-{c}.webp"), d)
                      for c in KLEIN_CASES]
    else:
        directions = [None if c is None else (r.format(c), m, descente if c.startswith("bas") else 0)
                      for c, m in tetes.CASES]
    reactions = [(r.format(cle), False) for cle, _ in REACTIONS]
    imgs = {"directions": tetes.composer(base, t, axe, directions, d),
            "reactions": tetes.composer(base, t, axe, reactions, d)}
    commune = tetes.boite_commune(*imgs.values())
    return {sorte: tetes.placer(i, commune) for sorte, i in imgs.items()}


def main() -> None:
    quoi = sys.argv[1]
    if quoi == "retouches":
        retouches(sys.argv[2:] or LIVRES)
        return
    dest = sys.argv[2]
    for nom in sys.argv[3:] or LIVRES:
        rendus = planches(nom)
        ia.ecrire(dest, f"alien-{nom}", rendus)
        for style in styles.STYLES:
            ia.ecrire(dest, f"alien-{nom}-{style}", styles.styliser(rendus, style))
        print(nom, "ok", flush=True)


if __name__ == "__main__":
    main()
