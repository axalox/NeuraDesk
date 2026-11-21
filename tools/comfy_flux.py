# Genere une image par FLUX.1-schnell via un serveur ComfyUI LOCAL
# (C:\IAs\ComfyUI, `run_nvidia_gpu.bat`, http://127.0.0.1:8188).
#
#   python tools/comfy_flux.py <sortie.png> <graine> "<invite>"
#
# Rien ne sort de la machine : le serveur ecoute sur 127.0.0.1 seulement.
import json
import sys
import time
import urllib.parse
import urllib.request

SERVEUR = "http://127.0.0.1:8188"
MODELE = "flux1-schnell-fp8.safetensors"
ATTENTE_MAX_S = 300


def graphe(graine: int, invite: str, cote: int = 1024) -> dict:
    """Le graphe minimal de ComfyUI pour FLUX schnell (4 pas, cfg 1)."""
    return {
        "1": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": MODELE}},
        "2": {"class_type": "CLIPTextEncode", "inputs": {"text": invite, "clip": ["1", 1]}},
        "3": {"class_type": "CLIPTextEncode", "inputs": {"text": "", "clip": ["1", 1]}},
        "4": {"class_type": "EmptySD3LatentImage", "inputs": {"width": cote, "height": cote, "batch_size": 1}},
        "5": {"class_type": "KSampler", "inputs": {
            "model": ["1", 0], "positive": ["2", 0], "negative": ["3", 0], "latent_image": ["4", 0],
            "seed": graine, "steps": 4, "cfg": 1.0, "sampler_name": "euler", "scheduler": "simple", "denoise": 1.0}},
        "6": {"class_type": "VAEDecode", "inputs": {"samples": ["5", 0], "vae": ["1", 2]}},
        "7": {"class_type": "SaveImage", "inputs": {"images": ["6", 0], "filename_prefix": "neuradesk"}},
    }


def generer(sortie: str, graine: int, invite: str) -> None:
    corps = json.dumps({"prompt": graphe(graine, invite)}).encode()
    req = urllib.request.Request(f"{SERVEUR}/prompt", corps, {"Content-Type": "application/json"})
    ident = json.load(urllib.request.urlopen(req, timeout=30))["prompt_id"]
    fin = time.time() + ATTENTE_MAX_S
    while time.time() < fin:
        hist = json.load(urllib.request.urlopen(f"{SERVEUR}/history/{ident}", timeout=30))
        if ident in hist:
            etat = hist[ident].get("status", {})
            if etat.get("status_str") == "error":
                raise RuntimeError(f"ComfyUI a echoue : {etat}")
            image = hist[ident]["outputs"]["7"]["images"][0]
            q = urllib.parse.urlencode({k: image[k] for k in ("filename", "subfolder", "type")})
            with urllib.request.urlopen(f"{SERVEUR}/view?{q}", timeout=60) as r, open(sortie, "wb") as f:
                f.write(r.read())
            return
        time.sleep(0.5)
    raise TimeoutError(f"pas d'image apres {ATTENTE_MAX_S} s")


if __name__ == "__main__":
    t0 = time.time()
    generer(sys.argv[1], int(sys.argv[2]), sys.argv[3])
    print(f"{sys.argv[1]} en {time.time() - t0:.1f} s")
