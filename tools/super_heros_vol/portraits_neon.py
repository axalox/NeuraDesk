# 20 super-heros originaux aux couleurs de l'ecran d'accueil NeuraDesk (neons arc-en-ciel sur noir).
# FLUX.2 Klein 4B, texte -> image. Sortie : output/super_heros/heros_11.. heros_30
import json, time, urllib.request

SERVEUR = "http://127.0.0.1:8188"
STYLE = ("classic golden-age American superhero comic book illustration, heroic full body pose, bold clean ink lines, "
         "costume glowing with vivid neon colors like electric blue, cyan, violet, magenta, red, orange, gold and bright green, "
         "luminous rainbow light trails and sparkles, dark night background with colorful glow, original character, no text, no logo letters")

HEROS = [
    "a muscular man in an electric blue and cyan suit, a glowing rainbow swirl vortex emblem on his chest, magenta cape, flying over a neon city",
    "a woman with long violet hair in a magenta and purple suit with glowing cyan lines, rainbow light ribbons swirling around her arms",
    "a tall man in a deep blue suit with a red-orange-gold gradient cape, a glowing white star at the center of a spiral emblem, floating among stars",
    "a young woman with short green hair in a bright green and gold suit, electric blue boots and gloves, crackling with neon energy",
    "a broad man with a red mohawk in a red and orange suit with yellow flame patterns, cyan glowing eyes, fists blazing with fire",
    "a slender woman in a black suit covered with glowing rainbow circuit lines, a cyan visor, hair streaming with light",
    "an older man with a white beard in a royal blue and violet armor with glowing gold trim, a radiant swirl shield",
    "a teenage boy in a cyan and lime green suit, rainbow speed trails behind him, running on a beam of light",
    "a woman with dark skin and a silver afro in a magenta and gold suit, glowing violet energy wings",
    "a giant man in a heavy orange and red armor with glowing blue cracks of energy, a glowing core in his chest",
    "a woman in a flowing cape that shifts from blue to red to green, a deep blue bodysuit, a glowing four-color swirl emblem",
    "a masked man in a violet and cyan suit with a hood, holding glowing rainbow energy discs",
    "a woman with a golden helmet with wings, an emerald green and electric blue suit, holding a glowing spear of light",
    "a lean man with spiky blue hair in a black and electric blue suit, lightning made of rainbow colors around him",
    "a strong woman with red braids in a crimson and orange suit with gold bracers, a glowing sun on her belt",
    "a mysterious man in a long dark cloak lined with glowing galaxy colors, glowing cyan eyes, a swirl amulet",
    "a cheerful young woman in a pink and cyan suit with glowing heart-shaped light bursts, rainbow cape",
    "an armored hero in sleek chrome armor reflecting neon blue, magenta and green lights, a glowing white core",
    "a man with a green beard in a forest green and gold suit, glowing green energy vines spiraling around him",
    "a woman in a violet suit with a glowing rainbow halo behind her head, arms raised, swirling light vortex around her",
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


ids = [(n, lancer(graphe(f"{d}, {STYLE}", 2000 + n, f"super_heros/heros_{n:02d}"))) for n, d in enumerate(HEROS, 11)]
for n, ident in ids:
    while True:
        h = json.load(urllib.request.urlopen(f"{SERVEUR}/history/{ident}", timeout=30))
        if ident in h:
            st = h[ident].get("status", {})
            if st.get("status_str") == "error":
                print(n, "ERREUR", json.dumps(st)[:500]); break
            if st.get("completed"):
                print(n, h[ident]["outputs"]["16"]["images"][0]["filename"]); break
        time.sleep(1)
