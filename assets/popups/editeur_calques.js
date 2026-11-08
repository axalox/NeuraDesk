"use strict";
// editeur_calques.js — LES CALQUES de l'éditeur d'image. DEMANDE EXPLICITE
// (2026-09-25) : « je voudrais pouvoir gérer les calques comme dans gimp, et oui
// il faut pouvoir rouvrir les fichiers » — d'où aussi l'enregistrement en PSD et
// en XCF (`win32::format_psd`, `win32::format_xcf`), qui gardent les calques.
//
// UN CALQUE : `{ nom, toile, x, y, opacite, visible, mode, objets }`. `toile` est
// un canevas JAMAIS modifié en place (comme l'ancien `fond`) : l'historique le
// garde par référence. `x`, `y` : sa place dans l'image — il peut déborder.
// `objets` : ses annotations (editeur_annotations.js), en pixels de l'IMAGE ;
// elles bougent avec lui. La pile va du BAS vers le HAUT.
//
// Comme dans GIMP, ce qui retravaille des pixels ne touche que le calque ACTIF :
// annotations, masques (flou, pixeliser… relisent le calque, pas l'image),
// réglages de couleur. La GÉOMÉTRIE, elle, s'applique à toute l'image — donc à
// tous les calques (`transformerDocument`).
//
// Les MODES sont ceux que canevas, GIMP et Photoshop partagent : le nom est
// celui de `globalCompositeOperation` (sauf « normal », `source-over`), et
// Rust le traduit dans chaque format (`document_image::Mode`).

const MODES_CALQUE = [
  ["normal", "Normal"],
  ["multiply", "Multiplier"],
  ["screen", "Écran"],
  ["overlay", "Superposer"],
  ["darken", "Assombrir seulement"],
  ["lighten", "Éclaircir seulement"],
  ["color-dodge", "Éclaircir"],
  ["color-burn", "Assombrir"],
  ["hard-light", "Lumière dure"],
  ["soft-light", "Lumière douce"],
  ["difference", "Différence"],
  ["exclusion", "Exclusion"],
  ["hue", "Teinte"],
  ["saturation", "Saturation"],
  ["color", "Couleur"],
  ["luminosity", "Luminosité"],
];
const NOM_DU_MODE = Object.fromEntries(MODES_CALQUE);
const operationDuMode = (mode) => (mode !== "normal" && NOM_DU_MODE[mode] ? mode : "source-over");

// L'outil qui déplace le calque actif (la touche M de GIMP est prise par la loupe).
const OUTILS_CALQUES = [["calque", "✥", "d", "Déplacer le calque actif"]];

function nouveauCalque(nom, toile, x = 0, y = 0) {
  return { nom, toile, x, y, opacite: 1, visible: true, mode: "normal", objets: [] };
}

function cadreDuCalque(k) {
  return { x: k.x, y: k.y, l: k.toile.width, h: k.toile.height };
}

function union(a, b) {
  const x = Math.min(a.x, b.x), y = Math.min(a.y, b.y);
  return { x, y, l: Math.max(a.x + a.l, b.x + b.l) - x, h: Math.max(a.y + a.h, b.y + b.h) - y };
}

// ── Rendu ─────────────────────────────────────────────────────────────────
// Peint le calque `k` sur `c`, décalé de (-ox, -oy) : sa toile, puis ses objets.
// `options` : `filtre` (l'aperçu des couleurs), `trace` (l'objet en cours de
// tracé), `cache` (l'index de l'objet que le champ de texte remplace).
function peindreCalque(c, k, ox, oy, options = {}) {
  c.save();
  c.imageSmoothingEnabled = false;
  c.filter = options.filtre || "none";
  c.drawImage(k.toile, k.x - ox, k.y - oy);
  c.restore();
  const decale = (o) => (ox || oy ? deplacer(o, -ox, -oy) : o);
  const tous = options.trace ? [...k.objets, options.trace] : k.objets;
  let numero = 0;
  tous.forEach((o, i) => {
    const n = o.type === "etape" ? ++numero : 0;
    if (options.cache === i) return;
    dessinerObjet(c, decale(o), n);
  });
  dessinerProjecteurs(c, tous.filter((o) => o.type === "projecteur").map(decale));
}

