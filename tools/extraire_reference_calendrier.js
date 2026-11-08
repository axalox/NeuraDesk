// Extrait la reference calendrier.com 2026 du PDF et la compare a
// src/win32/fetes_france.tsv.
//
// Le PDF stocke son texte en chaines HEXADECIMALES avec une police
// sous-ensemble : chaque glyphe est un CID de 2 octets, decode par la table
// /ToUnicode de SA police (il y en a trois : F6 titres, F7 corps, F8 numeros
// de semaine). Les positions viennent de « 1 0 0 -1 x y Tm » puis de
// « n 0 Td » relatifs.
const fs = require('fs'), zlib = require('zlib');

function extraire(chemin) {
  const buf = fs.readFileSync(chemin), s = buf.toString('latin1');
  const objFlux = new Map(), objEntete = new Map();
  { const re = /(\d+)\s+0\s+obj/g; let m;
    while ((m = re.exec(s))) {
      const num = +m[1], debut = m.index + m[0].length;
      const fin = s.indexOf('endobj', debut); if (fin < 0) continue;
      const corps = s.slice(debut, fin); const a = corps.indexOf('stream');
      if (a >= 0) {
        let st = debut + a + 6; if (s[st] === '\r') st++; if (s[st] === '\n') st++;
        const b = s.indexOf('endstream', st);
        try { objFlux.set(num, zlib.inflateSync(buf.subarray(st, b)).toString('latin1')); } catch (e) {}
        objEntete.set(num, corps.slice(0, a));
      } else objEntete.set(num, corps);
    } }
  const fontRef = {};
  for (const m of s.matchAll(/\/Font\s*<<([\s\S]*?)>>/g))
    for (const f of m[1].matchAll(/\/(F\d+)\s+(\d+)\s+0\s+R/g)) fontRef[f[1]] = +f[2];
  const decodeurs = {};
  for (const [nom, num] of Object.entries(fontRef)) {
    const tu = (objEntete.get(num) || '').match(/\/ToUnicode\s+(\d+)\s+0\s+R/); if (!tu) continue;
    const cm = objFlux.get(+tu[1]); if (!cm) continue;
    const map = {};
    for (const blk of cm.matchAll(/beginbfchar([\s\S]*?)endbfchar/g))
      for (const p of blk[1].matchAll(/<([0-9A-Fa-f]+)>\s*<([0-9A-Fa-f]+)>/g))
        map[p[1].toUpperCase()] = String.fromCharCode(parseInt(p[2].slice(0, 4), 16));
    for (const blk of cm.matchAll(/beginbfrange([\s\S]*?)endbfrange/g))
      for (const p of blk[1].matchAll(/<([0-9A-Fa-f]+)>\s*<([0-9A-Fa-f]+)>\s*<([0-9A-Fa-f]+)>/g)) {
        const a = parseInt(p[1], 16), b = parseInt(p[2], 16), c = parseInt(p[3].slice(0, 4), 16);
        for (let k = 0; k <= b - a; k++) map[(a + k).toString(16).toUpperCase().padStart(4, '0')] = String.fromCharCode(c + k);
      }
    decodeurs[nom] = map;
  }
  let contenu = null;
  for (const [, t] of objFlux) if (t && (t.match(/Tj/g) || []).length > 3000) contenu = t;
  const items = []; let police = null, x = 0, y = 0, dx = 0, dy = 0;
  const re = /\/(F\d+)\s+[\d.]+\s+Tf|1 0 0 -1 ([-\d.]+) ([-\d.]+) Tm|([-\d.]+) ([-\d.]+) Td|<([0-9A-Fa-f]+)>\s*Tj/g;
  let m;
  while ((m = re.exec(contenu))) {
    if (m[1]) police = m[1];
    else if (m[2] !== undefined) { x = +m[2]; y = +m[3]; dx = 0; dy = 0; }
    else if (m[4] !== undefined) { dx += +m[4]; dy += +m[5]; }
    else if (m[6] !== undefined) {
      const d = decodeurs[police]; if (!d) continue;
      const h = m[6].toUpperCase(); let t = '';
      for (let k = 0; k < h.length; k += 4) t += d[h.slice(k, k + 4)] || '';
      if (t.trim()) items.push({ x: x + dx, y: y + dy, t, police });
    }
  }
  // segments : on coupe quand l'ecart depasse ~9 unites (l'interlettre fait ~7)
  const parY = new Map();
  items.forEach(o => { const k = Math.round(o.y); if (!parY.has(k)) parY.set(k, []); parY.get(k).push(o); });
  const segs = [];
  for (const [yy, arr] of parY) {
    arr.sort((a, b) => a.x - b.x);
    let cur = [], prev = null;
    const flush = () => { if (cur.length) { segs.push({ y: yy, x: cur[0].x, t: cur.map(z => z.t).join(''), police: cur[0].police }); cur = []; } };
    for (const o of arr) { if (prev !== null && o.x - prev > 9) flush(); cur.push(o); prev = o.x + 2.2; }
    flush();
  }
  return segs;
}

