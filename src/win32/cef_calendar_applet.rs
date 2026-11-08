//! Fenêtre CEF de l'APPLET CALENDRIER (zone Intégrés de la barre de droite) —
//! DEMANDE EXPLICITE (2026-09-23) : « une applet calendrier [...] qui affichent
//! les fêtes du jour et les dates d'anniversaires avec la possibilité de mettre
//! un rappel automatique ».
//!
//! **CE N'EST PAS LE MINI CALENDRIER DE L'HORLOGE.** `win32::cef_calendar` est un
//! MENU (fondant, ancré sous l'horloge) dont la page calcule tout seule et qui
//! n'a AUCUNE IPC. Celui-ci est un PANNEAU — il remplit la zone libre du dock
//! comme le Dash et la visionneuse du presse-papiers (voir `app::dash_rect`,
//! `PopupSpec::fades: false`) — et il a besoin des deux sens d'IPC : Rust lui
//! pousse les fêtes et les anniversaires, la page lui renvoie des ajouts, des
//! retraits, des MODIFICATIONS et des bascules de rappel.
//!
//! **TOUTE LA CONNAISSANCE EST DANS `win32::fetes`** : ce module ne calcule ni
//! un jour férié, ni une fête, ni un rappel — il lit, affiche et écrit. C'est ce
//! qui rend la fenêtre vérifiable par les tests de ce module-là, sans CEF.
//!
//! Toute la mécanique fenêtre/rendu/relais souris est dans `win32::cef_popup` —
//! voir `win32::cef_test_window` pour la doc de module complète du pattern suivi
//! ici, et `win32::cef_clipboard`, dont ce module reprend la structure.

use std::cell::{Cell, RefCell};

use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::config;
use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::fetes::{self, Date};

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    /// Voir `app::content_scale_pour` — mémorisé à l'ouverture, relu à chaque
    /// frame, même mécanique que `win32::cef_calendar::CONTENT_SCALE`.
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// LA DERNIÈRE REQUÊTE DE RECHERCHE — voir `republier_recherche`, qui est la
    /// correction d'un DÉFAUT RÉEL SIGNALÉ : « en cliquant sur un prénom dans
    /// les résultats [...] le clic n'est pas pris en compte ».
    static RECHERCHE: RefCell<String> = const { RefCell::new(String::new()) };
    /// **CE QUE L'UTILISATEUR REGARDE** — (année, mois, jour) du mois AFFICHÉ et
    /// du jour SÉLECTIONNÉ. Sans elle la page ne pouvait montrer que le jour du
    /// jour : c'est ce que la grille vient changer.
    ///
    /// ⚠️ **POSÉE DANS `toggle`, JAMAIS DANS `push_state`** : celui-ci tourne à
    /// CHAQUE frame (voir `refresh_if_visible`), et y remettre la vue sur
    /// aujourd'hui écraserait la navigation soixante fois par seconde.
    static VUE: Cell<(i32, u32, u32)> = const { Cell::new((0, 0, 0)) };
    /// Le dernier `window.setState(...)` ÉMIS, avec la clé qui l'a produit.
    ///
    /// **Même mécanique que `win32::cef_calendar::PAYLOAD`, et elle devient
    /// NÉCESSAIRE ici** : sans grille, `push_state` ne calculait qu'UNE fête par
    /// frame ; avec elle il en calcule **tout un mois**. La clé porte donc la vue
    /// *et* l'empreinte des dates saisies — l'utilisateur peut en ajouter une
    /// pendant que le panneau est ouvert (même raison que `cef_calendar.rs`).
    static PAYLOAD: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    /// Une seule application du levier `NEURADESK_DEBUG_MOIS` — voir
    /// `debug_mois` : contrairement à `NEURADESK_DEBUG_EDIT`, il **accumule**.
    static MOIS_APPLIQUE: Cell<bool> = const { Cell::new(false) };
    /// Idem pour `NEURADESK_DEBUG_JOUR` — voir `debug_jour`.
    static JOUR_APPLIQUE: Cell<bool> = const { Cell::new(false) };
    /// Une seule application du levier `NEURADESK_DEBUG_DEFILER` — le défilement
    /// n'a de sens qu'une fois, et le levier est répété à chaque frame.
    static DEFILE: Cell<bool> = const { Cell::new(false) };
    /// Le levier `NEURADESK_DEBUG_EDIT` est **répété à chaque frame** (voir
    /// `app::run`, et pourquoi) : ce drapeau fait que sa trace n'est écrite
    /// qu'**une fois** — sans quoi le journal de diagnostic serait noyé sous
    /// soixante lignes identiques, et la ligne utile deviendrait introuvable.
    /// Mesuré : sans ce garde, un seul run en produisait une soixantaine.
    static EDIT_TRACE_ECRITE: Cell<bool> = const { Cell::new(false) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefCalendarAppletWindow"),
    window_title: w!("NeuraDesk — Calendrier"),
    html_file: "calendar_applet.html",
    wndproc: Some(wndproc),
    // PANNEAU — reste SOUS le dock agrandi, et ne fond pas : même famille que la
    // visionneuse du presse-papiers, dont cette fenêtre reprend la structure.
    above_a_magnified_dock: false,
    fades: false,
    fenetre: false,
};

/// La date du jour, telle que Windows la connaît — **PAS `Date()` côté page** :
/// la date affichée doit être celle de l'application, et une page qui la
/// déduirait du navigateur pourrait diverger (fuseau, horloge système simulée
/// par le harnais de capture).
pub fn aujourdhui() -> Date {
    let t = unsafe { GetLocalTime() };
    Date { annee: t.wYear as i32, mois: t.wMonth as u32, jour: t.wDay as u32 }
}

/// Précharge ce popup au démarrage — voir `win32::cef_popup::preload`.
pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// Ouvre (ou referme) la fenêtre — voir `win32::cef_popup::toggle_with`.
/// `anniversaires` : l'état COURANT, poussé dans la page à chaque ouverture.
pub fn toggle(
    rect: [i32; 4],
    gfx: &GfxDevice,
    content_scale: f32,
    dates: &[config::DatePersonnalisee],
    rappels: &[config::RappelFete],
) {
    // L'échelle d'écran × le réglage « Taille du contenu » du panneau.
    CONTENT_SCALE.with(|c| c.set(content_scale * crate::win32::taille_panneaux::facteur("calendar")));
    // **RÉOUVERTURE → AUJOURD'HUI** (demande explicite). C'est ICI et non dans
    // `push_state`, qui tourne à chaque frame.
    let auj = aujourdhui();
    VUE.with(|v| v.set((auj.annee, auj.mois, auj.jour)));
    // Le cache est vidé à l'ouverture : la fenêtre est PRÉCHARGÉE, et un
    // `execute_js` émis avant que la page ait son état ne fait rien en silence
    // (voir `debug_editer`). Repousser une fois par ouverture coûte un payload
    // et supprime ce risque.
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, || {
        // Même « zone libre » que le Dash : masque les fenêtres qui s'y
        // superposeraient — même liste que `win32::cef_clipboard::toggle`.
        crate::win32::cef_dash::hide_if_visible();
        crate::win32::cef_settings::hide_if_visible();
        crate::win32::cef_notes::hide_if_visible();
        crate::win32::cef_image_editor::hide_if_visible();
        crate::win32::cef_minuteur::hide_if_visible();
        crate::win32::cef_surveillance::hide_if_visible();
        crate::win32::cef_scanner::hide_if_visible();
        crate::win32::cef_shaders::hide_if_visible();
        crate::win32::cef_lueurs::hide_if_visible();
        crate::win32::cef_textures::hide_if_visible();
        crate::win32::cef_moniteur::hide_if_visible();
        crate::win32::cef_calculatrice::hide_if_visible();
        crate::win32::cef_recherche::hide_if_visible();
        crate::win32::cef_automatisations::hide_if_visible();
        crate::win32::cef_meteo::hide_if_visible();
        crate::win32::cef_aide::hide_if_visible();
        crate::win32::bandeau::hide_if_visible();
        crate::win32::cef_ssh::hide_if_visible();
        crate::win32::cef_terminal::hide_if_visible();
    });
    push_state(dates, rappels);
}

