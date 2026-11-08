// win32::cef_image_editor — LE MODÈLE DES ANNOTATIONS, lot 2 de
// docs/plans/editeur-image.md. Chargé par editeur_image.html, qui garde pour
// lui les gestes (souris, clavier) ; ici, rien que des fonctions sur des objets.
//
// UNE ANNOTATION EST UN OBJET, PAS DES PIXELS : `{ type, points, couleur,
// niveau, texte? }`, en PIXELS DE L'IMAGE. Tant que l'image n'est pas rendue,
// chaque objet se resélectionne, se déplace, change de couleur. Les objets ne
// sont jamais modifiés en place : `deplacer` et consorts rendent une COPIE, ce
// qui permet à l'historique de garder de simples instantanés.
//
// Les numéros des ÉTAPES ne sont pas stockés : c'est le rang de l'étape parmi
// les étapes, recalculé à chaque dessin — en supprimer une renumérote les autres.
"use strict";

// Vingt-quatre teintes, par rangées de quatre (la grille de la boîte à outils) —
// DEMANDE EXPLICITE (2026-10-25) : « tripler le nombre de couleurs ». Les huit
// d'origine gardent leur place (la première est la couleur par défaut), puis
// huit claires et huit foncées.
const COULEURS = [
  ["Rouge", "#e53935"], ["Orange", "#fb8c00"], ["Jaune", "#fdd835"], ["Vert", "#43a047"],
  ["Bleu", "#1e88e5"], ["Violet", "#8e24aa"], ["Noir", "#000000"], ["Blanc", "#ffffff"],
  ["Rose", "#f06292"], ["Saumon", "#ff8a65"], ["Jaune pâle", "#fff176"], ["Vert clair", "#81c784"],
  ["Bleu ciel", "#64b5f6"], ["Lavande", "#ba68c8"], ["Gris clair", "#bdbdbd"], ["Cyan", "#00acc1"],
  ["Rouge foncé", "#b71c1c"], ["Marron", "#6d4c41"], ["Rose vif", "#d81b60"], ["Vert foncé", "#1b5e20"],
  ["Bleu marine", "#0d47a1"], ["Sarcelle", "#00897b"], ["Gris", "#757575"], ["Gris foncé", "#424242"],
];

// Neuf épaisseurs, parcourues avec [ et ] comme la taille de Flameshot ; chacune
// fixe aussi la taille du texte, des étapes et la force des masques (lot 3).
const NIVEAUX = [
  { trait: 1, police: 14, rayon: 10, flou: 2, pixel: 3, loupe: 1.25 },
  { trait: 2, police: 18, rayon: 13, flou: 3, pixel: 4, loupe: 1.5 },
  { trait: 3, police: 24, rayon: 16, flou: 4, pixel: 6, loupe: 1.5 },
  { trait: 4, police: 32, rayon: 19, flou: 7, pixel: 10, loupe: 1.75 },
  { trait: 6, police: 40, rayon: 22, flou: 10, pixel: 14, loupe: 2 },
  { trait: 8, police: 52, rayon: 27, flou: 15, pixel: 20, loupe: 2.5 },
  { trait: 12, police: 64, rayon: 32, flou: 20, pixel: 28, loupe: 3 },
  { trait: 16, police: 80, rayon: 38, flou: 28, pixel: 36, loupe: 3.5 },
  { trait: 24, police: 96, rayon: 44, flou: 36, pixel: 48, loupe: 4 },
];
// Les trois boutons de la barre : des raccourcis vers trois des neuf niveaux.
const PRESELECTIONS = [["Fin", 2], ["Moyen", 4], ["Épais", 6]];
const NIVEAU_DEFAUT = 4;

