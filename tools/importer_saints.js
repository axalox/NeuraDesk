#!/usr/bin/env node
// IMPORTE UN CALENDRIER DES SAINTS DANS LA TABLE DES FÊTES.
//
// DEMANDE (2026-11-04) : « mets à jour les fêtes du calendrier avec ce
// fichier », `saints.json` — un objet { mois: { jour: [noms…] } } qui donne
// PLUSIEURS saints par jour, avec leurs dates déjà posées.
//
// ⛔ **IL FUSIONNE, IL NE REMPLACE PAS.** Mesuré avant d'écrire quoi que ce
// soit : un remplacement pur ferait perdre 327 noms sur 288 jours — et
// **cinq des huit alarmes de l'utilisateur** (`rappels_fetes.toml` est rangé
// par TRIPLET (mois, jour, nom), et le fichier ne connaît ni Davy au 20/09,
// ni Pierre au 29/06, ni Julien au 02/08, ni Sandrine au 02/04, ni Aurora au
// 04/10). La table existante GARDE donc ses lignes ; le fichier ajoute celles
// qui manquent à chaque jour.
//
// Ce que l'import fait, précisément :
//  1. nettoie les noms (espaces en trop, `Noà«l»` → `Noël`) ;
//  2. écarte une date IMPOSSIBLE (`6/31`, « Ste-Pétronille » — que le fichier
//     porte AUSSI au 5/31, donc rien n'est perdu) ;
//  3. retire le préfixe `St-`/`Ste-` des noms qu'il ajoute : la table stocke
//     des noms nus (« Basile », « Geneviève ») et la catégorie dit déjà
//     « saint » — voir `Fete::categorie` ;
//  4. n'ajoute RIEN sur un jour qui connaît déjà ce saint : la comparaison se
//     fait sur un nom replié (sans accents, sans parenthèse) et **par préfixe
//     de mots**, pour que « Basile » couvre « Basile le Grand » et « Michel »
//     couvre « Michel » ;
//  5. écrit les lignes existantes d'abord, les nouvelles ensuite, et une
//     section d'en-tête qui dit d'où elles viennent.
//
// ⚠️ **LE FICHIER SOURCE EST VERSIONNÉ** (`tools/saints.json`) et celui-ci
// reçoit son chemin en argument : sans lui, l'import ne serait pas rejouable.
//
// ⛔ **LE SCRIPT EST IDEMPOTENT** : relancé, il ne retrouve rien à ajouter
// (les lignes qu'il a écrites sont dans la table). C'est ce qui permet de le
// rejouer après `reconstruire_fetes.js`, dans cet ordre :
//     node tools/reconstruire_fetes.js …
//     node tools/importer_saints.js tools/saints.json
//
// Usage : node tools/importer_saints.js <saints.json> [sortie.tsv] [--essai]

"use strict";

const fs = require("fs");
const path = require("path");

const MARQUE = "# --- IMPORTÉ DE tools/saints.json";

// Le mois et sa longueur : une année bissextile, pour que le 29 février passe.
const JOURS = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

// **LES ACCENTS ABÎMÉS DE LA SOURCE.** Mesuré : neuf noms portent `à®` ou `à«`
// là où le français écrit `î` ou `ë` — `Benoà®t`, `Joà«l`, `Noà«l`,
// `Gwénaà«l`, `Judicaà«l`, `Israà«l`, `Boà®teux`. Aucun mot français ne
// contient ces deux suites, donc la réparation est sûre ; elle est aussi
// NÉCESSAIRE, sans quoi « Benoît » entrerait deux fois (la table porte déjà
// `Benoit`/`Benoît`).
const ABIMES = [
  [/à®/g, "î"],
  [/à«/g, "ë"],
];

// Ce qui reste après réparation et qui ressemble encore à un accent abîmé : le
// nom est ÉCARTÉ (et dit), plutôt que d'entrer illisible.
const ENCORE_ABIME = /[àâä][«®©¨§ª°»¼½¾¿]|[ÃÂ][\u0080-¿]|[\u0080-\u009F]/;

/** Replie un nom pour la COMPARAISON : minuscules, sans accents. */
function replier(texte) {
  return texte.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
}

/**
 * La clé d'un nom : repliée, sans parenthèse, sans préfixe `St`/`Ste`, et
 * **toute ponctuation réduite à un espace** — traits d'union, virgules,
 * apostrophes, barres obliques.
 *
 * ⚠️ **LA PONCTUATION EST CE QUI A MANQUÉ À LA PREMIÈRE MESURE** : la table
 * porte « Matthieu » et le fichier « Matthieu, apôtre » — sans la virgule
 * traitée, les deux entraient. Même chose pour « Jour de l’an / Nouvel An »
 * (apostrophe typographique) contre « Jour de l'an » (apostrophe droite).
 */
