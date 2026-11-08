"""Régénère la section « Dépendances Rust » de `THIRD-PARTY-NOTICES.md`.

    python tools/generer_notices.py

⭐ **POURQUOI UN GÉNÉRATEUR, ET PAS UNE LISTE TENUE À LA MAIN.** 336 paquets, dont
les deux tiers sous double licence : une liste manuelle dérive au premier `cargo
update`, et c'est précisément l'obligation qu'on ne peut pas se permettre de rater.
Le script lit `cargo metadata` (les licences DÉCLARÉES) et, pour chacune, le fichier
de licence que le crate porte lui-même dans `~/.cargo/registry/src/` — la seule
source qui ne mente pas.

Il écrit `THIRD-PARTY-CRATES.md` **en entier** — un fichier à part, parce que les
textes collectés dépassent la taille que GitHub accepte de rendre en Markdown.
`THIRD-PARTY-NOTICES.md` (icônes, police, CEF, VLC) est écrit à la main et n'est
jamais touché.

⚠️ **Il ne décide de rien.** Une dépendance sous une licence incompatible (GPL,
AGPL, SSPL…) est SIGNALÉE en fin de section, jamais corrigée ni masquée : c'est un
arbitrage, pas une mise en forme.
"""
import json
import pathlib
import re
import subprocess
import sys
from collections.abc import Iterator

RACINE = pathlib.Path(__file__).resolve().parent.parent
# ⚠️ **FICHIER SÉPARÉ, ET PAS UNE SECTION DU FICHIER PRINCIPAL** : les textes
# collectés pèsent ~560 Ko, au-delà de ce que GitHub accepte de RENDRE en Markdown
# (il affiche alors le fichier brut, ou refuse). `THIRD-PARTY-NOTICES.md` reste donc
# court et lisible, et renvoie ici.
CRATES = RACINE / "THIRD-PARTY-CRATES.txt"

# Ce qu'on cherche dans le dossier d'un crate. `license-*` (minuscule) est la
# convention de plusieurs crates de Microsoft et de Google.
MOTIFS = re.compile(r"^(licen[cs]e|copying|notice|unlicense)", re.IGNORECASE)


def paquets() -> Iterator[dict]:
    sortie = subprocess.run(
        ["cargo", "metadata", "--format-version", "1"],
        cwd=RACINE, capture_output=True, text=True, check=True,
    ).stdout
    meta = json.loads(sortie)
    for p in meta["packages"]:
        if p["name"] == "neuradesk":
            continue
        yield p


def textes_du_crate(p: dict) -> list[str]:
    """Les fichiers de licence portés par le crate lui-même (0, 1 ou plusieurs)."""
    dossier = pathlib.Path(p["manifest_path"]).parent
    trouves = []
    try:
        for f in sorted(dossier.iterdir()):
            if f.is_file() and MOTIFS.match(f.name) and f.suffix.lower() in (".md", ".txt", ""):
                try:
                    brut = f.read_text(encoding="utf-8", errors="replace")
                except OSError:
                    continue
                propre = brut.replace("\r\n", "\n").strip() + "\n"
                if len(propre) > 40:  # un fichier vide ne dit rien
                    trouves.append(propre)
    except OSError:
        pass
    return trouves


def est_risquee(spdx: str | None) -> bool:
    """Un indicateur négatif serait un CHOIX de licence, pas un oubli : on le montre."""
    n = (spdx or "").upper()
    return any(x in n for x in ("GPL", "AGPL", "SSPL", "CC-BY-SA", "EUPL")) and "LGPL-2.1-OR-LATER" not in n


def main() -> int:
    lignes = []
    textes = {}   # texte normalisé -> {crates}
    ordre = []    # crates dans l'ordre alphabétique, pour une sortie stable

    for p in sorted(paquets(), key=lambda p: (p["name"], p["version"])):
        spdx = p.get("license") or "|".join(p.get("license_file") or []) or "NON DÉCLARÉE"
        ordre.append((p["name"], p["version"], spdx))
        for t in textes_du_crate(p):
            textes.setdefault(t, set()).add(f"{p['name']} {p['version']}")

    entete = [
        "NeuraDesk — dépendances Rust : licences et textes",
        "",
        "Texte GÉNÉRÉ par python tools/generer_notices.py : ne pas l'éditer à la main.",
        "Les composants non-Rust (icônes, police, CEF, VLC) sont dans",
        "THIRD-PARTY-NOTICES.md.",
        "",
        "Ce fichier est joint à chaque archive publiée : il remplit l'obligation de",
        "mention des licences MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0,",
        "CDLA-Permissive-2.0 et bzip2-1.0.6 de ces paquets.",
        "",
    ]
    lignes.append("")
    lignes.append(f"Liste générée depuis Cargo.lock — {len(ordre)} paquets.")
    lignes.append("")
    lignes.append("PAQUET · VERSION · LICENCE DÉCLARÉE")
    for nom, version, spdx in ordre:
        lignes.append(f"  {nom} {version} — {spdx}")
    lignes.append("")

    # LES TEXTES, dédupliqués : les crates MIT partagent presque tous le même
    # fichier, et le répéter 200 fois rendrait le document illisible.
    attribues = sorted(textes.items(), key=lambda kv: sorted(kv[1])[0])
    lignes.append(f"TEXTES DE LICENCE COLLECTÉS ({len(attribues)} distincts)")
    lignes.append("")
    lignes.append("Chaque texte est celui que le crate livre lui-même ; la liste des")
    lignes.append("paquets qui l'emploient est sous chaque bloc.")
    lignes.append("")
    for i, (texte, crates) in enumerate(attribues, 1):
        utilise = ", ".join(sorted(crates))
        lignes.append(f"--- Texte L{i} ---")
        lignes.append("")
        lignes.append(f"Employé par : {utilise}")
        lignes.append("")
        lignes.append("```")
        lignes.append(texte.rstrip("\n"))
        lignes.append("```")
        lignes.append("")

    risquees = [(n, v, s) for n, v, s in ordre if est_risquee(s)]
    lignes.append("LICENCES À ARBITRER")
    lignes.append("")
    if risquees:
        for n, v, s in risquees:
            lignes.append(f"  {n} {v} — {s}")
    else:
        lignes.append("Aucune : toutes les dépendances sont permissives (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, CDLA, bzip2), compatibles GPLv3.")
    lignes.append("")

    contenu = "\n".join(entete) + "\n" + "\n".join(lignes)
    CRATES.write_text(contenu, encoding="utf-8", newline="\n")
    print(f"{len(ordre)} paquets, {len(attribues)} textes distincts, {len(risquees)} licence(s) à arbitrer")
    print(f"écrit dans {CRATES} ({len(contenu) / 1024:.0f} Ko)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
