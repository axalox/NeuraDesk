// Re-derivation of fetes_france.tsv.
//
// MODEL (measured):
//   The user's CSV is a ONE-SAINT-PER-DAY calendar. Each month block lists its
//   saints in TRUE CHRONOLOGICAL ORDER (2 inversions out of 238 anchored
//   entries), but the days are just the position in the list. A stray entry —
//   a saint that also lives in its real month — consumes a slot and shifts
//   everything after it by +1.
//
//   The true days come from TWO ANCHOR SOURCES, and they pin the shift, which
//   makes the realignment deterministic:
//     1. the XLSX (Nominis), which dates 237 of the entries;
//     2. calendrier.com, via `extraire_reference_calendrier.js`, whose 365 days
//        add 105 anchors the XLSX does not have.
//   Mesure : les deux s'accordent 227 fois sur 228. Le seul conflit est
//   Catherine (XLSX 24/11, reference 25/11) et c'est la REFERENCE qui a raison
//   — la tradition francaise la met au 25.
const fs = require('fs');

// Les chemins de la SOURCE sont propres a cette machine (l'archive xlsx
// extraite vit hors du depot) : on les passe en argument, avec les valeurs qui
// ont servi le 2026-09-24.
//   node tools/reconstruire_fetes.js <sheet2.xml> <fetes.csv> <sortie.tsv> [reference.json]
// La reference est OPTIONNELLE : sans elle la table se derive du seul XLSX,
// avec 105 ancres de moins et les derives locales qu'on a mesurees en octobre.
const path = require('path');
const XLSX_XML = process.argv[2] || 'C:/ndwork/xlsx/xl/worksheets/sheet2.xml';
const CSV = process.argv[3] || 'fetes_francaises_complet.csv';
const SORTIE = process.argv[4] || path.join(__dirname, '..', 'src', 'win32', 'fetes_france.tsv');
// ⚠️ La reference est VERSIONNEE dans tools/ : sans elle le script retomberait
// SILENCIEUSEMENT sur le xlsx seul (105 ancres de moins), et produirait une
// table differente — c'est exactement la derive d'octobre qu'on a corrigee.
const REF_JSON = process.argv[5] || path.join(__dirname, 'calendrier_2026.reference.json');
const OUT_JSON = SORTIE.replace(/\.tsv$/, '.json');