// Un calque SIMPLE (sans objet ni aperçu) se pose tel quel ; les autres sont
// peints sur un canevas à la taille de l'image — gardé par calque, puisqu'un
// calque ne change jamais (`RENDUS`), sauf s'il porte un geste en cours.
const RENDUS = new WeakMap();
const travail = nouvelleToile(1, 1);

function sourceDuCalque(k, L, H, options = {}) {
  const vivant = options.filtre && options.filtre !== "none" || options.trace || options.cache !== undefined;
  if (!k.objets.length && !vivant) return { source: k.toile, x: k.x, y: k.y };
  const garde = !vivant && RENDUS.get(k);
  if (garde && garde.width === L && garde.height === H) return { source: garde, x: 0, y: 0 };
  const t = vivant ? travail : nouvelleToile(L, H);
  if (vivant) {
    t.width = L;
    t.height = H;
  }
  peindreCalque(t.getContext("2d", { colorSpace: "srgb" }), k, 0, 0, options);
  if (!vivant) RENDUS.set(k, t);
  return { source: t, x: 0, y: 0 };
}

// Compose la pile sur `c` (à la taille de l'image). `optionsDe(i)` : les options
// de rendu du calque `i` (voir `peindreCalque`).
function composerCalques(c, calques, L, H, optionsDe = () => ({})) {
  calques.forEach((k, i) => {
    if (!k.visible) return;
    const { source, x, y } = sourceDuCalque(k, L, H, optionsDe(i));
    c.save();
    c.imageSmoothingEnabled = false;
    c.globalAlpha = k.opacite;
    c.globalCompositeOperation = operationDuMode(k.mode);
    c.drawImage(source, x, y);
    c.restore();
  });
}

// Le calque RENDU en pixels, objets compris, pour un fichier ou une fusion : sa
// toile s'il n'a pas d'objet, sinon l'union de son cadre et de l'image (ses
// objets peuvent déborder de sa toile).
function rasteriserCalque(k, L, H) {
  if (!k.objets.length) return { toile: k.toile, x: k.x, y: k.y };
  const u = union(cadreDuCalque(k), { x: 0, y: 0, l: L, h: H });
  const t = nouvelleToile(u.l, u.h);
  peindreCalque(t.getContext("2d", { colorSpace: "srgb" }), k, u.x, u.y);
  return { toile: t, x: u.x, y: u.y };
}

// ── Opérations sur la pile (toutes rendent une NOUVELLE pile) ──────────────
// FUSIONNER VERS LE BAS : le calque `i` peint sur celui du dessous, qui garde
// son nom, son mode et son opacité (comme GIMP).
function fusionnerVersLeBas(calques, i, L, H) {
  if (i < 1) return null;
  const bas = rasteriserCalque(calques[i - 1], L, H);
  const haut = rasteriserCalque(calques[i], L, H);
  const u = union({ x: bas.x, y: bas.y, l: bas.toile.width, h: bas.toile.height }, { x: haut.x, y: haut.y, l: haut.toile.width, h: haut.toile.height });
  const t = nouvelleToile(u.l, u.h);
  const c = contexteSansLissage(t);
  c.drawImage(bas.toile, bas.x - u.x, bas.y - u.y);
  c.globalAlpha = calques[i].opacite;
  c.globalCompositeOperation = operationDuMode(calques[i].mode);
  c.drawImage(haut.toile, haut.x - u.x, haut.y - u.y);
  const fusion = { ...calques[i - 1], toile: t, x: u.x, y: u.y, objets: [] };
  return [...calques.slice(0, i - 1), fusion, ...calques.slice(i + 1)];
}

// APLATIR : un seul calque, l'image telle qu'on la voit — les calques cachés
// disparaissent (comme GIMP).
function aplatirCalques(calques, L, H) {
  const t = nouvelleToile(L, H);
  composerCalques(t.getContext("2d", { colorSpace: "srgb" }), calques, L, H);
  return [nouveauCalque("Fond", t)];
}

function remplacerCalque(calques, i, f) {
  return calques.map((k, j) => (j === i ? f(k) : k));
}

// Déplacer un calque emporte ses annotations.
function decalerCalque(k, dx, dy) {
  return { ...k, x: k.x + dx, y: k.y + dy, objets: k.objets.map((o) => deplacer(o, dx, dy)) };
}

