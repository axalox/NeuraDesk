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

// `valeurs` : { luminosite, contraste, saturation } — une clé absente vaut 0.
function filtreCouleurs(valeurs) {
  const parties = REGLAGES_COULEUR.filter(([cle]) => valeurs[cle]).map(([cle, , f]) => `${f}(${100 + valeurs[cle]}%)`);
  return parties.length ? parties.join(" ") : "none";
}

// `null` si tout est à 0 : rien à appliquer, rien à noter dans l'historique.
function ajusterCouleurs(source, valeurs) {
  const filtre = filtreCouleurs(valeurs);
  if (filtre === "none") return null;
  const t = nouvelleToile(source.width, source.height);
  const c = contexteSansLissage(t);
  c.filter = filtre;
  c.drawImage(source, 0, 0);
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

// L'aperçu du geste en cours, à l'écran seulement.
function dessinerApercuGeometrie(c, o) {
  const [a, b] = o.points;
  const L = c.canvas.width, H = c.canvas.height;
  const r = cadreDe(a, b);
  c.save();
  c.fillStyle = OMBRE_RECADRAGE;
  c.beginPath();
  c.rect(0, 0, L, H);
  c.rect(r.x, r.y, r.l, r.h);
  c.fill("evenodd");
  c.restore();
}