const dec = s => s.replace(/&#(\d+);/g, (_, d) => String.fromCharCode(+d))
  .replace(/&amp;/g, '&').replace(/&lt;/g, '<').replace(/&gt;/g, '>')
  .replace(/&quot;/g, '"').replace(/&apos;/g, "'");
const norm = s => s.normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase()
  .replace(/[\u2019'`]/g, ' ').replace(/-/g, ' ').replace(/\s+/g, ' ').trim();
// ⚠️ Cle SANS AUCUN SEPARATEUR, pour la reference seulement : son extraction
// perd parfois les espaces entre mots (« CharlesleBon » = « Charles le Bon »)
// et la nôtre garde les siens, donc comparer sur `norm` raterait le nom.
const cleRef = s => s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase()
  .replace(/[^a-z0-9]/g, '');
const JDM = [0, 31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

// ---------------------------------------------------------------- sources
const xx = fs.readFileSync(XLSX_XML, 'utf8');
const xrows = [...xx.matchAll(/<row r="(\d+)"[^>]*>([\s\S]*?)<\/row>/g)].slice(1).map(m => {
  const c = {};
  for (const cc of m[2].matchAll(/<c r="([A-F])\d+"[^>]*>([\s\S]*?)<\/c>/g)) {
    const t = cc[2].match(/<t[^>]*>([\s\S]*?)<\/t>/);
    c[cc[1]] = t ? dec(t[1]) : '';
  }
  return { A: c.A || '', B: c.B || '', C: c.C || '', D: (c.D || '').trim() };
});
const baseDates = new Map();   // folded name -> Map(month -> Set(day)), base rows only
for (const r of xrows) {
  if (r.C !== 'Non') continue;
  const k = norm(r.A);
  if (!baseDates.has(k)) baseDates.set(k, new Map());
  const per = baseDates.get(k);
  for (const d of r.D.split(';').map(s => s.trim()).filter(Boolean)) {
    const [j, m] = d.split('/').map(Number);
    if (!per.has(m)) per.set(m, new Set());
    per.get(m).add(j);
  }
}
// ---- SECONDE SOURCE D'ANCRES : calendrier.com (voir l'en-tete).
// Le fichier nomme UN SAINT PAR JOUR ; certaines cellules en portent deux,
// separes par « / » (« Patrick/Patrice », « Éléonore/Prosper »), et chacun est
// une alternative. Les espaces entre mots y sont parfois perdus et les noms
// abreges (« Cath.deSuede ») : on ne fait donc PAS de correspondance exacte,
// seulement un recouvrement — les deux autorites se recoupent 227 fois sur 228,
// le seul ecart etant Catherine, tranchee en faveur de la reference.
const refDates = new Map();   // nom replie -> Map(mois -> Set(jour))
let refLue = 0;
try {
  const brut = JSON.parse(fs.readFileSync(REF_JSON, 'utf8'));
  for (const [cle, valeur] of Object.entries(brut)) {
    const [m, j] = cle.split('/').map(Number);
    for (const alt of String(valeur).split('/')) {
      const k = cleRef(alt); if (!k) continue;
      if (!refDates.has(k)) refDates.set(k, new Map());
      const pm = refDates.get(k);
      if (!pm.has(m)) pm.set(m, new Set());
      pm.get(m).add(j);
    }
  }
  refLue = Object.keys(brut).length;
} catch (e) { /* pas de reference : on se derive du seul XLSX */ }

// A name is an ORTHOGRAPHIC VARIANT when the XLSX carries a row whose A is that
// exact spelling with C = "Oui"; its B is then the reference form. Keyed on the
// RAW spelling, never the folded one: "Jean Claude" and "Jean-Claude" are two
// distinct search keys and the table must keep both.
const varianteDe = new Map();
for (const r of xrows) if (r.C === 'Oui' && r.A) varianteDe.set(r.A, r.B);

const csvTout = fs.readFileSync(CSV, 'utf8').replace(/^\uFEFF/, '')
  .split(/\r?\n/).filter(l => l.trim()).slice(1).map(l => {
    const p = l.split(';'); const [js, ms] = p[0].split('/');
    return { jj: +js, mm: +ms, nom: p[1], cat: p[2], date: p[0] };
  });
// Rows whose date is not JJ/MM are the CALCULATED feasts (Ascension, F\u00EAte des
// M\u00E8res...) \u2014 `fetes.rs` computes them, they never enter the table.
const sansDate = csvTout.filter(r => !/^\d{2}\/\d{2}$/.test(r.date));
const csv = csvTout.filter(r => /^\d{2}\/\d{2}$/.test(r.date));
const isSaint = r => /saint/i.test(r.cat);
const csvMonthsOf = new Map();  // folded name -> Set(months)  [saints only]
for (const r of csv) {
  if (!isSaint(r)) continue;
  const k = norm(r.nom);
  if (!csvMonthsOf.has(k)) csvMonthsOf.set(k, new Set());
  csvMonthsOf.get(k).add(r.mm);
}

// ---------------------------------------------------------------- per month
const kept = [], removed = [], notes = [];

for (let m = 1; m <= 12; m++) {
  const saints = csv.filter(r => r.mm === m && isSaint(r));
  const D = JDM[m];
  const excess = Math.max(0, saints.length - D);

  // Anchors: first occurrence of a name in the month only (a later repeat is a
  // different saint — Therese de Lisieux 1/10 vs Therese d'Avila 15/10).
  const seen = new Map();
  const anchors = [];
  saints.forEach((r, idx) => {
    const k = norm(r.nom);
    seen.set(k, (seen.get(k) || 0) + 1);
    if (seen.get(k) > 1) return;
    // La REFERENCE passe en premier : les deux sources s'accordent 227 fois sur
    // 228, et sur le seul desaccord (Catherine, 24 vs 25 novembre) c'est elle
    // qui a raison. L'ecart est signale plus bas.
    const duRef = refDates.get(cleRef(r.nom))?.get(m);
    const duXlsx = baseDates.get(k)?.get(m);
    let d = null, source = '';
    if (duRef && duRef.size) { d = Math.min(...duRef); source = 'reference'; }
    else if (duXlsx && duXlsx.size) { d = Math.min(...duXlsx); source = 'xlsx'; }
    if (d === null) return;
    if (duRef && duRef.size && duXlsx && duXlsx.size && Math.min(...duXlsx) !== d)
      notes.push(`mois ${m}: « ${r.nom} » — XLSX dit ${Math.min(...duXlsx)}, reference dit ${d} ; reference retenue`);
    anchors.push({ i: idx + 1, d, nom: r.nom, source });
  });
  // R(i) = i - trueDay(i) is NOT monotone: R RISING = a stray ate a slot, R
  // FALLING = a real day carries no saint (November: Flora and Catherine share
  // 24/11, then Delphine jumps to 26 — day 25 is empty). Only a DAY going
  // backwards contradicts the CSV's order, so only that drops an anchor.
  const pinned = [];
  let prevD = 0;
  for (const a of anchors) {
    if (a.d < prevD) { notes.push(`mois ${m}: ancre « ${a.nom} » ignorée (jour ${a.d} < ${prevD}) — ordre contredit`); continue; }
    pinned.push(a); prevD = a.d;
  }

  // Where the shift increases, a slot is lost — that is exactly where a stray
  // must sit. Rank the cross-month duplicates by how structurally forced they are.
  const anchored = new Set(pinned.map(a => a.i));
  const cands = [];
  saints.forEach((r, idx) => {
    const i = idx + 1;
    if (anchored.has(i)) return;
    const k = norm(r.nom);
    if (!(csvMonthsOf.get(k)?.size > 1)) return;      // not a cross-month duplicate
    // how many anchor pairs straddle i with a RISING shift (R = i - day)?
    let forced = 0;
    for (let a = 0; a < pinned.length; a++) {
      const lo = a === 0 ? { i: 0, d: 0 } : pinned[a - 1];
      const hi = pinned[a];
      const rLo = lo.i - lo.d, rHi = hi.i - hi.d;
      if (i > lo.i && i <= hi.i && rHi > rLo) forced += rHi - rLo;
    }
    cands.push({ i, nom: r.nom, forced, others: csvMonthsOf.get(k).size });
  });
  // Days for a given removal set: interpolate between anchors, count back
  // before the first and forward after the last, then clamp and enforce order.
  const jours = delSet => {
    const surv = saints.filter((_, idx) => !delSet.has(idx + 1));
    const anc = pinned.map(a => ({ s: a.i - [...delSet].filter(d => d < a.i).length, d: a.d }));
    const M = surv.length;
    const day = new Array(M + 1).fill(null);
    anc.forEach(a => day[a.s] = a.d);
    if (anc.length) {
      for (let s = anc[0].s - 1; s >= 1; s--) day[s] = day[s + 1] - 1;
      for (let a = 0; a < anc.length - 1; a++) {
        const p = anc[a], q = anc[a + 1];
        for (let s = p.s + 1; s < q.s; s++) day[s] = p.d + Math.floor((q.d - p.d) * (s - p.s) / (q.s - p.s));
      }
      const last = anc[anc.length - 1];
      for (let s = last.s + 1; s <= M; s++) day[s] = day[s - 1] + 1;
    } else for (let s = 1; s <= M; s++) day[s] = s;
    for (let s = 1; s <= M; s++) {
      day[s] = Math.max(1, Math.min(D, day[s]));
      if (s > 1 && day[s] < day[s - 1]) day[s] = day[s - 1];
    }
    return { surv, day, M };
  };

  // Score a removal set: empty days first, then XLSX disagreements, then size.
  const note = delSet => {
    const { surv, day, M } = jours(delSet);
    const vus = new Set();
    for (let s = 1; s <= M; s++) vus.add(day[s]);
    const vides = D - vus.size;
    let ecarts = 0;
    for (let s = 1; s <= M; s++) {
      const d = baseDates.get(norm(surv[s - 1].nom));
      if (!d) continue;                          // XLSX doesn't know this name
      let ok = false;
      for (const [, jours2] of d) if (jours2.has(day[s])) ok = true;
      if (!ok) ecarts++;
    }
    return { vides, ecarts, n: delSet.size };
  };
  const mieux = (a, b) => a.vides !== b.vides ? a.vides < b.vides
    : a.ecarts !== b.ecarts ? a.ecarts < b.ecarts : a.n < b.n;

  // Only cross-month duplicates are removable, and never an anchor. The
  // candidate pool is small, so enumerate subsets and let the objective choose:
  // neither the anchors nor `saints - daysInMonth` alone can tell a genuine
  // stray from a saint whose feast simply shares a day with another.
  let best = { set: new Set(), score: note(new Set()) };
  const pool = cands.map(c => c.i);
  const maxK = Math.min(pool.length, Math.max(0, saints.length - D) + 1);
  const combos = [];
  const rec = (start, cur) => {
    combos.push([...cur]);
    if (cur.length >= maxK) return;
    for (let a = start; a < pool.length; a++) { cur.push(pool[a]); rec(a + 1, cur); cur.pop(); }
  };
  rec(0, []);
  for (const c of combos) {
    const s = note(new Set(c));
    if (mieux(s, best.score)) best = { set: new Set(c), score: s };
  }
  const del = best.set;
  for (const i of del) {
    const r = saints[i - 1]; const k = norm(r.nom);
    const ailleurs = csv.filter(x => isSaint(x) && norm(x.nom) === k && x.mm !== m).map(x => x.jj + '/' + x.mm);
    removed.push({ mm: m, i, nom: r.nom, forced: cands.find(c => c.i === i)?.forced ?? 0, ailleurs });
  }
  notes.push(`mois ${m}: ${saints.length} saints / ${D} jours -> ${del.size} retrait(s), `
    + `${best.score.vides} jour(s) vide(s), ${best.score.ecarts} écart(s) XLSX`
    + (cands.length ? `  [candidats: ${cands.map(c => c.nom).join(', ')}]` : ''));

  const { surv, day, M } = jours(del);
  // A day left empty while an adjacent day carries two saints is almost always
  // the XLSX being one day out (Catherine d'Alexandrie is 25/11 in the French
  // calendar, not 24) — the month's own order supports the move. Only the last
  // entry of a doubled day moves, and only if the order still holds.
  for (let pass = 0; pass < D; pass++) {
    const compte = new Map();
    for (let s = 1; s <= M; s++) { if (!compte.has(day[s])) compte.set(day[s], []); compte.get(day[s]).push(s); }
    const vide = [...Array(D).keys()].map(k => k + 1).find(j => !compte.has(j));
    if (vide === undefined) break;
    let src = null;
    for (const [d, list] of compte) if (list.length >= 2 && (src === null || Math.abs(d - vide) < Math.abs(src - vide))) src = d;
    if (src === null) break;
    const list = compte.get(src); const s = list[list.length - 1];
    const lo = s > 1 ? day[s - 1] : 1, hi = s < M ? day[s + 1] : D;
    if (vide < lo || vide > hi) break;
    day[s] = vide;
    notes.push(`mois ${m}: « ${surv[s - 1].nom} » déplacé du ${src} au ${vide} (jour vide comblé)`);
  }
  // CSV rows are ALWAYS displayed. Only XLSX rows can be variants: "Alix"
  // legitimately appears twice — displayed on 10/01 (the CSV's own saint) and
  // as a variant of "Alice" on 16/12 (the XLSX's spelling). Both roles coexist.
  surv.forEach((r, idx) => kept.push({ mm: m, jj: day[idx + 1], nom: r.nom, cat: 'saint' }));
}

// non-saint rows keep their CSV day (fixed dates)
const catMap = c => /férié légal/i.test(c) ? 'ferie' : /fête civile/i.test(c) ? 'civil' : 'tradition';
for (const r of csv.filter(x => !isSaint(x))) kept.push({ mm: r.mm, jj: r.jj, nom: r.nom, cat: catMap(r.cat) });

// ---------------------------------------------------------------- XLSX additions
// Keyed on the RAW spelling: "Jean Claude" and "Jean-Claude" are two rows.
const present = new Set(kept.map(k => k.nom + '|' + k.mm + '/' + k.jj));
// A prénom appears at most ONCE per month. No multi-date XLSX row spans the
// same month (Jacques 03/05+25/07, Jean 24/06+27/12, Marie 15/08+08/09), so
// skipping a row whose name the month already carries loses nothing — and it
// stops the XLSX re-adding Catherine at 24/11 when the CSV put her at 25/11.
const parMois = new Set(kept.map(k => k.nom + '|' + k.mm));
let addedRef = 0, addedVar = 0;
for (const r of xrows) {
  if (!r.D) continue;
  for (const d of r.D.split(';').map(s => s.trim()).filter(Boolean)) {
    const [j, m] = d.split('/').map(Number);
    if (!(m >= 1 && m <= 12) || !(j >= 1 && j <= JDM[m])) continue;
    const key = r.A + '|' + m + '/' + j;
    if (present.has(key)) continue;
    const km = r.A + '|' + m;
    if (parMois.has(km)) continue;
    parMois.add(km);
    present.add(key);
    if (r.C === 'Non') { kept.push({ mm: m, jj: j, nom: r.A, cat: 'saint' }); addedRef++; }
    else { kept.push({ mm: m, jj: j, nom: r.A, cat: 'saint', variante: r.B }); addedVar++; }
  }
}
// A row identical to another except for its variant column must not exist.
const vues = new Set();
for (const k of [...kept]) {
  const key = k.nom + '|' + k.mm + '/' + k.jj;
  if (vues.has(key)) { kept.splice(kept.indexOf(k), 1); continue; }
  vues.add(key);
}

// ---------------------------------------------------------------- report
console.log('=== RETRAITS (' + removed.length + ') ===');
for (const r of removed)
  console.log(`  mois ${String(r.mm).padStart(2)}  i=${String(r.i).padStart(2)}  ${r.nom.padEnd(20)}`
    + `  forcé=${r.forced}  ailleurs: ${r.ailleurs.join(', ')}`);
console.log('\n=== NOTES (' + notes.length + ') ===');
notes.forEach(w => console.log('  ' + w));

const aff = kept.filter(k => !k.variante);
console.log('\n=== COMPTES ===');
console.log('  lignes totales : ' + kept.length + '   affichées : ' + aff.length + '   variantes : ' + kept.filter(k => k.variante).length);
console.log('  ajouts XLSX    : ' + addedRef + ' réf. + ' + addedVar + ' var.');
const pm = {}; aff.forEach(k => pm[k.mm] = (pm[k.mm] || 0) + 1);
console.log('  affichées/mois : ' + Object.entries(pm).map(([m, n]) => m + ':' + n).join(' '));
const dups = new Map();
for (const k of kept) { const key = k.mm + '/' + k.jj + '|' + k.nom; dups.set(key, (dups.get(key) || 0) + 1); }
const dbl = [...dups.entries()].filter(([, n]) => n > 1);
console.log('  doublons (nom EXACT) : ' + dbl.length + '   29/02 : ' + kept.some(k => k.mm === 2 && k.jj === 29));
dbl.slice(0, 10).forEach(([k]) => console.log('     ' + k));
const dblN = new Map();
for (const k of kept) { const key = k.mm + '/' + k.jj + '|' + norm(k.nom); dblN.set(key, (dblN.get(key) || 0) + 1); }
console.log('  (normalisés, tolérés : Jean Claude / Jean-Claude) : ' + [...dblN.values()].filter(n => n > 1).length);
const parJour = {};
aff.forEach(k => { const c = k.mm + '/' + k.jj; parJour[c] = (parJour[c] || 0) + 1; });
const joursVides = [];
for (let m = 1; m <= 12; m++) for (let j = 1; j <= JDM[m]; j++) if (!parJour[m + '/' + j]) joursVides.push(m + '/' + j);
console.log('  jours SANS fête : ' + joursVides.length + (joursVides.length ? '  -> ' + joursVides.join(' ') : ''));
console.log('  lignes CSV sans date (calculées) : ' + sansDate.length + ' -> ' + sansDate.map(r => r.nom).join(', '));

// ---- CONTRÔLE : chaque entrée dont le XLSX connaît une date tombe-t-elle dessus ?
const dateX = new Map();   // folded name -> Set("MM/JJ")
for (const r of xrows) for (const f of [r.A, r.B]) if (f) {
  const k = norm(f); if (!dateX.has(k)) dateX.set(k, new Set());
  for (const d of r.D.split(';').map(s => s.trim()).filter(Boolean)) {
    const [j, m] = d.split('/').map(Number);
    if (m >= 1 && m <= 12 && j >= 1 && j <= JDM[m]) dateX.get(k).add(m + '/' + j);
  }
}
let accord = 0, desaccord = 0;
const des = [];
for (const k of aff) {
  const dates = dateX.get(norm(k.nom));
  if (!dates || !dates.size) continue;
  if (dates.has(k.mm + '/' + k.jj)) accord++; else { desaccord++; des.push(k); }
}
console.log('\n=== CONTRÔLE XLSX (entrées affichées dont le XLSX connaît la date) ===');
console.log('  tombent sur une date XLSX : ' + accord);
console.log('  EN ÉCART                  : ' + desaccord);
des.slice(0, 30).forEach(k => console.log('     ' + String(k.mm).padStart(2) + '/' + String(k.jj).padStart(2)
  + '  ' + k.nom.padEnd(20) + ' XLSX: ' + [...(dateX.get(norm(k.nom)) || [])].join(', ')));

fs.writeFileSync(OUT_JSON, JSON.stringify(kept), 'utf8');

// ---------------------------------------------------------------- TSV
const ENTETE = `# Fetes francaises - une ligne par jour et par fete.
#
# PROVENANCE : DEUX fichiers fournis par l'utilisateur le 2026-09-23.
#  1. \`fetes_francaises_complet.csv\` (406 lignes) — la base par DATE.
#  2. \`calendrier_2026_prenoms_francais_exhaustif_et_variantes.xlsx\`
#     (610 formes) — complete la premiere, avec les VARIANTES.
#  Le xlsx cite ses sources : INSEE (prenoms 1900-2025) et Nominis /
#  Conference des eveques de France pour le calendrier des saints.
#
# ⚠️ LES DEUX SOURCES N'ECRIVENT PAS LEURS DATES DANS LE MEME ORDRE :
#  le CSV est en \`JJ/MM\`, le xlsx aussi, mais ce fichier est en \`MM<TAB>JJ\`.
#  Confondre les deux a produit DEUX fois une analyse entierement fausse.
#
# ⚠️ **LA TABLE A ETE RE-DERIVEE LE 2026-09-24.** Le CSV est un calendrier
#  UN-SAINT-PAR-JOUR : dans chaque mois les saints sont dans le BON ORDRE
#  chronologique (2 inversions sur 238 verifiees), mais les JOURS ne sont que
#  la POSITION dans la liste. Une intruse — un saint qui vit aussi dans son
#  vrai mois (Gregoire au 3/1 alors qu'il est du 3/9) — consomme un creneau et
#  decale tout ce qui suit de +1.
#  DEUX SOURCES DE DATES servent d'ANCRES, et elles epinglent le decalage, ce
#  qui rend le realignement deterministe :
#   1. le xlsx (Nominis), qui date 237 entrees ;
#   2. calendrier.com 2026, dont les 365 jours ajoutent 105 ancres de plus
#      (voir tools/extraire_reference_calendrier.js).
#  ⚠️ Les deux s'accordent 227 fois sur 228 ; leur SEUL desaccord est Catherine
#  (xlsx 24/11, reference 25/11) et c'est la REFERENCE qui a raison — la
#  tradition francaise la met au 25, et l'ordre du CSV le soutient.
#  Resultat : 346 des 365 jours de la reference confirmees (19 ecarts), et
#  AUCUN jour de l'annee sans fete.
#
# ⚠️ LES 19 ECARTS RESTANTS NE SONT PAS DES FAUTES, pour trois raisons :
#  · une dizaine sont des ABREVIATIONS de la reference qui designent le meme
#    saint (Cath.deSuede, Cath.deSienne, J.-M.Vianney, The.del'E.-Jesus,
#    J.-B.delaSalle, NativiteN.-D., N.-D.Mont-Car...) ;
#  · les autres sont des angles morts des DEUX sources, qui ne connaissent qu'un
#    saint par prenom — Catherine de Sienne (29/04), Antoine-Marie Zaccaria
#    (05/07), Jean de Dieu (08/03), Augustin de Cantorbery (27/05), Pierre
#    Fourier (09/12), Jacques de la Marche (28/11), Irenee de Lyon (28/06),
#    Vincent de Saragosse (22/01), Charles le Bon (02/03), Thomas apotre (03/07),
#    Therese d'Avila (15/10), Marie Mere de Dieu (01/01) ;
#  · quelques-unes sont des choix de VOCABULAIRE (notre « Aurore » la ou la
#    reference dit « Francois d'Assise »).
#  Les realigner sur la reference les SUPPRIMERAIT.
#
# ⚠️ LA 5e COLONNE EST LA FORME DE REFERENCE D'UNE VARIANTE, vide sinon.
#  Elle est STOCKEE mais PAS AFFICHEE : le 24/06 porte dix-sept formes de
#  « Jean-X » (avec et sans trait d'union) — les montrer toutes serait
#  absurde. \`fetes_du_jour\` les ECARTE, \`chercher\` les TROUVE (c'est tout
#  l'interet : taper « Jean Baptiste » ou « Katrin » doit repondre).
#  ⚠️ « Jean Claude » et « Jean-Claude » sont DEUX lignes : deux graphies,
#  donc deux cles de recherche. Ne pas dedoublonner sur les accents.
#  Un nom du CSV est TOUJOURS affiche ; seule une ligne du xlsx peut etre une
#  variante — « Alix » existe donc deux fois, affiche au 10/01 et variante
#  d'« Alice » au 16/12.
#
# LES FETES A DATE MOBILE NE SONT PAS ICI : elles se CALCULENT (voir
# \`fetes.rs\`) — Ascension, lundis de Paques et de Pentecote, Fete des
# Grands-Meres, des Meres et des Peres.
#
# mm <TAB> jj <TAB> nom <TAB> categorie <TAB> variante_de
`;

const lignes = kept.slice().sort((a, b) =>
  a.mm - b.mm || a.jj - b.jj || (a.nom < b.nom ? -1 : a.nom > b.nom ? 1 : 0));
const corps = lignes.map(k =>
  [String(k.mm).padStart(2, '0'), String(k.jj).padStart(2, '0'), k.nom, k.cat, k.variante || ''].join('\t'));
const tsv = ENTETE + corps.join('\n') + '\n';
fs.writeFileSync(SORTIE, tsv, 'utf8');
console.log('\n-> ' + SORTIE + '  (' + corps.length + ' lignes de donnees, '
  + Buffer.byteLength(tsv, 'utf8') + ' octets, LF seuls : '
  + (tsv.match(/\r\n/g) || []).length + ' CRLF)');
const cats = {};
lignes.forEach(k => cats[k.cat] = (cats[k.cat] || 0) + 1);
console.log('   categories : ' + JSON.stringify(cats));
console.log('   lignes « ferie » : ' + lignes.filter(k => k.cat === 'ferie').length);
