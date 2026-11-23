# Planche des poses A et W (3 tirages) : python revue_poses.py <sortie.png> n...
import sys, os
from PIL import Image, ImageDraw
ICI = os.path.dirname(os.path.abspath(__file__))
sortie, ns = sys.argv[1], [int(a) for a in sys.argv[2:]]
w = 200
s = Image.new("RGB", (w * 6 + 50, w * len(ns)), (40, 40, 40)); d = ImageDraw.Draw(s)
for r, n in enumerate(ns):
    d.text((5, r * w + 90), str(n), fill="white", font_size=22)
    for i, (p, t) in enumerate([(p, t) for p in "AW" for t in (1, 2, 3)]):
        s.paste(Image.open(f"{ICI}/poses/h{n:02d}_{p}{t}.png").convert("RGB").resize((w, w)), (50 + i * w, r * w))
        d.text((50 + i * w + 4, r * w + 4), f"{p}{t}", fill="black", font_size=16)
s.save(sortie)