function echanger(calques, i, j) {
  if (j < 0 || j >= calques.length) return null;
  const copie = [...calques];
  [copie[i], copie[j]] = [copie[j], copie[i]];
  return copie;
}

// ── Géométrie de toute l'image ──────────────────────────────────────────────
// `op` : { genre: "pivoter", sens } | { genre: "angle", degres } | { genre: "retourner", axe } |
// { genre: "recadrer", r } | { genre: "canevas", l, h, ancre } |
// { genre: "redimensionner", l, h }. Rend `{ largeur, hauteur, calques, carte }`
// — `carte` envoie un point de l'ancienne image dans la nouvelle — ou `null`.
//
// Recadrer et la taille du canevas ne font que DÉCALER les calques, sans
// rogner leurs pixels : agrandir puis recadrer rend l'image d'origine.
function transformerDocument(doc, op, mesure) {
  const L = doc.largeur, H = doc.hauteur;
  let largeur = L, hauteur = H, carte, toileDe = null;
  switch (op.genre) {
    case "pivoter":
      largeur = H;
      hauteur = L;
      carte = op.sens > 0 ? (p) => ({ x: H - p.y, y: p.x }) : (p) => ({ x: p.y, y: L - p.x });
      toileDe = (t) => pivoter(t, op.sens).toile;
      break;
    case "angle": {
      const degres = ((op.degres % 360) + 360) % 360;
      if (!Number.isFinite(degres) || degres === 0) return null;
      const b = boiteTournee(L, H, (degres * Math.PI) / 180);
      if (b.l > COTE_MAX || b.h > COTE_MAX) return null;
      largeur = b.l;
      hauteur = b.h;
      carte = carteAngle(L, H, b.l, b.h, degres);
      break;
    }
    case "retourner":
      carte = op.axe === "h" ? (p) => ({ x: L - p.x, y: p.y }) : (p) => ({ x: p.x, y: H - p.y });
      toileDe = (t) => retourner(t, op.axe).toile;
      break;
    case "recadrer": {
      const s = cadreDansToile(op.r, { width: L, height: H });
      if (!s) return null;
      largeur = s.l;
      hauteur = s.h;
      carte = (p) => ({ x: p.x - s.x, y: p.y - s.y });
      break;
    }
    case "canevas": {
      const dx = Math.round((op.l - L) * op.ancre.fx), dy = Math.round((op.h - H) * op.ancre.fy);
      largeur = op.l;
      hauteur = op.h;
      carte = (p) => ({ x: p.x + dx, y: p.y + dy });
      break;
    }
    case "redimensionner": {
      const kx = op.l / L, ky = op.h / H;
      largeur = op.l;
      hauteur = op.h;
      carte = (p) => ({ x: p.x * kx, y: p.y * ky });
      break;
    }
    default:
      return null;
  }
  const calques = doc.calques.map((k) => {
    const objets = k.objets.map((o) => transformerObjet(o, carte, mesure)).filter(Boolean);
    if (op.genre === "redimensionner") {
      const x = Math.round(k.x * op.l / L), y = Math.round(k.y * op.h / H);
      const l = Math.max(1, Math.round((k.x + k.toile.width) * op.l / L) - x);
      const h = Math.max(1, Math.round((k.y + k.toile.height) * op.h / H) - y);
      return { ...k, toile: redimensionner(k.toile, l, h).toile, x, y, objets };
    }
    // Un angle libre : le calque tourne autour de SON centre, que la carte
    // déplace ; sa toile grandit comme celle de l'image.
    if (op.genre === "angle") {
      const toile = pivoterAngle(k.toile, op.degres);
      const centre = carte({ x: k.x + k.toile.width / 2, y: k.y + k.toile.height / 2 });
      return { ...k, toile, x: Math.round(centre.x - toile.width / 2), y: Math.round(centre.y - toile.height / 2), objets };
    }
    // Les deux coins opposés passent par la carte ; le nouveau coin haut-gauche
    // est le plus petit des deux (un quart de tour ou un miroir les échange).
    const a = carte({ x: k.x, y: k.y }), b = carte({ x: k.x + k.toile.width, y: k.y + k.toile.height });
    const toile = toileDe ? toileDe(k.toile) : k.toile;
    return { ...k, toile, x: Math.round(Math.min(a.x, b.x)), y: Math.round(Math.min(a.y, b.y)), objets };
  });
  return { largeur, hauteur, calques, carte };
}

