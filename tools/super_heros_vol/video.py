# Wan 2.2 I2V 14B FP8 + lightx2v (4 pas, 2 haut bruit + 2 bas bruit), d'apres le modele officiel.
#   python video.py <image_depart.png> <prefixe_sortie> <graine> [consigne]
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
from comfy import envoyer_image, lancer, attendre, telecharger

GABARIT = r"C:\IAs\ComfyUI\python_embeded\Lib\site-packages\comfyui_workflow_templates_json\templates\video_wan2_2_14B_i2v.json"
L, H, IMAGES = int(os.environ.get("VOL_L", 832)), int(os.environ.get("VOL_H", 480)), int(os.environ.get("VOL_IMAGES", 81))
FIN = os.environ.get("VOL_FIN")   # image de fin differente de celle du debut (enchainement de deux poses)

CONSIGNE = ("The superhero flies forward and spins around the axis of his body in fast continuous barrel rolls, "
            "rotating 360 degrees again and again while flying, cape whipping and twirling around him. "
            "The camera tracks him so he stays in the center of the frame. "
            "The background stays a perfectly flat uniform pure green color during the whole video, nothing else appears.")
NEGATIF_EN = ("green background, scenery, sky, clouds, city, landscape, ground, shadow, background change, gradient background, text, watermark, extra people, "
              "letter S, S shield logo, superman logo, diamond shaped chest logo, blue costume, blue suit")
BOUCLE = os.environ.get("VOL_BOUCLE") == "1" or bool(FIN)   # meme image au debut et a la fin -> clip qui boucle


def negatif_du_gabarit():
    j = json.load(open(GABARIT, encoding="utf8"))
    for sg in [j] + j.get("definitions", {}).get("subgraphs", []):
        for n in sg.get("nodes", []):
            if n.get("id") == 89:
                return n["widgets_values"][0]
    raise RuntimeError("negatif introuvable dans le gabarit")


def graphe(image, graine, consigne, prefixe, fin=None):
    neg = negatif_du_gabarit() + ", " + NEGATIF_EN
    return {
        "1": {"class_type": "UNETLoader", "inputs": {"unet_name": "wan2.2_i2v_high_noise_14B_fp8_scaled.safetensors", "weight_dtype": "default"}},
        "2": {"class_type": "UNETLoader", "inputs": {"unet_name": "wan2.2_i2v_low_noise_14B_fp8_scaled.safetensors", "weight_dtype": "default"}},
        "3": {"class_type": "LoraLoaderModelOnly", "inputs": {"model": ["1", 0], "lora_name": "wan2.2_i2v_lightx2v_4steps_lora_v1_high_noise.safetensors", "strength_model": 1.0}},
        "4": {"class_type": "LoraLoaderModelOnly", "inputs": {"model": ["2", 0], "lora_name": "wan2.2_i2v_lightx2v_4steps_lora_v1_low_noise.safetensors", "strength_model": 1.0}},
        "5": {"class_type": "ModelSamplingSD3", "inputs": {"model": ["3", 0], "shift": 5.0}},
        "6": {"class_type": "ModelSamplingSD3", "inputs": {"model": ["4", 0], "shift": 5.0}},
        "7": {"class_type": "CLIPLoader", "inputs": {"clip_name": "umt5_xxl_fp8_e4m3fn_scaled.safetensors", "type": "wan"}},
        "8": {"class_type": "CLIPTextEncode", "inputs": {"clip": ["7", 0], "text": consigne}},
        "9": {"class_type": "CLIPTextEncode", "inputs": {"clip": ["7", 0], "text": neg}},
        "10": {"class_type": "VAELoader", "inputs": {"vae_name": "wan_2.1_vae.safetensors"}},
        "11": {"class_type": "LoadImage", "inputs": {"image": image}},
        "12": ({"class_type": "WanFirstLastFrameToVideo", "inputs": {"positive": ["8", 0], "negative": ["9", 0], "vae": ["10", 0],
                "width": L, "height": H, "length": IMAGES, "batch_size": 1, "start_image": ["11", 0], "end_image": ["17" if fin else "11", 0]}}
               if BOUCLE else
               {"class_type": "WanImageToVideo", "inputs": {"positive": ["8", 0], "negative": ["9", 0], "vae": ["10", 0],
                "width": L, "height": H, "length": IMAGES, "batch_size": 1, "start_image": ["11", 0]}}),
        "13": {"class_type": "KSamplerAdvanced", "inputs": {"model": ["5", 0], "add_noise": "enable", "noise_seed": graine, "steps": 4, "cfg": 1.0,
                                                          "sampler_name": "euler", "scheduler": "simple", "positive": ["12", 0], "negative": ["12", 1],
                                                          "latent_image": ["12", 2], "start_at_step": 0, "end_at_step": 2, "return_with_leftover_noise": "enable"}},
        "14": {"class_type": "KSamplerAdvanced", "inputs": {"model": ["6", 0], "add_noise": "disable", "noise_seed": 0, "steps": 4, "cfg": 1.0,
                                                          "sampler_name": "euler", "scheduler": "simple", "positive": ["12", 0], "negative": ["12", 1],
                                                          "latent_image": ["13", 0], "start_at_step": 2, "end_at_step": 10000, "return_with_leftover_noise": "disable"}},
        "15": {"class_type": "VAEDecode", "inputs": {"samples": ["14", 0], "vae": ["10", 0]}},
        **({"17": {"class_type": "LoadImage", "inputs": {"image": fin}}} if fin else {}),
        "16": {"class_type": "SaveImage", "inputs": {"images": ["15", 0], "filename_prefix": prefixe}},
    }


if __name__ == "__main__":
    depart, prefixe, graine = sys.argv[1], sys.argv[2], int(sys.argv[3])
    consigne = sys.argv[4] if len(sys.argv) > 4 else CONSIGNE
    sortie = os.path.join(os.path.dirname(__file__), "images", os.path.basename(prefixe))
    os.makedirs(sortie, exist_ok=True)
    ims = attendre(lancer(graphe(envoyer_image(depart), graine, consigne, prefixe, envoyer_image(FIN) if FIN else None)), "16")["images"]
    for k, im in enumerate(ims):
        telecharger(im, os.path.join(sortie, f"{k:03d}.png"))
    print(sortie, len(ims))
