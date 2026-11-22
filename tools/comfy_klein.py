# FLUX.2 [klein] 4B (Black Forest Labs, Apache-2.0) sur le serveur ComfyUI
# LOCAL : une retouche d'image entiere guidee par une consigne
# (ReferenceLatent, 4 pas, ~6 s par image sur la carte locale).
#
#   editer(entree, sortie, consigne, graine)
import json
import time
import urllib.error
import urllib.parse
import urllib.request

from comfy_qwen_edit import SERVEUR, envoyer_image

ETAPES = 4


def graphe(image, consigne, graine):
    return {
        "1": {"class_type": "UNETLoader", "inputs": {"unet_name": "flux-2-klein-4b.safetensors", "weight_dtype": "default"}},
        "2": {"class_type": "CLIPLoader", "inputs": {"clip_name": "qwen_3_4b.safetensors", "type": "flux2"}},
        "3": {"class_type": "VAELoader", "inputs": {"vae_name": "flux2-vae.safetensors"}},
        "4": {"class_type": "LoadImage", "inputs": {"image": image}},
        "5": {"class_type": "ImageScaleToTotalPixels", "inputs": {"image": ["4", 0], "upscale_method": "lanczos", "megapixels": 1.0, "resolution_steps": 1}},
        "6": {"class_type": "VAEEncode", "inputs": {"pixels": ["5", 0], "vae": ["3", 0]}},
        "7": {"class_type": "CLIPTextEncode", "inputs": {"clip": ["2", 0], "text": consigne}},
        "8": {"class_type": "ReferenceLatent", "inputs": {"conditioning": ["7", 0], "latent": ["6", 0]}},
        "9": {"class_type": "EmptyFlux2LatentImage", "inputs": {"width": 1024, "height": 1024, "batch_size": 1}},
        "10": {"class_type": "Flux2Scheduler", "inputs": {"steps": ETAPES, "width": 1024, "height": 1024}},
        "11": {"class_type": "KSamplerSelect", "inputs": {"sampler_name": "euler"}},
        "12": {"class_type": "RandomNoise", "inputs": {"noise_seed": graine}},
        "13": {"class_type": "BasicGuider", "inputs": {"model": ["1", 0], "conditioning": ["8", 0]}},
        "14": {"class_type": "SamplerCustomAdvanced", "inputs": {"noise": ["12", 0], "guider": ["13", 0], "sampler": ["11", 0],
                                                                "sigmas": ["10", 0], "latent_image": ["9", 0]}},
        "15": {"class_type": "VAEDecode", "inputs": {"samples": ["14", 0], "vae": ["3", 0]}},
        "16": {"class_type": "SaveImage", "inputs": {"images": ["15", 0], "filename_prefix": "neuradesk_klein"}},
    }


def editer(entree, sortie, consigne, graine=1):
    corps = json.dumps({"prompt": graphe(envoyer_image(entree), consigne, graine)}).encode()
    req = urllib.request.Request(f"{SERVEUR}/prompt", corps, {"Content-Type": "application/json"})
    try:
        ident = json.load(urllib.request.urlopen(req, timeout=60))["prompt_id"]
    except urllib.error.HTTPError as e:
        raise RuntimeError(e.read().decode())
    while True:
        h = json.load(urllib.request.urlopen(f"{SERVEUR}/history/{ident}", timeout=30))
        if ident in h:
            break
        time.sleep(0.5)
    if h[ident].get("status", {}).get("status_str") == "error":
        raise RuntimeError(json.dumps(h[ident]["status"])[:2000])
    im = h[ident]["outputs"]["16"]["images"][0]
    q = urllib.parse.urlencode({k: im[k] for k in ("filename", "subfolder", "type")})
    with open(sortie, "wb") as f:
        f.write(urllib.request.urlopen(f"{SERVEUR}/view?{q}", timeout=60).read())
