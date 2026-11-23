# Serie neon : heros 11..30, fond GRIS, clip en boucle (meme image debut/fin), detourage BiRefNet.
#   python serie_neon.py [n...]
import os, subprocess, sys

ICI = os.path.dirname(os.path.abspath(__file__))
PY = sys.executable
CHOIX = {11: 1, 12: 2, 13: 1, 14: 2, 15: 1, 16: 2, 17: 2, 18: 1, 19: 2, 20: 1,
         21: 1, 22: 2, 23: 2, 24: 3, 25: 3, 26: 1, 27: 1, 28: 1, 29: 2, 30: 2}
TENUE = {
    11: "a black-haired man in an electric blue and cyan suit with a glowing rainbow spiral emblem on his chest and a magenta cape",
    12: "a woman with long violet hair in a magenta suit with thin cyan lines and a violet cape",
    13: "a dark-haired man in a deep blue suit with a white star-spiral emblem and an orange-gold cape",
    14: "a woman with short green hair in a bright green and gold armored suit with blue gloves and boots and a red cape",
    15: "a man with a red mohawk in a red and orange suit with yellow flame patterns and a teal cape",
    16: "a blonde woman with a cyan visor in a dark blue suit with glowing circuit lines and a cyan circuit emblem, violet cape",
    17: "an old white-bearded man in a blue suit with a glowing rainbow spiral emblem and a magenta cape",
    18: "a brown-haired young man in a cyan suit with three lime green speed lines on his chest and a red cape",
    19: "a dark-skinned woman with a silver afro in a magenta and gold suit and a red cape",
    20: "a black-haired man in orange armor with a glowing white circle in his chest and an orange cape",
    21: "a brown-haired woman in a deep blue suit with a rainbow spiral emblem and a cape fading from blue to green and red",
    22: "a hooded masked man in a violet and cyan suit with a round emblem and a violet cape",
    23: "a woman with a golden horned helmet in a green and blue suit with a green triangle emblem and an orange-red cape",
    24: "a man with spiky blue hair in a black and electric blue suit and a blue cape",
    25: "a red-haired woman in a crimson suit with gold bracers, gold boots and a magenta cape",
    26: "a black-haired man in a blue suit with a gold spiral amulet and a violet cape",
    27: "a blonde woman in a pink and cyan suit with a gold heart emblem and a rainbow cape",
    28: "a hero in shiny blue armor and helmet with a glowing white circle in his chest and a violet cape",
    29: "a green-bearded man in a teal suit with a glowing green leaf emblem, gold boots and a red cape",
    30: "a brown-haired woman in a violet suit with a small orange emblem and an orange-gold cape",
}
CONSIGNE = ("The superhero, {t}, flies to the right. While flying the superhero does one full spin around the axis of the body, "
            "a pirouette: turns to face the viewer, keeps rotating, and comes back to exactly the same flying pose as at the start. "
            "The cape twirls around during the spin. The costume and chest emblem stay exactly the same at every moment. "
            "The camera tracks the hero so the hero stays in the center of the frame. "
            "The background stays a perfectly flat uniform neutral grey color during the whole video.")

env = dict(os.environ, VOL_BOUCLE="1")
for n in [int(a) for a in sys.argv[1:]] or list(CHOIX):
    depart = os.path.join(ICI, "depart", f"h{n:02d}_t{CHOIX[n]}.png")
    nom = f"vol_h{n:02d}_g1"
    r = subprocess.run([PY, "-I", os.path.join(ICI, "video.py"), depart, f"super_heros/{nom}", str(1000 * n + 1),
                        CONSIGNE.format(t=TENUE[n])], env=env, capture_output=True, text=True)
    if r.returncode:
        print("ECHEC", nom, r.stderr[-400:], flush=True); continue
    d = subprocess.run([PY, "-I", os.path.join(ICI, "detourer_ia.py"), os.path.join(ICI, "images", nom),
                        os.path.join(ICI, "rgba", nom)], capture_output=True, text=True)
    print(nom, (d.stdout.strip().splitlines() or [d.stderr[-300:]])[-1], flush=True)
