# Modifie une image par Qwen-Image-Edit-2509 via le serveur ComfyUI LOCAL
# (C:\IAs\ComfyUI, http://127.0.0.1:8188) — sert a TOURNER la tete d'une
# mascotte : « turn its head to the left »... Tout est Apache-2.0 : le modele,
# le LoRA Lightning (4 pas) et, en option, le LoRA « Multiple-angles ».
#
#   python tools/comfy_qwen_edit.py <entree.png> <sortie.png> <graine> "<consigne>" [--angles]
#
# Rien ne sort de la machine : le serveur ecoute sur 127.0.0.1 seulement.
import json
import os
import sys
import time
import urllib.parse
import urllib.request
import uuid

SERVEUR = "http://127.0.0.1:8188"
MODELE = "qwen_image_edit_2509_fp8_e4m3fn.safetensors"
ENCODEUR = "qwen_2.5_vl_7b_fp8_scaled.safetensors"
VAE = "qwen_image_vae.safetensors"
LORA_RAPIDE = "Qwen-Image-Edit-2509-Lightning-4steps-V1.0-fp32.safetensors"
LORA_ANGLES = "Qwen-Edit-2509-Multiple-angles.safetensors"
ATTENTE_MAX_S = 600


def envoyer_image(chemin: str) -> str:
    """Depose l'image dans le dossier `input` du serveur ; rend son nom."""
    limite = uuid.uuid4().hex
    nom = f"nd_{uuid.uuid4().hex[:8]}_{os.path.basename(chemin)}"
    with open(chemin, "rb") as f:
        donnees = f.read()
    corps = (f"--{limite}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"{nom}\"\r\n"
             f"Content-Type: image/png\r\n\r\n").encode() + donnees + f"\r\n--{limite}--\r\n".encode()
    req = urllib.request.Request(f"{SERVEUR}/upload/image", corps,
                                 {"Content-Type": f"multipart/form-data; boundary={limite}"})
    return json.load(urllib.request.urlopen(req, timeout=60))["name"]


def graphe(image: str, graine: int, consigne: str, angles: bool) -> dict:
    g = {
        "1": {"class_type": "UNETLoader", "inputs": {"unet_name": MODELE, "weight_dtype": "default"}},
        "2": {"class_type": "LoraLoaderModelOnly", "inputs": {"model": ["1", 0], "lora_name": LORA_RAPIDE, "strength_model": 1.0}},
        "3": {"class_type": "CLIPLoader", "inputs": {"clip_name": ENCODEUR, "type": "qwen_image"}},
        "4": {"class_type": "VAELoader", "inputs": {"vae_name": VAE}},
        "5": {"class_type": "LoadImage", "inputs": {"image": image}},
        "6": {"class_type": "ImageScaleToTotalPixels", "inputs": {"image": ["5", 0], "upscale_method": "lanczos", "megapixels": 1.0, "resolution_steps": 1}},
        "7": {"class_type": "TextEncodeQwenImageEditPlus", "inputs": {"clip": ["3", 0], "prompt": consigne, "vae": ["4", 0], "image1": ["6", 0]}},
        "8": {"class_type": "TextEncodeQwenImageEditPlus", "inputs": {"clip": ["3", 0], "prompt": "", "vae": ["4", 0], "image1": ["6", 0]}},
        "9": {"class_type": "VAEEncode", "inputs": {"pixels": ["6", 0], "vae": ["4", 0]}},
        "10": {"class_type": "ModelSamplingAuraFlow", "inputs": {"model": ["2", 0], "shift": 3.0}},
        "11": {"class_type": "KSampler", "inputs": {
            "model": ["10", 0], "positive": ["7", 0], "negative": ["8", 0], "latent_image": ["9", 0],
            "seed": graine, "steps": 4, "cfg": 1.0, "sampler_name": "euler", "scheduler": "simple", "denoise": 1.0}},
        "12": {"class_type": "VAEDecode", "inputs": {"samples": ["11", 0], "vae": ["4", 0]}},
        "13": {"class_type": "SaveImage", "inputs": {"images": ["12", 0], "filename_prefix": "neuradesk_edit"}},
    }
    if angles:
        g["14"] = {"class_type": "LoraLoaderModelOnly", "inputs": {"model": ["2", 0], "lora_name": LORA_ANGLES, "strength_model": 1.0}}
        g["10"]["inputs"]["model"] = ["14", 0]
    return g


def modifier(entree: str, sortie: str, graine: int, consigne: str, angles: bool = False) -> None:
    corps = json.dumps({"prompt": graphe(envoyer_image(entree), graine, consigne, angles)}).encode()
    req = urllib.request.Request(f"{SERVEUR}/prompt", corps, {"Content-Type": "application/json"})
    try:
        ident = json.load(urllib.request.urlopen(req, timeout=60))["prompt_id"]
    except urllib.error.HTTPError as e:
        raise RuntimeError(e.read().decode()) from e
    fin = time.time() + ATTENTE_MAX_S
    while time.time() < fin:
        hist = json.load(urllib.request.urlopen(f"{SERVEUR}/history/{ident}", timeout=30))
        if ident in hist:
            etat = hist[ident].get("status", {})
            if etat.get("status_str") == "error":
                raise RuntimeError(f"ComfyUI a echoue : {etat}")
            image = hist[ident]["outputs"]["13"]["images"][0]
            q = urllib.parse.urlencode({k: image[k] for k in ("filename", "subfolder", "type")})
            with urllib.request.urlopen(f"{SERVEUR}/view?{q}", timeout=60) as r, open(sortie, "wb") as f:
                f.write(r.read())
            return
        time.sleep(0.5)
    raise TimeoutError(f"pas d'image apres {ATTENTE_MAX_S} s")


if __name__ == "__main__":
    t0 = time.time()
    modifier(sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4], "--angles" in sys.argv[5:])
    print(f"{sys.argv[2]} en {time.time() - t0:.1f} s")
