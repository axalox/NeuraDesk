# Petit client ComfyUI : envoi d'image, file d'attente, attente, recuperation.
import json, time, uuid, urllib.request, urllib.parse, urllib.error

SERVEUR = "http://127.0.0.1:8188"


def envoyer_image(chemin):
    nom = f"vol_{uuid.uuid4().hex[:8]}.png"
    frontiere = uuid.uuid4().hex
    with open(chemin, "rb") as f:
        donnees = f.read()
    corps = (f"--{frontiere}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"{nom}\"\r\n"
             f"Content-Type: image/png\r\n\r\n").encode() + donnees + f"\r\n--{frontiere}--\r\n".encode()
    req = urllib.request.Request(f"{SERVEUR}/upload/image", corps,
                                 {"Content-Type": f"multipart/form-data; boundary={frontiere}"})
    return json.load(urllib.request.urlopen(req, timeout=60))["name"]


def lancer(graphe):
    req = urllib.request.Request(f"{SERVEUR}/prompt", json.dumps({"prompt": graphe}).encode(),
                                 {"Content-Type": "application/json"})
    try:
        return json.load(urllib.request.urlopen(req, timeout=60))["prompt_id"]
    except urllib.error.HTTPError as e:
        raise RuntimeError(e.read().decode())


def attendre(ident, noeud):
    while True:
        h = json.load(urllib.request.urlopen(f"{SERVEUR}/history/{ident}", timeout=30))
        if ident in h:
            st = h[ident].get("status", {})
            if st.get("status_str") == "error":
                raise RuntimeError(json.dumps(st)[:1500])
            if st.get("completed"):
                return h[ident]["outputs"][noeud]
        time.sleep(1)


def telecharger(im, sortie):
    q = urllib.parse.urlencode({k: im[k] for k in ("filename", "subfolder", "type")})
    with open(sortie, "wb") as f:
        f.write(urllib.request.urlopen(f"{SERVEUR}/view?{q}", timeout=120).read())