// ── Vers Rust ─────────────────────────────────────────────────────────────
const enPng = (toile) => new Promise((resolve) => toile.toBlob(resolve, "image/png"));

// Le paquet d'un enregistrement à calques (lu par `document_image::depuis_page`) :
// [longueur du JSON, u32 petit-boutiste][JSON][PNG de chaque calque][PNG composé].
async function paquetDocument(calques, L, H, composite) {
  const pngs = [];
  const entetes = [];
  for (const k of calques) {
    const r = rasteriserCalque(k, L, H);
    const png = new Uint8Array(await (await enPng(r.toile)).arrayBuffer());
    pngs.push(png);
    entetes.push({ nom: k.nom, x: r.x, y: r.y, opacite: k.opacite, visible: k.visible, mode: k.mode, taille: png.length });
  }
  const pngComposite = new Uint8Array(await (await enPng(composite)).arrayBuffer());
  const json = new TextEncoder().encode(JSON.stringify({ largeur: L, hauteur: H, calques: entetes, composite: pngComposite.length }));
  const parties = [new Uint8Array(new Uint32Array([json.length]).buffer), json, ...pngs, pngComposite];
  const paquet = new Uint8Array(parties.reduce((n, p) => n + p.length, 0));
  let pos = 0;
  for (const p of parties) {
    paquet.set(p, pos);
    pos += p.length;
  }
  return paquet;
}

// Un calque reçu de Rust (`document_image::vers_page`) : son PNG devient une toile.
async function calqueDepuisRust(d) {
  const blob = await (await fetch(d.png)).blob();
  const image = await createImageBitmap(blob, { colorSpaceConversion: "none", premultiplyAlpha: "none" });
  const t = nouvelleToile(image.width, image.height);
  t.getContext("2d", { colorSpace: "srgb" }).drawImage(image, 0, 0);
  image.close();
  const mode = NOM_DU_MODE[d.mode] ? d.mode : "normal";
  return { ...nouveauCalque(d.nom || "Calque", t, d.x, d.y), opacite: d.opacite, visible: d.visible, mode };
}

// ── Le panneau des calques ────────────────────────────────────────────────
// `api` : { etat() → { calques, actif, largeur, hauteur }, choisir(i),
// changerCalques(f, actif) (`f` : pile courante → nouvelle pile),
// regler(i, f) (sans historique : curseur en cours), noter() }.
// Comme GIMP, la liste montre le calque du HAUT en premier.
const VIGNETTE = { l: 44, h: 32 };

