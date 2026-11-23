# 10 super-heros originaux, style comics classique, rouge et or. FLUX.2 Klein 4B, texte -> image.
import json, time, urllib.request

SERVEUR = "http://127.0.0.1:8188"
STYLE = ("classic golden-age American superhero comic book illustration, heroic full body pose, "
         "bold clean ink lines, vibrant colors, dramatic sky background, "
         "costume in deep crimson red and shining gold, flowing cape, original character, no text")

HEROS = [
    "a broad-shouldered man with a square jaw and black wavy hair, gold sun emblem on his chest, red cape, hovering above a city skyline at sunrise",
    "a tall woman with short silver hair, red bodysuit with gold armored gauntlets and a gold phoenix emblem, wings of flame behind her, flying through clouds",
    "a powerful older man with a white beard, red and gold armor like a knight, golden lion emblem, standing on a mountain peak",
    "a young athletic man with curly brown hair, red suit with gold lightning stripes on the sides, short red cape, landing on a rooftop in a heroic crouch",
    "a muscular woman with dark skin and braided hair, red costume with a gold star crest and gold belt, long red cape, lifting a train car above her head",
    "a lean man with a golden domed helmet and visor, red bodysuit with gold circuitry lines, glowing gold eyes, flying over the ocean",
    "a woman with long red hair and a gold tiara, red costume with gold eagle emblem spreading across the chest, cape billowing, floating in front of the moon",
    "a giant of a man with a shaved head and a gold mask over his eyes, red suit with gold shoulder plates and a gold comet emblem, punching through a stone wall",
    "a young woman with a black bob haircut, red and gold costume with a gold crown emblem, holding a glowing golden shield, standing on top of a skyscraper at night",
    "a calm man with grey temples and a strong chin, red suit with a gold atom emblem, gold boots and gold bracers, arms crossed, floating above the Earth in space",
]

def graphe(consigne, graine, prefixe):
    return {
        "1": {"class_type": "UNETLoader", "inputs": {"unet_name": "flux-2-klein-4b.safetensors", "weight_dtype": "default"}},
        "2": {"class_type": "CLIPLoader", "inputs": {"clip_name": "qwen_3_4b.safetensors", "type": "flux2"}},
        "3": {"class_type": "VAELoader", "inputs": {"vae_name": "flux2-vae.safetensors"}},
        "7": {"class_type": "CLIPTextEncode", "inputs": {"clip": ["2", 0], "text": consigne}},
        "9": {"class_type": "EmptyFlux2LatentImage", "inputs": {"width": 832, "height": 1216, "batch_size": 1}},
        "10": {"class_type": "Flux2Scheduler", "inputs": {"steps": 4, "width": 832, "height": 1216}},
        "11": {"class_type": "KSamplerSelect", "inputs": {"sampler_name": "euler"}},
        "12": {"class_type": "RandomNoise", "inputs": {"noise_seed": graine}},
        "13": {"class_type": "BasicGuider", "inputs": {"model": ["1", 0], "conditioning": ["7", 0]}},
        "14": {"class_type": "SamplerCustomAdvanced", "inputs": {"noise": ["12", 0], "guider": ["13", 0], "sampler": ["11", 0], "sigmas": ["10", 0], "latent_image": ["9", 0]}},
        "15": {"class_type": "VAEDecode", "inputs": {"samples": ["14", 0], "vae": ["3", 0]}},
        "16": {"class_type": "SaveImage", "inputs": {"images": ["15", 0], "filename_prefix": prefixe}},
    }

def lancer(g):
    req = urllib.request.Request(f"{SERVEUR}/prompt", json.dumps({"prompt": g}).encode(), {"Content-Type": "application/json"})
    try:
        return json.load(urllib.request.urlopen(req, timeout=60))["prompt_id"]
    except urllib.error.HTTPError as e:
        raise RuntimeError(e.read().decode())

ids = [(i, lancer(graphe(f"{d}, {STYLE}", 1000 + i, f"super_heros/heros_{i:02d}"))) for i, d in enumerate(HEROS, 1)]
for i, ident in ids:
    while True:
        h = json.load(urllib.request.urlopen(f"{SERVEUR}/history/{ident}", timeout=30))
        if ident in h:
            st = h[ident].get("status", {})
            if st.get("status_str") == "error":
                print(i, "ERREUR", json.dumps(st)[:500]); break
            if st.get("completed"):
                print(i, h[ident]["outputs"]["16"]["images"][0]["filename"]); break
        time.sleep(1)