// Les outils, dans l'ordre de la barre, avec leur touche (celles de ShareX).
const OUTILS = [
  ["selection", "↖", "v", "Sélection"],
  ["rectangle", "▭", "r", "Rectangle"],
  ["pave", "■", "x", "Rectangle plein"],
  ["ellipse", "◯", "e", "Ellipse"],
  ["ligne", "╱", "l", "Ligne"],
  ["fleche", "➚", "a", "Flèche"],
  ["main", "✎", "f", "Main levée"],
  ["texte", "T", "t", "Texte"],
  ["surligneur", "▮", "h", "Surligneur"],
  ["etape", "①", "n", "Étape numérotée (glisser : avec une pointe)"],
  ["emoji", "☺", "j", "Émoticône (choisie dans le panneau de droite)"],
  ["flou", "≋", "b", "Flou"],
  ["pixeliser", "▦", "p", "Pixeliser"],
  ["gomme", "⌧", "w", "Gomme intelligente"],
  ["projecteur", "◐", "s", "Projecteur"],
  ["loupe", "⊕", "m", "Loupe"],
  ["inverser", "◑", "i", "Inverser les couleurs"],
];

// LOT 3 — LES MASQUES. Ils ne dessinent pas une forme : ils retravaillent ce
// qui est DÉJÀ sur le canevas sous eux (fond + annotations précédentes), en
// relisant ce canevas lui-même (`c.canvas`). Leur force suit l'épaisseur.
const OMBRE_PROJECTEUR = "rgba(0, 0, 0, 0.6)";

// Les formes tracées d'un point à un autre (glisser).
const A_DEUX_POINTS = new Set([
  "rectangle", "pave", "ellipse", "ligne", "fleche", "surligneur",
  "flou", "pixeliser", "gomme", "projecteur", "loupe", "inverser",
]);

// Les émoticônes proposées — DEMANDE EXPLICITE (2026-09-26) : « ajouter des
// émoticônes ». Une émoticône est un CARACTÈRE, dessiné avec la police couleur
// de Windows : aucune image à embarquer, et elle reste nette à toute taille.
const EMOTICONES = [
  "😀", "😂", "🤣", "😅", "😊", "😉", "😍", "😎",
  "🤔", "🙄", "😮", "🤯", "😴", "😢", "😡", "🥳",
  "👍", "👎", "👏", "🙏", "💪", "👀", "👉", "👈",
  "👆", "👇", "❤️", "💔", "🔥", "⭐", "💯", "🎉",
  "✅", "❌", "⚠️", "❓", "❗", "💡", "🚀", "📌",
];
// Une émoticône au niveau N fait deux fois la taille du texte au même niveau :
// à taille égale, un pictogramme se lit bien plus petit qu'une lettre.
const tailleEmoji = (n) => n.police * 2;
const policeEmoji = (taille) => `${taille}px "Segoe UI Emoji", sans-serif`;

const INTERLIGNE = 1.2;
const ANGLE_POINTE = Math.PI / 7;

const policeDe = (taille) => `600 ${taille}px "Segoe UI Variable Text", "Segoe UI", sans-serif`;
const longueurPointe = (trait) => Math.max(14, trait * 4);

function cadreDe(a, b) {
  return { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), l: Math.abs(b.x - a.x), h: Math.abs(b.y - a.y) };
}

function elargir(r, m) {
  return { x: r.x - m, y: r.y - m, l: r.l + 2 * m, h: r.h + 2 * m };
}

function dedans(r, p) {
  return p.x >= r.x && p.x <= r.x + r.l && p.y >= r.y && p.y <= r.y + r.h;
}

function distanceAuSegment(p, a, b) {
  const dx = b.x - a.x, dy = b.y - a.y;
  const l2 = dx * dx + dy * dy;
  const t = l2 ? Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / l2)) : 0;
  return Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy));
}

// Luminance relative approchée : le chiffre d'une étape claire s'écrit en noir.
function estClaire(hex) {
  const n = parseInt(hex.slice(1), 16);
  return (0.299 * (n >> 16) + 0.587 * ((n >> 8) & 255) + 0.114 * (n & 255)) / 255 > 0.6;
}

function lignesDe(o) {
  return o.texte.split("\n");
}

