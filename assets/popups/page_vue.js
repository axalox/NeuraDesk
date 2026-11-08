/*
  **LE RENDU D'UNE PAGE SCANNÉE EN GRAND — PARTAGÉ PAR SES DEUX HÔTES.**

  Une page scannée se regarde à deux endroits :
    - dans le PANNEAU SCANNER (`scanner.html`), en basculant depuis la liste ;
    - dans la FENÊTRE DE PAGE (`page_scan.html`), qu'on ouvre depuis le panneau.

  ⛔ **CE SONT DEUX HÔTES DU MÊME COMPORTEMENT, PAS DEUX FONCTIONNALITÉS.** Les
  recopier ferait diverger l'affichage d'un côté et pas de l'autre — et ce dépôt
  a déjà payé deux fois un `class="cache"` non défini dans une page sur deux.
  Ce fichier porte donc le rendu, et **chaque hôte garde ses `go(…)` LITTÉRAUX** :
  le test de parité des verbes lit les `.html`, pas ce fichier — c'est ce qui
  permet aux deux hôtes d'émettre des verbes différents sous la même apparence.

  ⚠️ **AUCUN VERBE ICI.** Les actions arrivent par des rappels que l'hôte fournit
  (voir `colonnes`) : un verbe écrit dans ce fichier échapperait au test de parité,
  qui est exactement ce qui empêche un bouton muet.

  Chargé par : `scanner.html`, `page_scan.html`. Copié à côté de l'exe par
  `build.rs::copy_popup_assets` (qui balaie `assets/popups/*.{html,css,js}`).
*/
(function () {
  "use strict";

  /** **L'IMAGE, OU SON ABSENCE DITE.**
   *
   *  ⛔ `null` SE MONTRE. La page a pu être retirée du panneau ou effacée depuis ;
   *  garder l'image précédente ferait croire qu'on regarde encore quelque chose.
   *  C'est aussi ce qui couvre le cas d'une capture du harnais sans session.
   *
   *  @param {HTMLImageElement} img l'élément qui porte la page
   *  @param {HTMLElement} absent le bloc qui dit pourquoi il n'y en a pas
   *  @param {string|null} uri l'image en `data:` URI, ou `null`
   *  @param {string} texteAbsent la phrase à montrer quand `uri` est `null`
   *  @returns {void} */
  function appliquerImage(img, absent, uri, texteAbsent) {
    if (uri) {
      img.src = uri;
      img.classList.remove("cache");
      absent.classList.add("cache");
    } else {
      img.removeAttribute("src");
      img.classList.add("cache");
      absent.classList.remove("cache");
      absent.textContent = texteAbsent;
    }
  }

  /** Une colonne d'OCR, construite UNE fois — voir `colonnes`, qui ne remplit
   *  ensuite que ce qui a changé. */
  function colonne(moteur, actions) {
    const bloc = document.createElement("div");
    bloc.className = "moteur";
    bloc.dataset.moteur = moteur.nom;

    const titre = document.createElement("h3");
    titre.textContent = moteur.libelle;

    const rangee = document.createElement("div");
    rangee.className = "rangee";
    const reconnaitre = document.createElement("button");
    reconnaitre.className = "reconnaitre";
    reconnaitre.textContent = "Reconnaître";
    reconnaitre.addEventListener("click", () => actions.reconnaitre(moteur.nom));
    const copier = document.createElement("button");
    copier.className = "copier";
    copier.textContent = "Copier";
    copier.addEventListener("click", () => actions.copier(moteur.nom));
    const enregistrer = document.createElement("button");
    enregistrer.className = "enregistrer";
    enregistrer.textContent = "Enregistrer .txt";
    enregistrer.addEventListener("click", () => actions.enregistrer(moteur.nom));
    rangee.append(reconnaitre, copier, enregistrer);

    const etat = document.createElement("div");
    etat.className = "etat";

    const texte = document.createElement("textarea");
    // ⛔ **EN LECTURE SEULE, MAIS SÉLECTIONNABLE** : on doit pouvoir prendre un
    // morceau à la souris, même si « Copier » prend tout.
    texte.readOnly = true;
    texte.spellcheck = false;

    bloc.append(titre, rangee, etat, texte);
    return bloc;
  }

  /** **UNE COLONNE PAR MOTEUR.**
   *
   *  ⚠️ **ON NE RECONSTRUIT QUE SI LE NOMBRE DE MOTEURS A CHANGÉ.** Refaire les
   *  `<textarea>` à chaque état ferait perdre à l'utilisateur la sélection qu'il
   *  vient de faire à la souris — et l'état est repoussé à chaque frame où il
   *  bouge. Les colonnes existantes sont donc REMPLIES, jamais remplacées.
   *
   *  `actions` = `{ reconnaitre, copier, enregistrer }`, chacune prenant le nom
   *  du moteur. C'est là que les hôtes mettent leurs verbes.
   *
   *  @param {HTMLElement} conteneur le conteneur des colonnes
   *  @param {Array<{nom: string, libelle: string,
   *                 etat: "encours"|"pret"|"echec"|null,
   *                 texte: string|null, message: string|null}>} moteurs
   *         l'état poussé par Rust (`cef_page_scan::etat_ocr`), dans l'ordre
   *  @param {{reconnaitre: (nom: string) => void,
   *           copier: (nom: string) => void,
   *           enregistrer: (nom: string) => void}} actions
   *  @returns {void} */
  function colonnes(conteneur, moteurs, actions) {
    if (conteneur.children.length !== moteurs.length) {
      conteneur.replaceChildren();
      for (const moteur of moteurs) conteneur.appendChild(colonne(moteur, actions));
    }
    for (const moteur of moteurs) {
      const bloc = conteneur.querySelector('[data-moteur="' + moteur.nom + '"]');
      if (bloc) remplir(bloc, moteur);
    }
  }

  /** **L'ÉTAT D'UN MOTEUR, DANS SA COLONNE.** */
  function remplir(bloc, moteur) {
    const etat = bloc.querySelector(".etat");
    const texte = bloc.querySelector("textarea");
    const enCours = moteur.etat === "encours";

    const bouton = bloc.querySelector(".reconnaitre");
    bouton.disabled = enCours;
    bouton.textContent = enCours ? "En cours…" : "Reconnaître";

    // ⚠️ **COPIER ET ENREGISTRER NE SONT ACTIFS QUE S'IL Y A UN TEXTE** : un
    // bouton qui ne peut rien faire doit se VOIR, pas échouer en silence.
    const pret = moteur.etat === "pret" && typeof moteur.texte === "string" && moteur.texte.length > 0;
    bloc.querySelector(".copier").disabled = !pret;
    bloc.querySelector(".enregistrer").disabled = !pret;

    // ⛔ **UN ÉCHEC PORTE SA PHRASE** — c'est Rust qui la fournit
    // (`ocr::Etat::Echec`), jamais la page.
    etat.className = moteur.etat === "echec" ? "etat echec" : "etat";
    if (moteur.etat === "echec") {
      etat.textContent = moteur.message;
    } else if (enCours) {
      etat.textContent = "Reconnaissance en cours…";
    } else if (moteur.etat === "pret" && !pret) {
      // ⭐ **« RIEN TROUVÉ » N'EST PAS UN ÉCHEC** — une page blanche donne zéro
      // caractère, et c'est une réponse honnête.
      //
      // ⛔ **MAIS IL DOIT SE VOIR, ET SE COMPRENDRE.** Signalé le 2026-10-09 :
      // « quand je clique sur les boutons reconnaître rien ne se passe ». Le
      // moteur avait bel et bien tourné et rendu son verdict — **zéro caractère**,
      // parce que la page numérisée était la vitre VIDE. La phrase existait, en
      // 12 px à 80 % d'opacité : indistinguable d'un bouton mort. Une réponse à
      // une action délibérée doit dire **ce qui s'est passé ET la cause
      // probable**, pas seulement constater le vide.
      etat.textContent = "Aucun texte trouvé — la page est probablement vierge "
        + "(une vitre vide, par exemple).";
    } else {
      etat.textContent = "";
    }

    // ⚠️ **ON NE RÉÉCRIT PAS SI C'EST DÉJÀ LE BON** : réassigner `value` remet le
    // curseur au début et efface la sélection.
    const voulu = moteur.texte || "";
    if (texte.value !== voulu) texte.value = voulu;
  }

  /** **LES PALIERS DE ZOOM, EN POURCENTAGE DE LA PAGE AJUSTÉE.** 100 = la page
   *  ENTIÈRE tient dans la zone : c'est le point de départ, et le point de retour.
   *  ⚠️ **CE N'EST PAS « la largeur de la zone »** — une A4 dans un panneau large
   *  serait coupée en bas dès l'ouverture, et l'utilisateur devrait faire défiler
   *  pour voir une page qu'il n'a pas encore lue. */
  const ZOOM_PALIERS = [25, 50, 75, 100, 125, 150, 200, 300, 400];
  const ZOOM_AJUSTE = ZOOM_PALIERS.indexOf(100);

  /** **LA LARGEUR QUI FAIT TENIR TOUTE LA PAGE DANS LA ZONE** — le plus petit des
   *  deux ajustements (par la largeur, par la hauteur). */
  function largeur_ajustee(image, zone) {
    // Le rembourrage de `.page-image` (10 px de chaque côté).
    const dispo_l = Math.max(1, zone.clientWidth - 20);
    const dispo_h = Math.max(1, zone.clientHeight - 20);
    // ⚠️ **TANT QUE L'IMAGE N'EST PAS CHARGÉE, `naturalWidth` VAUT 0** : on rend
    // la largeur disponible plutôt que de diviser par zéro — et `zoomer` réapplique
    // au chargement (voir l'écouteur `load`).
    const rapport = image.naturalWidth > 0 && image.naturalHeight > 0
      ? image.naturalWidth / image.naturalHeight
      : dispo_l / dispo_h;
    return Math.max(1, Math.min(dispo_l, dispo_h * rapport));
  }

  /** **ZOOMER LA PAGE, DANS LA ZONE QUI LA MONTRE.**
   *
   *  ⭐ **LA LOGIQUE EST ICI, LES BOUTONS SONT CHEZ CHAQUE HÔTE** : le panneau
   *  Scanner et la fenêtre de page montrent la même page et doivent zoomer pareil,
   *  mais chacun a son en-tête. Même découpage que `colonnes`.
   *
   *  ⚠️ **LE ZOOM EST UN AFFICHAGE, DONC IL RESTE DANS LA PAGE** : rien ne remonte
   *  à Rust, comme le volet d'OCR. Chaque hôte a donc le sien, et c'est voulu —
   *  on peut regarder une page entière dans le panneau et un détail dans la
   *  fenêtre.
   *
   *  @param {HTMLImageElement} image l'image de la page
   *  @param {HTMLElement} zone le conteneur qui défile (`.page-image`)
   *  @param {{moins: HTMLElement, ajuster: HTMLElement, plus: HTMLElement}} commandes
   *  @returns {void} */
  function zoomer(image, zone, commandes) {
    let palier = ZOOM_AJUSTE;

    function appliquer() {
      const pourcent = ZOOM_PALIERS[palier];
      // ⚠️ **UNE LARGEUR EN PIXELS, PAS UN `transform`** : une transformation ne
      // change pas la mise en page, donc la zone ne grandirait pas avec l'image
      // et les barres de défilement n'apparaîtraient pas — on zoomerait dans le
      // vide, en laissant le bas de la page hors d'atteinte.
      image.style.width = (largeur_ajustee(image, zone) * pourcent / 100).toFixed(1) + "px";
      commandes.ajuster.textContent = pourcent + " %";
      commandes.moins.disabled = palier === 0;
      commandes.plus.disabled = palier === ZOOM_PALIERS.length - 1;
      commandes.ajuster.disabled = pourcent === 100;
    }

    function decaler(delta) {
      palier = Math.min(ZOOM_PALIERS.length - 1, Math.max(0, palier + delta));
      appliquer();
    }

    commandes.moins.addEventListener("click", () => decaler(-1));
    commandes.plus.addEventListener("click", () => decaler(1));
    commandes.ajuster.addEventListener("click", () => {
      palier = ZOOM_AJUSTE;
      appliquer();
    });

    /** ⚠️ **CTRL+MOLETTE, ET LA MOLETTE SEULE CONTINUE DE DÉFILER.** C'est le
     *  geste habituel d'une visionneuse ; l'avaler sans modificateur priverait de
     *  défilement une page qui en a justement besoin quand elle est zoomée. */
    zone.addEventListener("wheel", function (ev) {
      if (!ev.ctrlKey) return;
      ev.preventDefault();
      decaler(ev.deltaY < 0 ? 1 : -1);
    }, { passive: false });

    // ⚠️ **LA PAGE DOIT RESTER AJUSTÉE QUAND LA ZONE CHANGE DE TAILLE** : la
    // fenêtre est redimensionnable, et une largeur figée en pixels laisserait la
    // page déborder après un agrandissement — ou minuscule après un rétrécissement.
    //
    // ⛔ **ET L'OBSERVATEUR NE RÉAGIT QU'À UN VRAI CHANGEMENT DE TAILLE.** Sans ce
    // garde, il se nourrit lui-même : `appliquer` règle la largeur de l'image, la
    // mise en page change, l'observateur rappelle `appliquer`… Chromium finit par
    // signaler « ResizeObserver loop completed with undelivered notifications » —
    // et ce n'est pas une erreur de ce code, c'est le symptôme de la boucle.
    if (typeof ResizeObserver === "function") {
      let derniere_zone = "";
      new ResizeObserver(function () {
        const taille = zone.clientWidth + "x" + zone.clientHeight;
        if (taille === derniere_zone) return;
        derniere_zone = taille;
        appliquer();
      }).observe(zone);
    }
    // Et au CHARGEMENT de l'image : `naturalWidth` n'est connu qu'à ce moment-là,
    // donc le premier ajustement est faux tant qu'elle n'est pas arrivée.
    image.addEventListener("load", appliquer);

    appliquer();
  }

  /** **LA BORNE DES TROIS CURSEURS — LA MÊME QUE COTE RUST**
   *  (`win32::amelioration::BORNE`) : −100 à +100, **0 = inchangé**. Deux bornes
   *  différentes feraient deux « au maximum » qui ne veulent pas dire la même
   *  chose entre l'aperçu et le fichier écrit. */
  const BORNE_REGLAGE = 100;

  /** Les trois réglages, dans l'ORDRE DU CONTRAT : luminosité, contraste,
   *  saturation — le même ordre que `Reglage::filtre_css` et qu'`appliquer_pixel`.
   *  Un ordre différent entre la page et Rust n'est pas une nuance de style : les
   *  trois filtres ne commutent pas (contraste puis luminosité ≠ luminosité puis
   *  contraste), et ce serait l'aperçu qui ne ressemble plus au fichier. */
  const REGLAGES = [["luminosite", "Luminosité"], ["contraste", "Contraste"], ["saturation", "Saturation"]];

  /** **LA BARRE D'AMÉLIORATION DE L'IMAGE — LES MÊMES COMMANDES POUR LES DEUX HÔTES.**
   *
   *  ⭐ **LE DOM EST ICI, LES VERBES RESTENT CHEZ CHAQUE HÔTE** — le même découpage
   *  que `colonnes` : recopier la barre dans les deux `.html` ferait diverger
   *  l'apparence d'un côté et pas de l'autre (ce dépôt a déjà payé deux fois un
   *  `class="cache"` défini dans une page sur deux), tandis que les `go(…)` doivent
   *  rester LITTÉRAUX dans les `.html` parce que c'est EUX que lit le test de
   *  parité des verbes.
   *
   *  ⭐ **L'APERÇU EST LE FILTRE QUE RUST ÉMET DANS L'ÉTAT.** La page ne calcule
   *  aucune formule : elle pose `etat.filtre` sur l'image. Écrire une formule ici en
   *  ferait deux — celle de l'aperçu et celle du fichier — et le défaut serait
   *  exactement celui qu'on promet d'éviter : ce que la personne regarde n'est pas
   *  ce qui est écrit.
   *
   *  ⚠️ **RIEN NE RECHARGE L'IMAGE.** Un réglage de quelques pourcents ne mérite
   *  pas de renvoyer une `data:` URI de plusieurs mégaoctets à chaque crantement
   *  d'ascenseur : `filter` ne touche que le rendu.
   *
   *  ⚠️ **LE DRAG ENVOIE LE JEU COMPLET À CHAQUE PAS, ET CE N'EST PAS UN GASPILLAGE.**
   *  Une navigation `neuradesk://` lancée avant la fin de la précédente est annulée
   *  par Chromium : des pas du milieu peuvent se perdre — **sans conséquence**,
   *  parce que chaque envoi porte les TROIS valeurs et que le dernier gagné décrit
   *  l'état entier. N'envoyer que la valeur bougée perdrait, elle, de l'information.
   *
   *  @param {HTMLElement} conteneur la barre vide où poser les commandes
   *  @param {HTMLImageElement} image l'image de la page
   *  @param {{auto: () => void, regler: (l: number, c: number, s: number) => void,
   *           zero: () => void, copie: () => void}} actions les verbes de l'hôte.
   *          `copie` ET PAS `enregistrer` : `actions.enregistrer` est déjà pris par
   *          la colonne d'OCR, qui enregistre le TEXTE reconnu — deux sens d'un
   *          même nom dans un même objet feraient l'un des deux boutons faux.
   *  @returns {{regler: (etat: object) => void}} `regler` reçoit
   *          `etat.amelioration` de Rust : `{luminosite, contraste, saturation, filtre}`
   */
  function amelioration(conteneur, image, actions) {
    const ascenseurs = {};

    /** **ENVOYER LES TROIS VALEURS D'UN COUP.** */
    function envoyer() {
      actions.regler(
        ascenseurs.luminosite.valueAsNumber || 0,
        ascenseurs.contraste.valueAsNumber || 0,
        ascenseurs.saturation.valueAsNumber || 0
      );
    }

    for (const [cle, libelle] of REGLAGES) {
      const etiquette = document.createElement("label");
      etiquette.className = "reglage";
      const titre = document.createElement("span");
      titre.textContent = libelle;
      const ascenseur = document.createElement("input");
      ascenseur.type = "range";
      ascenseur.min = String(-BORNE_REGLAGE);
      ascenseur.max = String(BORNE_REGLAGE);
      ascenseur.step = "1";
      ascenseur.value = "0";
      ascenseur.setAttribute("aria-label", libelle);
      etiquette.append(titre, ascenseur);
      conteneur.appendChild(etiquette);
      ascenseurs[cle] = ascenseur;
      ascenseur.addEventListener("input", envoyer);
    }

    const bouton = (texte, libelle, action) => {
      const b = document.createElement("button");
      b.textContent = texte;
      b.title = libelle;
      b.addEventListener("click", action);
      conteneur.appendChild(b);
      return b;
    };
    bouton("Auto", "Étirer les niveaux de la page : luminosité et contraste calculés sur son histogramme", () => actions.auto());
    bouton("Zéro", "Revenir à la page telle qu'elle est numérisée", () => actions.zero());
    bouton("Enregistrer la copie", "Écrire la page améliorée dans un NOUVEAU fichier, à côté de la numérisation (jamais par-dessus)", () => actions.copie());

    /** **L'ÉTAT POUSSé PAR RUST, REPORTé SUR LES COMMANDES.**
     *
     *  ⚠️ **ON NE RÉÉCRIT UN ASCENSEUR QUE S'IL NE VAUT PAS DéJÀ LA MÊME VALEUR** :
     *  réassigner la valeur pendant un drag remettrait le pouce et luterait contre
     *  la main. Même garde que `remplir` pour les `<textarea>` (une réécriture
     *  efface la sélection de l'utilisateur).
     *
     *  ⚠️ **ET LE FILTRE VIENT DE RUST, TEL QUEL.** `undefined` (état d'avant, ou
     *  page sans `amelioration`) devient `none` : pas de filtre sur une page qu'on
     *  n'a pas touchée.
     */
    function regler(etat) {
      for (const [cle] of REGLAGES) {
        const valeur = etat && typeof etat[cle] === "number" ? etat[cle] : 0;
        if (ascenseurs[cle].value !== String(valeur)) ascenseurs[cle].value = String(valeur);
      }
      image.style.filter = (etat && etat.filtre) || "none";
    }

    return { regler: regler };
  }

  window.PageVue = {
    appliquerImage: appliquerImage,
    colonnes: colonnes,
    zoomer: zoomer,
    amelioration: amelioration,
  };
})();