/// Repousse l'état SEULEMENT si la fenêtre est visible — à appeler après toute
/// modification des anniversaires (voir `app::run`), sinon la page resterait
/// figée sur ce qu'elle avait à l'ouverture.
pub fn refresh_if_visible(dates: &[config::DatePersonnalisee], rappels: &[config::RappelFete]) {
    if cef_popup::is_visible(&SLOT) {
        push_state(dates, rappels);
    }
}

/// La date du jour en toutes lettres — **localisée par WINDOWS**, jamais par une
/// table française écrite ici : le dépôt fait déjà confiance à
/// `GetDateFormatEx` pour la grille du mini calendrier (voir `win32::calendar`,
/// doc de module), et une seconde table finirait par diverger de la première.
fn date_en_toutes_lettres(date: Date) -> String {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::Globalization::GetDateFormatEx;
    let st = SYSTEMTIME {
        wYear: date.annee as u16,
        wMonth: date.mois as u16,
        wDay: date.jour as u16,
        ..Default::default()
    };
    // « dddd d MMMM yyyy » → « mercredi 23 septembre 2026 ». La casse initiale
    // est ajustée par la page (voir son `text-transform: capitalize`), parce que
    // Windows rend le nom du jour en minuscules en français.
    let format: Vec<u16> = "dddd d MMMM yyyy".encode_utf16().chain(std::iter::once(0)).collect();
    let mut buffer = [0u16; 128];
    // `PCWSTR::null()` pour les deux extrémités : LOCALISÉ PAR WINDOWS dans la
    // langue de l'utilisateur, exactement comme `win32::calendar::format_date`
    // — c'est ce qui donne « mercredi 23 septembre 2026 » sans table française
    // écrite ici.
    let n = unsafe {
        GetDateFormatEx(
            PCWSTR::null(),
            Default::default(),
            Some(&st),
            PCWSTR(format.as_ptr()),
            Some(&mut buffer),
            PCWSTR::null(),
        )
    };
    if n == 0 {
        return format!("{}/{}/{}", date.jour, date.mois, date.annee);
    }
    String::from_utf16_lossy(&buffer[..(n as usize - 1)])
}

/// Décale `(annee, mois)` de `delta` mois — l'arithmétique est triviale, et
/// `fetes::jours_dans_le_mois`/`plus_jours` sont **privées** : on ne duplique
/// rien, on ne fait simplement pas appel à elles pour si peu.
///
/// ⚠️ **PARTAGÉE, PAS RECOPIÉE** : `cef_calendar` (le mini calendrier de
/// l'horloge) s'en sert aussi pour borner sa fenêtre de mois. Les deux grilles
/// du dépôt DOIVENT compter les mois de la même façon — deux copies
/// divergeraient au premier changement, et un décalage d'un mois entre elles ne
/// se verrait qu'en feuilletant à la main.
pub(crate) fn decaler_mois(annee: i32, mois: u32, delta: i32) -> (i32, u32) {
    let total = annee * 12 + (mois as i32 - 1) + delta;
    (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32)
}

/// Le jour à sélectionner en arrivant sur un mois : **aujourd'hui s'il y est,
/// sinon le 1er** (décision utilisateur). Le 1er existe toujours, donc rien à
/// rogner — la longueur du mois est connue de la page, pas d'ici.
fn choix_du_jour(annee: i32, mois: u32, auj: Date) -> u32 {
    if auj.annee == annee && auj.mois == mois { auj.jour } else { 1 }
}

/// Déplace la vue de `delta` mois et **repose la sélection** — sans quoi elle
/// resterait sur un jour qui n'existe pas dans le mois d'arrivée.
///
/// ⚠️ **UN SEUL CHEMIN** : le verbe `calendar/mois` et le levier de capture
/// `NEURADESK_DEBUG_MOIS` passent tous deux par ici. Un levier qui prendrait un
/// raccourci vérifierait autre chose que ce que l'utilisateur déclenche.
fn aller_au_mois(delta: i32) {
    let auj = aujourdhui();
    let (annee, mois, _) = VUE.with(|v| v.get());
    let (a, m) = decaler_mois(annee, mois, delta);
    VUE.with(|v| v.set((a, m, choix_du_jour(a, m, auj))));
}

/// Ramène la vue sur aujourd'hui — mois **et** jour d'un seul coup.
fn revenir_a_aujourdhui() {
    let auj = aujourdhui();
    VUE.with(|v| v.set((auj.annee, auj.mois, auj.jour)));
}

/// Sélectionne un jour du mois affiché.
///
/// ⚠️ **BORNE LARGE (1..=31), COMME `analyser_date`** : le dépôt accepte déjà
/// qu'une date saisie déborde de son mois (« un 31 février existe », voir
/// `fetes::evenements_du_mois`). La grille ne propose que des jours réels, donc
/// ce cas ne vient jamais d'un clic — il vient d'une page modifiée, et il est
/// journalisé plutôt que « réparé ».
fn choisir_jour(jour: u32) {
    if !(1..=31).contains(&jour) {
        eprintln!("[neuradesk][cef] jour refusé (hors bornes) : {jour}");
        return;
    }
    let (annee, mois, _) = VUE.with(|v| v.get());
    VUE.with(|v| v.set((annee, mois, jour)));
}