function dessinerFleche(c, a, b, trait) {
  const angle = Math.atan2(b.y - a.y, b.x - a.x);
  const pointe = Math.min(longueurPointe(trait), Math.hypot(b.x - a.x, b.y - a.y));
  // Le fût s'arrête au pied de la pointe : sinon son bout rond dépasse d'elle.
  const pied = pointe * Math.cos(ANGLE_POINTE);
  c.beginPath();
  c.moveTo(a.x, a.y);
  c.lineTo(b.x - pied * Math.cos(angle), b.y - pied * Math.sin(angle));
  c.stroke();
  c.beginPath();
  c.moveTo(b.x, b.y);
  c.lineTo(b.x - pointe * Math.cos(angle - ANGLE_POINTE), b.y - pointe * Math.sin(angle - ANGLE_POINTE));
  c.lineTo(b.x - pointe * Math.cos(angle + ANGLE_POINTE), b.y - pointe * Math.sin(angle + ANGLE_POINTE));
  c.closePath();
  c.fill();
}

// Un canevas de travail partagé (pixeliser, inverser, projecteur) : jamais deux à la fois.
const brouillon = document.createElement("canvas");
const brouillonCtx = brouillon.getContext("2d", { colorSpace: "srgb" });

// Le cadre `r` arrondi au pixel et borné au canevas `toile` — `null` s'il est vide.
function cadreDansToile(r, toile) {
  const x = Math.max(0, Math.floor(r.x)), y = Math.max(0, Math.floor(r.y));
  const l = Math.min(toile.width, Math.ceil(r.x + r.l)) - x;
  const h = Math.min(toile.height, Math.ceil(r.y + r.h)) - y;
  return l > 0 && h > 0 ? { x, y, l, h } : null;
}

// FLOU : le canevas lui-même, relu sur une marge de deux rayons (sinon le bord
// du cadre se floute contre du vide), redessiné flouté DANS le cadre seulement.
function flouter(c, r, rayon) {
  const s = cadreDansToile(elargir(r, 2 * rayon), c.canvas);
  if (!s) return;
  c.save();
  c.beginPath();
  c.rect(r.x, r.y, r.l, r.h);
  c.clip();
  c.filter = `blur(${rayon}px)`;
  c.drawImage(c.canvas, s.x, s.y, s.l, s.h, s.x, s.y, s.l, s.h);
  c.restore();
}

// PIXELISER : réduire (le lissage fait la MOYENNE de chaque bloc), puis
// agrandir sans lissage. Les blocs partent du coin du cadre.
function pixeliser(c, r, taille) {
  const s = cadreDansToile(r, c.canvas);
  if (!s) return;
  const l = Math.ceil(s.l / taille), h = Math.ceil(s.h / taille);
  brouillon.width = l;
  brouillon.height = h;
  brouillonCtx.imageSmoothingEnabled = true;
  brouillonCtx.imageSmoothingQuality = "high";
  brouillonCtx.drawImage(c.canvas, s.x, s.y, s.l, s.h, 0, 0, l, h);
  c.save();
  c.beginPath();
  c.rect(s.x, s.y, s.l, s.h);
  c.clip();
  c.imageSmoothingEnabled = false;
  c.drawImage(brouillon, 0, 0, l, h, s.x, s.y, l * taille, h * taille);
  c.restore();
}

// LOUPE : ce qui est sous le CENTRE du cercle, grossi, dans le cercle.
function grossir(c, o, n, facteur) {
  const r = cadreDe(o.points[0], o.points[1]);
  const cx = r.x + r.l / 2, cy = r.y + r.h / 2;
  const l = r.l / facteur, h = r.h / facteur;
  c.save();
  c.beginPath();
  c.ellipse(cx, cy, r.l / 2, r.h / 2, 0, 0, 2 * Math.PI);
  c.save();
  c.clip();
  c.imageSmoothingEnabled = false;
  c.drawImage(c.canvas, cx - l / 2, cy - h / 2, l, h, r.x, r.y, r.l, r.h);
  c.restore();
  c.strokeStyle = o.couleur;
  c.lineWidth = n.trait;
  c.stroke();
  c.restore();
}

