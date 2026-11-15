"""Extrait les textes affichables de NeuraDesk et liste ceux que le dictionnaire
anglais (`assets/i18n/en.tsv`) ne couvre pas encore.

    python tools/i18n_extraire.py            # résumé
    python tools/i18n_extraire.py --manquants fichier.tsv
        écrit les phrases manquantes (colonne anglaise vide), par fichier source

Sources :
- les pages `assets/popups/*.html` (+ leurs `.js` maison) : nœuds de texte,
  attributs `title` / `placeholder` / `aria-label`, et chaînes JS — une
  concaténation `"A " + x + " B"` devient le motif `A {} B`, un gabarit
  `${x}` devient `{}` ;
- le Rust (`src/**/*.rs`) : littéraux qui ressemblent à du texte affiché
  (hors `diag!`, `eprintln!`, `panic!`, tests, commentaires).

C'est une HEURISTIQUE : elle ramasse plus large que nécessaire (une clé
inutile ne coûte rien à l'exécution) et peut rater une phrase construite en
plusieurs morceaux — la capture en anglais reste le juge.
"""
import pathlib
import re
import sys

RACINE = pathlib.Path(__file__).resolve().parent.parent
DICO = RACINE / "assets" / "i18n" / "en.tsv"
# Bibliothèques tierces : pas notre texte.
PAGES_EXCLUES = {"xterm.js", "xterm-addon-fit.js", "pages.js"}

LETTRE = re.compile(r"[A-Za-zÀ-ÿ]")
# Ce qui n'est pas une phrase : identifiants, sélecteurs CSS, chemins, couleurs,
# verbes d'action (`a-b-c`), unités.
NON_TEXTE = re.compile(
    r"^(?:[a-z0-9_\-.:/#%]+|[A-Za-z0-9_]+\.[a-z]{2,4}|#[0-9a-fA-F]{3,8}|rgba?\(.*|var\(--.*|"
    r"[a-z]+:[a-z/].*|\d+(?:px|ms|s|%|em|rem|vh|vw)?|[A-Z_0-9]+|[a-zA-Z]+[A-Z][a-zA-Z]*)$"
)
MOT_FRANCAIS = re.compile(
    r"[éèêàçùôîâûœÉÈÀÇ]|\b(?:le|la|les|des|du|un|une|et|ou|de|pour|avec|sans|sur|dans|pas|est|au|aux|en)\b",
    re.I,
)


def normaliser(s):
    return " ".join(s.split())


def est_texte(s):
    s = normaliser(s)
    if len(s) < 2 or not LETTRE.search(s):
        return False
    if NON_TEXTE.match(s):
        return False
    if s.startswith(("http", "file:", "neuradesk:", "data:", "--", ".", "#")):
        return False
    if re.search(r"[{};]|=>|\(\)|\.\w+\(|<\w", s):
        return False
    # Une valeur CSS : sans majuscule, faite d'unités et de mots-clés.
    if not re.search(r"[A-ZÀ-Ý]", s) and re.search(r"\d(?:px|%|em|s\b)|rgba?\(|var\(|calc\(|\b(?:solid|dashed|auto|none|inherit|ease|linear|reduce|px)\b", s):
        return False
    return True


# --- Pages -----------------------------------------------------------------

LITTERAL_JS = re.compile(r"""("(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'|`(?:[^`\\]|\\.)*`)""")


def decoder_js(lit):
    corps = lit[1:-1]
    if lit[0] == "`":
        corps = re.sub(r"\$\{[^}]*\}", "{}", corps)
    return (
        corps.replace("\\n", " ").replace('\\"', '"').replace("\\'", "'").replace("\\\\", "\\")
    )


def chaines_js(code):
    """Les littéraux, en fusionnant `"A" + x + "B"` en `A{}B`."""
    sortie = []
    for ligne in code.splitlines():
        if ligne.lstrip().startswith("//"):
            continue
        morceaux = []  # [(début, fin, texte)]
        for m in LITTERAL_JS.finditer(ligne):
            morceaux.append((m.start(), m.end(), decoder_js(m.group(0))))
        i = 0
        while i < len(morceaux):
            texte = morceaux[i][2]
            fin = morceaux[i][1]
            j = i
            while j + 1 < len(morceaux):
                entre = ligne[fin:morceaux[j + 1][0]]
                # Une concaténation : `+ expr +`, sans virgule ni fermeture.
                if re.fullmatch(r"\s*\+\s*", entre):
                    texte += morceaux[j + 1][2]
                elif re.fullmatch(r"\s*\+[^,;\[\]{}\"'`]+\+\s*", entre) and "?" not in entre:
                    texte += "{}" + morceaux[j + 1][2]
                else:
                    break
                j += 1
                fin = morceaux[j][1]
            # `"texte " + x` en fin de concaténation.
            apres = ligne[fin:]
            if re.match(r"\s*\+\s*[\w.()\[\]]+", apres) and not re.match(r"\s*\+\s*[\"'`]", apres):
                if texte.endswith(" ") or texte.endswith("("):
                    texte += "{}"
            avant = ligne[: morceaux[i][0]]
            if re.search(r"[\w.)\]]\s*\+\s*$", avant):
                if texte.startswith(" "):
                    texte = "{}" + texte
            sortie.append(texte)
            i = j + 1
    return sortie