function cle(nom) {
  return replier(nom)
    .replace(/\([^)]*\)/g, " ")
    .replace(/^(stes?|sts?|saints?|saintes?)[-\s]+/, "")
    .replace(/[^a-z0-9 ]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * Deux clés désignent-elles le même saint ? **OUI SI L'UNE COMMENCE L'AUTRE
 * SUR UNE FRONTIÈRE DE MOT** : « basile » couvre « basile le grand », mais pas
 * « basilisse ». C'est ce qui empêche la table de porter deux fois le même
 * saint sous deux graphies.
 */
function memeSaint(a, b) {
  if (!a || !b) return false;
  return a === b || a.startsWith(b + " ") || b.startsWith(a + " ");
}

/**
 * Le nom tel qu'il sera STOCKÉ : nettoyé, accents abîmés réparés, et sans son
 * préfixe s'il est saint. `null` s'il reste illisible — l'appelant l'écarte et
 * le dit.
 */
function nomStocke(brut) {
  let nom = brut;
  for (const [motif, vers] of ABIMES) nom = nom.replace(motif, vers);
  nom = nom.replace(/\s+/g, " ").trim();
  if (ENCORE_ABIME.test(nom)) return null;
  // `Sts-` (pluriel) est LAISSÉ : « Sts-Innocents » se lit mieux que
  // « Innocents ». Seuls les préfixes singuliers disparaissent.
  nom = nom.replace(/^(St|Ste|Saint|Sainte|Ste\.)[-\s]+/, "");
  return nom;
}

/** La famille d'une entrée : ce qui portait `St`/`Ste` est un saint. */
function categorie(brut) {
  return /^\s*(Sts?|Stes?|Saint|Sainte|Ste\.)[-\s]/.test(brut) ? "saint" : "tradition";
}

function lireTable(chemin) {
  const texte = fs.readFileSync(chemin, "utf8");
  const lignes = texte.split(/\r?\n/);
  const enTete = [];
  const lignesTable = [];
  let dansImport = false;
  for (const ligne of lignes) {
    if (ligne.startsWith("#")) {
      // La section d'un import PRÉCÉDENT est retirée : le script la réécrit.
      // ⚠️ Le bloc est ENTIÈREMENT fait de commentaires, et il finit à la
      // première ligne de données — pas à la première ligne « # » seule, qui
      // est un séparateur INTERNE (la première version s'arrêtait là et
      // laissait la moitié du bloc en place à chaque passage).
      if (ligne.startsWith(MARQUE)) dansImport = true;
      else if (!dansImport) enTete.push(ligne);
      continue;
    }
    if (ligne.trim() === "") continue;
    dansImport = false;
    const [mois, jour, nom, famille, variante] = ligne.split("\t");
    lignesTable.push({
      mois: Number(mois),
      jour: Number(jour),
      nom: nom.trim(),
      famille: (famille || "").trim(),
      variante: (variante || "").trim(),
      importee: false,
    });
  }
  // ⚠️ Le retrait ci-dessus laisse les lignes vides de la section : on les
  // élague pour que l'en-tête réécrit ne s'allonge pas à chaque passage.
  while (enTete.length && enTete[enTete.length - 1].trim() === "") enTete.pop();
  return { enTete, lignesTable };
}

function main() {
  const args = process.argv.slice(2).filter((a) => a !== "--essai");
  const essai = process.argv.includes("--essai");
  const source = args[0];
  const defaut = path.join(__dirname, "..", "src", "win32", "fetes_france.tsv");
  const sortie = args[1] || defaut;

  if (!source) {
    console.error("usage : node tools/importer_saints.js <saints.json> [sortie.tsv] [--essai]");
    process.exit(2);
  }

  const brut = JSON.parse(fs.readFileSync(source, "utf8").replace(/^﻿/, ""));
  const { enTete, lignesTable } = lireTable(sortie);

  const clesParJour = new Map();
  const poser = (m, j, nom) => {
    const k = `${m}/${j}`;
    if (!clesParJour.has(k)) clesParJour.set(k, []);
    const liste = clesParJour.get(k);
    const c = cle(nom);
    if (c) liste.push(c);
  };
  for (const l of lignesTable) poser(l.mois, l.jour, l.nom);

  const impossibles = [];
  const illisibles = [];
  const doublonsFichier = [];
  const ajouts = [];
  let lues = 0;

  for (const moisTexte of Object.keys(brut)) {
    const mois = Number(moisTexte);
    if (!(mois >= 1 && mois <= 12)) continue;
    for (const jourTexte of Object.keys(brut[moisTexte] || {})) {
      const jour = Number(jourTexte);
      const noms = brut[moisTexte][jourTexte];
      if (!Array.isArray(noms)) continue;
      if (!(jour >= 1 && jour <= JOURS[mois - 1])) {
        for (const n of noms) impossibles.push(`${jour}/${mois} « ${String(n).trim()} »`);
        continue;
      }
      for (const n of noms) {
        lues++;
        const nom = nomStocke(String(n));
        if (!nom) {
          illisibles.push(`${jour}/${mois} « ${String(n).trim()} »`);
          continue;
        }
        const c = cle(nom);
        const deja = clesParJour.get(`${mois}/${jour}`) || [];
        if (deja.some((autre) => memeSaint(autre, c))) {
          doublonsFichier.push(`${jour}/${mois} « ${nom} »`);
          continue;
        }
        // Une même clé deux fois DANS LE FICHIER : la seconde n'ajoute rien.
        deja.push(c);
        clesParJour.set(`${mois}/${jour}`, deja);
        ajouts.push({ mois, jour, nom, famille: categorie(String(n)), variante: "", importee: true });
      }
    }
  }

  const toutes = lignesTable.concat(ajouts);
  // Tri STABLE : à jour égal, l'ordre d'origine est gardé, donc les lignes de
  // la table restent AVANT celles de l'import (le mini calendrier montre le
  // premier évènement du jour, voir `evenements_du_mois`).
  toutes.sort((a, b) => a.mois - b.mois || a.jour - b.jour);

  // ⚠️ On compte les fêtes AFFICHÉES, pas les formes stockées : les variantes
  // (« Jean-A », « Jean-B »… le 24 juin en porte dix-huit) sont écartées du
  // dessin par `fetes_du_jour`, et les compter donnerait un maximum trompeur.
  const parJour = new Map();
  for (const l of toutes) {
    if (l.variante) continue;
    // Clé et libellé en JOUR/MOIS, l'ordre où un Français lit une date.
    const k = `${l.jour}/${l.mois}`;
    parJour.set(k, (parJour.get(k) || 0) + 1);
  }
  const maxi = Math.max(...parJour.values());
  const joursMax = [...parJour.entries()].filter(([, n]) => n === maxi).map(([k]) => k);

  const bloc = [
    MARQUE,
    "#",
    `# ${ajouts.length} lignes importées depuis ${path.basename(source)} (${lues} entrées lues).`,
    "# Le fichier donne PLUSIEURS saints par jour, déjà datés : ses lignes",
    "# s'AJOUTENT à celles ci-dessus, qui ne sont jamais retirées — un",
    "# remplacement ferait perdre des noms et casserait les alarmes de",
    "# `rappels_fetes.toml`, rangées par triplet (mois, jour, nom).",
    "#",
    "# · une date impossible est ÉCARTÉE (« 6/31 » — Pétronille est aussi au",
    "#   5/31 dans ce fichier, donc rien ne se perd) ;",
    "# · un saint déjà connu du jour n'est PAS ajouté deux fois : la",
    "#   comparaison ignore accents, parenthèses, préfixe St/Ste, et accepte le",
    "#   préfixe de mots (« Basile » couvre « Basile le Grand ») ;",
    "# · les noms importés sont stockés SANS leur « St- »/« Ste- » : la table",
    "#   écrit des noms nus et la catégorie porte déjà « saint » ;",
    "# · les entrées sans St/Ste (« Chandeleur », « Toussaint »…) sont classées",
    "#   `tradition`, et le calcul des fériés reste seul maître des dates",
    "#   MOBILES (Pâques, Ascension, Pentecôte).",
    "#",
    `# RÉPARTITION : au plus ${maxi} fêtes sur un jour (${joursMax.slice(0, 8).join(", ")}${joursMax.length > 8 ? "…" : ""}).`,
    "# Rejouer : node tools/importer_saints.js tools/saints.json",
    "#",
  ];

  const corps = toutes.map((l) => [String(l.mois).padStart(2, "0"), String(l.jour).padStart(2, "0"), l.nom, l.famille, l.variante].join("\t"));
  const texte = enTete.concat(bloc, corps).join("\n") + "\n";

  console.log(`entrées lues            : ${lues}`);
  console.log(`dates impossibles       : ${impossibles.length}${impossibles.length ? " → " + impossibles.join(" | ") : ""}`);
  console.log(`noms illisibles écartés : ${illisibles.length}${illisibles.length ? " → " + illisibles.join(" | ") : ""}`);
  console.log(`déjà connues du jour    : ${doublonsFichier.length}`);
  console.log(`lignes ajoutées         : ${ajouts.length}`);
  console.log(`table : ${lignesTable.length} → ${toutes.length} lignes`);
  console.log(`fêtes AFFICHÉES par jour, au plus : ${maxi} (${joursMax.slice(0, 8).join(", ")})`);

  if (essai) {
    const cheminEssai = sortie.replace(/\.tsv$/, ".essai.tsv");
    fs.writeFileSync(cheminEssai, texte);
    console.log(`(essai) écrit dans ${cheminEssai}`);
    return;
  }
  // ⚠️ RIEN À AJOUTER = ON N'ÉCRIT RIEN. Réécrire le fichier avec « 0 lignes
  // importées » ferait mentir l'en-tête sur ce que la table contient — la
  // section décrit un contenu, pas le dernier passage du script.
  if (!ajouts.length) {
    console.log("rien à ajouter — table inchangée");
    return;
  }
  fs.writeFileSync(sortie, texte);
  console.log(`écrit dans ${sortie}`);
}

main();
