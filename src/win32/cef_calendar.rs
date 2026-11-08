//! Fenêtre CEF pour le mini calendrier (clic sur l'horloge) — Phase 1 de
//! `docs/plans/cef-web-ui-migration.md`. Remplace `PopupKind::Calendar`
//! (`win32::popup_menu::render_calendar`) pour cette icône précise.
//!
//! **LA GRILLE DU MOIS est calculée côté JavaScript** (`Date()`) — mais **LE
//! PANNEAU DES ÉVÈNEMENTS, À SA DROITE, VIENT DE RUST** : fêtes de la table,
//! jours fériés (fixes ET mobiles calculés) et dates saisies par l'utilisateur.
//! C'est `push_state` qui les pousse, sur le modèle de `win32::cef_network`.
//!
//! ⚠️ **LA DOC DE CE MODULE A LONGTEMPS DIT « AUCUNE IPC », ET C'ÉTAIT FAUX.**
//! La page charge `assets/popups/menu.js`, qui **émet** déjà
//! `neuradesk://menu/size/<w>/<h>` (c'est ainsi que la fenêtre prend sa taille) —
//! et `toggle` fait déjà du Rust→JS par `window.__ndRefresh`. Le canal existait
//! donc dans les deux sens ; il n'y avait rien à créer, seulement à s'en servir.
//!
//! ⚠️ **ET LA PAGE N'ÉMET RIEN D'AUTRE, DÉLIBÉRÉMENT.** `menu.js` mesure la boîte
//! par un `MutationObserver`, donc **chaque** changement du contenu déclenche une
//! mesure, donc un `location.href` — et deux `location.href` émis dans la même
//! tâche se TÉLESCOPENT, l'un des deux étant perdu **en silence** (voir
//! `cef_popup::parse_content_size`). Cliquer un jour change la hauteur du panneau :
//! si la page demandait quoi que ce soit à ce moment-là, l'une des deux navigations
//! disparaîtrait. **Toute la donnée lui est donc poussée d'avance** (`push_state`,
//! qui envoie le mois entier), et la sélection d'un jour est purement locale.
//!
//! Toute la mécanique fenêtre/rendu/relais souris est dans
//! `win32::cef_popup` — voir `win32::cef_test_window` pour la doc de
//! module complète du pattern suivi ici.

use std::cell::{Cell, RefCell};
use std::path::Path;

use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::config;
use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::fetes;
use crate::win32::menu;

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    /// Voir `app::content_scale_pour`, doc de fonction — mémorisé ici à
    /// l'ouverture (voir `toggle`) et relu à chaque frame par
    /// `render_if_visible`.
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// **LA MESURE FORCÉE À REDEMANDER, une fois la fenêtre VISIBLE.**
    ///
    /// ⚠️ **C'EST LE RATTRAPAGE D'UN DÉFAUT RÉEL, ET IL EST MESURABLE.** À
    /// l'ouverture, `cef_popup` demande à la page de se mesurer
    /// (`request_content_measure`) — mais **pendant le délai où la fenêtre n'est
    /// pas encore montrée** (`pending_show`, 33 ms), et `drain_actions` **retire
    /// l'action de la file AVANT** de tester `is_visible` : la mesure est donc
    /// **consommée et jetée**. Or `menu.js` a déjà posé son `lastSent`, donc la
    /// page ne la réémettra jamais spontanément — et ce menu est justement la page
    /// qui ne mute JAMAIS toute seule.
    ///
    /// Conséquence observée avant ce correctif : **la taille négociée n'était
    /// jamais appliquée**, la fenêtre restait au repli — c'est ce que raconte le
    /// commentaire d'`app.rs` sur le contenu qui « s'arrêtait vers 53 % de la
    /// hauteur allouée », et qui avait été contourné en baissant le repli de 1440
    /// à 900 plutôt qu'en corrigeant la mesure.
    ///
    /// Armé par `toggle`, consommé par `poll` **quand `is_visible` est vrai**.
    static REMESURER: Cell<bool> = const { Cell::new(false) };
    /// L'expression JS complète du dernier état poussé, avec la CLÉ qui l'a
    /// produite — voir `push_state`, qui porte le pourquoi de la clé.
    static PAYLOAD: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    /// Une seule application du levier `NEURADESK_DEBUG_CEF_MOIS` — voir
    /// `debug_mois` : il **accumule**, comme celui de l'applet.
    static MOIS_APPLIQUE: Cell<bool> = const { Cell::new(false) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefCalendarWindow"),
    window_title: w!("NeuraDesk — Calendrier"),
    html_file: "calendar.html",
    wndproc: Some(wndproc),
    above_a_magnified_dock: true,
    // MENU — apparaît et disparaît en fondant, voir `PopupSpec::fades`.
    fades: true,
    fenetre: false,
};

