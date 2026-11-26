"use strict";
// editeur_geometrie.js — LOT 4 de docs/plans/editeur-image.md : la GÉOMÉTRIE
// de l'image (pivoter, retourner, recadrer, redimensionner, taille du canevas).
// La découpe d'une bande (✂) a été RETIRÉE le 2026-09-25, à la demande.
//
// Chaque opération rend un NOUVEAU fond — l'ancien reste intact, c'est lui que
// l'historique garde — et la CARTE qui envoie un point de l'ancienne image
// dans la nouvelle : les annotations suivent sans être aplaties.
//
// Les réglages de COULEUR (luminosité, contraste, saturation) suivent le même
// contrat, avec une carte identité : voir `ajusterCouleurs`.
//
// Pivoter et retourner copient les pixels SANS lissage : quatre quarts de
// tour rendent l'image d'origine au pixel près (mesuré par le levier
// `NEURADESK_DEBUG_EDITEUR_GEOMETRIE`).

// L'opération qui se TRACE à la souris, comme un outil d'annotation.
const OUTILS_TRACES_GEOMETRIE = [["recadrer", "⛶", "c", "Recadrer"]];
const TRACES_GEOMETRIE = new Set(OUTILS_TRACES_GEOMETRIE.map(([nom]) => nom));
const OMBRE_RECADRAGE = "rgba(0, 0, 0, 0.55)";
// Au-delà, Chromium refuse le canevas (il reste vide, sans erreur).
const COTE_MAX = 16384;

// LES RATIOS DU RECADRAGE — DEMANDE (2026-11-25, options de Telegram) : «
// recadrage avec proportions prédéfinies ». `null` = aucune contrainte ; le
// ratio « image » est calculé au moment du geste (voir la page).
const RATIOS_RECADRAGE = [
  ["libre", "Libre", null],
  ["1:1", "Carré", 1],
  ["4:5", "4:5", 4 / 5],
  ["16:9", "16:9", 16 / 9],
  ["9:16", "9:16", 9 / 16],
  ["image", "Image", null],
];

// `b` ramené au ratio `ratio` (largeur / hauteur), en gardant le signe du
// geste : la plus grande des deux dimensions du glisser commande. Le cadre ne
// sort pas de la toile (`L` × `H`) — sinon le rognage casserait le ratio.
function ajusterAuRatio(a, b, ratio, L, H) {
  if (!ratio) return b;
  const dx = b.x - a.x, dy = b.y - a.y;
  const sx = Math.sign(dx || 1), sy = Math.sign(dy || 1);
  const lMax = Math.min(sx > 0 ? L - a.x : a.x, (sy > 0 ? H - a.y : a.y) * ratio);
  const demande = Math.max(Math.abs(dx), Math.abs(dy) * ratio);
  const l = lMax > 1 ? Math.max(1, Math.min(demande, lMax)) : 1;
  return { x: a.x + sx * l, y: a.y + sy * (l / ratio) };
}

// Les neuf ancres de « taille du canevas », dans l'ordre de lecture.
const ANCRES = [0, 0.5, 1].flatMap((fy) => [0, 0.5, 1].map((fx) => ({ fx, fy })));

function nouvelleToile(l, h) {
  const t = document.createElement("canvas");
  t.width = l;
  t.height = h;
  return t;
}

function contexteSansLissage(t) {
  const c = t.getContext("2d", { colorSpace: "srgb" });
  c.imageSmoothingEnabled = false;
  return c;
}

// Un quart de tour : `sens` 1 = horaire, -1 = antihoraire.
function pivoter(source, sens) {
  const L = source.width, H = source.height;
  const t = nouvelleToile(H, L);
  const c = contexteSansLissage(t);
  c.translate(sens > 0 ? H : 0, sens > 0 ? 0 : L);
  c.rotate((sens * Math.PI) / 2);
  c.drawImage(source, 0, 0);
  const carte = sens > 0 ? (p) => ({ x: H - p.y, y: p.x }) : (p) => ({ x: p.y, y: L - p.x });
  return { toile: t, carte };
}

