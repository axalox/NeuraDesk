# Atterrissage : pour chaque heros, trois clips Wan (approche en boucle, atterrissage A->W, marche en boucle).
# Reprend ou il s'est arrete (un clip deja fait est saute).  python serie_atterrissage.py [n...]
import os, subprocess, sys
ICI = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, ICI)
PY = sys.executable
import importlib.util
def charger(nom):
    spec = importlib.util.spec_from_file_location(nom, os.path.join(ICI, nom + ".py"))
    src = open(os.path.join(ICI, nom + ".py"), encoding="utf8").read()
    debut = src.index("TENUE = {"); fin = src.index("}\n", debut) + 1
    ns = {}; exec(src[debut:fin], ns); return ns["TENUE"]
TENUE = {**charger("serie"), **charger("serie_neon")}
TENUE[1] = "a dark-haired man in a dark red suit with a round gold sun emblem on his chest, gold bracers, gold boots and a red cape"
POSES = {1: "A4 W2", 2: "A1 W2", 3: "A2 W1", 4: "A1 W2", 5: "A2 W2", 6: "A2 W2", 7: "A1 W2", 8: "A3 W3", 9: "A1 W2", 10: "A1 W2",
         11: "A1 W3", 12: "A1 W2", 13: "A3 W2", 14: "A2 W1", 15: "A1 W2", 16: "A2 W2", 17: "A1 W2", 18: "A1 W2", 19: "A2 W2", 20: "A3 W1",
         21: "A1 W1", 22: "A3 W3", 23: "A1 W2", 24: "A3 W3", 25: "A2 W2", 26: "A1 W2", 27: "A1 W2", 28: "A1 W3", 29: "A2 W1", 30: "A1 W2"}
GRIS = "The background stays a perfectly flat uniform neutral grey color during the whole video. Static camera."
CLIPS = [
    ("app", "A", None, 33, "The superhero, {t}, flies toward the viewer, cape flapping strongly in the wind, body gently bobbing up and down, arms forward. " + GRIS),
    ("att", "A", "W", 81, "The superhero, {t}, flying toward the viewer, descends and lands on both feet on the ground in the lower part of the frame, bends the knees to absorb the landing, straightens up, then turns to the right and starts walking to the right, seen from the side. " + GRIS),
    ("mar", "W", None, 33, "The superhero, {t}, walks to the right seen from the side, a natural walking cycle, arms swinging, cape swaying. The camera tracks the hero so the hero stays in the center of the frame. " + GRIS),
]
for n in [int(a) for a in sys.argv[1:]] or list(POSES):
    a, w = POSES[n].split()
    pose = {"A": os.path.join(ICI, "poses", f"h{n:02d}_{a}.png"), "W": os.path.join(ICI, "poses", f"h{n:02d}_{w}.png")}
    for k, (nom, debut, fin, longueur, consigne) in enumerate(CLIPS):
        dossier = os.path.join(ICI, "images", f"{nom}_h{n:02d}")
        if os.path.isdir(dossier) and len(os.listdir(dossier)) == longueur:
            continue
        env = dict(os.environ, VOL_L="640", VOL_H="640", VOL_IMAGES=str(longueur))
        if fin:
            env["VOL_FIN"] = pose[fin]
        else:
            env["VOL_BOUCLE"] = "1"
        r = subprocess.run([PY, "-I", os.path.join(ICI, "video.py"), pose[debut], f"super_heros/{nom}_h{n:02d}", str(5000 + 10 * n + k),
                            consigne.format(t=TENUE[n])], env=env, capture_output=True, text=True)
        print(n, nom, "ECHEC " + r.stderr[-300:] if r.returncode else "ok", flush=True)
