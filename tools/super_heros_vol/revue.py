# Planche de revue : 1 image sur 8 de chaque clip, sur fond sombre.  python revue.py <sortie.png> n...
import sys, os
from PIL import Image, ImageDraw
ICI = os.path.dirname(os.path.abspath(__file__))
sortie, ns = sys.argv[1], [int(a) for a in sys.argv[2:]]
w, h = 200, 115; ks = list(range(0, 80, 8))
s = Image.new("RGB", (w * len(ks) + 60, h * len(ns)), (25, 25, 35)); d = ImageDraw.Draw(s)
for r, n in enumerate(ns):
    d.text((5, r * h + 40), str(n), fill="white", font_size=22)
    for i, k in enumerate(ks):
        im = Image.open(f"{ICI}/rgba/vol_h{n:02d}_g1/{k:03d}.png"); im.thumbnail((w, h)); s.paste(im, (60 + i * w, r * h), im)
s.save(sortie)