// DEMANDE (2026-11-08) : « la possibilité de faire des rotations d'image ». Un
// angle LIBRE, en degrés, sens horaire (l'axe y du canevas descend). La toile
// grandit jusqu'au cadre de l'image tournée ; les coins sont transparents. Le
// seul rééchantillonnage avec `redimensionner` : d'où le lissage.
function boiteTournee(l, h, rad) {
  const c = Math.abs(Math.cos(rad)), s = Math.abs(Math.sin(rad));
  // Arrondi au millième : cos(90°) vaut 6e-17, pas 0 — sans lui, un quart de
  // tour saisi en degrés gagnerait un pixel.
  const net = (v) => Math.round(v * 1000) / 1000;
  return { l: Math.max(1, Math.ceil(net(l * c + h * s))), h: Math.max(1, Math.ceil(net(l * s + h * c))) };
}

function pivoterAngle(source, degres) {
  const rad = (degres * Math.PI) / 180;
  const b = boiteTournee(source.width, source.height, rad);
  const t = nouvelleToile(b.l, b.h);
  const c = t.getContext("2d", { colorSpace: "srgb" });
  c.imageSmoothingEnabled = true;
  c.imageSmoothingQuality = "high";
  c.translate(b.l / 2, b.h / 2);
  c.rotate(rad);
  c.drawImage(source, -source.width / 2, -source.height / 2);
  return t;
}

// La carte d'un angle libre : le centre de l'ancienne image va au centre de la
// nouvelle (`l` × `h`), le reste tourne autour.
function carteAngle(L, H, l, h, degres) {
  const rad = (degres * Math.PI) / 180, cos = Math.cos(rad), sin = Math.sin(rad);
  return (p) => {
    const dx = p.x - L / 2, dy = p.y - H / 2;
    return { x: l / 2 + dx * cos - dy * sin, y: h / 2 + dx * sin + dy * cos };
  };
}

// `axe` "h" : miroir gauche-droite ; "v" : haut-bas.
function retourner(source, axe) {
  const L = source.width, H = source.height;
  const t = nouvelleToile(L, H);
  const c = contexteSansLissage(t);
  if (axe === "h") {
    c.translate(L, 0);
    c.scale(-1, 1);
  } else {
    c.translate(0, H);
    c.scale(1, -1);
  }
  c.drawImage(source, 0, 0);
  const carte = axe === "h" ? (p) => ({ x: L - p.x, y: p.y }) : (p) => ({ x: p.x, y: H - p.y });
  return { toile: t, carte };
}

// Garde le cadre `r` (en pixels de l'image) — `null` s'il ne couvre rien.
function recadrer(source, r) {
  const s = cadreDansToile(r, source);
  if (!s) return null;
  const t = nouvelleToile(s.l, s.h);
  contexteSansLissage(t).drawImage(source, s.x, s.y, s.l, s.h, 0, 0, s.l, s.h);
  return { toile: t, carte: (p) => ({ x: p.x - s.x, y: p.y - s.y }) };
}

// Nouvelle taille `l`×`h`, l'image posée selon l'ancre (0 = bord gauche/haut,
// 1 = droit/bas) : agrandir ajoute du transparent, réduire rogne.
function tailleCanevas(source, l, h, ancre) {
  const dx = Math.round((l - source.width) * ancre.fx);
  const dy = Math.round((h - source.height) * ancre.fy);
  const t = nouvelleToile(l, h);
  contexteSansLissage(t).drawImage(source, dx, dy);
  return { toile: t, carte: (p) => ({ x: p.x + dx, y: p.y + dy }) };
}

// Mise à l'échelle, AVEC lissage : c'est la seule opération qui rééchantillonne.
function redimensionner(source, l, h) {
  const t = nouvelleToile(l, h);
  const c = t.getContext("2d", { colorSpace: "srgb" });
  c.imageSmoothingEnabled = true;
  c.imageSmoothingQuality = "high";
  c.drawImage(source, 0, 0, l, h);
  const kx = l / source.width, ky = h / source.height;
  return { toile: t, carte: (p) => ({ x: p.x * kx, y: p.y * ky }) };
}

// ── Luminosité, contraste, saturation ─────────────────────────────────────
// DEMANDE EXPLICITE (2026-09-25) : « ajoutes les réglages luminosité contraste
// saturation ». Chaque curseur va de −BORNE à +BORNE, 0 = inchangé ; la valeur
// devient le pourcentage d'une fonction `filter` du canevas (0 à 200 %). Seul le
// FOND est touché : les annotations restent des objets, avec leurs couleurs.
const REGLAGES_COULEUR = [
  ["luminosite", "Luminosité", "brightness"],
  ["contraste", "Contraste", "contrast"],
  ["saturation", "Saturation", "saturate"],
];
const BORNE_REGLAGE_COULEUR = 100;