// INVERSER (l'outil « Invert » de Flameshot) : le complément de chaque couleur,
// l'alpha intact. La « différence » avec du blanc inverse, mais peint aussi le
// transparent en blanc : l'alpha d'origine est remis par `destination-in`.
function inverser(c, r) {
  const s = cadreDansToile(r, c.canvas);
  if (!s) return;
  brouillon.width = s.l;
  brouillon.height = s.h;
  brouillonCtx.drawImage(c.canvas, s.x, s.y, s.l, s.h, 0, 0, s.l, s.h);
  brouillonCtx.globalCompositeOperation = "difference";
  brouillonCtx.fillStyle = "#fff";
  brouillonCtx.fillRect(0, 0, s.l, s.h);
  brouillonCtx.globalCompositeOperation = "destination-in";
  brouillonCtx.drawImage(c.canvas, s.x, s.y, s.l, s.h, 0, 0, s.l, s.h);
  brouillonCtx.globalCompositeOperation = "source-over";
  c.clearRect(s.x, s.y, s.l, s.h);
  c.drawImage(brouillon, s.x, s.y);
}

// L'ÉTAPE : une pastille en `a` ; glissée, une POINTE vers `b` (comme le
// compteur de Flameshot) — un triangle dont la base est le diamètre
// perpendiculaire à la pointe. Plus proche que la pastille : pas de pointe.
function dessinerEtape(c, o, n, numero) {
  const [a, b] = o.points;
  if (b && Math.hypot(b.x - a.x, b.y - a.y) > n.rayon) {
    const angle = Math.atan2(b.y - a.y, b.x - a.x);
    const nx = -Math.sin(angle) * n.rayon, ny = Math.cos(angle) * n.rayon;
    c.beginPath();
    c.moveTo(b.x, b.y);
    c.lineTo(a.x + nx, a.y + ny);
    c.lineTo(a.x - nx, a.y - ny);
    c.closePath();
    c.fill();
  }
  c.beginPath();
  c.arc(a.x, a.y, n.rayon, 0, 2 * Math.PI);
  c.fill();
  c.fillStyle = estClaire(o.couleur) ? "#000" : "#fff";
  c.font = policeDe(Math.round(n.rayon * 1.15));
  c.textAlign = "center";
  c.textBaseline = "middle";
  c.fillText(String(numero), a.x, a.y + n.rayon * 0.06);
}

// LA COULEUR DE LA GOMME : la moyenne des pixels du POURTOUR du cadre, lue sur
// `c` (le fond de l'image). Fixée à la pose — la gomme ne change pas de
// couleur quand on la déplace, comme un morceau de papier collé.
function teinteDesBords(c, r) {
  const s = cadreDansToile(r, c.canvas);
  if (!s) return "#808080";
  const bandes = [
    [s.x, s.y, s.l, 1], [s.x, s.y + s.h - 1, s.l, 1],
    [s.x, s.y, 1, s.h], [s.x + s.l - 1, s.y, 1, s.h],
  ];
  const somme = [0, 0, 0];
  let nombre = 0;
  for (const [x, y, l, h] of bandes) {
    const d = c.getImageData(x, y, l, h).data;
    for (let i = 0; i < d.length; i += 4) {
      somme[0] += d[i];
      somme[1] += d[i + 1];
      somme[2] += d[i + 2];
      nombre++;
    }
  }
  return "#" + somme.map((v) => Math.round(v / nombre).toString(16).padStart(2, "0")).join("");
}

// LES PROJECTEURS se dessinent ENSEMBLE, par-dessus tout : une seule ombre
// percée d'autant de trous. Chacun sa propre ombre, et chaque projecteur
// assombrirait la lumière des autres.
function dessinerProjecteurs(c, liste) {
  if (!liste.length) return;
  brouillon.width = c.canvas.width;
  brouillon.height = c.canvas.height;
  brouillonCtx.fillStyle = OMBRE_PROJECTEUR;
  brouillonCtx.fillRect(0, 0, brouillon.width, brouillon.height);
  // `destination-out` retire en proportion de l'alpha de la SOURCE : percer
  // avec la teinte de l'ombre (alpha 0,6) laissait 24 % d'ombre dans le trou.
  brouillonCtx.globalCompositeOperation = "destination-out";
  brouillonCtx.fillStyle = "#000";
  for (const o of liste) {
    const r = cadreDe(o.points[0], o.points[1]);
    brouillonCtx.beginPath();
    brouillonCtx.ellipse(r.x + r.l / 2, r.y + r.h / 2, r.l / 2, r.h / 2, 0, 0, 2 * Math.PI);
    brouillonCtx.fill();
  }
  brouillonCtx.globalCompositeOperation = "source-over";
  c.drawImage(brouillon, 0, 0);
}