const MOIS = ['JANVIER', 'FÉVRIER', 'MARS', 'AVRIL', 'MAI', 'JUIN', 'JUILLET', 'AOÛT', 'SEPTEMBRE', 'OCTOBRE', 'NOVEMBRE', 'DÉCEMBRE'];
const NUM = { JANVIER: 1, 'FÉVRIER': 2, MARS: 3, AVRIL: 4, MAI: 5, JUIN: 6, JUILLET: 7, 'AOÛT': 8, SEPTEMBRE: 9, OCTOBRE: 10, NOVEMBRE: 11, 'DÉCEMBRE': 12 };

//   node tools/extraire_reference_calendrier.js <calendrier.pdf> [table.tsv]
const path = require('path');
const PDF = process.argv[2] || 'calendrier.pdf';
const TABLE = process.argv[3] || path.join(__dirname, '..', 'src', 'win32', 'fetes_france.tsv');

const segs = extraire(PDF);
const titres = segs.filter(l => MOIS.includes(l.t.toUpperCase()));
const blocs = {};
titres.forEach(t => { const y = Math.round(t.y); (blocs[y] ||= []).push({ x: t.x, mois: NUM[t.t.toUpperCase()] }); });

// ⚠️ **ON NE DECOUPE PAS PAR TRANCHES DE `x`** : les colonnes n'ont pas la meme
// largeur (les titres sont centres sur leur mot), et une borne trop a droite
// fait fuir la lettre de la colonne suivante dans la precedente — la lettre de
// MAI est a x=544 alors que sa colonne commence a 547 .
// Le motif **(lettre de jour)(nombre)** est sans ambiguite : il n'apparait que
// la ou une cellule commence.
const LETTRES = new Set(['J', 'V', 'S', 'D', 'L', 'M']);

// Centre de chaque colonne : la lettre est a titreX-40 environ.
const reference = {};   // "M/J" -> nom
for (const [yTitre, cols] of Object.entries(blocs)) {
  cols.sort((a, b) => a.x - b.x);
  const y0 = +yTitre, yFin = y0 < 300 ? 571 : 1100;
  const rows = [...new Set(segs.filter(s => s.y > y0 && s.y < yFin && s.police === 'F7').map(s => s.y))].sort((a, b) => a - b);
  for (const y of rows) {
    const ss = segs.filter(s => s.y === y && s.police === 'F7').sort((p, q) => p.x - q.x);
    // ⚠️ **LE NUMERO DE JOUR ET LE NOM FUSIONNENT** sur la 2e ligne de chaque
    // semaine : « [32]11Paulin » = le 11 janvier colle a « Paulin », l'ecart
    // etant plus petit que l'interlettre. On detecte donc la cellule sur
    // (lettre)(segment COMMENCANT par un chiffre), et on separe les chiffres.
    const debuts = [];
    for (let k = 0; k + 1 < ss.length; k++)
      if (ss[k].t.length === 1 && LETTRES.has(ss[k].t) && /^\d/.test(ss[k + 1].t)) debuts.push(k);
    for (let d = 0; d < debuts.length; d++) {
      const k = debuts[d];
      const fin = d + 1 < debuts.length ? debuts[d + 1] : ss.length;
      const brut = ss[k + 1].t + ss.slice(k + 2, fin).map(z => z.t).join(' ');
      const mm = brut.match(/^(\d{1,2})\s*(.*)$/);
      if (!mm) continue;
      const jour = parseInt(mm[1], 10);
      if (!(jour >= 1 && jour <= 31)) continue;
      const nom = mm[2].trim();
      // a quelle colonne appartient cette cellule ? la plus proche a gauche
      const xc = ss[k].x;
      let col = null;
      for (const c of cols) if (c.x - 50 <= xc + 5) col = c;
      if (!col) continue;
      if (nom || !(col.mois + '/' + jour in reference)) reference[col.mois + '/' + jour] = nom;
    }
  }
}
const jours = Object.keys(reference).length;
console.log('=== REFERENCE calendrier.com 2026 : ' + jours + ' jours avec un nom ===');
// ⚠️ La sortie vit DANS tools/ et elle est VERSIONNEE : c'est l'entree de
// `reconstruire_fetes.js`, qui retomberait sinon silencieusement sur le xlsx
// seul. La re-extraire ecrase donc un fichier suivi — le relire avant.
fs.writeFileSync(path.join(__dirname, 'calendrier_2026.reference.json'), JSON.stringify(reference), 'utf8');