def textes_html(source):
    sortie = []
    # Les scripts : chaînes JS.
    for m in re.finditer(r"<script[^>]*>(.*?)</script>", source, re.S):
        sortie += chaines_js(m.group(1))
    sans_script = re.sub(r"<script[^>]*>.*?</script>|<style[^>]*>.*?</style>|<!--.*?-->", "", source, flags=re.S)
    for m in re.finditer(r">([^<>]+)<", sans_script):
        sortie.append(m.group(1))
    for m in re.finditer(r'(?:title|placeholder|aria-label|value)="([^"]+)"', sans_script):
        sortie.append(m.group(1))
    return sortie


# --- Rust ------------------------------------------------------------------

LITTERAL_RUST = re.compile(r'(?<![\w#])"((?:[^"\\\n]|\\.)*)"')
MACROS_IGNOREES = re.compile(r"\b(?:diag|eprintln|println|panic|assert\w*|unreachable|debug_assert\w*|log|trace)!\s*\(")


def textes_rust(source):
    sortie = []
    dans_tests = False
    for ligne in source.splitlines():
        brut = ligne.strip()
        if brut.startswith("#[cfg(test)]"):
            dans_tests = True
        if dans_tests or brut.startswith("//") or MACROS_IGNOREES.search(ligne):
            continue
        if ".expect(" in ligne or "config_path(" in ligne or "#[" in brut:
            continue
        for m in LITTERAL_RUST.finditer(ligne):
            s = m.group(1).replace('\\"', '"').replace("\\n", " ")
            if "{" in s:
                s = re.sub(r"\{[^}]*\}", "{}", s)
            # Le Rust est plein de clés internes : n'en garder que ce qui a
            # l'air d'une phrase française, ou un mot à majuscule initiale.
            if MOT_FRANCAIS.search(s) or re.fullmatch(r"[A-ZÀ-Ý][a-zà-ÿ'’ ]+", s):
                sortie.append(s)
    return sortie


def collecter():
    phrases = {}  # phrase -> fichier source (le premier)
    for chemin in sorted((RACINE / "assets" / "popups").glob("*.html")):
        # Une page déjà écrite dans une autre langue (`aide.en.html`) : la
        # version française suffit à dresser la liste.
        if chemin.name.count(".") > 1:
            continue
        for t in textes_html(chemin.read_text(encoding="utf-8")):
            if est_texte(t):
                phrases.setdefault(normaliser(t), chemin.name)
    for chemin in sorted((RACINE / "assets" / "popups").glob("*.js")):
        if chemin.name in PAGES_EXCLUES:
            continue
        for t in chaines_js(chemin.read_text(encoding="utf-8")):
            if est_texte(t):
                phrases.setdefault(normaliser(t), chemin.name)
    for chemin in sorted((RACINE / "src").rglob("*.rs")):
        if chemin.name == "langue.rs":
            continue
        for t in textes_rust(chemin.read_text(encoding="utf-8")):
            if est_texte(t):
                phrases.setdefault(normaliser(t), str(chemin.relative_to(RACINE)).replace("\\", "/"))
    return phrases


def dictionnaire():
    cles = set()
    if DICO.exists():
        for ligne in DICO.read_text(encoding="utf-8").splitlines():
            if ligne and not ligne.startswith("#") and "\t" in ligne:
                cles.add(normaliser(ligne.split("\t", 1)[0]))
    return cles


def main():
    phrases = collecter()
    connues = dictionnaire()
    manquantes = {p: f for p, f in phrases.items() if p not in connues}
    print(f"{len(phrases)} phrases trouvées, {len(phrases) - len(manquantes)} traduites, {len(manquantes)} manquantes")
    if "--manquants" in sys.argv:
        cible = pathlib.Path(sys.argv[sys.argv.index("--manquants") + 1])
        lignes = []
        courant = None
        for p, f in sorted(manquantes.items(), key=lambda kv: (kv[1], kv[0])):
            if f != courant:
                lignes.append(f"# == {f}")
                courant = f
            lignes.append(p + "\t")
        cible.write_text("\n".join(lignes) + "\n", encoding="utf-8")
        print(f"écrit : {cible}")


if __name__ == "__main__":
    main()
