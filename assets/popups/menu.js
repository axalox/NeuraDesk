// HAUTEUR D'UN MENU — la page MESURE son propre contenu et l'annonce au backend,
// qui redimensionne la fenêtre (voir `win32::cef_popup::drain_actions`).
//
// DEMANDE EXPLICITE : « je voudrais que le calcule de la taille des menus soient
// pris en charge par le backend automatiquement à l'ouverture d'un menu ».
// Auparavant la hauteur venait d'une estimation calibrée en Rust
// (`cef_popup::button_list_height`), systématiquement trop généreuse — mesuré par
// capture : 157 px de vide en bas du menu Contexte, 30 % de sa hauteur.
//
// **CE FICHIER EST PARTAGÉ par les neuf menus**, exactement comme `menu.css` —
// même motif, même chargement relatif. Les quatre PANNEAUX (Dash, Réglages,
// Presse-papiers, Notes) ne le chargent PAS : leur hauteur est celle de la zone
// libre du dock, pas de leur contenu, et le backend le sait de toute façon
// (`PopupSlot::content_factor` n'est posé que pour les menus).
//
// **PRÉREQUIS CÔTÉ CSS** : la boîte du menu doit être en `height: auto;
// max-height: 100vh` — en `height: 100vh` elle vaut TOUJOURS la fenêtre, donc la
// page ne peut rien annoncer de plus court qu'elle et le backend n'apprend rien.
// C'est ce qui a fait échouer une tentative précédente, en silence.
(function () {
  // La boîte du menu. Le repli sur `body.firstElementChild` vaut pour les neuf
  // pages actuelles ; **une page qui gagnerait un élément frère avant sa boîte
  // devra porter `data-menu-root`**, sans quoi elle mesurerait le mauvais
  // élément — c'est le seul point fragile de ce fichier.
  function box() {
    return document.querySelector("[data-menu-root]") || document.body.firstElementChild;
  }

  // **LA GARDE DE DÉDUPLICATION EST CE QUI REND CECI SÛR**, pas la configuration
  // de l'observateur : certains menus RECONSTRUISENT leur liste à chaque frame
  // (`sound.html`), donc l'observateur y tire en continu. Elle coupe aussi la
  // boucle « redimensionner -> la vue change -> mesurer -> renvoyer ».
  let lastSent = null;

  // `force` : demandé par le backend à CHAQUE ouverture (voir
  // `cef_popup::request_content_measure`), qui a besoin d'une mesure fraîche pour
  // l'état COURANT — la page a pu se mesurer alors qu'elle était encore cachée
  // (préchargement), et la garde ci-dessous empêcherait alors tout renvoi.
  //
  // **LA COMPARAISON EST STRICTE (`force !== true`) À DESSEIN, ET C'EST LE PIÈGE
  // DE CETTE FONCTION** : elle est aussi appelée par `addEventListener("load", …)`
  // et par le `MutationObserver` plus bas, qui lui passent respectivement un
  // `Event` et un tableau d'entrées — **deux valeurs VRAIES**. Un `if (force)`
  // forcerait donc à chaque mutation, c'est-à-dire à chaque frame pour
  // `sound.html` : très exactement la boucle que cette garde existe pour empêcher.
  function measure(force) {
    const el = box();
    if (!el) return;
    const rect = el.getBoundingClientRect();
    // LE MAXIMUM DES DEUX, et c'est voulu :
    // - `scrollHeight` couvre le cas où la boîte est PLAFONNÉE par `max-height`
    //   (il rend quand même la hauteur totale du contenu) et le cas d'une marge
    //   basse que le navigateur exclut de la boîte ;
    // - `getBoundingClientRect().height` est la boîte de BORDURE, donc le
    //   `padding` y est COMPRIS par construction — ce qui remplace la constante
    //   `OVERHEAD` du backend par une mesure.
    // Mesurer `documentElement` ou `body` serait FAUX : tous deux sont
    // dimensionnés sur la fenêtre, donc leur hauteur ne dit rien du contenu.
    // **LA LARGEUR AUSSI** — DEMANDE EXPLICITE : « le menu du wifi est trop large
    // par rapport à son contenu » (1380 px pour deux boutons, un littéral figé
    // cote Rust). Elle suppose `width: fit-content` sur la boîte (voir le CSS des
    // pages) : sans lui, la boîte fait toujours la largeur de la fenêtre et la
    // mesure ne dirait rien du contenu.
    const w = Math.ceil(Math.max(el.scrollWidth, rect.width)) + 1;
    const h = Math.ceil(Math.max(el.scrollHeight, rect.height)) + 1;

    // **LES DEUX DANS UNE SEULE ACTION, ET C'EST OBLIGATOIRE** : deux
    // `location.href` émis dans le même tick se TÉLESCOPENT (le second annule la
    // navigation du premier), donc une dimension se perdrait une fois sur deux,
    // en silence. Voir `cef_popup::parse_content_size`.
    const sent = w + "x" + h;
    if (sent === lastSent && force !== true) return;
    lastSent = sent;
    location.href = "neuradesk://menu/size/" + w + "/" + h;
  }

  // Appelable depuis Rust (`cef_popup::request_content_measure`, à chaque
  // ouverture) : c'est le rattrapage d'une action PERDUE — deux `location.href`
  // rapprochés se TÉLESCOPENT, et la garde ci-dessus empêche alors tout renvoi
  // spontané — et le SEUL moyen de rafraîchir un menu qui ne mute jamais
  // (`calendar.html`, dont la hauteur change pourtant d'un mois à l'autre). Le
  // booléen demandé par le backend passe la garde ; les appels internes de cette
  // page (chargement, observateur) ne le passent pas.
  window.__ndMeasure = measure;

  // Première mesure : la boîte est en place quand ce script tourne (il est chargé
  // en fin de `<body>`), mais les polices peuvent encore changer la mise en page
  // — d'où aussi `load`.
  window.addEventListener("load", measure);

  // **L'OBSERVATEUR PORTE SUR LA BOÎTE, JAMAIS SUR LE DOCUMENT** :
  // `push_menu_palette` écrit des variables CSS sur `documentElement` à CHAQUE
  // frame ; un observateur placé là tirerait à chaque frame pour rien. Le
  // `attributeFilter` est la seconde barrière, si quelqu'un le remonte un jour.
  const el = box();
  if (el) {
    new MutationObserver(measure).observe(el, {
      childList: true,
      subtree: true,
      characterData: true,
      attributes: true,
      attributeFilter: ["class", "style"],
    });
  }
})();