// Dessine `o` sur le contexte `c` (en pixels de l'image). `numero` ne sert
// qu'aux étapes. Les projecteurs n'y passent pas : voir `dessinerProjecteurs`.
function dessinerObjet(c, o, numero) {
  const n = NIVEAUX[o.niveau];
  const [a, b] = o.points;
  c.save();
  c.strokeStyle = c.fillStyle = o.couleur;
  c.lineWidth = n.trait;
  c.lineCap = "round";
  c.lineJoin = "round";
  switch (o.type) {
    case "rectangle": {
      const r = cadreDe(a, b);
      c.strokeRect(r.x, r.y, r.l, r.h);
      break;
    }
    case "pave": {
      // Le contour arrondi par-dessus le plein : des coins ronds, comme Flameshot.
      const r = cadreDe(a, b);
      c.fillRect(r.x, r.y, r.l, r.h);
      c.strokeRect(r.x, r.y, r.l, r.h);
      break;
    }
    case "ellipse": {
      const r = cadreDe(a, b);
      c.beginPath();
      c.ellipse(r.x + r.l / 2, r.y + r.h / 2, r.l / 2, r.h / 2, 0, 0, 2 * Math.PI);
      c.stroke();
      break;
    }
    case "ligne":
      c.beginPath();
      c.moveTo(a.x, a.y);
      c.lineTo(b.x, b.y);
      c.stroke();
      break;
    case "fleche":
      dessinerFleche(c, a, b, n.trait);
      break;
    case "main":
      c.beginPath();
      c.moveTo(a.x, a.y);
      for (const p of o.points.slice(1)) c.lineTo(p.x, p.y);
      // Un simple clic laisse un point, pas rien.
      if (o.points.length === 1) c.lineTo(a.x + 0.01, a.y);
      c.stroke();
      break;
    case "surligneur": {
      // « Produit » : le texte sombre dessous reste lisible, comme un vrai feutre.
      const r = cadreDe(a, b);
      c.globalCompositeOperation = "multiply";
      c.fillRect(r.x, r.y, r.l, r.h);
      break;
    }
    case "texte":
      c.font = policeDe(n.police);
      c.textBaseline = "top";
      lignesDe(o).forEach((ligne, i) => c.fillText(ligne, a.x, a.y + i * n.police * INTERLIGNE));
      break;
    case "etape":
      dessinerEtape(c, o, n, numero);
      break;
    case "emoji":
      // Son point est son CENTRE : une rotation ou un miroir de l'image le
      // déplace sans le décaler, et il reste droit.
      c.font = policeEmoji(tailleEmoji(n));
      c.textAlign = "center";
      c.textBaseline = "middle";
      c.fillText(o.emoji, a.x, a.y);
      break;
    case "flou":
      flouter(c, cadreDe(a, b), n.flou);
      break;
    case "pixeliser":
      pixeliser(c, cadreDe(a, b), n.pixel);
      break;
    case "inverser":
      inverser(c, cadreDe(a, b));
      break;
    case "gomme": {
      const r = cadreDe(a, b);
      c.fillRect(r.x, r.y, r.l, r.h);
      break;
    }
    case "loupe":
      grossir(c, o, n, n.loupe);
      break;
  }
  c.restore();
}

// Le rectangle qui contient `o` à l'écran. `c` sert à mesurer le texte.
function boiteDe(o, c) {
  const n = NIVEAUX[o.niveau];
  const [a, b] = o.points;
  if (A_DEUX_POINTS.has(o.type)) {
    return elargir(cadreDe(a, b), o.type === "fleche" ? longueurPointe(n.trait) / 2 : n.trait / 2);
  }
  if (o.type === "main") {
    const xs = o.points.map((p) => p.x), ys = o.points.map((p) => p.y);
    const x = Math.min(...xs), y = Math.min(...ys);
    return elargir({ x, y, l: Math.max(...xs) - x, h: Math.max(...ys) - y }, n.trait / 2);
  }
  if (o.type === "texte") {
    c.save();
    c.font = policeDe(n.police);
    const l = Math.max(...lignesDe(o).map((ligne) => c.measureText(ligne).width));
    c.restore();
    return { x: a.x, y: a.y, l, h: lignesDe(o).length * n.police * INTERLIGNE };
  }
  if (o.type === "emoji") {
    const cote = tailleEmoji(n);
    return { x: a.x - cote / 2, y: a.y - cote / 2, l: cote, h: cote };
  }
  const pastille = { x: a.x - n.rayon, y: a.y - n.rayon, l: 2 * n.rayon, h: 2 * n.rayon };
  if (!b) return pastille;
  const x = Math.min(pastille.x, b.x), y = Math.min(pastille.y, b.y);
  return { x, y, l: Math.max(pastille.x + pastille.l, b.x) - x, h: Math.max(pastille.y + pastille.h, b.y) - y };
}