/// Précharge ce popup au démarrage — fenêtre et navigateur naissent CACHÉS, pour
/// que la page ait fini de charger avant la première ouverture réelle. Voir
/// `win32::cef_popup::preload`, doc de fonction, qui porte le pourquoi complet.
pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// Voir `win32::cef_popup::toggle`, doc de fonction — même bascule réelle
/// (masque si déjà visible, montre + repositionne sinon), même anti-rebond.
///
/// `dates` : les dates saisies de l'utilisateur, poussées dans la page avec le
/// reste (voir `push_state`).
pub fn toggle(
    rect: [i32; 4],
    gfx: &GfxDevice,
    content_scale: f32,
    dates: &[config::DatePersonnalisee],
    rappels: &[config::RappelFete],
    notes: &[crate::win32::notes::Note],
) {
    CONTENT_SCALE.with(|c| c.set(content_scale));
    // Voir `REMESURER` : la mesure d'ouverture part trop tôt pour être appliquée,
    // on la redemandera dès que la fenêtre sera visible (voir `poll`).
    REMESURER.with(|c| c.set(true));
    // `on_show` — **LA CONTREPARTIE OBLIGATOIRE DU PRÉCHARGEMENT POUR CE POPUP** :
    // sa page calcule son mois AU CHARGEMENT, et une page préchargée au démarrage
    // de l'application afficherait donc le mois du démarrage, indéfiniment. On le
    // lui fait recalculer à chaque ouverture (voir `assets/popups/calendar.html`,
    // `window.__ndRefresh`).
    //
    // ⚠️ **ET LE RECALCUL DE LA SÉLECTION EST DU MÊME PROBLÈME** : la page est
    // préchargée, donc un jour « sélectionné » au chargement du script resterait
    // celui du démarrage — en octobre, cliquer le 12 interrogerait le 12
    // SEPTEMBRE, et le panneau dirait « aucun évènement » pour tout le mois, en
    // silence. C'est `render()` qui repose la sélection sur le mois courant (voir
    // sa doc) : `__ndRefresh` est donc ce qui la corrige aussi.
    //
    // La garde `window.__ndRefresh &&` couvre le cas d'un popup NON préchargé : la
    // page vient d'être créée, son script n'a pas encore tourné, et c'est son
    // propre `render()` de fin de script qui fera le travail.
    //
    // ⚠️ **LE CURSEUR DE MOIS EST DU MÊME PROBLÈME, ET EN PIRE** : il vit dans la
    // PAGE (les flèches naviguent localement, voir `push_state`), donc il
    // survivrait à la fermeture et le menu se rouvrirait sur un mois lointain.
    // Demande explicite : **à la réouverture, le mois courant**. D'où
    // `__ndResetMois` AVANT `__ndRefresh` — l'ordre compte, le second relit ce que
    // le premier vient de poser.
    // ⚠️ **LA CARTE DU BANDEAU CÈDE DEVANT CE PANNEAU, QUI LA RÉSUME.** C'est le
    // geste que font les douze panneaux voisins à l'ouverture (voir
    // `cef_notes::toggle`) ; sans lui, la carte resterait par-dessus le panneau
    // qui vient justement de reprendre son contenu.
    crate::win32::bandeau::hide_if_visible();
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, || {
        cef_popup::execute_js(
            &SLOT,
            "window.__ndResetMois && window.__ndResetMois(); window.__ndRefresh && window.__ndRefresh()",
        );
    });
    push_state(dates, rappels, notes);
}

