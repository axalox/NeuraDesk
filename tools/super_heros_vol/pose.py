# Une pose du heros sur fond gris uni (Klein edit depuis son portrait).
#   python pose.py <n> <pose: A|W> <tirages> [largeur hauteur]
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
from comfy import envoyer_image, lancer, attendre, telecharger
import depart_gris as dg

ICI = os.path.dirname(os.path.abspath(__file__))
SRC = r"C:\IAs\ComfyUI\ComfyUI\output\super_heros"
COMMUN = ("The exact same superhero character, same face, same hair, same costume colors, same chest emblem, same cape. "
          "The whole character and cape fit inside the frame with a wide empty margin. "
          "Background is a perfectly flat uniform neutral medium grey color (#808080), no scenery, no sky, no ground, no shadow, "
          "no light trails, no sparkles, no energy effects. Same comic book illustration style, bold ink lines. ")
POSES = {
    "A": "The character is flying straight toward the viewer and slightly downward, seen from the front, both arms stretched forward toward the viewer, body slightly leaning forward, cape flowing up behind, the character occupies about half of the frame height. ",
    "W": "The character is walking to the right, seen exactly from the side in profile, mid-stride, one leg forward and one leg back, arms swinging naturally, standing upright, cape hanging behind, feet on an invisible ground line, the character occupies about 70 percent of the frame height. ",
}


def graphe(image, graine, consigne, l, h):
    g = dg.graphe(image, graine)
    g["7"]["inputs"]["text"] = consigne
    g["9"]["inputs"].update(width=l, height=h)
    g["10"]["inputs"].update(width=l, height=h)
    g["16"]["inputs"]["filename_prefix"] = "super_heros/pose"
    return g


if __name__ == "__main__":
    n, pose, tirages = int(sys.argv[1]), sys.argv[2], int(sys.argv[3])
    l, h = (int(sys.argv[4]), int(sys.argv[5])) if len(sys.argv) > 5 else (640, 640)
    img = envoyer_image(os.path.join(SRC, f"heros_{n:02d}_00001_.png"))
    os.makedirs(os.path.join(ICI, "poses"), exist_ok=True)
    consigne = COMMUN + POSES[pose] + os.environ.get("POSE_EXTRA", "")
    ids = [(t, lancer(graphe(img, 300 * n + 10 * ord(pose) + t, consigne, l, h))) for t in range(1, tirages + 1)]
    for t, ident in ids:
        telecharger(attendre(ident, "16")["images"][0], os.path.join(ICI, "poses", f"h{n:02d}_{pose}{t}.png"))
    print("ok", n, pose)
