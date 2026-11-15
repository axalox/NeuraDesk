// ⭐ LE MOTEUR DE TRADUCTION DES PAGES — injecté par Rust (voir `ui::langue`)
// dans chaque page CEF, seulement quand l'interface n'est pas en français.
//
// Il traduit les nœuds de texte et les attributs affichés (`title`,
// `placeholder`, `aria-label`, `value` des boutons), une première fois sur tout
// le document, puis tout ce que la page ajoute ou réécrit ensuite.
//
// ⛔ NE TOUCHE PAS À CE QUE L'UTILISATEUR ÉCRIT : champs de saisie, zones
// éditables, terminaux (`.xterm`) et tout ce qui porte `data-no-i18n`.
function __ndI18n(dico) {
  if (window.__ndI18nActif) return;
  window.__ndI18nActif = true;
  const exacts = dico[0];
  const motifs = dico[1].map(([morceaux, en]) => [
    new RegExp("^" + morceaux.map((m) => m.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("(.+?)") + "$"),
    en,
  ]);
  const ATTRIBUTS = ["title", "placeholder", "aria-label"];
  const memo = new Map();
  // Ce que NOUS avons écrit : la réécriture déclenche l'observateur, qui ne
  // doit pas retraduire une traduction.
  const ecrits = new WeakMap();

  const norm = (s) => s.replace(/\s+/g, " ").trim();
  // `profondeur` : les motifs s'emboîtent (« Dimanche {} » → « {} novembre {} »).
  function tr(s, profondeur = 0) {
    const k = norm(s);
    if (!k || !/[A-Za-zÀ-ÿ]/.test(k)) return null;
    if (memo.has(k)) return memo.get(k);
    let v = Object.prototype.hasOwnProperty.call(exacts, k) ? exacts[k] : null;
    if (v === null) {
      for (const [re, en] of motifs) {
        const m = re.exec(k);
        // La valeur elle-même, si elle est au dictionnaire (« Papier clair »).
        if (m) { let i = 1; v = en.replace(/\{\}/g, () => { const x = m[i++]; const y = profondeur < 3 ? tr(x, profondeur + 1) : null; return y === null ? x : y; }); break; }
      }
    }
    if (memo.size > 5000) memo.clear();
    memo.set(k, v);
    return v;
  }

  // `pourAttributs` : le `placeholder` d'un champ se traduit, son contenu non.
  function exclu(el, pourAttributs) {
    for (let e = el; e && e.nodeType === 1; e = e.parentElement) {
      const t = e.tagName;
      if (t === "SCRIPT" || t === "STYLE") return true;
      if (t === "TEXTAREA" && !(pourAttributs && e === el)) return true;
      if (e.isContentEditable || e.hasAttribute("data-no-i18n")) return true;
      if (e.classList && e.classList.contains("xterm")) return true;
    }
    return false;
  }

  // ⭐ UN MOT À DEUX SENS (« Annuler » = Cancel ou Undo) : un ancêtre
  // `data-i18n-ctx="historique"` fait chercher d'abord « historique|Annuler ».
  function enContexte(el, t) {
    const c = el && el.closest && el.closest("[data-i18n-ctx]");
    if (!c) return null;
    const k = c.getAttribute("data-i18n-ctx") + "|" + norm(t);
    return Object.prototype.hasOwnProperty.call(exacts, k) ? exacts[k] : null;
  }

  // Le RELEVÉ (`NEURADESK_I18N_RELEVE`) : ce qui a l'air français et n'a pas
  // été traduit, journalisé une fois, par paquets.
  const RELEVE = !!window.__ndI18nReleve;
  const FRANCAIS = /[éèêàçùôîâûœ]|\b(le|la|les|des|du|une|et|pour|avec|sans|sur|dans|pas|est|aux)\b/i;
  const releves = new Set();
  let releveEnAttente = [];
  function relever(t) {
    if (!RELEVE) return;
    const k = norm(t);
    if (k.length < 2 || releves.has(k) || !FRANCAIS.test(k)) return;
    releves.add(k);
    releveEnAttente.push(k);
    if (releveEnAttente.length === 1) setTimeout(() => {
      const page = location.pathname.split("/").pop();
      for (const r of releveEnAttente) console.log("[i18n] " + page + "\t" + r);
      releveEnAttente = [];
    }, 500);
  }

  function texte(n) {
    const t = n.nodeValue;
    if (!t || ecrits.get(n) === t) return;
    if (exclu(n.parentElement)) return;
    const v = enContexte(n.parentElement, t) ?? tr(t);
    if (v === null) { relever(t); return; }
    const neuf = t.match(/^\s*/)[0] + v + t.match(/\s*$/)[0];
    ecrits.set(n, neuf);
    if (neuf !== t) n.nodeValue = neuf;
  }

  function attributs(el) {
    if (exclu(el, true)) return;
    for (const a of ATTRIBUTS) {
      const t = el.getAttribute(a);
      if (!t) continue;
      // Notre propre écriture revient par l'observateur : ne pas la relire.
      const deja = ecrits.get(el);
      if (deja && deja[a] === t) continue;
      const v = tr(t);
      if (v === null) relever(t);
      if (v !== null && v !== t) {
        ecrits.set(el, Object.assign(deja || {}, { [a]: v }));
        el.setAttribute(a, v);
      }
    }
    if (el.tagName === "INPUT" && (el.type === "button" || el.type === "submit") && el.value) {
      const v = tr(el.value);
      if (v !== null && v !== el.value) el.value = v;
    }
  }

  function parcourir(racine) {
    if (racine.nodeType === 3) { texte(racine); return; }
    if (racine.nodeType !== 1) return;
    attributs(racine);
    if (exclu(racine)) return;
    const w = document.createTreeWalker(racine, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
    for (let n = w.nextNode(); n; n = w.nextNode()) {
      if (n.nodeType === 3) texte(n); else attributs(n);
    }
  }

  document.documentElement.lang = "en";
  if (document.title) { const v = tr(document.title); if (v) document.title = v; }
  parcourir(document.documentElement);
  new MutationObserver((mutations) => {
    for (const m of mutations) {
      if (m.type === "characterData") texte(m.target);
      else if (m.type === "attributes") attributs(m.target);
      else m.addedNodes.forEach(parcourir);
    }
  }).observe(document.documentElement, {
    subtree: true, childList: true, characterData: true,
    attributes: true, attributeFilter: ATTRIBUTS,
  });
  // La page peut traduire à la demande une chaîne qui n'entre pas dans le DOM
  // (un `alert`, un dessin sur `<canvas>`).
  window.ndT = (s) => { const v = tr(String(s)); return v === null ? s : v; };
}