fn push_state(dates: &[config::DatePersonnalisee], rappels: &[config::RappelFete]) {
    let auj = aujourdhui();
    let (annee, mois, jour) = VUE.with(|v| v.get());
    // ⚠️ **LE JOUR DU JOUR ENTRE DANS LA CLÉ** en plus de la vue : la grille
    // marque « aujourd'hui », et ce repère doit suivre minuit même si personne
    // ne navigue.
    let mut cle = format!("{annee} {mois} {jour} | {} {} {}", auj.annee, auj.mois, auj.jour);
    for d in dates {
        cle.push_str(&format!("|{} {} {} {:?} {:?}", d.nom, d.jour, d.mois, d.genre, d.annee));
    }
    // ⚠️ **LES RAPPELS DE FÊTES AUSSI, ET C'EST UN PIÈGE PAYÉ D'AVANCE** : la
    // liste FÊTES les affiche désormais, or ils se modifient DANS CETTE PAGE
    // (la cloche appelle `calendar/rappel-fete`). Sans cette empreinte dans la
    // clé, éteindre une alarme ne repeindrait rien — la ligne resterait à
    // l'écran jusqu'au changement de jour, en silence.
    for r in rappels {
        cle.push_str(&format!("|r{} {} {}", r.nom, r.jour, r.mois));
    }
    if PAYLOAD.with(|c| c.borrow().as_ref().is_some_and(|(k, _)| *k == cle)) {
        return;
    }
    let selection = Date { annee, mois, jour };
    let fetes: Vec<serde_json::Value> = fetes::fetes_du_jour(selection)
        .into_iter()
        .map(|f| {
            json!({
                "nom": f.nom,
                "categorie": f.categorie.cle(),
                "varianteDe": f.variante_de,
                // **LA CLOCHE DE CETTE FÊTE** — demande explicite (2026-09-24) :
                // « pouvoir rajouter un rappel de fête depuis le dash calendrier
                // en cliquant sur une cloche affichée sur les prénoms du jour
                // sélectionné ».
                //
                // ⚠️ **LE TRIPLET EST CELUI DU JOUR AFFICHÉ**, pas celui
                // d'aujourd'hui : la page peut montrer le 21 septembre, et poser
                // l'alarme sur le 24 serait un contresens silencieux.
                "rappel": fetes::rappel_fete_actif(rappels, mois, jour, f.nom),
            })
        })
        .collect();
    // LE MOIS ENTIER, pour la grille — c'est ce que le cache ci-dessus rend
    // supportable, `evenements_du_mois` balayant les 31 jours.
    let jours: Vec<serde_json::Value> = fetes::evenements_du_mois(annee, mois, dates, rappels)
        .into_iter()
        .map(|(j, evs)| {
            json!({
                "jour": j,
                "evenements": evs.into_iter()
                    .map(|e| json!({ "nom": e.nom, "categorie": e.categorie.cle(), "annee": e.annee }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    let liste: Vec<serde_json::Value> = dates
        .iter()
        .map(|d| json!({ "nom": d.nom, "jour": d.jour, "mois": d.mois, "rappel": d.rappel, "genre": d.genre.cle(), "annee": d.annee }))
        .collect();
    // **LES PRÉNOMS DE LA TABLE QUI PORTENT UNE ALARME** — demande explicite :
    // « fais apparaitre les prénoms pour lesquels il y a une alarme dans la liste
    // fêtes personnalisées ». Ce ne sont PAS des dates saisies : elles viennent
    // du calendrier, et la page n'a donc qu'à les montrer et à savoir éteindre
    // leur cloche (`calendar/rappel-fete`, un verbe qui existe déjà).
    let rappels_fetes: Vec<serde_json::Value> = rappels
        .iter()
        .map(|r| json!({ "nom": r.nom, "jour": r.jour, "mois": r.mois }))
        .collect();
    let payload = json!({
        "date": date_en_toutes_lettres(selection),
        // L'année COURANTE part avec le reste : c'est elle qui permet à la page
        // de calculer l'âge sans lire l'horloge de son côté (voir sa doc).
        "aujourdhui": { "jour": auj.jour, "mois": auj.mois, "annee": auj.annee },
        // La vue, et de quoi écrire l'en-tête de la liste (« FÊTES DU JOUR » ou
        // « FÊTES DU 21 SEPTEMBRE »).
        "selection": { "jour": jour, "mois": mois, "annee": annee },
        "estAujourdhui": annee == auj.annee && mois == auj.mois && jour == auj.jour,
        "mois": { "annee": annee, "mois": mois, "jours": jours },
        "fetes": fetes,
        "dates": liste,
        "rappelsFetes": rappels_fetes,
        "contentScale": CONTENT_SCALE.with(|c| c.get()),
    });
    let js = format!("window.setState({payload})");
    PAYLOAD.with(|c| *c.borrow_mut() = Some((cle, js.clone())));
    cef_popup::execute_js(&SLOT, &js);
}

/// Les résultats d'une recherche, prêts à être poussés dans la page.
fn resultats_json(texte: &str, dates: &[config::DatePersonnalisee], rappels: &[config::RappelFete]) -> serde_json::Value {
    let liste: Vec<serde_json::Value> = fetes::chercher(texte, dates, rappels)
        .into_iter()
        .map(|r| {
            // « cible » dit à la page COMMENT agir sur ce résultat, et « index »
            // ne vaut quelque chose que pour une date saisie — c'est le seul cas
            // qui se désigne par un rang.
            let (cible, index) = match r.cible {
                fetes::Cible::Table => ("table", serde_json::Value::Null),
                fetes::Cible::Saisie(i) => ("saisie", json!(i)),
                fetes::Cible::Calculee => ("calculee", serde_json::Value::Null),
            };
            json!({
                "nom": r.nom, "jour": r.jour, "mois": r.mois,
                "categorie": r.categorie.cle(),
                "cible": cible, "index": index, "rappel": r.rappel,
                // Vide sauf pour une variante orthographique — la page écrit
                // alors « = Jean-Baptiste » (voir `Fete::variante_de`).
                "varianteDe": r.variante_de,
            })
        })
        .collect();
    serde_json::Value::Array(liste)
}

/// **REPOUSSE LES RÉSULTATS DE LA RECHERCHE EN COURS** — à appeler après TOUTE
/// action qui change ce qu'une recherche afficherait.
///
/// ⚠️ **C'EST LA CORRECTION D'UN DÉFAUT RÉEL SIGNALÉ** : « en cliquant sur un
/// prénom dans les résultats de la recherche [...] je n'arrive pas à cocher la
/// cloche d'une fête ni à décocher un anniversaire, le clic n'est pas pris en
/// compte ». Il ÉTAIT pris en compte — le rappel était bien enregistré — mais
/// **la page ne le savait jamais** : les résultats n'étaient poussés qu'en
/// réponse à une frappe, donc la cloche restait dessinée dans son ancien état,
/// et l'utilisateur voyait un clic mort.
///
/// ⚠️ **ET LE DÉFAUT ÉTAIT PIRE QUE COSMÉTIQUE** : `add` et `remove` DÉCALENT les
/// index des dates saisies, or c'est par cet index que la cloche les adresse
/// (`calendar/rappel/<index>`). Une liste de résultats périmée aurait donc coché
/// le rappel d'une AUTRE date que celle affichée. Republier après chaque
/// changement ferme les deux.
fn republier_recherche(dates: &[config::DatePersonnalisee], rappels: &[config::RappelFete]) {
    // Emprunt COURT et refermé avant tout appel : `execute_js` ne doit jamais
    // s'exécuter pendant que ce `RefCell` est emprunté (voir la mémoire du dépôt
    // sur les réentrances de `RefCell` dans un `wndproc`).
    let texte = RECHERCHE.with(|r| r.borrow().clone());
    if texte.trim().is_empty() {
        return;
    }
    cef_popup::execute_js(&SLOT, &format!("window.setResultats({})", resultats_json(&texte, dates, rappels)));
}

/// Analyse les arguments d'une date saisie — **PURE**, et c'est ce qui la rend
/// testable sans CEF : le harnais de capture ne sait ni cliquer dans ce panneau
/// ni taper au clavier, donc une validation qui vivrait en ligne dans les bras
/// du `match` ci-dessous ne serait vérifiable par AUCUN moyen.
///
/// **UNE SEULE RÈGLE, DEUX APPELANTS.** `["add"]` et `["modifier"]` passent tous
/// deux par ici : une modification est validée exactement comme un ajout, et il
/// n'existe pas deux jeux de bornes qui pourraient diverger.
///
/// Rend `None` en JOURNALISANT la raison — jamais de réparation silencieuse :
/// un 31 février n'a pas de correction devinable.
fn analyser_date(
    genre: &str,
    nom: &str,
    jour: &str,
    mois: &str,
    rappel: &str,
    annee: &str,
) -> Option<config::DatePersonnalisee> {
    let (Ok(jour), Ok(mois)) = (jour.parse::<u32>(), mois.parse::<u32>()) else {
        eprintln!("[neuradesk][cef] date refusée (date illisible) : {jour:?}/{mois:?}");
        return None;
    };
    // **UN GENRE INCONNU EST REFUSÉ**, jamais rangé d'office dans une
    // famille — même politique que `config::load_dates`, qui écarte
    // aussi une entrée qu'il ne sait pas classer.
    let Some(genre) = config::Genre::depuis_cle(genre) else {
        eprintln!("[neuradesk][cef] date refusée (genre inconnu) : {genre:?}");
        return None;
    };
    let nom = nom.trim();
    // 31 est la borne LARGE (le mois le plus long) ; le jour exact est
    // ensuite confronté à la longueur du mois par `fetes`, qui est le
    // seul à savoir si l'année est bissextile.
    if nom.is_empty() || !(1..=12).contains(&mois) || !(1..=31).contains(&jour) {
        eprintln!("[neuradesk][cef] date refusée (date impossible) : {nom:?} {jour}/{mois}");
        return None;
    }
    // L'ANNÉE est FACULTATIVE — DEMANDE EXPLICITE (2026-09-23) :
    // « pour les dates d'anniversaire j'aimerais pouvoir ajouter
    // l'année sur quatre chiffres ». Vide = pas d'année, et c'est le
    // cas ordinaire : on ne connaît pas toujours l'âge.
    //
    // ⚠️ UNE ANNÉE HORS BORNES EST REFUSÉE, jamais corrigée — même
    // politique que la date elle-même (voir `config::ANNEE_MIN`).
    let annee = if annee.trim().is_empty() {
        None
    } else {
        match annee.trim().parse::<u32>() {
            Ok(a) if (config::ANNEE_MIN..=config::ANNEE_MAX).contains(&a) => Some(a),
            _ => {
                eprintln!("[neuradesk][cef] date refusée (année invalide, 4 chiffres attendus) : {annee:?}");
                return None;
            }
        }
    };
    Some(config::DatePersonnalisee {
        nom: nom.to_string(),
        jour,
        mois,
        // La case est cochée par défaut dans la page ; son absence ne
        // doit pas activer un rappel (même prudence que la relecture).
        rappel: rappel == "1",
        genre,
        annee,
    })
}

/// Remplace la date au rang `index` — **PURE**, donc testable sans CEF.
///
/// Rend `false` (et journalise) si le rang n'existe pas : la page tient
/// `editionIndex` d'un état antérieur, et un rang périmé ne doit **rien**
/// modifier plutôt que de viser la voisine. C'est la garde qui compte, parce que
/// `remove` fait glisser les rangs.
fn remplacer_date(dates: &mut [config::DatePersonnalisee], index: usize, date: config::DatePersonnalisee) -> bool {
    match dates.get_mut(index) {
        Some(place) => {
            *place = date;
            true
        }
        None => {
            eprintln!("[neuradesk][cef] modification refusée (rang inconnu) : {index}");
            false
        }
    }
}

/// Vide et applique les actions de `assets/popups/calendar_applet.html` (format
/// `"calendar/<verbe>[/<argument>…]"`).
///
/// ⚠️ **RUST REVALIDE TOUT, ET C'EST LUI QUI FAIT FOI.** La page valide déjà la
/// saisie (pour répondre tout de suite), mais une page peut se tromper ou être
/// modifiée : une date impossible est donc REFUSÉE ici, avec la même règle que
/// `config::load_dates` — et jamais « réparée », parce qu'un 31 février n'a
/// pas de correction devinable. Voir `analyser_date`, qui porte cette règle
/// pour les DEUX verbes qui écrivent une date.
pub fn dispatch_actions(dates: &mut Vec<config::DatePersonnalisee>, rappels: &mut Vec<config::RappelFete>) {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("calendar/") else {
            eprintln!("[neuradesk][cef] action calendrier inconnue : {action:?}");
            continue;
        };
        // Le décodage rend des `String` (voir `cef_app::percent_decode`) : on les
        // possède le temps de l'action, donc rien ne fuit — un prénom contenant
        // « / » ou un espace arrive encodé et redevient entier ici.
        let parts: Vec<String> = reste.split('/').map(crate::win32::cef_app::percent_decode).collect();
        match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["close"] => cef_popup::fermer_par_geste(&SLOT),
            // **LA PAGE EST PRÊTE : ON OUBLIE CE QUI A ÉTÉ POUSSÉ.** Un état
            // envoyé avant ce verbe est tombé dans le vide (la fenêtre est
            // PRÉCHARGÉE, et `execute_js` sur une page sans `window.setState` ne
            // fait rien). Sans cette purge, le cache de `push_state` — qui ne
            // repousse que si la clé change — laissait l'applet VIDE jusqu'à la
            // prochaine navigation. Même geste que `cef_aide`, `cef_calculatrice`
            // et `cef_couleurs`.
            ["ready"] => PAYLOAD.with(|c| *c.borrow_mut() = None),
            // "calendar/mois/<delta>" — LES FLÈCHES ◀ ▶ : déplace le mois affiché
            // de `delta` mois (−1 ou +1).
            ["mois", delta] => {
                let Ok(delta) = delta.parse::<i32>() else {
                    eprintln!("[neuradesk][cef] déplacement de mois refusé (delta illisible) : {delta:?}");
                    continue;
                };
                aller_au_mois(delta);
                push_state(dates, rappels);
            }
            // "calendar/aujourdhui" — LE BOUTON « aujourd'hui ».
            //
            // ⚠️ **UN VERBE À PART, ET PAS DEUX `go(...)` À LA SUITE** : deux
            // `location.href` émis dans la MÊME TÂCHE se télescopent, l'un des
            // deux étant perdu en silence (règle du dépôt, voir l'en-tête de
            // `assets/popups/calendar.html`). Il faudrait sinon `mois` *puis*
            // `jour`, et le second pourrait ne jamais arriver.
            ["aujourdhui"] => {
                revenir_a_aujourdhui();
                push_state(dates, rappels);
            }
            // "calendar/jour/<jour>" — clic sur une case de la grille du mois.
            ["jour", jour] => {
                let Ok(jour) = jour.parse::<u32>() else {
                    eprintln!("[neuradesk][cef] jour refusé (illisible) : {jour:?}");
                    continue;
                };
                choisir_jour(jour);
                push_state(dates, rappels);
            }
            ["add", genre, nom, jour, mois, rappel, annee] => {
                let Some(date) = analyser_date(genre, nom, jour, mois, rappel, annee) else {
                    continue;
                };
                dates.push(date);
                config::save_dates(dates);
                push_state(dates, rappels);
                republier_recherche(dates, rappels);
            }
            // "calendar/modifier/<index>/<genre>/<nom>/<jour>/<mois>/<0|1>/<annee>"
            // — MODIFIER une date SAISIE existante, DEMANDE EXPLICITE
            // (2026-09-23) : « je voudrais avoir la possibilité de modifier les
            // données d'un anniversaire depuis le dash calendrier ».
            //
            // ⚠️ **LE RANG EST CELUI DE `state.dates`, TOUTES FAMILLES
            // CONFONDUES** — le même que `remove` et `rappel`, jamais le rang
            // affiché dans une des deux listes. Le GENRE fait partie des
            // arguments parce que la modification peut AUSSI changer de famille :
            // un anniversaire peut devenir une fête personnalisée. La ligne ne
            // change pas de place pour autant (on remplace sur place), donc
            // l'index reste valide après coup.
            ["modifier", index, genre, nom, jour, mois, rappel, annee] => {
                // L'index est vérifié AVANT la validation de la date : un rang
                // périmé est la condition la plus alarmante (l'utilisateur a
                // édité une ligne qui a bougé), et le journal doit le dire
                // d'abord.
                let Ok(index) = index.parse::<usize>() else {
                    eprintln!("[neuradesk][cef] modification refusée (rang illisible) : {index:?}");
                    continue;
                };
                if index >= dates.len() {
                    eprintln!("[neuradesk][cef] modification refusée (rang inconnu) : {index}");
                    continue;
                }
                let Some(date) = analyser_date(genre, nom, jour, mois, rappel, annee) else {
                    continue;
                };
                if !remplacer_date(dates, index, date) {
                    continue;
                }
                // **LES MÊMES SUITES QUE `add`, ET POUR LES MÊMES RAISONS.** La
                // republication des résultats de recherche n'est pas décorative :
                // c'est elle qui manquait quand « le clic n'est pas pris en
                // compte » a été signalé, et l'index est justement ce par quoi
                // une cloche adresse une date.
                config::save_dates(dates);
                push_state(dates, rappels);
                republier_recherche(dates, rappels);
            }
            // "calendar/chercher/<texte>" — RECHERCHE dans toute la table des
            // fêtes, DEMANDE EXPLICITE (2026-09-23). **C'EST RUST QUI CHERCHE**,
            // et pas la page : la table vit ici (400 entrées embarquées), et la
            // faire descendre dans la page à chaque frame coûterait des dizaines
            // de kilo-octets par image. Un aller-retour par frappe est bien moins
            // cher — et la recherche reste ainsi TESTABLE sans CEF (voir
            // `fetes::chercher`, qui porte ses propres tests).
            ["chercher", texte] => {
                RECHERCHE.with(|r| *r.borrow_mut() = texte.to_string());
                cef_popup::execute_js(&SLOT, &format!("window.setResultats({})", resultats_json(texte, dates, rappels)));
            }
            // "calendar/rappel-fete/<mois>/<jour>/<nom>/<0|1>" — poser ou retirer
            // le rappel d'une fête DE LA TABLE, depuis un résultat de recherche
            // (DEMANDE EXPLICITE, 2026-09-23).
            //
            // ⚠️ **LE NOM FAIT PARTIE DE LA CLÉ** : 54 jours portent deux fêtes
            // (mesuré le 2026-09-24), et n'identifier que par la date poserait
            // le rappel sur les deux à la fois (voir `config::RappelFete`).
            ["rappel-fete", mois, jour, nom, actif] => {
                let (Ok(mois), Ok(jour)) = (mois.parse::<u32>(), jour.parse::<u32>()) else {
                    eprintln!("[neuradesk][cef] rappel de fête refusé (date illisible) : {parts:?}");
                    continue;
                };
                let nom = nom.trim();
                if nom.is_empty() || !(1..=12).contains(&mois) || !(1..=31).contains(&jour) {
                    eprintln!("[neuradesk][cef] rappel de fête refusé (date impossible) : {nom:?} {jour}/{mois}");
                    continue;
                }
                // On retire D'ABORD toute entrée portant ce triplet, puis on la
                // repose si on l'allume : sans ce retrait, deux clics rapides
                // laisseraient deux fois la même ligne.
                rappels.retain(|r| !(r.mois == mois && r.jour == jour && r.nom == nom));
                if *actif == "1" {
                    rappels.push(config::RappelFete { nom: nom.to_string(), jour, mois });
                }
                // ⚠️ **JOURNALISÉ, ET C'EST UN AJOUT DÉLIBÉRÉ.** Ce bras ÉCRIT
                // dans un fichier de l'utilisateur, et il ne disait RIEN quand il
                // réussissait : le 2026-09-24, `rappels_fetes.toml` est passé de
                // 13 à 24 entrées sans qu'on puisse établir si c'étaient des clics
                // réels ou des émissions parasites — le `retain` ci-dessus rend
                // les doublons impossibles, donc **il masque aussi bien un double
                // tir qu'il le prévient**. Une ligne par bascule ne noiera rien, et
                // la prochaine fois la réponse sera dans le journal.
                crate::diag!(
                    "[calendrier] rappel-fete {} — « {} » {}/{} ; {} au total",
                    if *actif == "1" { "POSÉ" } else { "RETIRÉ" },
                    nom,
                    jour,
                    mois,
                    rappels.len()
                );
                config::save_rappels_fetes(rappels);
                // La cloche doit changer D'ÉTAT À L'ÉCRAN : sans cette
                // republication, elle resterait dessinée comme avant, et le clic
                // paraîtrait ignoré alors qu'il a bien été enregistré.
                republier_recherche(dates, rappels);
            }
            ["remove", index] => {
                if let Ok(index) = index.parse::<usize>() {
                    if index < dates.len() {
                        dates.remove(index);
                        config::save_dates(dates);
                        push_state(dates, rappels);
                        // Les rangs ont GLISSÉ : sans republication, la cloche
                        // d'un résultat viserait la date suivante.
                        republier_recherche(dates, rappels);
                    }
                }
            }
            ["rappel", index, actif] => {
                if let Ok(index) = index.parse::<usize>() {
                    if let Some(a) = dates.get_mut(index) {
                        a.rappel = *actif == "1";
                        config::save_dates(dates);
                        push_state(dates, rappels);
                        // ⚠️ **MEME RAISON QUE `rappel-fete` CI-DESSUS** : c'est
                        // CETTE republication qui manquait quand le défaut a été
                        // signalé (« je n'arrive pas [...] à décocher un
                        // anniversaire »). `push_state` ne rafraîchit que les
                        // listes habituelles, JAMAIS les résultats de recherche.
                        republier_recherche(dates, rappels);
                    }
                }
            }
            _ => eprintln!("[neuradesk][cef] action calendrier inconnue : {action:?}"),
        }
    }
}

pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

/// Repositionne/redimensionne PUIS dessine — voir `win32::cef_clipboard::
/// render_if_visible`, même raison : cette fenêtre suit `app::dash_rect`, qui
/// peut changer pendant qu'elle est ouverte (curseurs d'épaisseur de barre).
pub fn render_if_visible(rect: [i32; 4], tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::resize_if_needed(&SLOT, rect);
    cef_popup::render_if_visible(&SLOT, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

/// **LEVIER DE VÉRIFICATION** — met la page en mode édition **SANS AUCUN CLIC**
/// (`NEURADESK_DEBUG_EDIT=<rang>`, lu dans `app::run`).
///
/// ⚠️ **C'EST UN APPEL DE FONCTION DIRECT, JAMAIS UNE ENTRÉE SIMULÉE** : la règle
/// du dépôt interdit d'automatiser la souris ou le clavier sur ce poste de travail
/// réel — et ce levier n'en a pas besoin, il appelle la fonction de la page.
/// Le clic du harnais, lui, n'est **délibérément pas** étendu à ce panneau : une
/// coordonnée aveugle tombant sur un `✕` supprimerait un vrai anniversaire.
///
/// ⚠️ **ET IL N'ÉCRIT RIEN** : `commencerEdition` ne fait que remplir les champs du
/// formulaire, aucun `go(...)` n'est émis, donc `dates.toml` n'est pas touché.
/// C'est ce qui rend ce levier utilisable sur les **vraies** données de
/// l'utilisateur — le geste qui enregistre reste le sien.
pub fn debug_editer(index: usize) {
    // Journalisé (visible avec `NEURADESK_DIAG=1`) : un levier qui ne fait rien
    // en silence serait exactement le piège « une absence de trace n'est pas une
    // preuve » — ici, l'absence de fenêtre est LA cause à écarter en premier.
    // Une seule fois par lancement : le levier est répété à chaque frame.
    if !EDIT_TRACE_ECRITE.with(|c| c.replace(true)) {
        crate::diag!("[calendrier] NEURADESK_DEBUG_EDIT={index} — fenêtre visible : {}", cef_popup::is_visible(&SLOT));
    }
    cef_popup::execute_js(&SLOT, &format!("window.commencerEdition({index})"));
}

/// **LEVIER DE VÉRIFICATION** de la grille — déplace le mois affiché de
/// `delta` mois (`NEURADESK_DEBUG_MOIS=<delta>`, lu dans `app::run`).
///
/// Même famille que `debug_editer` : **un appel de fonction direct, jamais une
/// entrée simulée**, et il **n'écrit rien** (`dates.toml` n'est pas touché).
///
/// ⚠️ **SANS LUI LA GRILLE N'EST PAS VÉRIFIABLE PAR CAPTURE** : le harnais ne
/// peut pas cliquer, et l'applet s'ouvre toujours sur aujourd'hui — une capture
/// ne montrerait donc jamais qu'un seul mois. Il passe par `aller_au_mois`, le
/// **même chemin** que la flèche de la page.
/// **LEVIER DE VÉRIFICATION** du choix de jour (`NEURADESK_DEBUG_JOUR=<jour>`) —
/// même famille, mêmes garanties : appel direct, aucune entrée simulée, et il
/// n'écrit rien.
///
/// ⚠️ **IL NE SERT QU'APRÈS `NEURADESK_DEBUG_MOIS`** : il choisit un jour **du
/// mois affiché**, comme le fait un clic. Sans le mois, on ne pourrait vérifier
/// que la sélection du mois courant — c'est-à-dire jamais le cas intéressant
/// (les fêtes d'un autre jour que le jour du jour).
pub fn debug_jour(jour: u32) {
    if JOUR_APPLIQUE.with(|c| c.replace(true)) || !cef_popup::is_visible(&SLOT) {
        return;
    }
    crate::diag!("[calendrier] NEURADESK_DEBUG_JOUR={jour} — fenêtre visible : {}", cef_popup::is_visible(&SLOT));
    choisir_jour(jour);
}

/// **LEVIER DE VÉRIFICATION** — fait défiler le panneau de `y` pixels
/// (`NEURADESK_DEBUG_DEFILER=<y>`, lu dans `app::run`).
///
/// ⚠️ **SANS LUI, TOUT CE QUI EST SOUS LA LIGNE DE FLOTTAISON N'EST PAS
/// VÉRIFIABLE.** Le panneau remplit la zone libre du dock (~700 px de haut) et
/// son contenu est plus long : la grille du mois, les fêtes du jour, puis les
/// deux colonnes. Une capture ne montre que le haut — c'est ce qui a rendu la
/// liste FÊTES invisible alors qu'elle était bien construite.
/// (La mémoire note déjà que `NEURADESK_DEBUG_SCROLL` n'a jamais fonctionné sur
/// `settings.html` ; celui-ci est un appel direct à `window.scrollTo`, sans
/// aucune entrée simulée — même famille que `debug_editer`.)
pub fn debug_defiler(y: f64) {
    if DEFILE.with(|c| c.get()) || !cef_popup::is_visible(&SLOT) {
        return;
    }
    DEFILE.with(|c| c.set(true));
    crate::diag!("[calendrier] NEURADESK_DEBUG_DEFILER={y} — fenêtre visible : {}", cef_popup::is_visible(&SLOT));
    cef_popup::execute_js(&SLOT, &format!("window.scrollTo(0, {y})"));
}

pub fn debug_mois(delta: i32) {
    // ⚠️ **IL ACCUMULE, DONC IL NE S'APPLIQUE QU'UNE FOIS** — c'est ce qui le
    // sépare de `debug_editer`, qui est idempotent et pouvait donc se répéter à
    // chaque frame. Appelé soixante fois par seconde, il ferait défiler les mois
    // à toute vitesse.
    // Et il attend que la fenêtre soit VISIBLE : `toggle` remet la vue sur
    // aujourd'hui, donc un levier appliqué avant l'ouverture serait effacé.
    if MOIS_APPLIQUE.with(|c| c.replace(true)) || !cef_popup::is_visible(&SLOT) {
        return;
    }
    crate::diag!("[calendrier] NEURADESK_DEBUG_MOIS={delta} — fenêtre visible : {}", cef_popup::is_visible(&SLOT));
    aller_au_mois(delta);
}

/// Voir `win32::cef_popup::debug_dump_backbuffer` — réservé au harnais de
/// vérification (`NEURADESK_DEBUG_OPEN=calendar-applet`).
///
/// **SANS CETTE FONCTION, LA FENÊTRE N'EST PAS CAPTURABLE**, et c'est un piège
/// payé une fois : le harnais a DEUX listes à tenir — celle qui OUVRE (voir
/// `app::run`, `debug_capture.open`) et celle qui CAPTURE (le second `match`
/// des `debug_dump_backbuffer`, plus bas dans la même boucle). N'étendre que la
/// première donne un run qui réussit sans produire le moindre fichier, et
/// **aucun message nulle part** : l'avertissement d'un nom inconnu part par
/// `eprintln!`, qui ne va nulle part dans une application lancée détachée.
pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(result) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    //! Ce module teste les **morceaux purs** de `dispatch_actions`, extraits
    //! précisément pour être testables ici : le harnais de capture ne sait ni
    //! cliquer dans ce panneau ni taper au clavier, donc rien de ce qui suit
    //! n'est vérifiable autrement.

    use super::*;

    fn date(nom: &str, jour: u32, mois: u32, annee: Option<u32>) -> config::DatePersonnalisee {
        config::DatePersonnalisee {
            nom: nom.to_string(),
            jour,
            mois,
            rappel: true,
            genre: config::Genre::Anniversaire,
            annee,
        }
    }

    #[test]
    fn une_date_valide_passe_telle_quelle() {
        let d = analyser_date("anniversaire", "Léa", "12", "3", "1", "1990").expect("acceptée");
        assert_eq!(d.nom, "Léa");
        assert_eq!((d.jour, d.mois), (12, 3));
        assert_eq!(d.annee, Some(1990));
        assert!(d.rappel);
        assert_eq!(d.genre, config::Genre::Anniversaire);
    }

    #[test]
    fn l_annee_est_facultative_et_le_rappel_suit_la_case() {
        let d = analyser_date("fete", "Retour de mission", "14", "7", "0", "").expect("acceptée");
        assert_eq!(d.annee, None, "vide = pas d'année");
        assert!(!d.rappel, "« 0 » éteint le rappel");
        assert_eq!(d.genre, config::Genre::Fete);
    }

    #[test]
    fn le_nom_est_rogne_de_ses_espaces() {
        let d = analyser_date("anniversaire", "  Manon  ", "15", "8", "1", "").expect("acceptée");
        assert_eq!(d.nom, "Manon");
    }

    #[test]
    fn un_nom_vide_ou_en_espaces_est_refuse() {
        assert!(analyser_date("anniversaire", "", "1", "1", "1", "").is_none());
        assert!(analyser_date("anniversaire", "   ", "1", "1", "1", "").is_none());
    }

    #[test]
    fn un_genre_inconnu_est_refuse_pas_devine() {
        assert!(analyser_date("", "Léa", "12", "3", "1", "").is_none());
        assert!(analyser_date("fete-perso", "Léa", "12", "3", "1", "").is_none());
    }

    #[test]
    fn un_jour_ou_un_mois_hors_bornes_est_refuse() {
        for (jour, mois) in [("0", "1"), ("32", "1"), ("1", "0"), ("1", "13")] {
            assert!(
                analyser_date("anniversaire", "Léa", jour, mois, "1", "").is_none(),
                "{jour}/{mois} aurait dû être refusé"
            );
        }
    }

    #[test]
    fn une_date_ou_une_annee_illisible_est_refusee() {
        assert!(analyser_date("anniversaire", "Léa", "douze", "3", "1", "").is_none());
        assert!(analyser_date("anniversaire", "Léa", "12", "mars", "1", "").is_none());
        // Trois chiffres, deux chiffres, cinq chiffres : aucun n'est une année
        // sur quatre chiffres.
        assert!(analyser_date("anniversaire", "Léa", "12", "3", "1", "990").is_none());
        assert!(analyser_date("anniversaire", "Léa", "12", "3", "1", "99").is_none());
        assert!(analyser_date("anniversaire", "Léa", "12", "3", "1", "10000").is_none());
    }

    #[test]
    fn les_bornes_de_l_annee_sont_incluses() {
        // Les deux bornes SONT acceptées : un test qui ne les toucherait pas
        // laisserait passer un `..` là où le code doit porter un `..=`.
        let min = config::ANNEE_MIN.to_string();
        let max = config::ANNEE_MAX.to_string();
        assert!(analyser_date("anniversaire", "Léa", "12", "3", "1", &min).is_some());
        assert!(analyser_date("anniversaire", "Léa", "12", "3", "1", &max).is_some());
    }

    #[test]
    fn un_trente_et_un_fevrier_passe_la_borne_large() {
        // ⚠️ CE TEST FIGE UNE LIMITE ASSUMÉE, il ne la recommande pas : la borne
        // est LARGE (le mois le plus long), le jour exact étant confronté à la
        // longueur du mois par `fetes`, seul à connaître les bissextiles. Le
        // figer ici fait qu'un resserrement futur sera un choix conscient et non
        // un effet de bord silencieux.
        assert!(analyser_date("anniversaire", "Léa", "31", "2", "1", "").is_some());
    }

    #[test]
    fn remplacer_ne_touche_que_le_rang_vise() {
        let mut dates =
            vec![date("Léa", 12, 3, None), date("Manon", 15, 8, None), date("Yvon", 19, 5, None)];
        assert!(remplacer_date(&mut dates, 1, date("Manon", 16, 8, Some(1985))));
        assert_eq!(dates[0].nom, "Léa");
        assert_eq!(dates[1].jour, 16, "le rang visé a bien changé");
        assert_eq!(dates[1].annee, Some(1985));
        assert_eq!(dates[2].nom, "Yvon", "les voisines sont intactes");
    }

    #[test]
    fn un_rang_hors_bornes_ne_modifie_rien() {
        let mut dates = vec![date("Léa", 12, 3, None)];
        assert!(!remplacer_date(&mut dates, 1, date("Manon", 15, 8, None)), "un rang périmé est refusé");
        assert!(!remplacer_date(&mut dates, usize::MAX, date("Manon", 15, 8, None)));
        assert_eq!(dates.len(), 1);
        assert_eq!(dates[0].nom, "Léa", "la liste est intacte");
    }

    /// Les verbes que `dispatch_actions` accepte, lus dans CE fichier.
    fn verbes_du_dispatch() -> Vec<String> {
        // ⚠️ **BORNÉ À AVANT `#[cfg(test)]`.** Ce module de tests vit dans le
        // MÊME fichier : sans cette borne, une ligne de test commençant par `["`
        // (une liste d'actions attendue, écrite verticalement) serait comptée
        // comme un bras, et le test finirait par se comparer à lui-même.
        let source = include_str!("cef_calendar_applet.rs");
        source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(source)
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//"))
            .filter_map(|l| {
                let reste = l.strip_prefix("[\"")?;
                let (verbe, fin) = reste.split_once('"')?;
                // Un bras du `match` finit par `]` (arité 1) ou `,` (au-delà) :
                // ce garde écarte une ligne qui ne serait pas un bras.
                matches!(fin.chars().next(), Some(']') | Some(',')).then(|| verbe.to_string())
            })
            .collect()
    }

    /// **L'APPLET S'OUVRAIT VIDE DANS LES PREMIÈRES SECONDES** (constaté le
    /// 2026-11-04, à la capture : titres présents, listes vides). La fenêtre est
    /// PRÉCHARGÉE ; `push_state` mémorise l'état qu'il a poussé et ne repousse
    /// que si la clé change — un état poussé avant que le script de la page ne
    /// tourne était donc perdu DÉFINITIVEMENT. Le remède est le contrat des
    /// panneaux frères : la page dit `ready`, le dispatch oublie ce qu'il a
    /// poussé, la frame suivante repart.
    ///
    /// ⚠️ Le miroir des verbes ne suffit pas à garder ceci : il compare des
    /// ENSEMBLES, donc retirer `ready` DES DEUX CÔTÉS le laisserait vert.
    #[test]
    fn la_page_annonce_qu_elle_est_prete_et_l_etat_est_alors_oublie() {
        let page = include_str!("../../assets/popups/calendar_applet.html");
        let rust = include_str!("cef_calendar_applet.rs").split("#[cfg(test)]").next().unwrap_or_default();
        assert!(page.contains("go(\"ready\")"), "la page n'annonce pas qu'elle est prête");
        assert!(rust.contains("[\"ready\"] => PAYLOAD"), "le dispatch ne vide pas le cache d'état sur `ready`");
    }

    /// Les verbes que la PAGE émet, lus dans son source.
    fn verbes_de_la_page() -> Vec<String> {
        const SOURCE: &str = include_str!("../../assets/popups/calendar_applet.html");
        let mut verbes = Vec::new();
        for (i, _) in SOURCE.match_indices("go(") {
            let reste = &SOURCE[i + 3..];
            // Les DEUX guillemets sont possibles : le bouton Fermer écrit
            // `go('close')`, parce qu'il vit dans un attribut `onclick="…"`.
            let Some(guillemet) = reste.chars().next().filter(|c| *c == '"' || *c == '\'') else {
                continue;
            };
            if let Some(verbe) = reste[1..].split(guillemet).next() {
                verbes.push(verbe.to_string());
            }
        }
        verbes
    }

    /// ⚠️ **LE CONTRÔLE QUI A DÉJÀ RATTRAPÉ CE FICHIER UNE FOIS.** Un bras du
    /// `match` de `dispatch_actions` a été **supprimé en silence** (le bras
    /// `rappel-fete`, emporté par un découpage par INDEX), et c'est en
    /// **recomptant les verbes acceptés** puis en les comparant à ceux que la
    /// page émet qu'on l'a vu. Ce test encode ce contrôle, pour qu'il ne dépende
    /// plus de la mémoire de quelqu'un.
    ///
    /// ⚠️ **ET CE QU'IL NE GARDE PAS : L'ARITÉ.** Il compare des ENSEMBLES de
    /// verbes, donc une action émise avec un argument de trop ou de moins
    /// passerait ici — elle tomberait dans le `_ =>` du `match`, dont
    /// l'`eprintln!` ne va **nulle part** dans une application lancée détachée.
    /// C'est pourquoi la forme COMPLÈTE de chaque action est écrite en tête des
    /// DEUX fichiers (et corrigée quand elle dérive — la ligne `add` de la page
    /// omettait `/<annee>` depuis que l'année existe), et pourquoi `add` et
    /// `modifier` passent tous deux par `analyser_date` : l'arité reste la seule
    /// chose que ce test ne peut pas voir.
    #[test]
    fn la_page_et_le_dispatch_acceptent_les_memes_verbes() {
        let mut page = verbes_de_la_page();
        let mut rust = verbes_du_dispatch();
        page.sort();
        page.dedup();
        rust.sort();
        rust.dedup();
        assert_eq!(
            page, rust,
            "un verbe émis par la page que Rust n'accepte pas, ou l'inverse — c'est ainsi \
             qu'un bras supprimé en silence se voit"
        );
    }

    // --- LA GRILLE DU MOIS (2026-09-24) ---

    /// Le décalage franchit les années **dans les deux sens**, y compris pour un
    /// delta de plus d'un an — le cas qu'une arithmétique naïve sur le mois seul
    /// laisserait partir hors bornes.
    #[test]
    fn le_decalage_de_mois_traverse_les_annees_dans_les_deux_sens() {
        assert_eq!(decaler_mois(2026, 1, -1), (2025, 12));
        assert_eq!(decaler_mois(2026, 12, 1), (2027, 1));
        assert_eq!(decaler_mois(2026, 9, 0), (2026, 9));
        assert_eq!(decaler_mois(2026, 3, -14), (2025, 1));
        assert_eq!(decaler_mois(2026, 1, 12), (2027, 1));
        assert_eq!(decaler_mois(2026, 1, 25), (2028, 2));
    }

    /// Changer de mois **repose la sélection** : aujourd'hui s'il est dans le
    /// mois d'arrivée, sinon le 1er — qui existe toujours.
    #[test]
    fn changer_de_mois_repose_la_selection() {
        let auj = Date { annee: 2026, mois: 9, jour: 24 };
        assert_eq!(choix_du_jour(2026, 9, auj), 24, "le mois d'aujourd'hui garde son jour");
        assert_eq!(choix_du_jour(2026, 8, auj), 1, "un autre mois prend le 1er");
        assert_eq!(choix_du_jour(2026, 2, auj), 1);
        // ⚠️ **MÊME MOIS, AUTRE ANNÉE** : sans la comparaison d'année, le
        // 24 septembre 2025 hériterait du 24 — un jour juste dans un mois juste,
        // donc un défaut qui ne se verrait pas.
        assert_eq!(choix_du_jour(2025, 9, auj), 1);
    }

    /// Un jour hors bornes ne change RIEN — même « borne large » que
    /// `analyser_date` (1..=31), mais jamais silencieusement accepté.
    #[test]
    fn un_jour_hors_bornes_est_refuse_sans_rien_changer() {
        VUE.with(|v| v.set((2026, 9, 24)));
        for mauvais in [0u32, 32, 99] {
            choisir_jour(mauvais);
            assert_eq!(
                VUE.with(|v| v.get()),
                (2026, 9, 24),
                "un jour {mauvais} ne doit rien changer"
            );
        }
        choisir_jour(21);
        assert_eq!(VUE.with(|v| v.get()), (2026, 9, 21), "un jour valide, lui, est retenu");
    }

    /// `aller_au_mois` déplace bien le mois, et laisse un jour **toujours
    /// valide** — on n'assert pas sa valeur exacte, qui dépend de la date de la
    /// machine (aujourd'hui si c'est le mois courant, sinon le 1er).
    #[test]
    fn aller_au_mois_deplace_le_mois_et_garde_un_jour_valide() {
        VUE.with(|v| v.set((2026, 1, 15)));
        aller_au_mois(-1);
        let (annee, mois, jour) = VUE.with(|v| v.get());
        assert_eq!((annee, mois), (2025, 12), "janvier moins un mois, c'est décembre précédent");
        assert!((1..=31).contains(&jour), "jour hors bornes : {jour}");

        VUE.with(|v| v.set((2026, 12, 31)));
        aller_au_mois(1);
        assert_eq!(VUE.with(|v| v.get()).0, 2027, "décembre plus un mois passe à l'année suivante");
    }
}