// ---- comparaison avec la table
const t = fs.readFileSync(TABLE, 'utf8');
const table = {};
t.split('\n').filter(l => l.includes('\t')).map(l => l.split('\t'))
  .forEach(c => { if (c[4]) return; const k = +c[0] + '/' + +c[1]; (table[k] ||= []).push(c[2]); });

// \u26a0\ufe0f La reference perd parfois les espaces entre mots (\u00ab CharlesleBon \u00bb,
// \u00ab AntoinedePadoue \u00bb) parce que l'interlettre et l'espace se ressemblent : on
// compare donc SANS aucun separateur, et par PREFIXE dans les deux sens \u2014 la
// reference abrege aussi (\u00ab J.-B.delaSalle \u00bb, \u00ab Th\u00e9.del'E.-J\u00e9sus \u00bb).
const norm = x => x.normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase()
  .replace(/[^a-z0-9]/g, '');
// La reference ecrit parfois DEUX prenoms dans une cellule (\u00ab Patrick/Patrice \u00bb,
// \u00ab \u00c9l\u00e9onore/Prosper \u00bb, \u00ab Judica\u00ebl/Ga\u00ebl \u00bb) : chacun est une alternative.
const alternatives = x => norm(x).split('/').filter(Boolean);

// \u26d4 PAS de table d'abreviations par prefixe : un remplacement \u00ab ^the -> therese \u00bb
// corrompt Theophane, Theodore, Theophile et Thecle (mesure : 4 faux ecarts).
// On compare par RECOUVREMENT \u2014 prefixe ou inclusion dans un sens ou l'autre \u2014
// ce qui attrape aussi les abreges (\u00ab Cath.deSuede \u00bb ~ \u00ab Catherine \u00bb... non :
// ceux-la sont listes a part, car un abreg\u00e9 ne se devine pas).
let accord = 0, ecart = 0, tableSeule = 0;
const ecarts = [];
for (const [k, nom] of Object.entries(reference)) {
  const nos = (table[k] || []).map(norm).filter(Boolean);
  const trouve = alternatives(nom).some(n => nos.some(x =>
    x === n || x.startsWith(n) || n.startsWith(x) || x.includes(n) || n.includes(x)));
  if (trouve) accord++; else { ecart++; ecarts.push({ k, ref: nom, nous: (table[k] || []).join(', ') || '(rien)' }); }
}
for (const k of Object.keys(table)) if (!reference[k]) tableSeule++;
console.log('\naccords (le jour porte bien le saint nomme par la reference) : ' + accord);
console.log('ECARTS                                                        : ' + ecart);
console.log('jours de notre table absents de la reference                  : ' + tableSeule);
console.log('\n=== LES ' + ecarts.length + ' ECARTS ===');
ecarts.forEach(e => console.log('  ' + e.k.padStart(5) + '  reference « ' + e.ref.padEnd(26) + ' »  nous : ' + e.nous));
