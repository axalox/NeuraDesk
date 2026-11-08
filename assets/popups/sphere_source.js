// LA VUE SPHÈRE, CÔTÉ PANNEAUX — chargé par `terminal.html` et `ssh.html`
// (voir `docs/plans/vue-sphere-terminaux.md` et `win32::cef_sphere`).
//
// Trois choses, les mêmes dans les deux pages :
// - fabriquer les VIGNETTES de la page et les envoyer à Rust (canal POST
//   `<source>/vignettes`, puis l'action `vignettes`) ;
// - capter le CLIC MOLETTE qui ouvre la sphère — sauf sur un onglet (il ferme
//   déjà le groupe), dans un champ de saisie, et dans un terminal dont le
//   programme suit la souris (htop, mc, vim `mouse=a`…) ;
// - poser l'INDICATEUR du mode dans la barre : « Terminal · nom » ou « Vue
//   sphère · N sessions » ; un clic dessus bascule de l'un à l'autre, quel que
//   soit le raccourci réglé.
// La sphère elle-même est `sphere_vue.js`.
//
// ⚠️ La page garde la main sur ses sessions : ce script ne lit que ce qu'on lui
// donne (`decrire`, `actif`, `termSous`).
(function () {
  "use strict";

  const LIGNES_APERCU = 14;
  let options = null;
  let raccourci = "molette";
  let indicateur = null;

  // L'indicateur : en CREUX (noir), comme les cartes des applets — un blanc
  // faible est écrasé par la composition de la fenêtre.
  const STYLE = `
    .sphere-indicateur {
      display: inline-flex; align-items: center; gap: 6px; flex: 0 1 auto; min-width: 0;
      max-width: 34ch; margin: 0 4px; padding: 3px 10px; border-radius: 999px;
      background: rgba(0, 0, 0, 0.22); border: 1px solid rgba(var(--popup-fg-rgb, 255, 255, 255), 0.30);
      color: inherit; font: inherit; font-size: 12px; white-space: nowrap; overflow: hidden;
      text-overflow: ellipsis; cursor: pointer; zoom: var(--text-scale, 1);
    }
    .sphere-indicateur:hover { border-color: rgba(var(--popup-fg-rgb, 255, 255, 255), 0.60); }
    .sphere-point {
      flex: 0 0 auto; width: 7px; height: 7px; border-radius: 50%;
      background: rgb(var(--popup-accent-solid-rgb, 96, 160, 255));
    }
  `;

  // Les dernières lignes NON VIDES de l'écran d'un terminal xterm, de haut en bas.
  function derniersLignes(term, combien = LIGNES_APERCU) {
    const tampon = term.buffer.active;
    const fin = Math.min(tampon.length, tampon.baseY + term.rows) - 1;
    const lignes = [];
    for (let y = fin; y >= 0 && y > fin - 60; y--) {
      const ligne = tampon.getLine(y);
      lignes.unshift(ligne ? ligne.translateToString(true) : "");
    }
    while (lignes.length && !lignes[lignes.length - 1].trim()) lignes.pop();
    return lignes.slice(-combien);
  }

  // Le dossier courant lu dans la DERNIÈRE invite reconnue : PowerShell
  // (`PS C:\x>`), cmd (`C:\x>`), bash/zsh (`moi@hôte:~/x$`). `""` sinon — une
  // invite personnalisée ne se devine pas, et un faux dossier serait pire.
  function dossierDuPrompt(lignes) {
    const motifs = [
      /^PS ([A-Za-z]:\\[^>]*|\\\\[^>]*)>/,
      /^([A-Za-z]:\\[^>]*)>/,
      /^[\w.-]+@[\w.-]+:\s*(~[^\s$#]*|\/[^\s$#]*)\s*[$#]/,
      /^\[[\w.-]+@[\w.-]+ ([^\]]+)\][$#]/,
    ];
    for (let i = lignes.length - 1; i >= 0; i--) {
      for (const motif of motifs) {
        const m = lignes[i].match(motif);
        if (m) return m[1];
      }
    }
    return "";
  }

  function envoyer() {
    if (!options) return;
    const vignettes = options.decrire();
    options.poster(options.source + "/vignettes", JSON.stringify(vignettes))
      .then(() => options.go("vignettes"), () => {});
  }

  // La sphère s'ouvre DANS le panneau (voir `sphere_vue.js`) ; l'indicateur la
  // referme quand elle est ouverte.
  function ouvrirLaSphere() {
    if (!options || !window.sphereVue) return;
    if (sphereVue.ouverte()) sphereVue.fermer(options.actif());
    else sphereVue.ouvrir(options.actif());
  }

  // Le clic molette compte-t-il comme le raccourci de la sphère ?
  function estLeRaccourci(e) {
    if (e.button !== 1) return false;
    if (raccourci === "aucun") return false;
    if (raccourci === "ctrl-molette") return e.ctrlKey;
    return !e.ctrlKey && !e.shiftKey && !e.altKey;
  }

  // Les endroits où le clic molette appartient déjà à quelqu'un d'autre.
  function appartientAilleurs(e) {
    const cible = e.target instanceof Element ? e.target : null;
    if (!cible) return false;
    if (cible.closest(".onglet, input, textarea, select, [contenteditable='true']")) return true;
    const term = options.termSous(cible);
    return !!(term && term.modes && term.modes.mouseTrackingMode !== "none");
  }

  // Sphère ouverte : le clic molette est à elle (voir `sphere_vue.js`).
  const sphereOuverte = () => !!(window.sphereVue && sphereVue.ouverte());

  function surAppui(e) {
    if (!options || sphereOuverte() || !estLeRaccourci(e) || appartientAilleurs(e)) return;
    e.preventDefault();
    e.stopPropagation();
    ouvrirLaSphere();
  }

  // Le clic molette « au relâcher » (`auxclick`) d'un endroit capté ne doit
  // atteindre personne : sans ça, un onglet sous le pointeur au moment du
  // relâcher pourrait se fermer.
  function surRelache(e) {
    if (!options || sphereOuverte() || !estLeRaccourci(e) || appartientAilleurs(e)) return;
    e.preventDefault();
    e.stopPropagation();
  }

  window.sphereSource = {
    // `source` : "terminal" | "ssh" — le préfixe des routes et des actions.
    // `decrire()` : les vignettes de la page ; `actif()` : l'onglet actif ;
    // `termSous(element)` : le terminal xterm sous un élément, ou null ;
    // `texte()` : le texte de l'indicateur ; `barre`/`avant` : où le poser ;
    // `go`/`poster` : ceux de la page.
    installer(o) {
      options = o;
      const style = document.createElement("style");
      style.textContent = STYLE;
      document.head.appendChild(style);
      document.addEventListener("mousedown", surAppui, true);
      document.addEventListener("auxclick", surRelache, true);
      indicateur = document.createElement("button");
      indicateur.className = "sphere-indicateur";
      indicateur.type = "button";
      indicateur.addEventListener("click", ouvrirLaSphere);
      indicateur.addEventListener("mousedown", (e) => e.stopPropagation());
      o.barre.insertBefore(indicateur, o.avant || null);
      this.rafraichir();
    },
    // Le texte de l'indicateur (`options.texte()`) — « Terminal · <onglet> »
    // ou « SFTP · <hôte> ». La page l'appelle chaque fois que ses onglets changent.
    rafraichir() {
      if (!indicateur) return;
      const point = document.createElement("span");
      point.className = "sphere-point";
      const enSphere = sphereOuverte();
      indicateur.replaceChildren(point, document.createTextNode(enSphere ? sphereVue.libelle() : options.texte()));
      const touche = raccourci === "ctrl-molette" ? "Ctrl + clic molette" : raccourci === "aucun" ? "ce bouton" : "clic molette";
      indicateur.title = enSphere ? "Vue sphère — revenir au mode normal : Échap ou ce bouton" : "Mode normal — ouvrir la vue sphère : " + touche;
    },
    raccourci(valeur) {
      if (!["molette", "ctrl-molette", "aucun"].includes(valeur) || valeur === raccourci) return;
      raccourci = valeur;
      if (window.sphereVue) sphereVue.raccourci(valeur);
      this.rafraichir();
    },
    envoyer,
    derniersLignes,
    dossierDuPrompt,
  };
})();