function creerPanneauCalques(racine, api) {
  const liste = racine.querySelector(".liste-calques");
  const opacite = racine.querySelector("#k-opacite");
  const valeurOpacite = racine.querySelector("#k-opacite-valeur");
  const boutonMode = racine.querySelector("#k-mode");
  const listeModes = racine.querySelector("#k-modes");
  let renomme = -1;
  let opaciteNotee = false;

  for (const [mode, libelle] of MODES_CALQUE) {
    const b = document.createElement("button");
    b.textContent = libelle;
    b.dataset.mode = mode;
    b.addEventListener("click", () => {
      listeModes.hidden = true;
      const { actif } = api.etat();
      api.changerCalques((pile) => remplacerCalque(pile, actif, (k) => ({ ...k, mode })), actif);
    });
    listeModes.appendChild(b);
  }
  boutonMode.addEventListener("click", () => { listeModes.hidden = !listeModes.hidden; });

  // Un geste de curseur = UN pas d'historique, noté à son premier mouvement.
  opacite.addEventListener("pointerdown", () => { opaciteNotee = false; });
  opacite.addEventListener("input", () => {
    if (!opaciteNotee) {
      api.noter();
      opaciteNotee = true;
    }
    const v = Number(opacite.value) / 100;
    api.regler(api.etat().actif, (k) => ({ ...k, opacite: v }));
  });
  opacite.addEventListener("change", () => {
    opaciteNotee = false;
    opacite.blur();
  });
  opacite.addEventListener("keydown", (e) => e.stopPropagation());

  function renommer(i, champ) {
    const nom = champ.value.trim();
    renomme = -1;
    const { calques, actif } = api.etat();
    if (nom && nom !== calques[i].nom) api.changerCalques((pile) => remplacerCalque(pile, i, (k) => ({ ...k, nom })), actif);
    else afficher(true);
  }

  function ligne(k, i, actif) {
    const rangee = document.createElement("div");
    rangee.className = "calque" + (i === actif ? " actif" : "");
    const oeil = document.createElement("button");
    oeil.className = "oeil";
    oeil.innerHTML = k.visible ? '<span class="ico ico-eye"></span>' : "";
    oeil.title = k.visible ? "Cacher ce calque" : "Montrer ce calque";
    oeil.addEventListener("click", (e) => {
      e.stopPropagation();
      api.changerCalques((pile) => remplacerCalque(pile, i, (c) => ({ ...c, visible: !c.visible })), api.etat().actif);
    });
    const vignette = document.createElement("canvas");
    vignette.width = VIGNETTE.l;
    vignette.height = VIGNETTE.h;
    const { largeur, hauteur } = api.etat();
    const echelle = Math.min(VIGNETTE.l / largeur, VIGNETTE.h / hauteur);
    const v = vignette.getContext("2d");
    v.save();
    v.translate((VIGNETTE.l - largeur * echelle) / 2, (VIGNETTE.h - hauteur * echelle) / 2);
    v.scale(echelle, echelle);
    v.beginPath();
    v.rect(0, 0, largeur, hauteur);
    v.clip();
    const { source, x, y } = sourceDuCalque(k, largeur, hauteur);
    v.drawImage(source, x, y);
    v.restore();
    const nom = document.createElement("span");
    nom.className = "nom-calque";
    if (renomme === i) {
      const champ = document.createElement("input");
      champ.value = k.nom;
      champ.addEventListener("keydown", (e) => {
        e.stopPropagation();
        if (e.key === "Enter") champ.blur();
        else if (e.key === "Escape") {
          renomme = -1;
          afficher(true);
        }
      });
      champ.addEventListener("blur", () => { if (renomme === i) renommer(i, champ); });
      nom.appendChild(champ);
      setTimeout(() => {
        champ.focus();
        champ.select();
      }, 0);
    } else {
      nom.textContent = k.nom || "Sans nom";
      nom.title = `${k.nom} — ${NOM_DU_MODE[k.mode]}, ${Math.round(k.opacite * 100)} % (double-clic : renommer)`;
    }
    rangee.append(oeil, vignette, nom);
    rangee.addEventListener("click", () => { if (i !== api.etat().actif) api.choisir(i); });
    rangee.addEventListener("dblclick", () => {
      renomme = i;
      afficher(true);
    });
    return rangee;
  }

  let affiches = null, actifAffiche = -1;
  // `force` : reconstruire même si la pile n'a pas changé (renommage).
  function afficher(force = false) {
    const { calques, actif } = api.etat();
    if (!force && calques === affiches && actif === actifAffiche) return;
    affiches = calques;
    actifAffiche = actif;
    liste.replaceChildren(...calques.map((k, i) => ligne(k, i, actif)).reverse());
    const k = calques[actif];
    for (const b of racine.querySelectorAll("button, input")) b.disabled = !k;
    if (!k) return;
    if (document.activeElement !== opacite) opacite.value = String(Math.round(k.opacite * 100));
    valeurOpacite.textContent = Math.round(k.opacite * 100) + " %";
    boutonMode.textContent = NOM_DU_MODE[k.mode] + " ▾";
    for (const b of listeModes.children) b.classList.toggle("actif", b.dataset.mode === k.mode);
    racine.querySelector("#k-monter").disabled = actif >= calques.length - 1;
    racine.querySelector("#k-descendre").disabled = actif <= 0;
    racine.querySelector("#k-fusionner").disabled = actif <= 0;
    racine.querySelector("#k-supprimer").disabled = calques.length <= 1;
    // Un seul calque, visible, opaque, sans objet, qui couvre juste l'image : déjà plat.
    const { largeur, hauteur } = api.etat();
    const plat = calques.length === 1 && k.visible && k.opacite === 1 && !k.objets.length
      && k.x === 0 && k.y === 0 && k.toile.width === largeur && k.toile.height === hauteur;
    racine.querySelector("#k-aplatir").disabled = plat;
  }

  return { afficher };
}