/// **À APPELER À CHAQUE FRAME** (voir `app::run`, à côté des autres `poll`).
///
/// Deux raisons, et la première est un correctif :
/// 1. **redemander la mesure** une fois la fenêtre visible (voir `REMESURER`) —
///    sans quoi la taille négociée n'est jamais appliquée ;
/// 2. **repousser l'état**, sur le modèle de `cef_network::poll` : la poussée de
///    `toggle` peut partir avant que la page n'ait défini `window.setState`, et
///    c'est le remède établi du défaut « il faut ouvrir deux fois les menus pour
///    qu'ils soient remplis ».
pub fn poll(dates: &[config::DatePersonnalisee], rappels: &[config::RappelFete], notes: &[crate::win32::notes::Note]) {
    if !cef_popup::is_visible(&SLOT) {
        return;
    }
    if REMESURER.with(|c| c.replace(false)) {
        cef_popup::request_content_measure(&SLOT);
    }
    push_state(dates, rappels, notes);
}

/// Pousse dans la page les évènements du mois affiché.
///
/// ⚠️ **LE MOIS ENTIER, ET PAS SEULEMENT AUJOURD'HUI** : la page doit pouvoir
/// répondre à un clic sur n'importe quelle cellule **sans rien demander** (voir la
/// doc de module : une requête à ce moment-là télescoperait la mesure de taille).
///
/// ⚠️ **LA CLÉ DU CACHE CONTIENT LE JOUR *ET* LES DATES SAISIES.** Le jour seul ne
/// suffirait pas : l'utilisateur peut ajouter ou modifier une date dans l'applet
/// Calendrier **pendant que ce menu est ouvert**, et le panneau montrerait alors
/// une liste périmée jusqu'à la réouverture. Même motif que
/// `fetes::empreinte_des_rappels`, qui a exactement le même problème à résoudre.
///
/// ⚠️ **ON MET EN CACHE L'EXPRESSION JS ENTIÈRE**, pas le JSON : c'est ce qui rend
/// la poussée par frame presque gratuite (une comparaison de clé, puis une copie
/// de chaîne). Reconstruire le JSON à chaque frame coûterait un balayage des 31
/// jours du mois, chacun appelant `fetes_du_jour`.
/// **COMBIEN DE MOIS SONT POUSSÉS DE CHAQUE CÔTÉ DU MOIS COURANT** — 12 donne
/// « deux ans autour d'aujourd'hui » (décision utilisateur), soit 25 mois.
///
/// ⚠️ **C'EST LA CONSÉQUENCE DIRECTE DE LA RÈGLE « LA PAGE N'ÉMET RIEN »** : les
/// flèches ◀ ▶ sont purement locales, donc elles ne peuvent atteindre que ce qui
/// a été poussé d'avance. Élargir cette constante est le SEUL moyen d'élargir la
/// navigation — demander le mois à Rust rouvrirait le télescopage que toute la
/// page est construite pour éviter.
const MOIS_DE_CHAQUE_COTE: i32 = 12;

