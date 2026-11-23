# Image de depart : le heros en vol horizontal, de profil, sur fond vert uni (Klein edit).
#   python depart.py <numero> <nb_tirages>
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
from comfy import envoyer_image, lancer, attendre, telecharger

ICI = os.path.dirname(__file__)
SRC = r"C:\IAs\ComfyUI\ComfyUI\output\super_heros"
L, H = 832, 480

CONSIGNE = ("The exact same superhero character, same face, same hair, same costume colors and emblem. "
            "The character is flying horizontally toward the right, body straight, both arms stretched forward, hands empty, "
            "body turned three-quarters toward the viewer so that the chest emblem is clearly visible, cape flowing behind. "
            "Remove any object the character was holding or lifting. "
            "The whole character and his cape fit inside the frame with a wide empty margin, the character occupies about half of the width. "
            "Background is a perfectly flat uniform pure green chroma key color (#00FF00), no scenery, no sky, no city, no clouds, no shadow, no ground. "
            "Same comic book illustration style, bold ink lines.")


def graphe(image, graine):
    return {
        "1": {"class_type": "UNETLoader", "inputs": {"unet_name": "flux-2-klein-4b.safetensors", "weight_dtype": "default"}},
        "2": {"class_type": "CLIPLoader", "inputs": {"clip_name": "qwen_3_4b.safetensors", "type": "flux2"}},
        "3": {"class_type": "VAELoader", "inputs": {"vae_name": "flux2-vae.safetensors"}},
        "4": {"class_type": "LoadImage", "inputs": {"image": image}},
        "5": {"class_type": "ImageScaleToTotalPixels", "inputs": {"image": ["4", 0], "upscale_method": "lanczos", "megapixels": 1.0, "resolution_steps": 1}},
        "6": {"class_type": "VAEEncode", "inputs": {"pixels": ["5", 0], "vae": ["3", 0]}},
        "7": {"class_type": "CLIPTextEncode", "inputs": {"clip": ["2", 0], "text": CONSIGNE}},
        "8": {"class_type": "ReferenceLatent", "inputs": {"conditioning": ["7", 0], "latent": ["6", 0]}},
        "9": {"class_type": "EmptyFlux2LatentImage", "inputs": {"width": L, "height": H, "batch_size": 1}},
        "10": {"class_type": "Flux2Scheduler", "inputs": {"steps": 4, "width": L, "height": H}},
        "11": {"class_type": "KSamplerSelect", "inputs": {"sampler_name": "euler"}},
        "12": {"class_type": "RandomNoise", "inputs": {"noise_seed": graine}},
        "13": {"class_type": "BasicGuider", "inputs": {"model": ["1", 0], "conditioning": ["8", 0]}},
        "14": {"class_type": "SamplerCustomAdvanced", "inputs": {"noise": ["12", 0], "guider": ["13", 0], "sampler": ["11", 0], "sigmas": ["10", 0], "latent_image": ["9", 0]}},
        "15": {"class_type": "VAEDecode", "inputs": {"samples": ["14", 0], "vae": ["3", 0]}},
        "16": {"class_type": "SaveImage", "inputs": {"images": ["15", 0], "filename_prefix": "super_heros/depart"}},
    }


if __name__ == "__main__":
    n, tirages = int(sys.argv[1]), int(sys.argv[2])
    img = envoyer_image(os.path.join(SRC, f"heros_{n:02d}_00001_.png"))
    os.makedirs(os.path.join(ICI, "depart"), exist_ok=True)
    ids = [(t, lancer(graphe(img, 100 * n + t))) for t in range(1, tirages + 1)]
    for t, ident in ids:
        sortie = os.path.join(ICI, "depart", f"h{n:02d}_t{t}.png")
        telecharger(attendre(ident, "16")["images"][0], sortie)
        print(sortie)