// LES FILTRES PRÉDÉFINIS — DEMANDE (2026-11-25, options de Telegram). Chacun
// est une suite de fonctions `filter` que le canevas sait déjà faire ; il se
// COMBINE avec les trois curseurs (voir `filtreCouleurs`), et se retire en
// recliquant dessus.
const FILTRES_PREDEFINIS = [
  ["nb", "Noir et blanc", "grayscale(1) contrast(1.05)"],
  ["sepia", "Sépia", "sepia(0.85) contrast(1.05)"],
  ["chaud", "Chaud", "sepia(0.35) saturate(1.4) hue-rotate(-8deg) brightness(1.02)"],
  ["froid", "Froid", "sepia(0.2) hue-rotate(170deg) saturate(1.2) brightness(1.02)"],
  ["eclatant", "Éclatant", "saturate(1.5) contrast(1.12)"],
  ["estompe", "Estompé", "saturate(0.65) contrast(0.85) brightness(1.07)"],
];

// LES EFFETS qui ne sont PAS des fonctions `filter` : appliqués sur les
// pixels, et pour la vignette seulement, aperçus à l'écran (la netteté, une
// convolution, ne s'applique qu'au clic sur « Appliquer »).
const EFFETS = [
  ["vignette", "Vignette", ""],
  ["nettete", "Netteté", " — sans aperçu, appliquée au clic sur Appliquer"],
];
const BORNE_EFFET = 100;

// `valeurs` : { luminosite, contraste, saturation, filtre, vignette, nettete }
// — une clé absente vaut 0 (ou aucun filtre).
function filtreCouleurs(valeurs) {
  const parties = [];
  const preset = FILTRES_PREDEFINIS.find(([cle]) => cle === valeurs.filtre);
  if (preset) parties.push(preset[2]);
  for (const [cle, , f] of REGLAGES_COULEUR) {
    if (valeurs[cle]) parties.push(`${f}(${100 + valeurs[cle]}%)`);
  }
  return parties.length ? parties.join(" ") : "none";
}

// La vignette : un dégradé radial posé sur les pixels DÉJÀ là (`source-atop`),
// du transparent au centre au noir sur les bords. `r` : le cadre du calque,
// pour que l'aperçu et l'application s'accordent.
function vignetter(c, r, force) {
  const cx = r.x + r.l / 2, cy = r.y + r.h / 2;
  const rayon = Math.hypot(r.l, r.h) / 2;
  if (!(rayon > 0)) return;
  const g = c.createRadialGradient(cx, cy, rayon * 0.45, cx, cy, rayon);
  g.addColorStop(0, "rgba(0, 0, 0, 0)");
  g.addColorStop(1, `rgba(0, 0, 0, ${0.75 * Math.min(1, force / BORNE_EFFET)})`);
  c.save();
  c.globalCompositeOperation = "source-atop";
  c.fillStyle = g;
  c.fillRect(r.x, r.y, r.l, r.h);
  c.restore();
}

// La netteté : une accentuation (noyau 3×3, centre moins les quatre voisins),
// relue sur une COPIE des pixels pour que chaque voisin soit l'original.
function affuter(c, force) {
  const l = c.canvas.width, h = c.canvas.height;
  if (l < 3 || h < 3) return;
  const image = c.getImageData(0, 0, l, h);
  const d = image.data;
  const src = new Uint8ClampedArray(d);
  const a = (Math.min(1, force / BORNE_EFFET)) * 0.8;
  const centre = 1 + 4 * a;
  const voisin = a;
  for (let y = 1; y < h - 1; y++) {
    for (let x = 1; x < l - 1; x++) {
      const i = (y * l + x) * 4;
      if (!src[i + 3]) continue;
      for (let p = 0; p < 3; p++) {
        d[i + p] = centre * src[i + p]
          - voisin * (src[i - 4 + p] + src[i + 4 + p] + src[i - l * 4 + p] + src[i + l * 4 + p]);
      }
    }
  }
  c.putImageData(image, 0, 0);
}

// `null` si tout est neutre : rien à appliquer, rien à noter dans l'historique.
function ajusterCouleurs(source, valeurs) {
  const filtre = filtreCouleurs(valeurs);
  const vignette = valeurs.vignette || 0;
  const nettete = valeurs.nettete || 0;
  if (filtre === "none" && !vignette && !nettete) return null;
  const t = nouvelleToile(source.width, source.height);
  const c = contexteSansLissage(t);
  c.filter = filtre;
  c.drawImage(source, 0, 0);
  c.filter = "none";
  if (vignette) vignetter(c, { x: 0, y: 0, l: t.width, h: t.height }, vignette);
  if (nettete) affuter(c, nettete);
  return { toile: t, carte: (p) => p };
}

