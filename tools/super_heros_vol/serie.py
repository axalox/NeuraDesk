# Serie : 10 heros x 2 graines, clip en boucle (meme image debut/fin), puis detourage.
import os, subprocess, sys

ICI = os.path.dirname(os.path.abspath(__file__))
PY = sys.executable
CHOIX = {1: 1, 2: 2, 3: 3, 4: 3, 5: 3, 6: 2, 7: 2, 8: 2, 9: 1, 10: 2}
TENUE = {
    1: "a dark-haired man in a dark red suit with a round gold sun emblem on his chest",
    2: "a silver-haired woman in a red bodysuit with gold armored gauntlets and a gold phoenix emblem on her chest",
    3: "an old white-bearded man in red and gold knight armor with a gold lion emblem on his chest",
    4: "a curly-haired young man in a red suit with gold lightning stripes on his sides",
    5: "a dark-skinned woman with braided hair in a red costume with a small gold star on her chest and a gold belt",
    6: "a man wearing a golden domed helmet in a red bodysuit with thin gold circuit lines",
    7: "a red-haired woman with a gold tiara in a red costume with a gold eagle emblem on her chest",
    8: "a bald man with a gold eye mask in a red suit with gold shoulder plates",
    9: "a young woman with a black bob haircut and a small gold crown, in a red costume with a gold crown emblem on her chest",
    10: "a grey-haired man in a red suit with a gold atom emblem on his chest, gold boots and gold bracers",
}
CONSIGNE = ("The superhero, {t}, flies to the right. While flying he does one full spin around himself, "
            "a pirouette around the axis of his body: he turns to face the viewer, keeps rotating, and comes back "
            "to exactly the same flying pose as at the start. His cape twirls around him during the spin. "
            "His costume and chest emblem stay exactly the same at every moment. "
            "The camera tracks him so he stays in the center of the frame. "
            "The background stays a perfectly flat uniform pure green color during the whole video.")

env = dict(os.environ, VOL_BOUCLE="1")
seuls = [int(a) for a in sys.argv[1:]] or list(CHOIX)
for n in seuls:
    depart = os.path.join(ICI, "depart", f"h{n:02d}_t{CHOIX[n]}.png")
    for g in [int(x) for x in os.environ.get("VOL_GRAINES", "1,2").split(",")]:
        nom = f"vol_h{n:02d}_g{g}"
        r = subprocess.run([PY, "-I", os.path.join(ICI, "video.py"), depart, f"super_heros/{nom}", str(1000 * n + g),
                            CONSIGNE.format(t=TENUE[n])], env=env, capture_output=True, text=True)
        if r.returncode:
            print("ECHEC", nom, r.stderr[-400:], flush=True); continue
        d = subprocess.run([PY, "-I", os.path.join(ICI, "detourer.py"), os.path.join(ICI, "images", nom),
                            os.path.join(ICI, "rgba", nom)], capture_output=True, text=True)
        print(nom, d.stdout.strip() or d.stderr[-300:], flush=True)
