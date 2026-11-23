# Ou commence la marche DANS le clip d'atterrissage ? Wan y fait marcher le heros SUR PLACE (sa
# derniere image est clouee a la premiere de la marche) : l'application doit deja le faire avancer.
# Mesure, d'image en image, le recul de la bande des pieds (le pied pose glisse vers l'arriere
# quand le heros marche sur place) et ecrit `marche_des=<image>` dans la fiche.
#   python debut_marche.py <dossier_mascottes> [n...]   (--sec : n'ecrit rien, affiche)
import glob, os, sys
import numpy as np
from PIL import Image

COLONNES = 8


def cases(chemin, f, combien):
    p = np.asarray(Image.open(chemin).convert("RGBA"))
    L, H = f["largeur"], f["hauteur"]
    return [p[(k // COLONNES) * H:(k // COLONNES + 1) * H, (k % COLONNES) * L:(k % COLONNES + 1) * L, 3] > 128
            for k in range(combien)]


def reculs(images, f):
    """Recul (px, >0 vers l'arriere = vers la gauche) de la bande des pieds entre images voisines."""
    bas = f["pieds"]
    haut = bas - max(4, f["debout"] // 20)
    v = []
    for a, b in zip(images, images[1:]):
        sa, sb = a[haut:bas + 1], b[haut:bas + 1]
        if sa.sum() < 20 or sb.sum() < 20:
            v.append(0.0)
            continue
        d = min(range(-25, 26), key=lambda d: np.logical_xor(np.roll(sa, d, axis=1), sb).sum())
        v.append(-float(d))
    return np.array(v)


def debut_marche(att, f):
    """La premiere image ou la marche s'installe : la MEDIANE du recul sur les 8 images suivantes
    atteint la moitie du pas de la marche (une image aberrante n'y fait rien ; la fin du clip
    ralentit pour rejoindre la premiere image de la marche, on ne remonte donc pas depuis la fin)."""
    pas = f["foulee"] / f["marche"]
    r = reculs(att, f)
    for k in range(len(r) // 2, len(r) - 8 + 1):   # une fois debout : pas pendant la reception
        if np.median(r[k:k + 8]) >= 0.5 * pas:
            return k, r
    return len(att) - 1, r


if __name__ == "__main__":
    d = sys.argv[1]
    sec = "--sec" in sys.argv
    ns = [int(a) for a in sys.argv[2:] if a != "--sec"]
    for fiche in sorted(glob.glob(os.path.join(d, "heros-*-atterrissage.txt"))):
        n = int(os.path.basename(fiche)[6:8])
        if ns and n not in ns:
            continue
        lignes = [l.strip() for l in open(fiche, encoding="utf8") if l.strip()]
        f = {k: int(v) for k, v in (l.split("=") for l in lignes)}
        att = cases(os.path.join(d, f"heros-{n:02d}-atterrissage.webp"), f, f["atterrissage"])
        k, r = debut_marche(att, f)
        print(f"{n:2d} marche_des={k:2d} ({(f['atterrissage'] - k) / 16:.2f} s sur place)")
        if not sec:
            lignes = [l for l in lignes if not l.startswith("marche_des=")] + [f"marche_des={k}"]
            with open(fiche, "w", encoding="utf8", newline="\n") as o:
                o.write("\n".join(lignes) + "\n")
