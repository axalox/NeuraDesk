# Atterrissage d'un heros : detoure (BiRefNet) ses trois clips -- approche, atterrissage, marche --
# avec UN cadre et UNE echelle communs (les raccords tombent au pixel), puis les range en planches
# de 8 colonnes et ecrit la fiche lue par l'application.
#   python sequence.py <dossier_sortie> <n>...
# Entrees : images/app_hNN (boucle, la derniere = la premiere), images/att_hNN, images/mar_hNN (boucle).
import glob, os, sys
import numpy as np
from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import detourer_ia as ia

ICI = os.path.dirname(os.path.abspath(__file__))
COLONNES = 8
HAUTEUR_DEBOUT = 280          # hauteur du heros debout (clip de marche) dans les planches


def detourer(dossier):
    images = [Image.open(f).convert("RGB") for f in sorted(glob.glob(os.path.join(dossier, "*.png")))]
    alphas = []
    for k in range(0, len(images), 4):
        alphas += ia.alpha_de(images[k:k + 4])
    sorties = []
    for im, a in zip(images, alphas):
        rgb = np.asarray(im)
        bords = np.concatenate([rgb[:12, :12].reshape(-1, 3), rgb[:12, -12:].reshape(-1, 3),
                                rgb[-12:, :12].reshape(-1, 3), rgb[-12:, -12:].reshape(-1, 3)])
        fond = np.median(bords, 0).astype(np.float32) / 255
        a = ia.nettoyer(a)
        sorties.append(np.dstack([ia.defond(rgb, a, fond), a]))
    return sorties


def foulee(marche, pieds):
    """Pixels parcourus par cycle : le pied POSE recule a la vitesse de la marche (la camera suit).
    On suit, d'image en image, le glissement de la bande des pieds qui colle le mieux."""
    h = marche[0].shape[0]
    bas = max(pieds - int(0.05 * HAUTEUR_DEBOUT / ECHELLE), 0)
    total = 0.0
    for a, b in zip(marche, marche[1:] + marche[:1]):
        sa, sb = a[bas:pieds + 1, :, 3] > 0.5, b[bas:pieds + 1, :, 3] > 0.5
        meilleur = min(range(-25, 6), key=lambda d: np.logical_xor(np.roll(sa, d, axis=1), sb).sum())
        total += -meilleur
    return total


def planche(images, chemin, echelle, cadre):
    y0, y1, x0, x1 = cadre
    l, h = round((x1 - x0) * echelle), round((y1 - y0) * echelle)
    lignes = (len(images) + COLONNES - 1) // COLONNES
    p = Image.new("RGBA", (l * COLONNES, h * lignes), (0, 0, 0, 0))
    for k, o in enumerate(images):
        im = Image.fromarray((o[y0:y1, x0:x1] * 255 + 0.5).astype(np.uint8), "RGBA").resize((l, h), Image.LANCZOS)
        p.paste(im, ((k % COLONNES) * l, (k // COLONNES) * h))
    p.save(chemin, "WEBP", quality=88, method=6, alpha_quality=100)
    return (l, h), os.path.getsize(chemin)


def traiter(n, dst):
    app = detourer(os.path.join(ICI, "images", f"app_h{n:02d}"))[:-1]   # boucle : la derniere = la premiere
    att = detourer(os.path.join(ICI, "images", f"att_h{n:02d}"))
    mar = detourer(os.path.join(ICI, "images", f"mar_h{n:02d}"))[:-1]
    union = np.zeros(att[0].shape[:2], bool)
    for o in app + att + mar:
        union |= o[..., 3] > 0.03
    ys, xs = np.where(union)
    m = 4
    cadre = (max(ys.min() - m, 0), min(ys.max() + m + 1, union.shape[0]), max(xs.min() - m, 0), min(xs.max() + m + 1, union.shape[1]))
    # la ligne des pieds et la hauteur debout, mesurees sur la marche
    bas_images = [np.where((o[..., 3] > 0.5).any(1))[0] for o in mar]
    pieds = int(np.median([b.max() for b in bas_images]))
    debout = float(np.median([b.max() - b.min() for b in bas_images]))
    global ECHELLE
    ECHELLE = HAUTEUR_DEBOUT / debout
    tailles = {}
    for nom, images in (("approche", app), ("atterrissage", att), ("marche", mar)):
        taille, octets = planche(images, os.path.join(dst, f"heros-{n:02d}-{nom}.webp"), ECHELLE, cadre)
        tailles[nom] = (len(images), taille, octets)
    y0, y1, x0, x1 = cadre
    fiche = {
        "approche": len(app), "atterrissage": len(att), "marche": len(mar),
        "largeur": tailles["marche"][1][0], "hauteur": tailles["marche"][1][1],
        "pieds": round((pieds - y0) * ECHELLE),
        "debout": HAUTEUR_DEBOUT,
        "foulee": round(foulee(mar, pieds) * ECHELLE),
    }
    with open(os.path.join(dst, f"heros-{n:02d}-atterrissage.txt"), "w", encoding="utf8", newline="\n") as f:
        f.writelines(f"{k}={v}\n" for k, v in fiche.items())
    print(n, fiche, {k: f"{v[2] / 1e6:.1f} Mo" for k, v in tailles.items()})


if __name__ == "__main__":
    dst = sys.argv[1]
    os.makedirs(dst, exist_ok=True)
    for n in map(int, sys.argv[2:]):
        traiter(n, dst)
