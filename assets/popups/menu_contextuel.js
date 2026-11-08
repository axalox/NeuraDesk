// Le clic droit façon Windows, commun aux panneaux CEF.
//
// Le bouton droit est transmis à CEF (voir `cef_popup::dispatch_input`,
// `WM_RBUTTONDOWN`) : `contextmenu` part avec sa vraie cible et ses vraies
// coordonnées, et le menu natif de Chromium est vidé côté Rust
// (`cef_app::NeuraDeskContextMenuHandler`). Ce script dessine le menu à sa place :
//   * dans un champ de texte : Annuler, Rétablir, Couper, Copier, Coller,
//     Supprimer, Tout sélectionner — grisés comme sous Windows ;
//   * sur un texte sélectionné ailleurs : Copier ;
//   * une page peut ouvrir SON menu : `menuContextuel.ouvrir(e, [...])` dans
//     son propre gestionnaire `contextmenu`, qui appelle `e.preventDefault()`
//     — ce script ignore alors l'évènement.
//
// Les commandes d'édition partent vers Rust (`neuradesk://edition/<commande>`,
// voir `cef_app::editer`) et s'exécutent sur le cadre : `execCommand("paste")`
// est refusé par Chromium, `CefFrame::Paste` ne l'est pas. ⛔ Le menu ne prend
// JAMAIS le focus (`mousedown` annulé) : le champ garde sa sélection, et c'est
// sur elle que la commande agit.
(function () {
  const style = document.createElement("style");
  style.textContent = `
    .menu-contextuel {
      position: fixed; z-index: 2147483647; min-width: 190px; padding: 4px;
      box-sizing: border-box; font-family: var(--nd-police, "Segoe UI", sans-serif); font-size: 13px;
      background: rgb(var(--popup-bg-rgb, 32, 32, 36));
      color: rgb(var(--popup-fg-rgb, 255, 255, 255));
      border: 1px solid rgba(var(--popup-fg-rgb, 255, 255, 255), .16);
      border-radius: var(--nd-rayon-surface, 8px);
      box-shadow: 0 6px 18px rgba(0, 0, 0, .45);
      user-select: none;
      /* À la taille des AUTRES menus de l'app (« Taille des menus » × « Taille
         des textes ») : Rust pose le rapport avec le zoom de la page à chaque
         frame (cef_popup::echelle_du_menu_en_page). Un transform, pas un
         zoom : left/top restent en pixels de la page. ⛔ PAS D'ACCENT GRAVE
         ici : cette feuille est une chaîne JS entre accents graves. */
      transform: scale(var(--nd-echelle-menu, 1)); transform-origin: 0 0;
    }
    .menu-contextuel .element {
      display: flex; justify-content: space-between; gap: 24px;
      padding: 5px 10px; border-radius: var(--nd-rayon-petit, 3px); cursor: default; white-space: nowrap;
    }
    .menu-contextuel .element:hover:not(.grise) { background: rgba(var(--popup-accent-rgb, 0, 120, 215), .45); }
    .menu-contextuel .element.grise { opacity: .4; }
    .menu-contextuel .raccourci { opacity: .6; }
    .menu-contextuel .trait { height: 1px; margin: 4px 6px; background: rgba(var(--popup-fg-rgb, 255, 255, 255), .14); }
  `;
  document.head.appendChild(style);

  let menu = null;

  function fermer() {
    if (menu) { menu.remove(); menu = null; }
  }

  // `elements` : "-" (un trait) ou { texte, action, raccourci?, actif? } —
  // `actif: false` grise l'élément. `e` : l'évènement `contextmenu`.
  function ouvrir(e, elements) {
    fermer();
    const visibles = elements.filter((x, i, t) => x !== "-" || (i > 0 && i < t.length - 1 && t[i - 1] !== "-"));
    if (!visibles.length) return;
    menu = document.createElement("div");
    menu.className = "menu-contextuel";
    menu.addEventListener("mousedown", (ev) => ev.preventDefault());
    menu.addEventListener("contextmenu", (ev) => ev.preventDefault());
    for (const x of visibles) {
      if (x === "-") {
        menu.appendChild(Object.assign(document.createElement("div"), { className: "trait" }));
        continue;
      }
      const ligne = document.createElement("div");
      ligne.className = "element" + (x.actif === false ? " grise" : "");
      ligne.appendChild(Object.assign(document.createElement("span"), { textContent: x.texte }));
      if (x.raccourci) ligne.appendChild(Object.assign(document.createElement("span"), { className: "raccourci", textContent: x.raccourci }));
      ligne.addEventListener("click", (ev) => {
        ev.stopPropagation();
        if (x.actif === false) return;
        fermer();
        x.action();
      });
      menu.appendChild(ligne);
    }
    document.body.appendChild(menu);
    poserLeFond(menu);
    // Comme Windows : à droite et sous le curseur, retourné s'il déborde. La
    // boîte mesurée est la boîte MISE À L'ÉCHELLE (origine du `transform` au
    // coin haut-gauche : elle commence à left/top).
    const r = menu.getBoundingClientRect();
    const gauche = e.clientX + r.width > innerWidth ? Math.max(0, e.clientX - r.width) : e.clientX;
    const haut = e.clientY + r.height > innerHeight ? Math.max(0, e.clientY - r.height) : e.clientY;
    menu.style.left = gauche + "px";
    menu.style.top = haut + "px";
  }

  // ⛔ **LE FOND SE CHOISIT CONTRE LE TEXTE QUAND RUST NE LE DONNE PAS.** Le menu lit
  // `--popup-bg-rgb`, que Rust pousse aux PANNEAUX (le pôle opposé au texte, voir
  // `cef_popup::push_window_palette`). Sans elle, il retombait sur du gris foncé
  // sous un texte devenu FONCÉ en thème clair — défaut SIGNALÉ (2026-10-11) : noir
  // sur noir dans le dash dossier. Une page qui ne la reçoit pas prend donc un fond
  // clair sous un texte foncé, et un fond foncé sous un texte clair.
  function poserLeFond(boite) {
    if (getComputedStyle(document.documentElement).getPropertyValue("--popup-bg-rgb").trim()) return;
    const rgb = (getComputedStyle(boite).color.match(/\d+(\.\d+)?/g) || ["255", "255", "255"]).map(Number);
    const luminance = (0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]) / 255;
    boite.style.background = luminance < 0.5 ? "rgb(246, 246, 248)" : "rgb(32, 32, 36)";
    boite.style.boxShadow = luminance < 0.5 ? "0 6px 18px rgba(0, 0, 0, .25)" : "";
  }

  function editer(commande) {
    location.href = "neuradesk://edition/" + commande;
  }

  const TYPES_TEXTE = ["text", "search", "url", "tel", "email", "password", "number"];

  function champDe(cible) {
    const c = cible && cible.closest ? cible.closest("input, textarea, [contenteditable]") : null;
    if (c instanceof HTMLInputElement) return TYPES_TEXTE.includes(c.type) ? c : null;
    return c && (c instanceof HTMLTextAreaElement || c.isContentEditable) ? c : null;
  }

  function aUneSelection(champ) {
    if (champ.isContentEditable) return String(getSelection()).length > 0;
    try { return champ.selectionStart !== champ.selectionEnd; }
    catch { return false; } // `number` n'expose pas sa sélection
  }

  function menuEdition(e, champ) {
    const ecriture = !champ.disabled && !champ.readOnly;
    const choisi = aUneSelection(champ);
    const secret = champ.type === "password";
    const vide = champ.isContentEditable ? !champ.textContent : !champ.value;
    champ.focus();
    ouvrir(e, [
      { texte: "Annuler", raccourci: "Ctrl+Z", actif: ecriture, action: () => editer("annuler") },
      { texte: "Rétablir", raccourci: "Ctrl+Y", actif: ecriture, action: () => editer("retablir") },
      "-",
      { texte: "Couper", raccourci: "Ctrl+X", actif: ecriture && choisi && !secret, action: () => editer("couper") },
      { texte: "Copier", raccourci: "Ctrl+C", actif: choisi && !secret, action: () => editer("copier") },
      { texte: "Coller", raccourci: "Ctrl+V", actif: ecriture, action: () => editer("coller") },
      { texte: "Supprimer", raccourci: "Suppr", actif: ecriture && choisi, action: () => editer("supprimer") },
      "-",
      { texte: "Tout sélectionner", raccourci: "Ctrl+A", actif: !vide, action: () => editer("tout-selectionner") },
    ]);
  }

  // Phase de BOUILLONNEMENT sur `document` : les gestionnaires des pages
  // passent avant, et un `preventDefault` de leur part veut dire « déjà traité ».
  document.addEventListener("contextmenu", (e) => {
    if (e.defaultPrevented) return;
    e.preventDefault();
    const champ = champDe(e.target);
    if (champ) { menuEdition(e, champ); return; }
    if (String(getSelection()).length > 0) {
      ouvrir(e, [{ texte: "Copier", raccourci: "Ctrl+C", action: () => editer("copier") }]);
      return;
    }
    fermer();
  });

  // Se referme au clic ailleurs, à Échap, et quand la page perd le focus ou défile.
  document.addEventListener("mousedown", (e) => { if (menu && !menu.contains(e.target)) fermer(); }, true);
  document.addEventListener("keydown", (e) => { if (menu && e.key === "Escape") { e.preventDefault(); e.stopPropagation(); fermer(); } }, true);
  addEventListener("blur", fermer);
  addEventListener("resize", fermer);
  document.addEventListener("scroll", (e) => { if (menu && !menu.contains(e.target)) fermer(); }, true);

  window.menuContextuel = { ouvrir, fermer, editer };
})();