// `p` touche-t-il `o` ? `tol` en pixels de l'image (quelques pixels d'écran).
function touche(o, p, tol, c) {
  const marge = tol + NIVEAUX[o.niveau].trait / 2;
  const [a, b] = o.points;
  switch (o.type) {
    case "ligne":
    case "fleche":
      return distanceAuSegment(p, a, b) <= marge;
    case "main":
      if (o.points.length === 1) return Math.hypot(p.x - a.x, p.y - a.y) <= marge;
      return o.points.slice(1).some((q, i) => distanceAuSegment(p, o.points[i], q) <= marge);
    case "ellipse": {
      const r = cadreDe(a, b);
      const rx = r.l / 2 + marge, ry = r.h / 2 + marge;
      const dx = (p.x - r.x - r.l / 2) / rx, dy = (p.y - r.y - r.h / 2) / ry;
      return dx * dx + dy * dy <= 1;
    }
    default:
      return dedans(elargir(boiteDe(o, c), tol), p);
  }
}

function deplacer(o, dx, dy) {
  return { ...o, points: o.points.map((p) => ({ x: p.x + dx, y: p.y + dy })) };
}

// Maj pendant le tracé : un carré, un cercle, une ligne à 45°.
function contraindre(type, a, b) {
  const dx = b.x - a.x, dy = b.y - a.y;
  if (type === "ligne" || type === "fleche" || type === "etape") {
    const pas = Math.PI / 4;
    const angle = Math.round(Math.atan2(dy, dx) / pas) * pas;
    const l = Math.hypot(dx, dy);
    return { x: a.x + l * Math.cos(angle), y: a.y + l * Math.sin(angle) };
  }
  const cote = Math.max(Math.abs(dx), Math.abs(dy));
  return { x: a.x + Math.sign(dx || 1) * cote, y: a.y + Math.sign(dy || 1) * cote };
}

// ANNULER/REFAIRE PAR INSTANTANÉS (décision du plan) : on note l'état AVANT
// chaque changement ; annuler rend l'instantané, jamais une commande inverse.
// Un état = `{ calques, actif, largeur, hauteur }` (editeur_calques.js) : les
// objets de chaque calque sont figés en JSON, sa toile (un canevas) est gardée
// PAR RÉFÉRENCE — une opération de géométrie en crée une nouvelle au lieu de
// modifier l'ancienne (voir editeur_geometrie.js).
function creerHistorique(limite = 200) {
  let passe = [];
  let futur = [];
  const figer = (etat) => ({ ...etat, calques: etat.calques.map((k) => ({ ...k, objets: JSON.stringify(k.objets) })) });
  const degeler = (e) => ({ ...e, calques: e.calques.map((k) => ({ ...k, objets: JSON.parse(k.objets) })) });
  return {
    noter(etat) {
      passe = [...passe, figer(etat)].slice(-limite);
      futur = [];
    },
    annuler(etat) {
      if (!passe.length) return null;
      futur = [...futur, figer(etat)];
      const precedent = passe[passe.length - 1];
      passe = passe.slice(0, -1);
      return degeler(precedent);
    },
    retablir(etat) {
      if (!futur.length) return null;
      passe = [...passe, figer(etat)];
      const suivant = futur[futur.length - 1];
      futur = futur.slice(0, -1);
      return degeler(suivant);
    },
    vider() {
      passe = [];
      futur = [];
    },
  };
}
