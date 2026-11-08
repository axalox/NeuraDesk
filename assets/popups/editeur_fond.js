"use strict";
// editeur_fond.js — LE FOND DE PRÉSENTATION de l'éditeur d'image. DEMANDE
// (2026-10-24), d'après Snip (CleanShot X) : un dégradé ou une couleur derrière
// la capture, une marge, des coins arrondis, une ombre, un format.
//
// Le fond est CUIT dans le document, comme la géométrie : l'image (aplatie) est
// posée en calque « Capture », arrondie, au-dessus d'un calque « Fond » qui
// porte la couleur et l'ombre. Annulable d'un Ctrl+Z ; tant que rien d'autre
// n'a changé, retoucher un réglage REFAIT le fond depuis l'image d'avant (voir
// `fondPose` dans editeur_image.html) — les réglages ne s'empilent pas.

const DEGRADES_FOND = [
  ["Améthyste", "#667eea", "#764ba2"],
  ["Flamant", "#f093fb", "#f5576c"],
  ["Lagon", "#4facfe", "#00f2fe"],
  ["Menthe", "#43e97b", "#38f9d7"],
  ["Couchant", "#fa709a", "#fee140"],
  ["Abysse", "#30cfd0", "#330867"],
  ["Dragée", "#a8edea", "#fed6e3"],
  ["Ardoise", "#232526", "#414345"],
];

const COULEURS_FOND = [
  ["Blanc", "#ffffff"],
  ["Perle", "#e9e9ee"],
  ["Anthracite", "#1e1e22"],
  ["Noir", "#000000"],
  ["Bleu", "#3b82f6"],
  ["Rouge", "#ef4444"],
  ["Vert", "#22c55e"],
  ["Ambre", "#f59e0b"],
];

// `null` : le format suit l'image et la marge.
const FORMATS_FOND = [
  ["Libre", null],
  ["16:9", [16, 9]],
  ["4:3", [4, 3]],
  ["1:1", [1, 1]],
];

const REGLAGES_FOND_DEFAUT = { genre: "aucun", teinte: 0, marge: 40, coins: 30, ombre: 50, format: 0 };

// PURE : la taille du document et la place de l'image, pour une image `L`×`H`.
// La marge (0 à 100) vaut jusqu'à 20 % du plus grand côté ; le format élargit
// le côté qui manque, l'image reste centrée.
function geometrieFond(L, H, r) {
  const marge = Math.round((Math.max(L, H) * r.marge) / 500);
  let l = L + 2 * marge, h = H + 2 * marge;
  const format = FORMATS_FOND[r.format] && FORMATS_FOND[r.format][1];
  if (format) {
    const [a, b] = format;
    if (l * b < h * a) l = Math.round((h * a) / b);
    else h = Math.round((l * b) / a);
  }
  l = Math.min(l, COTE_MAX);
  h = Math.min(h, COTE_MAX);
  return { l, h, x: Math.floor((l - L) / 2), y: Math.floor((h - H) / 2), rayon: Math.round((Math.min(L, H) * r.coins) / 1000) };
}

function cheminArrondi(c, x, y, l, h, rayon) {
  c.beginPath();
  if (rayon > 0) c.roundRect(x, y, l, h, rayon);
  else c.rect(x, y, l, h);
}

// Le document avec son fond : `{ largeur, hauteur, calques }`, ou `null` sans fond.
// `plat` : l'image aplatie, à sa taille.
function documentAvecFond(plat, r) {
  if (r.genre === "aucun") return null;
  const L = plat.width, H = plat.height;
  const g = geometrieFond(L, H, r);
  const fond = nouvelleToile(g.l, g.h);
  const c = fond.getContext("2d", { colorSpace: "srgb" });
  if (r.genre === "degrade") {
    const [, a, b] = DEGRADES_FOND[r.teinte] || DEGRADES_FOND[0];
    const d = c.createLinearGradient(0, 0, g.l, g.h);
    d.addColorStop(0, a);
    d.addColorStop(1, b);
    c.fillStyle = d;
  } else {
    c.fillStyle = (COULEURS_FOND[r.teinte] || COULEURS_FOND[0])[1];
  }
  c.fillRect(0, 0, g.l, g.h);
  if (r.ombre > 0) {
    // L'ombre est peinte SOUS l'image, dans le calque du fond : la forme qui la
    // projette est recouverte par l'image elle-même.
    const flou = (Math.max(L, H) * 0.05 * r.ombre) / 100;
    c.save();
    c.shadowColor = `rgba(0, 0, 0, ${(0.6 * r.ombre) / 100})`;
    c.shadowBlur = flou;
    c.shadowOffsetY = flou / 4;
    c.fillStyle = "#000";
    cheminArrondi(c, g.x, g.y, L, H, g.rayon);
    c.fill();
    c.restore();
  }
  const image = nouvelleToile(L, H);
  const ci = image.getContext("2d", { colorSpace: "srgb" });
  cheminArrondi(ci, 0, 0, L, H, g.rayon);
  ci.clip();
  ci.drawImage(plat, 0, 0);
  return {
    largeur: g.l,
    hauteur: g.h,
    calques: [nouveauCalque("Fond", fond), nouveauCalque("Capture", image, g.x, g.y)],
  };
}
