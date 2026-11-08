// Le volet des COMMANDES ENREGISTRÉES, commun aux panneaux Terminal et SSH —
// DEMANDES EXPLICITES (2026-10-03) : « un panneau rétractable à droite des
// terminaux classiques et ssh où je puisse enregistrer des commandes pour les
// envoyer directement dans le terminal actif », puis « un système de dossiers
// pour gérer les commandes ».
//
// La liste vit dans Rust (`win32::commandes`, `commandes.toml`), PARTAGÉE par
// les deux panneaux : `{ commandes: [{ nom, texte, executer, dossier }],
// dossiers: [noms] }`. Chaque commande dit dans quel dossier elle se range (vide :
// hors dossier ; un seul niveau). ⭐ **TOUTES apparaissent dans LES DEUX panneaux**
// — DEMANDE EXPLICITE (2026-10-11) : « que les terminaux et les ssh partagent les
// mêmes commandes enregistrées ». Une « portée » (tous / terminal / ssh) les
// cachait à l'autre panneau ; elle est retirée. La page la reçoit dans `state.commandes` et la
// renvoie ENTIÈRE à chaque changement (POST `<route>` puis l'action
// `commandes`). L'envoi, lui, reste dans la page : le texte part comme un
// COLLAGE dans le terminal actif (`term.paste`), puis Entrée si la commande le
// demande.
//
// Un dossier replié le reste d'une ouverture à l'autre (stockage de la page).
//
// La page fournit (voir `installer`) :
//   route      la route POST de la liste (`terminal/commandes`, `ssh/commandes`) ;
//   go, poster les deux canaux vers Rust de la page ;
//   avant      l'élément de la barre du haut avant lequel poser le bouton ;
//   milieu     le conteneur en ligne où le volet se range à droite ;
//   cible()    le libellé du terminal qui recevrait la commande, ou null ;
//   envoyer(texte, executer)  true si la commande est partie.
(function () {
  const CLE_REPLIS = "commandes-dossiers-replies";
  let o = null;
  let donnees = { commandes: [], dossiers: [] };
  let recherche = "";
  // null, ou { index (-1 : nouvelle), nom, texte, executer, dossier }.
  let edition = null;
  // Le dossier dont le nom est en saisie : null, "" (un NOUVEAU dossier) ou
  // son nom actuel.
  let saisieDossier = null;
  // La suppression qui attend un second clic : "c<index>" ou "d<nom>".
  let aConfirmer = "";
  let replies = new Set();
  try { replies = new Set(JSON.parse(localStorage.getItem(CLE_REPLIS) || "[]")); } catch (_) {}
  let volet, bouton, corpsListe, champRecherche, ligneCible;

  function el(tag, attrs, enfants) {
    const n = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs || {})) {
      if (k === "class") n.className = v;
      else if (k.startsWith("on")) n.addEventListener(k.slice(2), v);
      else n.setAttribute(k, v);
    }
    for (const e of enfants || []) n.appendChild(typeof e === "string" ? document.createTextNode(e) : e);
    return n;
  }

  function outil(texte, title, action, classe = "") {
    return el("button", {
      class: "vc-outil " + classe, title,
      onclick: (e) => { e.stopPropagation(); action(); },
    }, [texte]);
  }


  function retenirReplis() {
    try { localStorage.setItem(CLE_REPLIS, JSON.stringify([...replies])); } catch (_) {}
  }

  function enregistrer(commandes, dossiers = donnees.dossiers) {
    donnees = { commandes, dossiers };
    rendre();
    o.poster(o.route, JSON.stringify(donnees)).then(() => o.go("commandes"), () => {});
  }

  function envoyer(c) {
    if (!o.envoyer(c.texte, c.executer)) {
      ligneCible.classList.add("alerte");
      setTimeout(() => ligneCible.classList.remove("alerte"), 900);
    }
  }

  // Un second clic dans les 3 s confirme ; sinon le bouton redevient ✕.
  function boutonSupprimer(cle, title, supprimer) {
    if (aConfirmer === cle) {
      return outil("Sûr ?", "Cliquer encore pour supprimer", () => { aConfirmer = ""; supprimer(); }, "danger");
    }
    return outil(el("span", { class: "ico ico-x" }), title, () => {
      aConfirmer = cle;
      rendreListe();
      setTimeout(() => { if (aConfirmer === cle) { aConfirmer = ""; rendreListe(); } }, 3000);
    });
  }

  function rendreCible() {
    if (!ligneCible) return;
    const cible = o.cible();
    ligneCible.textContent = cible ? "→ " + cible : "Aucun terminal actif";
    ligneCible.classList.toggle("vide", !cible);
  }

  function ligneCommande(c, i) {
    return el("div", {
      class: "vc-commande" + (c.dossier ? " dans-dossier" : ""),
      title: c.texte + "\n\nClic : envoyer dans le terminal actif" + (c.executer ? " et exécuter" : " (sans Entrée)"),
      onclick: () => envoyer(c),
    }, [
      // Le TEXTE ENTIER, qui passe à la ligne (voir `.vc-nom`) : sous son nom
      // quand elle en a un, à sa place sinon.
      el("div", { class: "vc-ligne" }, [
        c.nom ? el("span", { class: "vc-nom" }, [c.nom]) : el("span", { class: "vc-nom code" }, [c.texte]),
        c.executer ? el("span", { class: "vc-badge", title: "Exécutée (Entrée envoyée)" }, ["↵"]) : el("span"),
        outil("✎", "Modifier (et changer de dossier)", () => editer(i)),
        boutonSupprimer("c" + i, "Supprimer", () => enregistrer(donnees.commandes.filter((_, j) => j !== i))),
      ]),
      c.nom ? el("div", { class: "vc-apercu" }, [c.texte]) : el("span"),
    ]);
  }

  // Le champ de nom d'un dossier : `ancien` "" pour un nouveau dossier.
  function champDossier(ancien) {
    const champ = el("input", { type: "text", maxlength: "80", placeholder: "Nom du dossier" });
    champ.value = ancien;
    let fini = false;
    const finir = (valider) => {
      if (fini) return;
      fini = true;
      saisieDossier = null;
      const nom = champ.value.trim();
      const pris = donnees.dossiers.includes(nom) && nom !== ancien;
      if (!valider || !nom || nom === ancien || pris) { rendreListe(); return; }
      if (!ancien) {
        enregistrer(donnees.commandes, [...donnees.dossiers, nom]);
        return;
      }
      // Renommer : le dossier garde sa place, ses commandes le suivent.
      if (replies.delete(ancien)) { replies.add(nom); retenirReplis(); }
      enregistrer(
        donnees.commandes.map((c) => (c.dossier === ancien ? { ...c, dossier: nom } : c)),
        donnees.dossiers.map((d) => (d === ancien ? nom : d)),
      );
    };
    champ.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Enter") { e.preventDefault(); finir(true); }
      else if (e.key === "Escape") { e.preventDefault(); finir(false); }
    });
    champ.addEventListener("blur", () => finir(true));
    champ.addEventListener("click", (e) => e.stopPropagation());
    requestAnimationFrame(() => { champ.focus(); champ.select(); });
    return el("div", { class: "vc-dossier-saisie" }, ["📁 ", champ]);
  }

  function enteteDossier(nom, nombre, ouvert) {
    if (saisieDossier === nom) return champDossier(nom);
    const index = donnees.dossiers.indexOf(nom);
    const deplacer = (pas) => {
      const d = [...donnees.dossiers];
      [d[index], d[index + pas]] = [d[index + pas], d[index]];
      enregistrer(donnees.commandes, d);
    };
    const outils = [
      outil("+", "Enregistrer une commande dans ce dossier", () => editer(-1, nom)),
      outil("✎", "Renommer le dossier", () => { saisieDossier = nom; rendreListe(); }),
    ];
    if (index > 0) outils.push(outil("↑", "Monter le dossier", () => deplacer(-1)));
    if (index < donnees.dossiers.length - 1) outils.push(outil("↓", "Descendre le dossier", () => deplacer(1)));
    outils.push(boutonSupprimer("d" + nom, "Supprimer le dossier (ses commandes restent, hors dossier)", () => {
      replies.delete(nom);
      retenirReplis();
      enregistrer(
        donnees.commandes.map((c) => (c.dossier === nom ? { ...c, dossier: "" } : c)),
        donnees.dossiers.filter((d) => d !== nom),
      );
    }));
    return el("div", {
      class: "vc-dossier",
      title: recherche ? nom : (ouvert ? "Replier" : "Déplier") + " — double-clic : renommer",
      onclick: () => {
        if (recherche) return;
        if (!replies.delete(nom)) replies.add(nom);
        retenirReplis();
        rendreListe();
      },
      ondblclick: (e) => { e.stopPropagation(); saisieDossier = nom; rendreListe(); },
    }, [
      el("span", { class: "vc-fleche" }, [ouvert ? "▾" : "▸"]),
      el("span", { class: "vc-nom" }, ["📁 " + nom]),
      el("span", { class: "vc-compte" }, [String(nombre)]),
      ...outils,
    ]);
  }

  function rendreListe() {
    const filtre = recherche.trim().toLowerCase();
    const correspond = (c) => !filtre || (c.nom + "\n" + c.texte + "\n" + c.dossier).toLowerCase().includes(filtre);
    const indexees = donnees.commandes.map((c, i) => [c, i]).filter(([c]) => correspond(c));
    const lignes = [];
    if (saisieDossier === "") lignes.push(champDossier(""));
    for (const [c, i] of indexees) if (!c.dossier) lignes.push(ligneCommande(c, i));
    for (const nom of donnees.dossiers) {
      const contenu = indexees.filter(([c]) => c.dossier === nom);
      // Pendant une recherche : seuls les dossiers qui ont un résultat, ouverts.
      if (filtre && !contenu.length && !nom.toLowerCase().includes(filtre)) continue;
      const ouvert = !!filtre || !replies.has(nom);
      lignes.push(enteteDossier(nom, contenu.length, ouvert));
      if (!ouvert) continue;
      if (!contenu.length && !filtre) lignes.push(el("div", { class: "vc-vide dans-dossier" }, ["Dossier vide — « + » pour y ranger une commande."]));
      for (const [c, i] of contenu) lignes.push(ligneCommande(c, i));
    }
    if (!lignes.length) {
      lignes.push(el("div", { class: "vc-vide" }, [
        donnees.commandes.length ? "Aucune commande ne correspond." : "Aucune commande. « + » pour en enregistrer une.",
      ]));
    }
    corpsListe.replaceChildren(...lignes);
  }

  // `dossier` : celui où ranger une NOUVELLE commande.
  function editer(index, dossier = "") {
    const c = donnees.commandes[index];
    saisieDossier = null;
    edition = c
      ? { index, nom: c.nom, texte: c.texte, executer: c.executer, dossier: c.dossier }
      : { index: -1, nom: "", texte: "", executer: true, dossier };
    rendre();
  }

  function rendreEdition() {
    const e = edition;
    const nom = el("input", { type: "text", maxlength: "80", placeholder: "Nom (facultatif)" });
    nom.value = e.nom;
    const texte = el("textarea", { rows: "5", spellcheck: "false", placeholder: "La commande — plusieurs lignes possibles" });
    texte.value = e.texte;
    const garder = () => { e.nom = nom.value; e.texte = texte.value; };
    const choix = (valeurs, actuelle, poser) => el("div", { class: "vc-choix" }, valeurs.map(([v, libelle]) =>
      el("button", { class: v === actuelle ? "actif" : "", onclick: () => { garder(); poser(v); rendre(); } }, [libelle])));
    const valider = () => {
      garder();
      if (!e.texte.trim()) { texte.focus(); return; }
      const c = { nom: e.nom.trim(), texte: e.texte, executer: e.executer, dossier: e.dossier };
      edition = null;
      // Une commande rangée dans un dossier replié l'ouvre : on la voit arriver.
      if (c.dossier && replies.delete(c.dossier)) retenirReplis();
      enregistrer(e.index < 0 ? [...donnees.commandes, c] : donnees.commandes.map((x, j) => (j === e.index ? c : x)));
    };
    const annuler = () => { edition = null; rendre(); };
    const clavier = (ev) => {
      ev.stopPropagation();
      if (ev.key === "Escape") { ev.preventDefault(); annuler(); }
      else if (ev.key === "Enter" && (ev.ctrlKey || ev.target === nom)) { ev.preventDefault(); valider(); }
    };
    nom.addEventListener("keydown", clavier);
    texte.addEventListener("keydown", clavier);
    const champs = [
      el("label", {}, ["Nom"]), nom,
      el("label", {}, ["Commande"]), texte,
      el("label", {}, ["Au clic"]),
      choix([[true, "Exécuter (Entrée)"], [false, "Écrire seulement"]], e.executer, (v) => { e.executer = v; }),
    ];
    if (donnees.dossiers.length) {
      champs.push(
        el("label", {}, ["Dossier"]),
        choix([["", "Aucun"], ...donnees.dossiers.map((d) => [d, "📁 " + d])], e.dossier, (v) => { e.dossier = v; }),
      );
    }
    champs.push(el("div", { class: "vc-actions" }, [
      el("button", { onclick: annuler }, ["Annuler"]),
      el("button", { class: "principal", title: "Ctrl+Entrée", onclick: valider }, ["Enregistrer"]),
    ]));
    corpsListe.replaceChildren(el("div", { class: "vc-edition" }, champs));
    requestAnimationFrame(() => (e.texte ? texte : nom).focus());
  }

  function rendre() {
    if (!volet) return;
    champRecherche.parentNode.classList.toggle("cache", !!edition);
    if (edition) rendreEdition();
    else rendreListe();
    rendreCible();
  }

  function basculer(ouvrir) {
    const ouvert = ouvrir === undefined ? volet.classList.contains("cache") : ouvrir;
    volet.classList.toggle("cache", !ouvert);
    bouton.classList.toggle("actif", ouvert);
    if (ouvert) rendre();
  }

  // LA LARGEUR DU VOLET, RÉGLÉE EN TIRANT SA POIGNÉE (voir `#vc-poignee`).
  // En px CSS du volet, avant son `zoom` (`.zoome`) : le rapport entre la
  // largeur mesurée et la largeur posée convertit le mouvement de la souris.
  const CLE_LARGEUR = "commandes-largeur";
  const LARGEUR_MIN = 160;
  function poserLargeur(px) {
    volet.style.flexBasis = Math.round(px) + "px";
  }
  function installerPoignee() {
    const retenue = Number(localStorage.getItem(CLE_LARGEUR));
    if (retenue >= LARGEUR_MIN) poserLargeur(retenue);
    const poignee = el("div", { id: "vc-poignee", class: "cible-24", title: "Tirer : largeur du volet · double-clic : largeur d'origine" });
    volet.prepend(poignee);
    poignee.addEventListener("dblclick", () => {
      volet.style.flexBasis = "";
      try { localStorage.removeItem(CLE_LARGEUR); } catch (_) {}
    });
    poignee.addEventListener("mousedown", (e) => {
      if (e.button !== 0) return;
      e.preventDefault();
      const depart = e.clientX;
      const posee = parseFloat(getComputedStyle(volet).flexBasis) || 270;
      const echelle = volet.getBoundingClientRect().width / posee || 1;
      // Jamais plus que ce qui laisse 120 px aux terminaux.
      const max = (o.milieu.getBoundingClientRect().width - 120) / echelle;
      let largeur = posee;
      poignee.classList.add("tire");
      document.body.classList.add("glisse");
      const bouger = (m) => {
        largeur = Math.max(LARGEUR_MIN, Math.min(max, posee - (m.clientX - depart) / echelle));
        poserLargeur(largeur);
      };
      const lacher = () => {
        document.removeEventListener("mousemove", bouger, true);
        document.removeEventListener("mouseup", lacher, true);
        poignee.classList.remove("tire");
        document.body.classList.remove("glisse");
        try { localStorage.setItem(CLE_LARGEUR, String(Math.round(largeur))); } catch (_) {}
      };
      document.addEventListener("mousemove", bouger, true);
      document.addEventListener("mouseup", lacher, true);
    });
  }

  function installer(options) {
    o = options;
    bouton = el("button", {
      id: "vc-bouton", class: "discret",
      title: "Commandes enregistrées (Ctrl+Maj+K)",
      onclick: () => basculer(),
    }, ["❯_"]);
    o.avant.parentNode.insertBefore(bouton, o.avant);
    champRecherche = el("input", { type: "search", placeholder: "Rechercher…" });
    champRecherche.addEventListener("input", () => { recherche = champRecherche.value; rendreListe(); });
    champRecherche.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape") { champRecherche.value = recherche = ""; rendreListe(); }
    });
    corpsListe = el("div", { class: "vc-liste" });
    ligneCible = el("div", { class: "vc-cible" });
    volet = el("aside", { id: "volet-commandes", class: "zoome cache" }, [
      el("div", { class: "vc-entete" }, [
        el("span", { class: "vc-titre" }, ["Commandes"]),
        el("button", { class: "vc-outil", title: "Enregistrer une commande", onclick: () => editer(-1) }, ["+"]),
        el("button", {
          class: "vc-outil", title: "Nouveau dossier",
          onclick: () => { edition = null; saisieDossier = ""; rendre(); },
        }, ["📁+"]),
        // Sauvegarder les commandes dans un fichier, et les y reprendre (ajoutées
        // à la liste, jamais à sa place) : `commandes::exporter` / `importer`.
        el("button", { class: "vc-outil", title: "Exporter les commandes dans un fichier", onclick: () => o.go("commandes-exporter") }, [el("span", { class: "ico ico-download-simple" })]),
        el("button", { class: "vc-outil", title: "Importer des commandes d'un fichier (ajoutées à la liste)", onclick: () => o.go("commandes-importer") }, [el("span", { class: "ico ico-upload-simple" })]),
        el("button", { class: "vc-outil", title: "Replier le volet", onclick: () => basculer(false) }, ["»"]),
      ]),
      ligneCible,
      el("div", { class: "vc-recherche" }, [champRecherche]),
      corpsListe,
    ]);
    o.milieu.appendChild(volet);
    installerPoignee();
    document.addEventListener("keydown", (e) => {
      if (e.ctrlKey && e.shiftKey && !e.altKey && e.key.toLowerCase() === "k") { e.preventDefault(); basculer(); }
    }, true);
  }

  window.voletCommandes = {
    installer,
    basculer,
    // L'état reçu de Rust. Un état identique ne reconstruit rien (une saisie
    // en cours n'est pas emportée).
    liste(nouvelle) {
      const n = { commandes: (nouvelle && nouvelle.commandes) || [], dossiers: (nouvelle && nouvelle.dossiers) || [] };
      if (JSON.stringify(n) === JSON.stringify(donnees)) return;
      donnees = n;
      if (!edition && saisieDossier === null) rendre();
    },
    // Le terminal actif a changé.
    cible: rendreCible,
  };
})();
