// LA VUE SPHÈRE, DANS LE PANNEAU — chargé par `terminal.html` et `ssh.html`
// (voir `docs/plans/vue-sphere-terminaux.md` et `win32::cef_sphere`).
//
// ⭐ **LES TERMINAUX DE LA SPHÈRE SONT LES VRAIS** (choix de l'utilisateur, le
// 2026-11-01 : « le terminal dans la sphère n'est pas le même que dans la
// fenêtre normale », « les terminaux ne sont pas interactifs en mode sphère »).
// L'élément de chaque session (le `.terminal-hote` du Terminal, la `.vue` d'une
// connexion SSH) est DÉPLACÉ dans sa carte, À SA TAILLE RÉELLE, puis réduit par
// `transform: scale` : xterm ne change ni de colonnes ni de rangées, le shell
// ne voit rien, et la sortie continue d'arriver. On tape dans le terminal
// choisi — Entrée lui appartient.
//
// Les sessions de L'AUTRE panneau sont des APERÇUS (vignettes fabriquées par
// sa page et relayées par Rust) : un clic dessus bascule vers l'autre panneau,
// dans sa propre sphère.
//
// ⛔ **CHROMIUM NE TROUVE PAS LES CARTES 3D SOUS LE POINTEUR** (mesuré le
// 2026-11-01 : `e.target` = la scène, même sur une carte). `carteSous(x, y)`
// lit le rectangle PROJETÉ de chaque carte. Dans le terminal CHOISI, la souris
// est relayée à l'élément visé, coordonnées corrigées du `scale` (voir « LA
// SOURIS DANS LE TERMINAL CHOISI ») : sélection, clic droit Copier / Coller.
//
// Gestes : glisser → tourner ; roulette → zoomer ; clic → choisir (et taper
// dedans) ; dans le terminal choisi : glisser → sélectionner, clic droit → le
// menu de la page ; clic molette, ou double-clic hors de son terminal → mode
// normal ; Échap → mode normal sur le terminal choisi.
(function () {
  "use strict";

  const DEG = Math.PI / 180;
  const ECART = 26;           // px CSS entre deux cartes
  const PITCH_MAX = 70;       // on ne regarde pas plus haut / plus bas
  const DUREE_ARRIVEE = 260;  // ms
  const DUREE_DEPART = 200;   // ms
  const ZOOM_AVANT = 0.5, ZOOM_ARRIERE = 0.7; // × rayon
  const PART_DE_LA_LARGEUR = 0.30;
  const TETE = 64;            // px CSS (× l'échelle du texte) : l'en-tête d'une carte
  const RACCOURCIS = [["molette", "Clic molette"], ["ctrl-molette", "Ctrl + clic molette"], ["aucun", "Désactivé"]];

  const STYLE = `
    #sph-racine { position: fixed; left: 0; right: 0; bottom: 0; z-index: 40; display: flex; flex-direction: column; }
    #sph-racine.cache { display: none; }
    #sph-scene { position: relative; flex: 1 1 auto; overflow: hidden; cursor: grab; }
    #sph-scene.glisse { cursor: grabbing; }
    #sph-scene.texte:not(.glisse) { cursor: text; }
    #sph-monde { position: absolute; left: 50%; top: 50%; width: 0; height: 0; transform-style: preserve-3d; }
    .sph-ciel {
      position: absolute; width: 4px; height: 4px; margin: -2px 0 0 -2px; border-radius: 50%;
      background: rgba(var(--popup-fg-rgb, 255, 255, 255), 0.45); backface-visibility: hidden;
    }
    .sph-carte {
      position: absolute; box-sizing: border-box; display: flex; flex-direction: column; gap: 6px;
      padding: 10px 12px; border-radius: 10px; overflow: hidden; backface-visibility: hidden;
      background: rgba(0, 0, 0, 0.42); border: 1px solid rgba(var(--popup-fg-rgb, 255, 255, 255), 0.30);
      font-size: calc(14px * var(--text-scale, 1));
    }
    .sph-carte.survol { border-color: rgba(var(--popup-fg-rgb, 255, 255, 255), 0.65); }
    .sph-carte.choisie {
      border: 2px solid rgb(var(--popup-accent-solid-rgb, 96, 160, 255));
      box-shadow: 0 0 22px rgba(var(--popup-accent-solid-rgb, 96, 160, 255), 0.55);
    }
    .sph-tete { flex: 0 0 auto; display: flex; flex-direction: column; gap: 3px; min-width: 0; }
    .sph-ligne1 { display: flex; align-items: center; gap: 7px; min-width: 0; }
    .sph-sorte {
      flex: 0 0 auto; font-size: 0.72em; font-weight: 700; letter-spacing: .06em; padding: 1px 6px; border-radius: 4px;
      background: rgba(var(--popup-fg-rgb, 255, 255, 255), 0.30);
    }
    .sph-sorte.ssh { background: rgb(var(--popup-accent-solid-rgb, 96, 160, 255)); }
    .sph-titre { flex: 1 1 auto; min-width: 0; font-weight: 700; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .sph-badge { flex: 0 0 auto; font-size: 0.7em; padding: 1px 6px; border-radius: 4px; background: rgba(0, 0, 0, 0.45); border: 1px solid rgba(var(--popup-fg-rgb, 255, 255, 255), 0.30); }
    .sph-etat { flex: 0 0 auto; width: 8px; height: 8px; border-radius: 50%; background: #5c5; }
    .sph-etat.connexion, .sph-etat.authentification { background: #db3; }
    .sph-etat.ferme { background: #d44; }
    .sph-info { font-size: 0.82em; opacity: 0.85; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .sph-info .ico { margin-right: 4px; }
    .sph-fenetre { position: relative; flex: 1 1 auto; overflow: hidden; border-radius: 6px; }
    .sph-echelle { position: absolute; left: 0; top: 0; transform-origin: 0 0; }
    .sph-echelle > .sph-dedans { position: absolute !important; inset: 0 !important; width: 100% !important; height: 100% !important; }
    .sph-apercu {
      flex: 1 1 auto; padding: 6px 8px; border-radius: 6px; overflow: hidden; background: rgba(0, 0, 0, 0.40);
      font-family: "Cascadia Mono", Consolas, monospace; font-size: 0.66em; line-height: 1.3;
      white-space: pre; display: flex; flex-direction: column; justify-content: flex-end;
    }
    .sph-apercu.fichiers { justify-content: flex-start; font-family: inherit; font-size: 0.78em; }
    .sph-apercu .vide { opacity: 0.6; font-style: italic; }
    #sph-vide { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; text-align: center; opacity: 0.8; }
    #sph-pied {
      flex: 0 0 auto; display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 10px;
      padding: 6px 12px 8px; font-size: 12px; zoom: var(--text-scale, 1);
    }
    #sph-aide { opacity: 0.8; }
    .sph-segments { display: inline-flex; border-radius: var(--nd-rayon-controle, 6px); overflow: hidden; border: 1px solid rgba(var(--popup-fg-rgb, 255, 255, 255), 0.30); }
    .sph-segments button { color: inherit; font: inherit; border: none; border-radius: 0; padding: 4px 10px; cursor: pointer; background: rgba(0, 0, 0, 0.22); }
    .sph-segments button + button { border-left: 1px solid rgba(var(--popup-fg-rgb, 255, 255, 255), 0.30); }
    .sph-segments button.actif { background: rgb(var(--popup-accent-solid-rgb, 96, 160, 255)); }
  `;

  let o = null;               // les options de la page
  let ouverte = false;
  let racine, scene, monde, vide, segments;
  const cartes = new Map();   // clé → { el, vivante, onglet, element?, echelle?, theta, phi, empreinte }
  let ordre = [];
  let choisie = null;
  let survolee = null;
  let rangs = 1, colonnes = 1;
  let autres = [];            // les vignettes de l'autre panneau
  let raccourci = "molette";
  let reel = { w: 800, h: 500 };
  let minuterie = 0;

  const cam = { yaw: 0, pitch: 0, zoom: 0, vYaw: 0, vPitch: 0 };
  let zoomUtilisateur = 0;    // la distance COURANTE de la caméra
  let vitesseZoom = 1;        // réglée dans Réglages (100 % = 1)
  // Les réglages de la sphère (Réglages → « Vue sphère des terminaux ») —
  // mêmes possibilités que la sphère des programmes, voir `cef_sphere`.
  let zoomPourcent = 100;     // la distance d'ouverture, en % (100 = à la surface)
  let zoomAutoActif = false;  // reculer juste assez pour tout faire tenir
  let fonduApparition = 0, fonduDisparition = 0;  // ms ; 0 = aucun fondu
  let cible = null;
  let rayon = 900;
  let largeur = 320, hauteur = 220, hauteurFenetre = 160;
  let depart = false;
  let glisse = null;

  const echelleTexte = () => parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--text-scale")) || 1;
  const cleVivante = (onglet) => "v:" + onglet;
  const cleAutre = (onglet) => "a:" + onglet;

  // ---- La géométrie ----
  function dimensions() {
    const w = scene.clientWidth || window.innerWidth;
    const h = scene.clientHeight || window.innerHeight;
    const avant = rayon;
    rayon = Math.max(w, h) * 0.85;
    zoomUtilisateur *= rayon / avant;
    largeur = Math.max(260, Math.round(w * PART_DE_LA_LARGEUR));
    hauteurFenetre = Math.round((largeur - 24) * reel.h / Math.max(1, reel.w));
    hauteur = hauteurFenetre + Math.round(TETE * echelleTexte()) + 26;
    scene.style.perspective = rayon + "px";
  }
  const pasYaw = () => 2 * Math.atan((largeur / 2 + ECART / 2) / rayon) / DEG;
  const pasPitch = () => 2 * Math.atan((hauteur / 2 + ECART / 2) / rayon) / DEG;

  function disposer() {
    const n = ordre.length;
    const parTour = Math.max(1, Math.floor(360 / pasYaw()));
    colonnes = Math.max(1, n <= 5 ? n : Math.min(parTour, Math.ceil(Math.sqrt(n * 2.2))));
    rangs = Math.max(1, Math.ceil(n / colonnes));
    ordre.forEach((cle, i) => {
      const rang = Math.floor(i / colonnes);
      const dansLeRang = rang === rangs - 1 ? n - rang * colonnes : colonnes;
      const c = cartes.get(cle);
      c.theta = (i % colonnes - (dansLeRang - 1) / 2) * pasYaw();
      c.phi = (rang - (rangs - 1) / 2) * pasPitch();
      Object.assign(c.el.style, {
        width: largeur + "px", height: hauteur + "px", left: -largeur / 2 + "px", top: -hauteur / 2 + "px",
        transform: `rotateY(${-c.theta}deg) rotateX(${c.phi}deg) translateZ(${-rayon}px)`,
      });
      if (c.echelle) {
        const k = Math.min((largeur - 24) / reel.w, hauteurFenetre / reel.h);
        Object.assign(c.echelle.style, { width: reel.w + "px", height: reel.h + "px", transform: `scale(${k})` });
      }
    });
    poserLeCiel();
  }

  // Des points fixes sur toute la sphère : sans eux, tourner dans le vide ne se voit pas.
  let ciel = [];
  function poserLeCiel() {
    ciel.forEach((p) => p.el.remove());
    ciel = [];
    const N = 180;
    for (let i = 0; i < N; i++) {
      const theta = (i * 137.508) % 360;
      const phi = Math.asin(1 - (2 * (i + 0.5)) / N) / DEG;
      const el = document.createElement("div");
      el.className = "sph-ciel";
      el.style.transform = `rotateY(${-theta}deg) rotateX(${phi}deg) translateZ(${-rayon * 1.04}px)`;
      monde.appendChild(el);
      ciel.push({ el, theta, phi });
    }
  }

  // ---- Le contenu des cartes ----
  function ligneIco(ico, texte) {
    const d = document.createElement("div");
    d.className = "sph-info";
    const i = document.createElement("span");
    i.className = "ico ico-" + ico;
    d.append(i, document.createTextNode(texte));
    return d;
  }

  function tete(v, vivante) {
    const t = document.createElement("div");
    t.className = "sph-tete";
    const l1 = document.createElement("div");
    l1.className = "sph-ligne1";
    const sorte = document.createElement("span");
    sorte.className = "sph-sorte" + (v.source === "ssh" ? " ssh" : "");
    sorte.textContent = v.source === "ssh" ? (v.mode === "fichiers" ? "SFTP" : "SSH") : v.admin ? "ADMIN" : "TERM";
    const titre = document.createElement("span");
    titre.className = "sph-titre";
    titre.textContent = v.titre || "(sans titre)";
    l1.append(sorte, titre);
    if (!vivante) {
      const b = document.createElement("span");
      b.className = "sph-badge";
      b.textContent = "aperçu · clic : y aller";
      l1.appendChild(b);
    }
    if (v.source === "ssh") {
      const e = document.createElement("span");
      e.className = "sph-etat " + (v.etat || "");
      l1.appendChild(e);
    }
    t.append(l1, ligneIco(v.source === "ssh" ? "globe" : "terminal-window", (v.source === "ssh" ? v.hote : v.sorte) || ""));
    t.appendChild(ligneIco("folder", v.dossier || "dossier inconnu"));
    return t;
  }

  function apercu(v) {
    const a = document.createElement("div");
    if (v.mode === "fichiers") {
      a.className = "sph-apercu fichiers";
      for (const e of v.entrees || []) a.appendChild(ligneIco(e.dossier ? "folder" : "file", e.nom));
    } else {
      a.className = "sph-apercu";
      for (const l of v.lignes || []) {
        const d = document.createElement("div");
        d.textContent = l || " ";
        a.appendChild(d);
      }
    }
    if (!a.childElementCount) {
      const d = document.createElement("div");
      d.className = "vide";
      d.textContent = "rien à montrer pour l'instant";
      a.appendChild(d);
    }
    return a;
  }

  function nouvelleCarte(cle, vivante, onglet) {
    const el = document.createElement("div");
    el.className = "sph-carte" + (vivante ? " vivante" : "");
    monde.appendChild(el);
    const c = { el, vivante, onglet, theta: 0, phi: 0, empreinte: "", teteEl: null, element: null, echelle: null };
    if (vivante) {
      c.teteEl = document.createElement("div");
      const fenetre = document.createElement("div");
      fenetre.className = "sph-fenetre";
      c.echelle = document.createElement("div");
      c.echelle.className = "sph-echelle";
      fenetre.appendChild(c.echelle);
      el.append(c.teteEl, fenetre);
    }
    cartes.set(cle, c);
    return c;
  }

  // L'élément d'une session entre dans sa carte : à sa taille réelle, visible
  // même si la page le marque caché (onglet inactif).
  function accueillir(c, element) {
    if (c.element !== element || element.parentNode !== c.echelle) {
      c.element = element;
      element.classList.add("sph-dedans");
      element.style.setProperty("display", o.affichage, "important");
      c.echelle.appendChild(element);
    }
  }

  function rendre(element) {
    element.classList.remove("sph-dedans");
    element.style.removeProperty("display");
  }

  // Les cartes suivent les sessions : une session neuve entre, une fermée sort.
  // Rend `true` si l'ensemble a changé.
  function synchroniser() {
    const descriptions = new Map(o.decrire().map((v) => [v.onglet, v]));
    const vues = new Set();
    const nouvelOrdre = [];
    for (const { onglet, element } of o.vivantes()) {
      const cle = cleVivante(onglet);
      vues.add(cle);
      nouvelOrdre.push(cle);
      const c = cartes.get(cle) || nouvelleCarte(cle, true, onglet);
      accueillir(c, element);
      const v = descriptions.get(onglet) || { source: o.source, titre: "" };
      const empreinte = JSON.stringify([v.titre, v.sorte, v.admin, v.hote, v.mode, v.etat, v.dossier]);
      if (empreinte !== c.empreinte) {
        c.empreinte = empreinte;
        const t = tete(v, true);
        c.teteEl.replaceWith(t);
        c.teteEl = t;
      }
    }
    for (const v of autres) {
      const cle = cleAutre(v.onglet);
      vues.add(cle);
      nouvelOrdre.push(cle);
      const c = cartes.get(cle) || nouvelleCarte(cle, false, v.onglet);
      const empreinte = JSON.stringify(v);
      if (empreinte !== c.empreinte) {
        c.empreinte = empreinte;
        c.el.replaceChildren(tete(v, false), apercu(v));
      }
    }
    for (const [cle, c] of cartes) {
      if (vues.has(cle)) continue;
      if (c.element) rendre(c.element);
      c.el.remove();
      cartes.delete(cle);
    }
    const change = nouvelOrdre.join("|") !== ordre.join("|");
    ordre = nouvelOrdre;
    vide.style.display = ordre.length ? "none" : "";
    if (!cartes.has(choisie)) choisir(ordre[0] || null, false);
    if (window.sphereSource) sphereSource.rafraichir();
    return change;
  }

  // ---- La caméra ----
  // La position d'une carte vue de la caméra : z < 0 = devant.
  function versLaCamera(theta, phi, r) {
    let x = 0, y = r * Math.sin(phi * DEG), z = -r * Math.cos(phi * DEG);
    const a = -theta * DEG;
    [x, z] = [x * Math.cos(a) + z * Math.sin(a), -x * Math.sin(a) + z * Math.cos(a)];
    const b = cam.yaw * DEG;
    [x, z] = [x * Math.cos(b) + z * Math.sin(b), -x * Math.sin(b) + z * Math.cos(b)];
    const p = -cam.pitch * DEG;
    [y, z] = [y * Math.cos(p) - z * Math.sin(p), y * Math.sin(p) + z * Math.cos(p)];
    return { x, y, z };
  }
  const devant = (theta, phi) => versLaCamera(theta, phi, rayon).z + cam.zoom < -rayon * 0.2;

  // ---- Le zoom par défaut et le recadrage automatique ----
  // Le déplacement de caméra du réglage « Zoom par défaut », dans l'unité de
  // `zoomUtilisateur` — borné aux limites de la molette.
  function zoomParDefaut() {
    const part = Math.max(-0.7, Math.min(0.5, ((zoomPourcent - 100) / 100) * 0.5));
    return part * rayon;
  }

  // Le centre écran et l'échelle d'une carte pour une distance DONNÉE. La
  // formule est celle de la perspective CSS (`scene.style.perspective = rayon`,
  // monde translaté de `rayon + zoom`) : échelle = rayon / profondeur.
  function projeterAvec(theta, phi, zoom) {
    const p = versLaCamera(theta, phi, rayon);
    const profondeur = -(p.z + zoom);
    if (profondeur < rayon * 0.2) return null;
    const s = rayon / profondeur;
    return { x: scene.clientWidth / 2 + p.x * s, y: scene.clientHeight / 2 + p.y * s, s, profondeur };
  }

  // La distance de caméra qui fait tenir TOUTES les cartes dans la scène.
  // Recherche BINAIRE, comme la sphère des programmes : la perspective n'est
  // pas une homothétie, une formule fermée dépendrait de chaque carte.
  function cadrageAuto() {
    const marge = 12;
    const W = scene.clientWidth || window.innerWidth, H = scene.clientHeight || window.innerHeight;
    const tient = (zoom) => {
      for (const c of cartes.values()) {
        const p = projeterAvec(c.theta, c.phi, zoom);
        if (!p) return false;
        if (p.x - largeur * p.s / 2 < marge || p.x + largeur * p.s / 2 > W - marge) return false;
        if (p.y - hauteur * p.s / 2 < marge || p.y + hauteur * p.s / 2 > H - marge) return false;
      }
      return true;
    };
    const loinMax = -rayon * 6, presMax = rayon * ZOOM_AVANT;
    if (!tient(loinMax)) return loinMax;   // même de très loin : impossible
    if (tient(presMax)) return presMax;    // tient déjà au plus près
    let loin = loinMax, pres = presMax;
    for (let i = 0; i < 60; i++) {
      const milieu = (loin + pres) / 2;
      if (tient(milieu)) loin = milieu; else pres = milieu;
    }
    return loin;
  }

  // La scène a changé de taille ou de contenu : en zoom automatique, on
  // recadre SANS animation — conséquence du réglage, pas geste de l'utilisateur.
  function recadrerSiAuto() {
    if (!zoomAutoActif || !cartes.size) return;
    zoomUtilisateur = Math.min(zoomParDefaut(), cadrageAuto());
    cam.zoom = zoomUtilisateur;
  }

  function bornerPitch(p) {
    const haut = Math.min(PITCH_MAX, (rangs - 1) / 2 * pasPitch() + 20);
    return Math.max(-haut, Math.min(haut, p));
  }

  function peindre() {
    monde.style.transform = `translateZ(${rayon + cam.zoom}px) rotateX(${-cam.pitch}deg) rotateY(${cam.yaw}deg)`;
    // Ce qui passe derrière la caméra ne se dessine pas : le rendu 3D d'un
    // élément qui traverse le plan de l'œil est faux.
    for (const c of cartes.values()) c.el.style.visibility = devant(c.theta, c.phi) ? "visible" : "hidden";
    for (const p of ciel) p.el.style.visibility = devant(p.theta, p.phi) ? "visible" : "hidden";
  }

  let boucle = 0;
  const animer = () => { if (!boucle) boucle = requestAnimationFrame(pas); };
  const adoucir = (t) => 1 - Math.pow(1 - t, 3);
  function pas(maintenant) {
    boucle = 0;
    let encore = false;
    if (cible) {
      const t = Math.min(1, (maintenant - cible.debut) / cible.duree);
      const k = adoucir(t);
      cam.yaw = cible.depuis.yaw + (cible.yaw - cible.depuis.yaw) * k;
      cam.pitch = cible.depuis.pitch + (cible.pitch - cible.depuis.pitch) * k;
      cam.zoom = cible.depuis.zoom + (cible.zoom - cible.depuis.zoom) * k;
      if (t < 1) encore = true;
      else { const fin = cible.fin; cible = null; if (fin) fin(); }
    } else if (!glisse && (Math.abs(cam.vYaw) > 0.01 || Math.abs(cam.vPitch) > 0.01)) {
      cam.yaw += cam.vYaw;
      cam.pitch = bornerPitch(cam.pitch + cam.vPitch);
      cam.vYaw *= 0.92;
      cam.vPitch *= 0.92;
      encore = true;
    }
    if (ouverte) peindre();
    if (encore) animer();
  }

  function viser(cle, zoom, duree, fin) {
    const c = cartes.get(cle);
    if (!c) { if (fin) fin(); return; }
    const yaw = cam.yaw + ((((c.theta - cam.yaw) % 360) + 540) % 360 - 180);
    cam.vYaw = cam.vPitch = 0;
    cible = { yaw, pitch: c.phi, zoom, duree, debut: performance.now(), depuis: { ...cam }, fin };
    animer();
  }

  function choisir(cle, centrer = true) {
    if (choisie && cartes.has(choisie)) cartes.get(choisie).el.classList.remove("choisie");
    choisie = cle;
    const c = cle && cartes.get(cle);
    if (!c) return;
    c.el.classList.add("choisie");
    if (centrer) viser(cle, zoomUtilisateur, 320);
    // Le clavier va au terminal choisi : on tape dedans sans quitter la sphère.
    if (c.vivante) o.focaliser(c.onglet);
  }

  // ---- Ouvrir, fermer ----
  function ouvrir(onglet) {
    if (!o) return;
    if (ouverte) { choisir(cleVivante(onglet)); return; }
    reel = o.taille();
    ouverte = true;
    racine.style.top = Math.round(o.haut.getBoundingClientRect().bottom) + "px";
    apparaitre();
    o.milieu.style.visibility = "hidden";
    dimensions();
    synchroniser();
    disposer();
    renderRaccourcis();
    const cle = cartes.has(cleVivante(onglet)) ? cleVivante(onglet) : ordre[0];
    choisir(cle || null, false);
    // L'ARRIVÉE : la caméra part tout près de la carte d'où l'on vient, et recule.
    const c = cle && cartes.get(cle);
    cam.vYaw = cam.vPitch = 0;
    if (c) { cam.yaw = c.theta; cam.pitch = c.phi; }
    // ⭐ Le zoom par défaut (Réglages), ou le recadrage automatique — qui ne
    // fait que RECULER par rapport à lui, jamais avancer.
    zoomUtilisateur = zoomAutoActif && cartes.size
      ? Math.min(zoomParDefaut(), cadrageAuto())
      : zoomParDefaut();
    cam.zoom = Math.max(zoomUtilisateur, 0) + rayon * 0.55;
    cible = { yaw: cam.yaw, pitch: cam.pitch, zoom: zoomUtilisateur, duree: DUREE_ARRIVEE, debut: performance.now(), depuis: { ...cam } };
    peindre();
    animer();
    clearInterval(minuterie);
    minuterie = setInterval(() => {
      if (synchroniser()) { disposer(); recadrerSiAuto(); }
      peindre();
    }, 1000);
    o.go("sphere-etat", "oui");
    if (window.sphereSource) sphereSource.rafraichir();
  }

  // ---- Le fondu du panneau (Réglages) ----
  // 0 ms = le comportement d'avant ce réglage : la sphère apparaît d'un coup.
  // ⛔ Une transition ne part pas d'un élément qui vient de `display: none` :
  // il faut lire le style calculé (le `offsetWidth` ci-dessous) entre les deux.
  function apparaitre() {
    racine.classList.remove("cache");
    racine.style.transition = "none";
    if (fonduApparition <= 0) { racine.style.opacity = ""; return; }
    racine.style.opacity = "0";
    void racine.offsetWidth;
    racine.style.transition = `opacity ${fonduApparition}ms ease`;
    racine.style.opacity = "1";
  }

  function disparaitre(fin) {
    racine.style.transition = "none";
    if (fonduDisparition <= 0) { racine.style.opacity = ""; fin(); return; }
    racine.style.transition = `opacity ${fonduDisparition}ms ease`;
    racine.style.opacity = "0";
    let fait = false;
    const une = (e) => {
      // `transitionend` remonte aussi des enfants : seule la nôtre compte.
      if (e && e.target !== racine) return;
      if (fait) return;
      fait = true;
      racine.removeEventListener("transitionend", une);
      racine.style.transition = "none";
      racine.style.opacity = "";
      fin();
    };
    racine.addEventListener("transitionend", une);
    // Filet : une transition coupée ne doit ni laisser la sphère à moitié
    // éteinte, ni garder les terminaux à l'intérieur.
    setTimeout(une, fonduDisparition + 80);
  }

  // Rend chaque session à sa page, qui la remet à sa place (`sortir`).
  function fermer(onglet, instantane = false) {
    if (!ouverte || depart) return;
    const finir = () => {
      depart = false;
      ouverte = false;
      clearInterval(minuterie);
      for (const c of cartes.values()) {
        if (c.element) rendre(c.element);
        c.el.remove();
      }
      cartes.clear();
      ordre = [];
      choisie = survolee = null;
      ciel.forEach((p) => p.el.remove());
      ciel = [];
      racine.classList.add("cache");
      o.milieu.style.visibility = "";
      o.sortir(onglet);
      o.go("sphere-etat", "non");
      if (window.sphereSource) sphereSource.rafraichir();
    };
    const cle = cleVivante(onglet);
    if (instantane || !cartes.has(cle)) { finir(); return; }
    depart = true;
    choisir(cle, false);
    viser(cle, Math.max(zoomUtilisateur, 0) + rayon * 0.55, DUREE_DEPART, () => disparaitre(finir));
  }

  // Une carte de l'AUTRE panneau : on y va, dans sa sphère.
  function allerVers(c) {
    fermer(o.actif(), true);
    o.basculer(c.onglet);
  }

  // Le geste « ouvrir » (double-clic, clic molette) sur une carte.
  function activerCarte(cle) {
    const c = cartes.get(cle);
    if (!c) return;
    if (c.vivante) fermer(c.onglet);
    else allerVers(c);
  }

  // ---- Les gestes ----
  // La carte sous (x, y) — voir l'en-tête : parmi les cartes visibles dont le
  // rectangle projeté contient le point, la plus proche de la caméra.
  function carteSous(x, y) {
    let meilleure = null, plusProche = Infinity;
    for (const [cle, c] of cartes) {
      if (c.el.style.visibility === "hidden") continue;
      const r = c.el.getBoundingClientRect();
      if (x < r.left || x > r.right || y < r.top || y > r.bottom) continue;
      const distance = -(versLaCamera(c.theta, c.phi, rayon).z + cam.zoom);
      if (distance < plusProche) { plusProche = distance; meilleure = cle; }
    }
    return meilleure;
  }

  function survoler(cle) {
    if (cle === survolee) return;
    if (survolee && cartes.has(survolee)) cartes.get(survolee).el.classList.remove("survol");
    survolee = cle;
    if (cle && cartes.has(cle)) cartes.get(cle).el.classList.add("survol");
  }

  // ---- LA SOURIS DANS LE TERMINAL CHOISI ----
  // DEMANDE (2026-11-01) : « sélectionner du texte et interagir pour le copier
  // ou le coller à la souris ». Chromium ne trouve pas les cartes sous le
  // pointeur (voir l'en-tête), et xterm, à travers le `scale`, viserait la
  // mauvaise cellule : il calcule `clientX - rect.left` (rectangle RÉDUIT) puis
  // divise par la largeur d'une cellule (NON réduite). La sphère prend donc
  // l'événement réel et en RELAIE une copie à l'élément visé, les coordonnées
  // ramenées à l'échelle 1 autour du coin de l'écran d'xterm.
  //
  // Seulement pour la carte CHOISIE, vivante, EN FACE de la caméra (sa
  // projection n'est alors qu'une mise à l'échelle) : ailleurs, un clic choisit
  // et centre, un glisser tourne.
  const ALIGNE = 4;           // degrés d'écart tolérés avec l'axe de la caméra
  let pointe = null;          // { cible, base, s } de l'appui au relâcher
  let relache = 0;            // l'instant du dernier relâcher relayé

  const ecartAngle = (a, b) => Math.abs((((a - b) % 360) + 540) % 360 - 180);

  // La carte choisie si (x, y) tombe dans la fenêtre de son terminal.
  function terminalSous(x, y) {
    if (!ouverte || depart || cible) return null;
    const c = choisie && cartes.get(choisie);
    if (!c || !c.vivante || !c.element) return null;
    if (ecartAngle(c.theta, cam.yaw) > ALIGNE || Math.abs(c.phi - cam.pitch) > ALIGNE) return null;
    const f = c.echelle.parentNode.getBoundingClientRect();
    if (x < f.left || x > f.right || y < f.top || y > f.bottom) return null;
    return carteSous(x, y) === choisie ? c : null;
  }

  // L'élément le plus profond sous (x, y), par les rectangles projetés — le
  // test de Chromium ne voit pas dans la carte. Les derniers enfants sont dessus.
  function toucher(el, x, y) {
    for (let i = el.children.length - 1; i >= 0; i--) {
      const enfant = el.children[i];
      const r = enfant.getBoundingClientRect();
      if (!r.width || !r.height || x < r.left || x > r.right || y < r.top || y > r.bottom) continue;
      const st = getComputedStyle(enfant);
      if (st.display === "none" || st.visibility === "hidden") continue;
      const dessous = toucher(enfant, x, y);
      if (dessous) return dessous;
      if (st.pointerEvents !== "none") return enfant;
    }
    return null;
  }

  // Le repère des coordonnées : l'écran d'xterm s'il y en a un (c'est sur lui
  // qu'il mesure), sinon l'élément lui-même.
  function baseDe(el) {
    const x = el.closest(".xterm");
    return (x && x.querySelector(".xterm-screen")) || el;
  }

  function echelleDe(c) {
    const r = c.echelle.getBoundingClientRect();
    return { x: r.width / Math.max(1, c.echelle.offsetWidth), y: r.height / Math.max(1, c.echelle.offsetHeight) };
  }

  // `reelles` : garder les coordonnées de l'écran (le menu du clic droit se
  // pose sous le pointeur, pas dans le repère du terminal).
  function relayer(type, e, cibleEl, base, s, reelles = false) {
    let cx = e.clientX, cy = e.clientY;
    if (!reelles) {
      const b = base.getBoundingClientRect();
      cx = b.left + (e.clientX - b.left) / s.x;
      cy = b.top + (e.clientY - b.top) / s.y;
    }
    cibleEl.dispatchEvent(new MouseEvent(type, {
      bubbles: true, cancelable: true, composed: true, view: window, detail: e.detail,
      screenX: e.screenX, screenY: e.screenY, clientX: cx, clientY: cy,
      ctrlKey: e.ctrlKey, shiftKey: e.shiftKey, altKey: e.altKey, metaKey: e.metaKey,
      button: e.button, buttons: e.buttons,
    }));
  }

  const prendre = (e) => { e.preventDefault(); e.stopImmediatePropagation(); };
  const estUnChamp = (el) => el.matches("input, textarea, [contenteditable]") && !el.closest(".xterm");

  // En phase de CAPTURE sur `window` : avant la scène et avant les écouteurs
  // qu'xterm pose sur `document` pendant une sélection. Les copies relayées
  // (`isTrusted` faux) traversent sans être reprises.
  function installerLaSouris() {
    window.addEventListener("mousedown", (e) => {
      // Le clic molette garde son rôle : revenir en mode normal.
      if (!e.isTrusted || e.button === 1 || !scene.contains(e.target)) return;
      const c = terminalSous(e.clientX, e.clientY);
      if (!c) return;
      // `preventDefault` : l'appui réel ne retire pas le clavier au terminal.
      prendre(e);
      const cibleEl = toucher(c.element, e.clientX, e.clientY) || c.element;
      pointe = { cible: cibleEl, base: baseDe(cibleEl), s: echelleDe(c) };
      if (estUnChamp(cibleEl)) cibleEl.focus();
      else o.focaliser(c.onglet);
      relayer("mousedown", e, cibleEl, pointe.base, pointe.s);
    }, true);
    window.addEventListener("mousemove", (e) => {
      if (!e.isTrusted) return;
      if (pointe) {
        prendre(e);
        relayer("mousemove", e, pointe.cible, pointe.base, pointe.s);
        return;
      }
      if (ouverte && !glisse) scene.classList.toggle("texte", !!terminalSous(e.clientX, e.clientY));
    }, true);
    window.addEventListener("mouseup", (e) => {
      if (!e.isTrusted || !pointe) return;
      prendre(e);
      const p = pointe;
      pointe = null;
      relache = performance.now();
      relayer("mouseup", e, p.cible, p.base, p.s);
      // Une copie d'appui ne fabrique pas de `click` : on le relaie (les
      // boutons de l'explorateur SFTP en ont besoin).
      const c = e.button === 0 && terminalSous(e.clientX, e.clientY);
      const sous = c && toucher(c.element, e.clientX, e.clientY);
      if (sous && (sous === p.cible || sous.contains(p.cible) || p.cible.contains(sous))) relayer("click", e, sous, p.base, p.s);
    }, true);
    window.addEventListener("click", (e) => {
      if (e.isTrusted && ouverte && performance.now() - relache < 400 && scene.contains(e.target)) prendre(e);
    }, true);
    // Le double-clic DANS le terminal choisi sélectionne un mot (xterm le lit
    // dans `detail` de l'appui) : il ne ramène pas en mode normal.
    window.addEventListener("dblclick", (e) => {
      if (!e.isTrusted || !scene.contains(e.target)) return;
      const c = terminalSous(e.clientX, e.clientY);
      if (!c) return;
      prendre(e);
      const sous = toucher(c.element, e.clientX, e.clientY) || c.element;
      relayer("dblclick", e, sous, baseDe(sous), echelleDe(c));
    }, true);
    // LE CLIC DROIT : le menu Copier / Coller de la page, sous le pointeur.
    window.addEventListener("contextmenu", (e) => {
      if (!e.isTrusted || !scene.contains(e.target)) return;
      const c = terminalSous(e.clientX, e.clientY);
      if (!c) return;
      prendre(e);
      const sous = toucher(c.element, e.clientX, e.clientY) || c.element;
      relayer("contextmenu", e, sous, sous, echelleDe(c), true);
    }, true);
  }

  function installerLesGestes() {
    installerLaSouris();
    scene.addEventListener("mousedown", (e) => {
      if (!e.isTrusted) return;
      // ⚠️ `preventDefault` partout : un appui sur la scène ne doit pas retirer
      // le clavier au terminal choisi.
      e.preventDefault();
      if (e.button === 1) {
        const cle = carteSous(e.clientX, e.clientY);
        if (cle) activerCarte(cle);
        return;
      }
      if (e.button !== 0) return;
      glisse = { x: e.clientX, y: e.clientY, x0: e.clientX, y0: e.clientY, carte: carteSous(e.clientX, e.clientY) };
      cible = null;
      cam.vYaw = cam.vPitch = 0;
      scene.classList.add("glisse");
    });
    scene.addEventListener("auxclick", (e) => { if (e.isTrusted && e.button === 1) e.preventDefault(); });
    window.addEventListener("mousemove", (e) => {
      if (!ouverte || !e.isTrusted) return;
      if (!glisse) { survoler(carteSous(e.clientX, e.clientY)); return; }
      const k = 1 / (rayon * DEG);
      const dx = e.clientX - glisse.x, dy = e.clientY - glisse.y;
      glisse.x = e.clientX;
      glisse.y = e.clientY;
      cam.vYaw = -dx * k;
      cam.vPitch = -dy * k;
      cam.yaw += cam.vYaw;
      cam.pitch = bornerPitch(cam.pitch + cam.vPitch);
      peindre();
    });
    window.addEventListener("mouseup", (e) => {
      if (!glisse || !e.isTrusted || e.button !== 0) return;
      const immobile = Math.hypot(e.clientX - glisse.x0, e.clientY - glisse.y0) < 5;
      const cle = glisse.carte;
      glisse = null;
      scene.classList.remove("glisse");
      if (!immobile) { animer(); return; }
      cam.vYaw = cam.vPitch = 0;
      const c = cle && cartes.get(cle);
      if (!c) return;
      if (c.vivante) choisir(cle);
      else allerVers(c);
    });
    scene.addEventListener("dblclick", (e) => {
      if (!e.isTrusted) return;
      const cle = carteSous(e.clientX, e.clientY);
      if (cle) activerCarte(cle);
    });
    // LA ROULETTE ZOOME : vers le haut, la caméra avance.
    scene.addEventListener("wheel", (e) => {
      e.preventDefault();
      if (depart) return;
      cible = null;
      // Le recadrage automatique peut poser la caméra PLUS LOIN que la limite
      // de la molette : on garde cette distance comme fond, sinon le premier
      // cran ferait sauter la vue (voir la sphère des programmes).
      const fond = Math.min(-rayon * ZOOM_ARRIERE, zoomUtilisateur);
      zoomUtilisateur = Math.max(fond, Math.min(rayon * ZOOM_AVANT, zoomUtilisateur - e.deltaY * rayon / 1500 * vitesseZoom));
      cam.zoom = zoomUtilisateur;
      peindre();
    }, { passive: false });
    // ÉCHAP, en phase de CAPTURE : il ne doit pas atteindre le terminal choisi.
    // C'est la SEULE touche prise à la sphère — Entrée, les flèches… vont au
    // terminal. Rien n'est écouté quand la sphère est fermée.
    document.addEventListener("keydown", (e) => {
      if (!ouverte || e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      const c = choisie && cartes.get(choisie);
      fermer(c && c.vivante ? c.onglet : o.actif());
    }, true);
    new ResizeObserver(() => {
      if (!ouverte) return;
      dimensions();
      disposer();
      recadrerSiAuto();
      peindre();
    }).observe(scene);
  }

  function renderRaccourcis() {
    segments.replaceChildren(...RACCOURCIS.map(([valeur, texte]) => {
      const b = document.createElement("button");
      b.type = "button";
      b.textContent = texte;
      b.className = raccourci === valeur ? "actif" : "";
      b.addEventListener("mousedown", (e) => e.stopPropagation());
      b.addEventListener("click", () => o.go("raccourci", valeur));
      return b;
    }));
  }

  window.sphereVue = {
    // Options : `source`, `go`, `haut` (la barre), `milieu` (caché pendant la
    // sphère), `affichage` ("block" | "flex" : l'affichage d'un élément de
    // session), `taille()` (sa taille réelle), `vivantes()` → [{ onglet,
    // element }], `decrire()` (les vignettes de la page, pour les en-têtes),
    // `actif()`, `focaliser(onglet)`, `sortir(onglet)` (remettre la page en
    // mode normal sur cet onglet), `basculer(onglet)` (aller à l'autre panneau).
    installer(options) {
      o = options;
      const style = document.createElement("style");
      style.textContent = STYLE;
      document.head.appendChild(style);
      racine = document.createElement("div");
      racine.id = "sph-racine";
      racine.className = "cache";
      scene = document.createElement("div");
      scene.id = "sph-scene";
      monde = document.createElement("div");
      monde.id = "sph-monde";
      vide = document.createElement("div");
      vide.id = "sph-vide";
      vide.textContent = "Aucune session ouverte — Échap : revenir.";
      scene.append(monde, vide);
      const pied = document.createElement("div");
      pied.id = "sph-pied";
      const aide = document.createElement("span");
      aide.id = "sph-aide";
      aide.textContent = "Glisser : tourner · Molette : zoomer · Clic : choisir · Dans le terminal choisi : sélectionner, clic droit pour copier / coller · Clic molette ou double-clic sur l'en-tête : mode normal · Échap : mode normal";
      const reglage = document.createElement("span");
      reglage.textContent = "Ouvrir la sphère : ";
      segments = document.createElement("span");
      segments.className = "sph-segments";
      reglage.appendChild(segments);
      pied.append(aide, reglage);
      racine.append(scene, pied);
      document.body.appendChild(racine);
      installerLesGestes();
    },
    ouvrir,
    fermer,
    ouverte: () => ouverte,
    // La page a refait sa disposition (onglet ouvert, fermé…) : les éléments
    // qu'elle a remis dans `#corps` reviennent dans leurs cartes.
    resynchroniser() {
      if (ouverte && synchroniser()) { disposer(); recadrerSiAuto(); }
      if (ouverte) peindre();
    },
    // Les vignettes de l'autre panneau (relayées par Rust).
    autres(liste) {
      autres = Array.isArray(liste) ? liste : [];
      if (ouverte && synchroniser()) { disposer(); recadrerSiAuto(); }
      if (ouverte) peindre();
    },
    // La vitesse du zoom à la molette, en % (Réglages → « Vue sphère des terminaux »).
    vitesseZoom(pourcent) {
      const v = Number(pourcent);
      if (Number.isFinite(v) && v > 0) vitesseZoom = v / 100;
    },
    // Les réglages PROPRES à la sphère (mêmes possibilités que la sphère des
    // programmes) : distance d'ouverture, recadrage automatique, fondu.
    reglages(etat) {
      if (!etat) return;
      const pourcent = Number(etat.sphereZoomDefaut);
      if (Number.isFinite(pourcent) && pourcent > 0) zoomPourcent = pourcent;
      const autoAvant = zoomAutoActif;
      zoomAutoActif = !!etat.sphereZoomAuto;
      const apparition = Number(etat.sphereFonduOuverture);
      const disparition = Number(etat.sphereFonduFermeture);
      if (Number.isFinite(apparition) && apparition >= 0) fonduApparition = apparition;
      if (Number.isFinite(disparition) && disparition >= 0) fonduDisparition = disparition;
      // La case vient d'être cochée pendant que la sphère est ouverte : on
      // recadre tout de suite. Un curseur, lui, n'y touche pas — il s'applique
      // à la prochaine ouverture (et ne doit pas défaire un zoom à la molette).
      if (ouverte && zoomAutoActif && !autoAvant) { recadrerSiAuto(); peindre(); }
    },
    raccourci(valeur) {
      raccourci = valeur;
      if (segments) renderRaccourcis();
    },
    libelle() {
      const n = ordre.length;
      return "Vue sphère · " + n + (n > 1 ? " sessions" : " session");
    },
  };
})();
