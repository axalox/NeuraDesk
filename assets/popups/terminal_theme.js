// LE THÈME DES TERMINAUX (xterm.js) — partagé par `terminal.html` et `ssh.html`.
//
// DEMANDE EXPLICITE (2026-10-15) : « que les terminaux aient des fonds colorés
// utilisant le thème plutôt que noir ». Le fond est peint par la page (la
// surface du thème, `--popup-tint-rgb`) : xterm reste TRANSPARENT par-dessus.
//
// ⚠️ **LE TEXTE SUIT LE THÈME, SINON IL DISPARAÎT** : xterm écrit en blanc par
// défaut, illisible sur la surface d'un thème CLAIR. Le texte prend donc
// `--popup-fg-rgb` (lisible sur la surface : c'est le couple des familles de
// thèmes), et les 16 couleurs ANSI une palette pour fond clair ou sombre selon
// ce texte. Le curseur et la sélection prennent l'accent.
//
// Les variables sont poussées par Rust à CHAQUE frame et changent avec le thème :
// `suivreLeThemeDesTerminaux` relit et réapplique quand elles changent.
(function () {
  // Les couleurs ANSI de VS Code (Dark+ et Light+), faites pour ces deux fonds.
  const ANSI_FOND_SOMBRE = {
    black: "#000000", red: "#cd3131", green: "#0dbc79", yellow: "#e5e510",
    blue: "#2472c8", magenta: "#bc3fbc", cyan: "#11a8cd", white: "#e5e5e5",
    brightBlack: "#666666", brightRed: "#f14c4c", brightGreen: "#23d18b", brightYellow: "#f5f543",
    brightBlue: "#3b8eea", brightMagenta: "#d670d6", brightCyan: "#29b8db", brightWhite: "#ffffff",
  };
  const ANSI_FOND_CLAIR = {
    black: "#000000", red: "#cd3131", green: "#00862f", yellow: "#949800",
    blue: "#0451a5", magenta: "#bc05bc", cyan: "#0598bc", white: "#555555",
    brightBlack: "#666666", brightRed: "#cd3131", brightGreen: "#14a014", brightYellow: "#b5ba00",
    brightBlue: "#0451a5", brightMagenta: "#bc05bc", brightCyan: "#0598bc", brightWhite: "#a5a5a5",
  };

  // LA PALETTE DES COULEURS PERSONNALISÉES — DEMANDE EXPLICITE (2026-10-15) :
  // « choisir une couleur personnalisée pour les terminaux en utilisant Ctrl +
  // clic droit sur un fond de terminal », puis « mets la même palette de couleurs
  // que pour les lueurs » : le nuancier PARTAGÉ (`nuancier.js`, chargé avant ce
  // fichier par `terminal.html`), à plat, avec le code pour nom. Le texte posé
  // dessus est clair ou sombre selon le fond (`texteSur`). Vide là où le nuancier
  // n'est pas chargé (`ssh.html`, qui ne propose pas de couleur).
  window.COULEURS_DES_TERMINAUX = (window.NUANCIER || []).flat().map((hex) => [hex, hex]);
  const TEXTE_CLAIR = "242, 242, 242";
  const TEXTE_SOMBRE = "28, 28, 28";

  function luminance(r, g, b) {
    const f = (c) => { c /= 255; return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
  }

  // Le texte (triplet « r, g, b ») le plus lisible sur le fond `hex` (#rrggbb).
  function texteSur(hex) {
    const fond = luminance(...[1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)));
    const ratio = (triplet) => {
      const l = luminance(...triplet.split(",").map(Number));
      return (Math.max(l, fond) + 0.05) / (Math.min(l, fond) + 0.05);
    };
    return ratio(TEXTE_CLAIR) >= ratio(TEXTE_SOMBRE) ? TEXTE_CLAIR : TEXTE_SOMBRE;
  }

  function lire(nom, defaut) {
    const valeur = getComputedStyle(document.documentElement).getPropertyValue(nom).trim();
    return valeur || defaut;
  }

  // Vrai si le triplet « r, g, b » est sombre — un texte sombre veut dire un fond clair.
  function estSombre(triplet) {
    const [r, g, b] = triplet.split(",").map(Number);
    return 0.2126 * r + 0.7152 * g + 0.0722 * b < 128;
  }

  // `fond` : la couleur personnalisée du terminal (#rrggbb), sinon celle du thème.
  window.themeDesTerminaux = function (fond) {
    const texte = fond ? texteSur(fond) : lire("--popup-fg-rgb", "255, 255, 255");
    const accent = lire("--popup-accent-solid-rgb", "96, 160, 255");
    const surAccent = lire("--popup-on-accent-rgb", "0, 0, 0");
    return Object.assign({}, estSombre(texte) ? ANSI_FOND_CLAIR : ANSI_FOND_SOMBRE, {
      background: "rgba(0,0,0,0)",
      foreground: `rgb(${texte})`,
      cursor: `rgb(${accent})`,
      cursorAccent: `rgb(${surAccent})`,
      selectionBackground: `rgba(${accent}, .35)`,
    });
  };

  // Applique à `term` le thème de son fond (`term.__fond`, sa couleur
  // personnalisée, ou rien pour celle du thème) — s'il a changé.
  window.repeindreLeTerminal = function (term) {
    const theme = themeDesTerminaux(term.__fond);
    const cle = JSON.stringify(theme);
    if (term.__cleTheme === cle) return;
    term.options.theme = theme;
    term.__cleTheme = cle;
  };

  // ⭐ LA TEXTURE DES TERMINAUX (voir `win32::texture_terminaux`) : une image de
  // l'écran entier, en pixels PHYSIQUES, que Rust régénère quand ses réglages
  // changent — posée sur le fond de chaque terminal (`.terminal-hote`), à un
  // pixel d'image par pixel d'écran (`devicePixelRatio` inclut le zoom de la
  // page). `null` : aucune texture.
  window.poserTextureDesTerminaux = function (url, largeur, hauteur) {
    const racine = document.documentElement.style;
    const r = window.devicePixelRatio || 1;
    if (!url) {
      racine.removeProperty("--texture-terminaux");
      return;
    }
    racine.setProperty("--texture-terminaux", `url("${url}")`);
    racine.setProperty("--texture-terminaux-taille", `${largeur / r}px ${hauteur / r}px`);
  };

  // `terminaux` rend la liste des `Terminal` du moment (les onglets changent).
  window.suivreLeThemeDesTerminaux = function (terminaux) {
    setInterval(() => terminaux().forEach(repeindreLeTerminal), 500);
  };

  // LA PILE DES POLICES DE TERMINAL — DEMANDE EXPLICITE (2026-10-28) : « ajoute
  // le réglage de police des terminaux ». Écrite ICI et pas dans chaque page :
  // `terminal.html` et `ssh.html` peignent toutes deux du texte en xterm.js, et
  // un réglage qui se dit « des terminaux » sans en gouverner qu'un seul serait
  // un mensonge. Une liste écrite deux fois se vérifie mal (la règle du dépôt).
  //
  // ⚠️ **Le choix de l'utilisateur PRÉCÈDE la pile, il ne la remplace pas.** Une
  // police qui ne dessine pas les boîtes et les blocs — Nordwand Mono, mesuré le
  // 2026-10-28 : `U+250F` et `U+2588` ABSENTS de son cmap — emprunte ces glyphes
  // aux suivants de la pile. Le glyphe de repli mesure 18 % de plus que la
  // cellule, mais xterm avance d'une cellule fixe : la grille tient, seul le
  // trait change de graisse. Retirer DejaVu de la pile laisserait `monospace`
  // generique les dessiner.
  const PILE_PAR_DEFAUT = '"Nordwand Mono", "DejaVu Sans Mono", "Cascadia Mono", Consolas, monospace';
  window.PILE_PAR_DEFAUT_DES_TERMINAUX = PILE_PAR_DEFAUT;

  // `choisie` : le nom réglé par l'utilisateur (`""` : la pile par défaut). Le
  // nom vient d'une page et finit dans une chaîne CSS ; `ui::style::police_valide`
  // l'a déjà borné côté Rust (alphanumérique, espace, tiret — ni virgule ni
  // guillemet), donc le quotage ici ne peut pas casser la pile.
  window.pileDesPolicesDesTerminaux = function (choisie) {
    const nom = (choisie || "").trim();
    return nom ? '"' + nom + '", ' + PILE_PAR_DEFAUT : PILE_PAR_DEFAUT;
  };
})();