fn push_state(
    dates: &[config::DatePersonnalisee],
    rappels: &[config::RappelFete],
    notes: &[crate::win32::notes::Note],
) {
    // La MÊME arithmétique que la grille de l'applet — une seule copie, dans
    // `cef_calendar_applet::decaler_mois`, pour que les deux ne puissent pas
    // diverger d'un mois sans qu'on le voie.
    use crate::win32::cef_calendar_applet::decaler_mois;
    let date = crate::win32::cef_calendar_applet::aujourdhui();
    let mut cle = format!("{} {} {}", date.annee, date.mois, date.jour);
    for d in dates {
        // ⚠️ **`rappel` EST DANS LA CLÉ, PAS SEULEMENT `dates`** — il a fallu
        // l'y ajouter le 2026-09-24 : la cloche de l'applet change ce drapeau
        // SANS toucher à `dates`, donc sans cette empreinte, allumer une alarme
        // sur une fête saisie ne recoloriait rien ici avant le lendemain.
        cle.push_str(&format!("|{} {} {} {:?} {:?} {}", d.nom, d.jour, d.mois, d.genre, d.annee, d.rappel));
    }
    // Même raison, pour les fêtes de la TABLE : c'est `rappels_fetes.toml` qui
    // porte LEUR alarme, et il se modifie depuis l'applet.
    for r in rappels {
        cle.push_str(&format!("|r{} {} {}", r.nom, r.jour, r.mois));
    }
    // ⚠️ **ET LES ALARMES DE NOTES, SINON ELLES N'APPARAISSENT JAMAIS.** Le
    // panneau les montre désormais (voir `fetes::alarmes_de_notes_du_mois`), et
    // elles se posent depuis le dash Notes : sans cette empreinte, une alarme
    // fraîchement posée ne serait pas poussée avant le changement de jour. C'est
    // le même piège que `rappel` juste au-dessus, et celui déjà payé deux fois
    // sur ce module.
    for n in notes {
        if let Some(alarme) = &n.alarme {
            cle.push_str(&format!("|n{} {:?} {:?} {}", n.id, alarme.date, alarme.heure, alarme.jours));
        }
    }
    // ⚠️ **LES NOTIFICATIONS AUSSI, SINON LA SECTION NE SE REMPLIT JAMAIS.**
    // Même piège que `rappel` et les alarmes de notes ci-dessus, et pour la même
    // raison : elles vivent AILLEURS (`win32::bandeau`) et changent sans que rien
    // d'autre ici ne bouge — sans cette empreinte, un message qui arrive pendant
    // que le panneau est ouvert ne s'y afficherait pas. Le compte précède les
    // contenus : deux messages identiques restent deux messages.
    //
    // ⚠️ **L'IDENTIFIANT EST DANS LA CLÉ, PAS SEULEMENT DANS LE CONTENU.** Deux
    // messages de contenus identiques ne diffèrent QUE par lui : les omettre
    // ferait considérer un état comme inchangé alors que les identités ont bougé
    // — la croix d'une ligne enverrait alors celle d'un message disparu. C'est la
    // forme exacte du piège déjà payé deux fois sur ce module.
    let notifications = crate::win32::bandeau::messages();
    // ⚠️ **LA MÉTÉO AUSSI, PAR SA VERSION** (voir `win32::meteo_horloge`) : elle
    // arrive d'un fil, sans que rien d'autre ici ne bouge — même piège que les
    // notifications juste en dessous.
    cle.push_str(&format!("|w{}", crate::win32::meteo_horloge::version()));
    // Les horloges du monde : même piège, elles changent depuis les Réglages.
    cle.push_str(&format!("|h{}", crate::win32::horloges_monde::version()));
    cle.push_str(&format!("|m{}", notifications.len()));
    for m in &notifications {
        cle.push_str(&format!("|{} {} {} {}", m.id, m.titre, m.texte, m.quand));
    }
    // Emprunt COURT, refermé avant tout appel — même précaution que partout
    // ailleurs dans le dépôt : un `RefCell` ne doit jamais rester emprunté pendant
    // qu'on appelle du code qui pourrait le reprendre.
    let cache = PAYLOAD.with(|p| p.borrow().as_ref().filter(|(k, _)| *k == cle).map(|(_, js)| js.clone()));
    let js = match cache {
        Some(js) => js,
        None => {
            // ⚠️ **LE MOIS AFFICHÉ N'EST PLUS CELUI D'AUJOURD'HUI** : les flèches
            // de la page naviguent localement, donc il faut lui donner d'avance
            // TOUS les mois qu'elle peut atteindre. Le cache ci-dessus rend ce
            // balayage payé une fois, pas une fois par frame.
            let mut mois = serde_json::Map::new();
            let mut total_jours = 0usize;
            for delta in -MOIS_DE_CHAQUE_COTE..=MOIS_DE_CHAQUE_COTE {
                let (annee, m) = decaler_mois(date.annee, date.mois, delta);
                // ⚠️ **LES NOTES SE MÉLANGENT AUX FÊTES, ET PEUVENT CRÉER UN
                // JOUR.** `evenements_du_mois` ne rend que les jours NON VIDES ;
                // une alarme posée un jour sans fête doit donc ajouter sa propre
                // entrée, sinon elle n'aurait nulle part où s'afficher.
                let mut par_jour: std::collections::BTreeMap<u32, Vec<fetes::Evenement>> =
                    fetes::evenements_du_mois(annee, m, dates, rappels).into_iter().collect();
                for (jour, ev) in fetes::alarmes_de_notes_du_mois(annee, m, notes) {
                    par_jour.entry(jour).or_default().push(ev);
                }
                let jours: Vec<serde_json::Value> = par_jour
                    .into_iter()
                    .map(|(jour, evs)| {
                        json!({
                            "jour": jour,
                            "evenements": evs
                                .into_iter()
                                // `rappel` dit à la page de faire RESSORTIR les
                                // lettres (voir `calendar.html`) — il vient de la
                                // date saisie elle-même, donc la clé de cache
                                // ci-dessus le couvre déjà via `dates`.
                                .map(|e| json!({ "nom": e.nom, "categorie": e.categorie.cle(), "annee": e.annee, "rappel": e.rappel }))
                                .collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                total_jours += jours.len();
                // La clé « annee-mois » est celle que la page reconstruit pour
                // retrouver un mois — voir son `cleDuMois`.
                mois.insert(format!("{annee}-{m}"), json!({ "annee": annee, "mois": m, "jours": jours }));
            }
            // Sonde de vérification (voir `win32::debug_capture`) : elle dit ce qui
            // a été TROUVÉ, ce qu'aucune capture ne peut montrer d'un coup d'œil —
            // c'est elle qui a révélé qu'une date saisie manquait à l'appel.
            let (amin, mmin) = decaler_mois(date.annee, date.mois, -MOIS_DE_CHAQUE_COTE);
            let (amax, mmax) = decaler_mois(date.annee, date.mois, MOIS_DE_CHAQUE_COTE);
            crate::diag!(
                "[calendrier] fenetre {}/{} .. {}/{} ({} mois) : {} jour(s) avec evenement au total, \
                 {} date(s) saisie(s) ; le {} : {:?}",
                mmin,
                amin,
                mmax,
                amax,
                MOIS_DE_CHAQUE_COTE * 2 + 1,
                total_jours,
                dates.len(),
                date.jour,
                mois.get(&format!("{}-{}", date.annee, date.mois))
                    .and_then(|m| m["jours"].as_array())
                    .and_then(|js| js.iter().find(|j| j["jour"] == json!(date.jour)))
                    .map(|j| j["evenements"].clone())
            );
            let payload = json!({
                // L'année COURANTE part avec le reste : c'est elle qui permet à la
                // page de calculer l'âge sans lire l'horloge de son côté.
                "aujourdhui": { "jour": date.jour, "mois": date.mois, "annee": date.annee },
                // Les bornes de la fenêtre, pour que les flèches sachent se
                // désactiver — sans quoi elles promettraient des mois que la page
                // n'a pas.
                "bornes": { "min": { "annee": amin, "mois": mmin }, "max": { "annee": amax, "mois": mmax } },
                // Seulement les jours qui ont quelque chose : un mois ordinaire en
                // a une quinzaine, et la page n'a rien à faire de 31 tableaux vides.
                "mois": mois,
                // **LES NOTIFICATIONS DE NeuraDesk** — la trace que l'utilisateur
                // veut lire ici (DEMANDE EXPLICITE : « les notifications dans le
                // menu au clic gauche sur l'horloge »). Du plus ANCIEN au plus
                // récent, comme `MESSAGES` ; c'est la page qui les inverse (voir
                // sa section « Notifications », qui reprend la mise en page de
                // `bandeau.html` — les deux pages sont autonomes, d'où la
                // recopie plutôt qu'un import).
                "notifications": notifications
                    .iter()
                    // ⚠️ **L'IDENTIFIANT PART AVEC LE RESTE** : c'est lui que la
                    // croix de la page renvoie (voir `bandeau::Message::id`). Un
                    // rang ne conviendrait pas — l'historique change sous les
                    // doigts, et deux messages identiques sont légitimes.
                    .map(|m| json!({ "id": m.id, "titre": m.titre, "texte": m.texte, "quand": m.quand }))
                    .collect::<Vec<_>>(),
                // ⭐ **LA MÉTÉO SOUS LE CALENDRIER** — DEMANDE EXPLICITE
                // (2026-10-14). `null` sans ville réglée : la page ne montre rien.
                "meteo": crate::win32::meteo_horloge::json(),
                // ⭐ **LES HORLOGES DU MONDE** (voir `win32::horloges_monde`) — la
                // page calcule les heures, Rust ne donne que les fuseaux.
                "horloges": crate::win32::horloges_monde::json(),
            });
            let js = format!("window.setState({payload})");
            PAYLOAD.with(|p| *p.borrow_mut() = Some((cle, js.clone())));
            js
        }
    };
    cef_popup::execute_js(&SLOT, &js);
}

pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

/// **LEVIER DE VÉRIFICATION** — `NEURADESK_DEBUG_SUPPRIMER=<rang>` : actionne la
/// croix de la ligne `rang` des notifications, par un appel de fonction de la
/// page (voir `bandeau::debug_supprimer`, qui porte le pourquoi de la forme).
///
/// ⭐ **C'EST LE SEUL LEVIER QUI ÉPROUVE LE RENVOI INTER-MODULES** : l'action part
/// d'ICI, dans le `SLOT` du calendrier, et c'est `traiter_ou_signaler` qui la
/// remet à `win32::bandeau`. Une suppression qui marche depuis l'historique mais
/// pas depuis ce panneau est exactement le défaut que ce levier rend visible.
pub fn debug_supprimer(rang: usize) -> bool {
    if !cef_popup::is_visible(&SLOT) {
        return false;
    }
    cef_popup::execute_js(&SLOT, &format!("go('supprimer', window.__ndDerniersIds[{rang}])"));
    // ⚠️ **L'appel a été ENVOYÉ, pas exécuté** — voir `bandeau::debug_supprimer`,
    // qui porte la même réserve. La preuve de l'effet est la ligne
    // « message <id> : retire=… » du bras `supprimer`, qui n'est écrite que si
    // l'action a traversé le renvoi inter-modules.
    crate::diag!("[calendrier] NEURADESK_DEBUG_SUPPRIMER={rang} — appel de go() envoye");
    true
}

/// Vide les actions demandées par `assets/popups/calendar.html`.
///
/// **Le calendrier était le SEUL menu à ne JAMAIS drainer ses actions**, et ça
/// n'avait aucune conséquence tant qu'il n'en envoyait aucune — sa grille est
/// calculée en JS. Mais la hauteur pilotée par la page (voir
/// `win32::cef_popup::drain_actions`) lui en fait envoyer une, et sans ce
/// drainage elle **resterait dans la file pour toujours** : la file est un `Vec`
/// que rien ne vide, donc une fuite non bornée, en plus d'un menu qui ne se
/// redimensionnerait jamais.
///
/// ⚠️ **`menu/size/…` N'ARRIVE PAS JUSQU'ICI** : `cef_popup::drain_actions` la
/// consomme lui-même (voir `parse_content_size`).
///
/// ⭐ **ET DEPUIS LE 2026-10-09, CETTE PAGE ÉMET UNE VRAIE ACTION** : la croix
/// d'une notification (voir `calendar.html`, section « Notifications »). Or
/// **une file d'actions appartient à UN popup** : l'action arrive donc ICI, dans
/// le `SLOT` du calendrier, alors que les messages appartiennent à
/// `win32::bandeau`. D'où le renvoi ci-dessous — c'est le seul endroit qui peut
/// le faire, et l'oublier donnerait exactement ce que ce module journalisait
/// avant : une croix qui ne fait rien, en silence.
///
/// Même famille que `win32::cef_network::drain_actions` pour le reste : les
/// actions qu'on ne connaît pas sont DITES, jamais avalées (règle du dépôt).
pub fn drain_actions() {
    let inconnues = traiter_ou_signaler(cef_popup::drain_actions(&SLOT));
    if !inconnues.is_empty() {
        eprintln!("[neuradesk][cef] action inconnue pour le calendrier : {inconnues:?}");
    }
}

/// **RÉPARTIT LES ACTIONS DRAINÉES** : celles du bandeau sont **traitées** (c'est
/// `bandeau::dispatch_action` qui les exécute), les autres sont **rendues** pour
/// être signalées.
///
/// ⚠️ **Elle est extraite pour être vérifiable**, et ce n'est pas cosmétique :
/// c'est la seule moitié du renvoi inter-modules qu'on puisse éprouver **sans
/// fenêtre**. La brancher sur `drain_actions` est ce qui garantit que le test
/// porte sur le code réellement exécuté, et non sur une copie.
fn traiter_ou_signaler(actions: Vec<String>) -> Vec<String> {
    actions.into_iter().filter(|action| !crate::win32::bandeau::dispatch_action(action)).collect()
}

pub fn render_if_visible(tint_rgba: [f32; 4]) {
    let scale = CONTENT_SCALE.with(|c| c.get());
    cef_popup::render_if_visible(&SLOT, tint_rgba, menu::menu_corner_radius(), cef_popup::PopupPalette::Menu { zoom_level: cef_popup::menu_zoom_level(scale) });
}

pub fn debug_dump_backbuffer(path: &Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

/// **LEVIER DE VÉRIFICATION** — déplace le mois affiché de `delta` mois
/// (`NEURADESK_DEBUG_CEF_MOIS=<delta>`, lu dans `app::run`).
///
/// ⚠️ **SANS LUI, « CHANGER DE MOIS » N'EST PAS VÉRIFIABLE PAR CAPTURE** : le
/// harnais ne clique pas, et ce menu s'ouvre toujours sur le mois courant — une
/// capture ne montrerait donc jamais qu'un seul mois.
///
/// C'est un **appel de fonction direct** (`window.__ndAllerMois`), jamais une
/// entrée simulée — même règle que `cef_calendar_applet::debug_editer`, et il
/// n'écrit rien.
pub fn debug_mois(delta: i32) {
    if MOIS_APPLIQUE.with(|c| c.replace(true)) || !cef_popup::is_visible(&SLOT) {
        return;
    }
    crate::diag!("[calendrier] NEURADESK_DEBUG_CEF_MOIS={delta} — fenêtre visible : {}", cef_popup::is_visible(&SLOT));
    // ⚠️ `allerAuMois` ACCUMULE (contrairement à `debug_editer`, idempotent),
    // d'où la garde ci-dessus : répété à chaque frame, il ferait défiler les mois
    // à toute vitesse.
    cef_popup::execute_js(&SLOT, &format!("window.__ndAllerMois && window.__ndAllerMois({delta})"));
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(result) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::win32::cef_calendar_applet::decaler_mois;

    /// **LA FENÊTRE COUVRE EXACTEMENT CE QU'ELLE ANNONCE** — et c'est ce que les
    /// flèches de la page peuvent atteindre, puisqu'elles ne demandent rien à
    /// Rust. Un mois de moins à chaque bout, et la flèche correspondante
    /// promettrait un mois que la page n'a pas.
    #[test]
    fn la_fenetre_couvre_deux_annees_autour_du_mois_courant() {
        assert_eq!(MOIS_DE_CHAQUE_COTE, 12, "« deux ans autour d'aujourd'hui » (décision utilisateur)");
        assert_eq!(MOIS_DE_CHAQUE_COTE * 2 + 1, 25, "vingt-cinq mois poussés en tout");
        assert_eq!(decaler_mois(2026, 9, -MOIS_DE_CHAQUE_COTE), (2025, 9), "bord gauche");
        assert_eq!(decaler_mois(2026, 9, MOIS_DE_CHAQUE_COTE), (2027, 9), "bord droit");
    }

    /// Les deux grilles du dépôt comptent les mois **de la même façon** — c'est
    /// toute la raison d'avoir partagé `decaler_mois` plutôt que de le recopier.
    #[test]
    fn le_decalage_de_mois_est_celui_de_l_applet() {
        for (annee, mois, delta) in [(2026, 1, -1), (2026, 12, 1), (2026, 3, -14), (2026, 1, 25)] {
            let (a, m) = decaler_mois(annee, mois, delta);
            assert!((1..=12).contains(&m), "mois hors bornes : {m}");
            // Le mois obtenu est bien à `delta` mois de distance, mesuré en mois
            // absolus — c'est la seule façon d'exprimer la propriété sans
            // réécrire l'arithmétique qu'on veut vérifier.
            let ecart = (a * 12 + m as i32) - (annee * 12 + mois as i32);
            assert_eq!(ecart, delta, "{mois}/{annee} + {delta}");
        }
    }

    /// ⭐ **LA CROIX DU PANNEAU ATTEINT LE MODULE QUI POSSÈDE LES MESSAGES.**
    ///
    /// C'est le point que rien d'autre ne couvre : une file d'actions appartient
    /// à UN popup, donc l'action émise par `calendar.html` arrive dans le `SLOT`
    /// du calendrier — et sans le renvoi, `bandeau` ne la verrait **jamais**.
    /// Elle serait signalée « inconnue » et la croix ne ferait rien, en silence.
    #[test]
    fn une_croix_du_panneau_supprime_le_message_du_bandeau() {
        use crate::win32::bandeau;
        // Deux messages identiques : c'est le cas où un rang se tromperait, et
        // donc celui qui prouve que c'est bien l'IDENTIFIANT qui voyage.
        bandeau::montrer("Discord", "nouveau message");
        bandeau::montrer("Discord", "nouveau message");
        let ids: Vec<u64> = bandeau::messages().iter().map(|m| m.id).collect();
        assert_eq!(ids.len(), 2, "l'historique doit etre vide avant ce test");

        // L'action EXACTE que la page émet, préfixe compris.
        let restantes = traiter_ou_signaler(vec![format!("bandeau/supprimer/{}", ids[0])]);
        assert!(restantes.is_empty(), "une action du bandeau ne doit pas etre signalee inconnue");
        let apres: Vec<u64> = bandeau::messages().iter().map(|m| m.id).collect();
        assert_eq!(apres, vec![ids[1]], "le PREMIER des deux identiques doit etre parti");

        // Et une action d'un autre module reste signalée : c'est ce que ce
        // module doit continuer de dire.
        let restantes = traiter_ou_signaler(vec!["calendar/inconnu".to_string()]);
        assert_eq!(restantes, vec!["calendar/inconnu".to_string()]);

        for m in bandeau::messages() {
            bandeau::supprimer(m.id);
        }
    }

    /// ⭐ **LES DEUX PAGES SUPPRIMENT AVEC LE MÊME VERBE, ET PAR LE MÊME CHEMIN.**
    ///
    /// `calendar.html` et `bandeau.html` sont **autonomes** (aucun import possible
    /// entre elles) : leur mise en page de message est recopiée, et le contrat
    /// écrit dans le calendrier dit que « si l'une bouge, l'autre doit suivre ».
    /// Le verbe, lui, ne peut pas être recopié au hasard — c'est celui de
    /// `win32::bandeau`, qui POSSÈDE la donnée, et son préfixe est ce que
    /// `traiter_ou_signaler` reconnaît pour le lui renvoyer. Une page qui
    /// divergerait n'aurait **aucune erreur** : juste une croix sans effet.
    ///
    /// Le même test tient l'ascenseur des deux listes : c'est l'autre moitié de
    /// la demande, et elle se perdrait aussi en silence. Il vient de `style.css`,
    /// commun à toute l'application : chaque page doit la charger.
    #[test]
    fn les_deux_pages_suppriment_avec_le_meme_verbe_et_le_meme_prefixe() {
        const CALENDRIER: &str = include_str!("../../assets/popups/calendar.html");
        const HISTORIQUE: &str = include_str!("../../assets/popups/bandeau.html");
        const STYLE: &str = include_str!("../../assets/popups/style.css");
        for (nom, page) in [("calendar.html", CALENDRIER), ("bandeau.html", HISTORIQUE)] {
            assert!(
                page.contains("neuradesk://bandeau/"),
                "{nom} doit adresser `bandeau`, qui possede les messages"
            );
            assert!(page.contains(r#"go("supprimer""#), "{nom} n'emet pas le verbe de suppression");
            assert!(
                page.contains(r#"href="style.css""#),
                "{nom} ne charge pas `style.css` : Chromium n'en peindra qu'une barre fluide, invisible au repos"
            );
        }
        assert!(STYLE.contains("\n::-webkit-scrollbar { width:"), "l'ascenseur commun manque à `style.css`");
    }
}