// Le cadre des points d'un objet — sans épaisseur de trait.
function etendue(points) {
  const xs = points.map((p) => p.x), ys = points.map((p) => p.y);
  return { l: Math.max(...xs) - Math.min(...xs), h: Math.max(...ys) - Math.min(...ys) };
}

// Un objet suit la carte — ou DISPARAÎT (`null`) si elle l'aplatit (mesuré du
// temps de la découpe : les masques d'une bande retirée devenaient des filets). Un texte reste HORIZONTAL : c'est le CENTRE de son cadre qui suit,
// sinon un miroir ou un quart de tour le déporterait de toute sa largeur (son
// point est son coin haut-gauche).
function transformerObjet(o, carte, c) {
  if (o.type !== "texte") {
    const points = o.points.map(carte);
    if (points.length > 1) {
      const avant = etendue(o.points), apres = etendue(points);
      // Moins d'UN pixel, pas zéro : une carte arrondie au pixel laissait un
      // masque large de 0,3 px.
      if ((avant.l >= 1 && apres.l < 1) || (avant.h >= 1 && apres.h < 1)) return null;
    }
    return { ...o, points };
  }
  const r = boiteDe(o, c);
  const centre = carte({ x: r.x + r.l / 2, y: r.y + r.h / 2 });
  return { ...o, points: [{ x: centre.x - r.l / 2, y: centre.y - r.h / 2 }] };
}

// L'aperçu du geste en cours, à l'écran seulement : le dehors assombri, la
// grille des tiers (demande 2026-11-25, options de Telegram), les huit
// poignées et la taille du cadre.
function dessinerApercuGeometrie(c, o) {
  const [a, b] = o.points;
  const L = c.canvas.width, H = c.canvas.height;
  const r = cadreDe(a, b);
  const k = pixelsParEcran();
  c.save();
  c.fillStyle = OMBRE_RECADRAGE;
  c.beginPath();
  c.rect(0, 0, L, H);
  c.rect(r.x, r.y, r.l, r.h);
  c.fill("evenodd");
  if (r.l < 1 || r.h < 1) {
    c.restore();
    return;
  }
  // La grille des tiers : deux verticales, deux horizontales.
  c.strokeStyle = "rgba(255, 255, 255, 0.55)";
  c.lineWidth = k;
  c.beginPath();
  for (const t of [1 / 3, 2 / 3]) {
    c.moveTo(r.x + r.l * t, r.y);
    c.lineTo(r.x + r.l * t, r.y + r.h);
    c.moveTo(r.x, r.y + r.h * t);
    c.lineTo(r.x + r.l, r.y + r.h * t);
  }
  c.stroke();
  // Le cadre et ses huit poignées (coins et milieux de côté).
  c.strokeStyle = "#fff";
  c.lineWidth = 1.5 * k;
  c.strokeRect(r.x, r.y, r.l, r.h);
  const cote = 8 * k;
  const poignees = [
    [r.x, r.y], [r.x + r.l / 2, r.y], [r.x + r.l, r.y],
    [r.x, r.y + r.h / 2], [r.x + r.l, r.y + r.h / 2],
    [r.x, r.y + r.h], [r.x + r.l / 2, r.y + r.h], [r.x + r.l, r.y + r.h],
  ];
  c.fillStyle = "#fff";
  c.strokeStyle = "#000";
  c.lineWidth = k;
  for (const [x, y] of poignees) {
    c.fillRect(x - cote / 2, y - cote / 2, cote, cote);
    c.strokeRect(x - cote / 2, y - cote / 2, cote, cote);
  }
  // La taille du cadre, sur une pastille sombre au-dessus du coin haut-gauche.
  const taille = `${Math.round(r.l)} × ${Math.round(r.h)}`;
  c.font = `600 ${12 * k}px "Segoe UI", sans-serif`;
  c.textAlign = "left";
  c.textBaseline = "top";
  const largeur = c.measureText(taille).width + 12 * k;
  const haut = Math.max(0, r.y - 22 * k);
  c.fillStyle = "rgba(0, 0, 0, 0.75)";
  c.fillRect(r.x, haut, largeur, 18 * k);
  c.fillStyle = "#fff";
  c.fillText(taille, r.x + 6 * k, haut + 3 * k);
  c.restore();
}
