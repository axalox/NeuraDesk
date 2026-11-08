//! Le PANNEAU « Serveurs SSH » (voir `win32::ssh`, le client, `win32::ssh_serveurs`,
//! les serveurs enregistrés, `assets/popups/ssh.html` et `docs/plans/ssh-sftp.md`).
//!
//! Un PANNEAU comme la Calculatrice. **LES ONGLETS VIVENT DANS LA PAGE** : c'est
//! elle qui numérote les connexions ; Rust ne fait que relayer. Masquer le
//! panneau ne ferme rien — le navigateur préchargé garde ses terminaux, et
//! [`dispatch_actions`] continue d'y verser la sortie des serveurs.
//!
//! Trois chemins :
//! - page → Rust, commandes : `neuradesk://ssh/<verbe>/…` (segments ENCODÉS) ;
//! - page → Rust, frappes / formulaire / secret saisi : le canal POST
//!   (`win32::cef_data`, routes `ssh/…`) — ⛔ un secret ne passe JAMAIS par une
//!   URL, qui peut finir au journal ;
//! - Rust → page : `execute_js` (`window.ssh.*`).
//!
//! Actions reçues (`"ssh/<verbe>/…"`) :
//! - `ready`, `close`
//! - `connecter/<serveur>/<onglet>/<colonnes>/<lignes>` — secret saisi éventuel
//!   déposé AVANT sur la route `ssh/secret/<onglet>`
//! - `redimensionner/<onglet>/<colonnes>/<lignes>`
//! - `fermer/<onglet>`
//! - `cle/<onglet>/<oui|non>` — réponse à « faire confiance à cette clé ? »
//! - `enregistrer` — fiche déposée AVANT sur la route `ssh/fiche`
//! - `supprimer/<serveur>`
//! - `choisir-cle`
//! - `copier` — texte déposé AVANT sur la route `ssh/copie` ; `coller/<onglet>`
//!   (le presse-papiers passe par Rust : `navigator.clipboard` exige une
//!   permission que rien n'accorde dans ces pages)
//! - vue Fichiers, volet distant (chemins absolus) : `lister/<onglet>/<chemin>`,
//!   `telecharger/<onglet>/<dossier local>/<chemin>…`,
//!   `televerser/<onglet>/<dossier distant>/<chemin local>…`,
//!   `creer-dossier/<onglet>/<dossier>/<nom>`, `renommer/<onglet>/<chemin>/<nom>`,
//!   `effacer/<onglet>/<chemin>…`, `droits/<onglet>/<chemin>/<octal>`
//! - vue Fichiers, volet « ce PC » : `lister-local/<onglet>/<chemin>` (`*` =
//!   Ce PC, `~` = dossier personnel — voir `ssh::local`)

use std::cell::{Cell, RefCell};

use base64::Engine;
use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

use crate::gfx::device::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec};
use crate::win32::ssh::{self, Etat, Evenement, ReponseSftp, RequeteSftp};
use crate::win32::{cef_data, file_dialog, ssh_serveurs};
use std::path::PathBuf;

thread_local! {
    static SLOT: PopupSlot = const { PopupSlot::new() };
    static CONTENT_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// L'échelle d'ÉCRAN reçue au dernier `toggle` (sans « Taille du contenu ») :
    /// la vue sphère rouvre le panneau avec elle (voir `win32::cef_sphere`).
    static ECHELLE_ECRAN: Cell<f32> = const { Cell::new(1.0) };
    /// La clé du dernier `window.setState(...)` émis — voir
    /// `cef_minuteur::PAYLOAD` : vidée à l'ouverture ET au `ready`.
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
}

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskCefSshWindow"),
    window_title: w!("NeuraDesk — Serveurs SSH"),
    html_file: "ssh.html",
    wndproc: Some(wndproc),
    above_a_magnified_dock: false,
    fades: false,
    fenetre: false,
};

pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

fn masquer_les_voisins() {
    crate::win32::cef_dash::hide_if_visible();
    crate::win32::cef_settings::hide_if_visible();
    crate::win32::cef_notes::hide_if_visible();
    crate::win32::cef_image_editor::hide_if_visible();
    crate::win32::cef_calendar_applet::hide_if_visible();
    crate::win32::cef_clipboard::hide_if_visible();
    crate::win32::cef_surveillance::hide_if_visible();
    crate::win32::cef_scanner::hide_if_visible();
    crate::win32::cef_shaders::hide_if_visible();
    crate::win32::cef_lueurs::hide_if_visible();
    crate::win32::cef_textures::hide_if_visible();
    crate::win32::cef_moniteur::hide_if_visible();
    crate::win32::cef_minuteur::hide_if_visible();
    crate::win32::cef_calculatrice::hide_if_visible();
    crate::win32::cef_recherche::hide_if_visible();
    crate::win32::cef_automatisations::hide_if_visible();
    crate::win32::cef_meteo::hide_if_visible();
    crate::win32::cef_aide::hide_if_visible();
    crate::win32::bandeau::hide_if_visible();
    crate::win32::cef_terminal::hide_if_visible();
}

pub fn toggle(rect: [i32; 4], gfx: &GfxDevice, content_scale: f32) {
    ECHELLE_ECRAN.with(|c| c.set(content_scale));
    // L'échelle d'écran × le réglage « Taille du contenu » du panneau.
    CONTENT_SCALE.with(|c| c.set(content_scale * crate::win32::taille_panneaux::facteur("ssh")));
    PAYLOAD.with(|c| *c.borrow_mut() = None);
    cef_popup::toggle_with(&SLOT, &SPEC, rect, gfx, masquer_les_voisins);
    push_state();
}

/// L'échelle d'écran du dernier `toggle` — voir [`ECHELLE_ECRAN`].
pub fn echelle_d_ecran() -> f32 {
    ECHELLE_ECRAN.with(Cell::get)
}

/// LA VUE SPHÈRE de l'autre panneau bascule ici (voir `win32::cef_sphere`) :
/// le panneau se montre, sa sphère ouverte sur `onglet`.
pub fn montrer_en_sphere(rect: [i32; 4], gfx: &GfxDevice, onglet: u32) {
    if !cef_popup::is_visible(&SLOT) {
        toggle(rect, gfx, echelle_d_ecran());
    }
    appeler("sphereOuvrir", &[json!(onglet)]);
}

/// Les aperçus des sessions de l'autre panneau, pour la sphère de celui-ci.
pub fn sphere_autres(apercus: &serde_json::Value) {
    appeler("sphereAutres", &[apercus.clone()]);
}

pub fn is_visible() -> bool {
    cef_popup::is_visible(&SLOT)
}

/// Voir `cef_popup::donner_le_clavier` (vue sphère).
pub fn donner_le_clavier() -> bool {
    cef_popup::donner_le_clavier(&SLOT)
}

/// La sphère redemande les vignettes de la page, panneau visible ou non (voir
/// `assets/popups/sphere_source.js`).
pub fn demander_vignettes() {
    cef_popup::execute_js(&SLOT, "window.sphereSource && window.sphereSource.envoyer()");
}

pub fn refresh_if_visible() {
    if cef_popup::is_visible(&SLOT) {
        push_state();
    }
}

/// L'état de la page : la liste des serveurs — ⛔ SANS le secret, seulement
/// `secretDefini`.
fn etat_json(serveurs: &ssh_serveurs::Serveurs) -> serde_json::Value {
    let liste: Vec<serde_json::Value> = serveurs
        .liste
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "nom": s.nom,
                "libelle": s.libelle(),
                "hote": s.hote,
                "port": s.port,
                "utilisateur": s.utilisateur,
                "methode": s.methode.nom(),
                "cle": s.cle,
                "secretDefini": s.secret_defini(),
            })
        })
        .collect();
    json!({
        "serveurs": liste,
        "commandes": crate::win32::commandes::json(),
        "contentScale": CONTENT_SCALE.with(Cell::get),
        // LA POLICE DES TERMINAUX — la MÊME que `cef_terminal` (voir
        // `ui::style::police_des_terminaux`) : une session SSH est un terminal, et
        // le réglage se dit « des terminaux ». `refresh_if_visible` rappelle
        // `push_state` tant que le panneau est ouvert, donc un clic dans Réglages
        // change la police des sessions ouvertes SANS les reconnexion.
        "policeTerminal": crate::ui::style::police_des_terminaux().unwrap_or_default(),
        // Le raccourci de la vue sphère (voir `win32::cef_sphere::raccourci`).
        "raccourci": crate::win32::cef_sphere::raccourci(),
        // La vitesse du zoom à la molette, en % (réglée dans Réglages).
        "sphereZoom": crate::win32::cef_sphere::vitesse_zoom(),
        // Les réglages PROPRES à la sphère (voir `win32::cef_sphere`) : distance
        // d'ouverture, recadrage automatique, et fondu du panneau.
        "sphereZoomDefaut": crate::win32::cef_sphere::zoom_defaut(),
        "sphereZoomAuto": crate::win32::cef_sphere::zoom_auto(),
        "sphereFonduOuverture": crate::win32::cef_sphere::fondu().0,
        "sphereFonduFermeture": crate::win32::cef_sphere::fondu().1,
    })
}

fn push_state() {
    let payload = ssh_serveurs::lire(etat_json).to_string();
    if PAYLOAD.with(|p| p.borrow().as_deref() == Some(payload.as_str())) {
        return;
    }
    cef_popup::execute_js(&SLOT, &format!("window.setState({payload})"));
    PAYLOAD.with(|p| *p.borrow_mut() = Some(payload));
}

fn appeler(fonction: &str, arguments: &[serde_json::Value]) {
    let arguments: Vec<String> = arguments.iter().map(serde_json::Value::to_string).collect();
    cef_popup::execute_js(&SLOT, &format!("window.ssh && window.ssh.{fonction}({})", arguments.join(",")));
}

fn nom_etat(etat: &Etat) -> &'static str {
    match etat {
        Etat::Connexion => "connexion",
        Etat::Authentification => "authentification",
        Etat::Ouvert => "ouvert",
        Etat::Ferme => "ferme",
    }
}

/// Verse les évènements du client dans la page — **À CHAQUE FRAME, PANNEAU
/// VISIBLE OU NON** : un terminal masqué doit continuer de recevoir sa sortie.
/// Les sorties consécutives d'un même onglet partent en UN appel.
fn relayer_evenements() {
    let mut sortie: Option<(u32, Vec<u8>)> = None;
    let vider = |sortie: &mut Option<(u32, Vec<u8>)>| {
        if let Some((onglet, octets)) = sortie.take() {
            let b64 = base64::engine::general_purpose::STANDARD.encode(octets);
            appeler("sortie", &[json!(onglet), json!(b64)]);
        }
    };
    for evenement in ssh::prendre_evenements() {
        if let Evenement::Sortie { onglet, octets } = evenement {
            match &mut sortie {
                Some((o, tampon)) if *o == onglet => tampon.extend_from_slice(&octets),
                _ => {
                    vider(&mut sortie);
                    sortie = Some((onglet, octets));
                }
            }
            continue;
        }
        vider(&mut sortie);
        match evenement {
            Evenement::Sortie { .. } => {}
            Evenement::Etat { onglet, etat } => appeler("etat", &[json!(onglet), json!(nom_etat(&etat))]),
            Evenement::Erreur { onglet, message } => appeler("erreur", &[json!(onglet), json!(message)]),
            Evenement::CleInconnue { onglet, algorithme, empreinte } => {
                appeler("cleInconnue", &[json!(onglet), json!(algorithme), json!(empreinte)])
            }
            Evenement::Sftp { onglet, reponse } => {
                let reponse = match reponse {
                    ReponseSftp::Liste { chemin, entrees } => json!({
                        "chemin": chemin,
                        "entrees": entrees.iter().map(|e| json!({
                            "nom": e.nom, "dossier": e.dossier, "lien": e.lien,
                            "taille": e.taille, "modifie": e.modifie, "droits": e.droits,
                        })).collect::<Vec<_>>(),
                    }),
                    ReponseSftp::Fait { message, erreurs } => json!({ "fait": message, "erreurs": erreurs }),
                    ReponseSftp::Progres { nom, fait, total } => {
                        json!({ "progres": { "nom": nom, "fait": fait, "total": total } })
                    }
                    ReponseSftp::Erreur { message } => json!({ "erreur": message }),
                };
                appeler("sftp", &[json!(onglet), reponse]);
            }
            Evenement::Local { onglet, reponse } => {
                let reponse = match reponse {
                    Ok(liste) => json!({
                        "chemin": liste.chemin,
                        "parent": liste.parent,
                        "entrees": liste.entrees.iter().map(|e| json!({
                            "nom": e.nom, "chemin": e.chemin, "dossier": e.dossier, "lien": e.lien,
                            "taille": e.taille, "modifie": e.modifie,
                        })).collect::<Vec<_>>(),
                    }),
                    Err(message) => json!({ "erreur": message }),
                };
                appeler("local", &[json!(onglet), reponse]);
            }
        }
    }
    vider(&mut sortie);
}

fn nombre(texte: &str) -> Option<u32> {
    texte.parse().ok()
}

fn connecter(serveur: u32, onglet: u32, colonnes: u32, lignes: u32) {
    let secret = cef_data::prendre(&format!("ssh/secret/{onglet}")).and_then(|octets| String::from_utf8(octets).ok());
    let cible = ssh_serveurs::lire(|s| s.trouver(serveur).map(|s| s.cible(secret, colonnes, lignes)));
    match cible {
        Some(Ok(cible)) => {
            crate::diag!("[ssh] onglet {onglet} : connexion à {}:{}", cible.hote, cible.port);
            ssh::connecter(onglet, cible);
        }
        // Le client n'a pas démarré : c'est ICI qu'il faut dire « fermé »,
        // sinon l'onglet reste à « Connexion… » sans bouton pour réessayer.
        Some(Err(message)) => refuser(onglet, &message),
        None => refuser(onglet, "Ce serveur n'existe plus."),
    }
}

fn refuser(onglet: u32, message: &str) {
    appeler("erreur", &[json!(onglet), json!(message)]);
    appeler("etat", &[json!(onglet), json!(nom_etat(&Etat::Ferme))]);
}

fn enregistrer() {
    let resultat = cef_data::prendre("ssh/fiche")
        .ok_or_else(|| "Le formulaire n'est pas arrivé.".to_string())
        .and_then(|octets| ssh_serveurs::Fiche::depuis_json(&octets))
        .and_then(|fiche| ssh_serveurs::modifier(|s| s.enregistrer(fiche)));
    match resultat {
        Ok(id) => appeler("ficheEnregistree", &[json!(id), json!(null)]),
        Err(message) => appeler("ficheEnregistree", &[json!(null), json!(message)]),
    }
    push_state();
}

fn choisir_cle() {
    let Some(proprietaire) = cef_popup::hwnd(&SLOT) else { return };
    // Annulé : pas une erreur, rien au journal (voir `file_dialog::ouvrir`).
    if let Some(chemin) = file_dialog::ouvrir(proprietaire, "Choisir une clé privée", "*", "Tous les fichiers") {
        appeler("cleChoisie", &[json!(chemin.to_string_lossy())]);
    }
}

/// Une requête SFTP de la page ; un onglet sans connexion n'a rien à dire.
fn demander_sftp(onglet: &str, requete: impl FnOnce() -> Option<RequeteSftp>) {
    match (nombre(onglet), requete()) {
        (Some(o), Some(requete)) => {
            if !ssh::sftp(o, requete) {
                refuser_sftp(o, "Cet onglet n'est plus connecté.");
            }
        }
        _ => crate::diag!("[ssh] requête de fichiers illisible (onglet {onglet:?})"),
    }
}

fn refuser_sftp(onglet: u32, message: &str) {
    appeler("sftp", &[json!(onglet), json!({ "erreur": message })]);
}

/// Un chemin de CE PC venu de la page : absolu, sinon rien (`*` et `~` ne
/// désignent pas un dossier où écrire).
fn chemin_local(texte: &str) -> Option<PathBuf> {
    Some(PathBuf::from(texte)).filter(|p| p.is_absolute())
}

/// Vers le dossier affiché dans le volet « ce PC ».
fn telecharger(onglet: &str, dossier_local: &str, distants: &[&str]) {
    let Some(dossier_local) = chemin_local(dossier_local).filter(|d| d.is_dir()) else {
        if let Some(o) = nombre(onglet) {
            refuser_sftp(o, "Ouvrez d'abord, à gauche, le dossier qui recevra les fichiers.");
        }
        return;
    };
    let distants = distants.iter().map(|d| d.to_string()).collect();
    demander_sftp(onglet, || Some(RequeteSftp::Telecharger { distants, dossier_local }));
}

/// Depuis la sélection du volet « ce PC ».
fn televerser(onglet: &str, dossier_distant: &str, locaux: &[&str]) {
    let locaux: Option<Vec<PathBuf>> = locaux.iter().map(|l| chemin_local(l)).collect();
    let dossier_distant = dossier_distant.to_string();
    demander_sftp(onglet, || Some(RequeteSftp::Televerser { locaux: locaux?, dossier_distant }));
}

pub fn dispatch_actions() {
    relayer_evenements();
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(reste) = action.strip_prefix("ssh/") else {
            crate::diag!("[ssh] action inconnue : {action:?}");
            continue;
        };
        let parts: Vec<String> = reste.split('/').map(crate::win32::cef_app::percent_decode).collect();
        match parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["ready"] => PAYLOAD.with(|c| *c.borrow_mut() = None),
            ["close"] => cef_popup::fermer_par_geste(&SLOT),
            // LA VUE SPHÈRE (voir `win32::cef_sphere`) : le clic molette ou
            // l'indicateur de la barre ; les vignettes déposées avant par POST.
            ["sphere-etat", etat] => crate::win32::cef_sphere::etat(crate::win32::cef_sphere::SSH, *etat == "oui"),
            ["sphere-autre", onglet] => match nombre(onglet) {
                Some(o) => crate::win32::cef_sphere::basculer_depuis(crate::win32::cef_sphere::SSH, o),
                None => crate::diag!("[ssh] onglet illisible : {action:?}"),
            },
            ["raccourci", valeur] => crate::win32::cef_sphere::regler_raccourci(valeur),
            ["vignettes"] => crate::win32::cef_sphere::recevoir(
                crate::win32::cef_sphere::SSH,
                cef_data::prendre("ssh/vignettes"),
            ),
            ["commandes"] => crate::win32::commandes::remplacer_depuis("ssh/commandes"),
            ["commandes-exporter"] => {
                if let Some(h) = cef_popup::hwnd(&SLOT) { crate::win32::commandes::exporter(h) }
            }
            ["commandes-importer"] => {
                if let Some(h) = cef_popup::hwnd(&SLOT) { crate::win32::commandes::importer(h) }
            }
            ["connecter", serveur, onglet, colonnes, lignes] => {
                match (nombre(serveur), nombre(onglet), nombre(colonnes), nombre(lignes)) {
                    (Some(s), Some(o), Some(c), Some(l)) => connecter(s, o, c, l),
                    _ => crate::diag!("[ssh] connexion illisible : {action:?}"),
                }
            }
            ["redimensionner", onglet, colonnes, lignes] => match (nombre(onglet), nombre(colonnes), nombre(lignes)) {
                (Some(o), Some(c), Some(l)) => {
                    ssh::redimensionner(o, c, l);
                }
                _ => crate::diag!("[ssh] taille illisible : {action:?}"),
            },
            ["fermer", onglet] => match nombre(onglet) {
                Some(o) => ssh::fermer(o),
                None => crate::diag!("[ssh] onglet illisible : {action:?}"),
            },
            ["cle", onglet, reponse] => match nombre(onglet) {
                Some(o) => ssh::confirmer_cle(o, *reponse == "oui"),
                None => crate::diag!("[ssh] onglet illisible : {action:?}"),
            },
            ["enregistrer"] => enregistrer(),
            ["supprimer", serveur] => match nombre(serveur) {
                Some(id) => {
                    ssh_serveurs::modifier(|s| s.supprimer(id));
                    push_state();
                }
                None => crate::diag!("[ssh] serveur illisible : {action:?}"),
            },
            ["choisir-cle"] => choisir_cle(),
            ["copier"] => match cef_data::prendre("ssh/copie").and_then(|octets| String::from_utf8(octets).ok()) {
                Some(texte) if !texte.is_empty() => {
                    // Le booléen est ignoré : ce panneau n'a pas d'endroit où
                    // dire l'échec (le presse-papiers occupé est banal).
                    let _ = crate::win32::clipboard::write_text(&texte);
                }
                _ => {}
            },
            ["coller", onglet] => match (nombre(onglet), crate::win32::clipboard::read_text()) {
                (Some(o), Some(texte)) => appeler("coller", &[json!(o), json!(texte)]),
                (None, _) => crate::diag!("[ssh] onglet illisible : {action:?}"),
                (Some(_), None) => {}
            },
            ["lister", onglet, chemin] => {
                demander_sftp(onglet, || Some(RequeteSftp::Lister { chemin: chemin.to_string() }))
            }
            // Un chemin VIDE n'arrive jamais : `on_before_browse` retire les `/`
            // finaux, donc `lister/<onglet>/` devient `lister/<onglet>`. Même
            // sens : le dossier personnel.
            ["lister", onglet] => demander_sftp(onglet, || Some(RequeteSftp::Lister { chemin: String::new() })),
            ["lister-local", onglet, chemin] => match nombre(onglet) {
                Some(o) => ssh::lister_local(o, chemin.to_string()),
                None => crate::diag!("[ssh] onglet illisible : {action:?}"),
            },
            ["telecharger", onglet, dossier, distants @ ..] if !distants.is_empty() => {
                telecharger(onglet, dossier, distants)
            }
            ["televerser", onglet, dossier, locaux @ ..] if !locaux.is_empty() => televerser(onglet, dossier, locaux),
            ["creer-dossier", onglet, dossier, nom] => demander_sftp(onglet, || {
                Some(RequeteSftp::CreerDossier { dossier: dossier.to_string(), nom: nom.to_string() })
            }),
            ["renommer", onglet, chemin, nom] => demander_sftp(onglet, || {
                Some(RequeteSftp::Renommer { chemin: chemin.to_string(), nom: nom.to_string() })
            }),
            ["effacer", onglet, chemins @ ..] if !chemins.is_empty() => demander_sftp(onglet, || {
                Some(RequeteSftp::Supprimer { chemins: chemins.iter().map(|c| c.to_string()).collect() })
            }),
            ["droits", onglet, chemin, mode] => demander_sftp(onglet, || {
                let mode = u32::from_str_radix(mode, 8).ok().filter(|m| *m <= 0o7777)?;
                Some(RequeteSftp::Droits { chemin: chemin.to_string(), mode })
            }),
            _ => crate::diag!("[ssh] action inconnue : {action:?}"),
        }
    }
}

pub fn hide_if_focus_elsewhere() {
    cef_popup::hide_if_focus_elsewhere(&SLOT);
}

pub fn hide_if_visible() {
    cef_popup::hide_if_visible(&SLOT);
}

pub fn render_if_visible(rect: [i32; 4], tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::resize_if_needed(&SLOT, rect);
    cef_popup::render_if_visible(&SLOT, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
    // La texture de SES terminaux (voir `win32::texture_terminaux`).
    crate::win32::texture_terminaux::tenir_a_jour(&SLOT, "ssh");
}

/// Voir `win32::cef_calendar_applet::debug_dump_backbuffer`
/// (`NEURADESK_DEBUG_OPEN=ssh`).
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
    /// Les verbes que `dispatch_actions` accepte — même lecture que
    /// `cef_calculatrice::tests`.
    fn verbes_du_dispatch() -> Vec<String> {
        let source = include_str!("cef_ssh.rs");
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
                matches!(fin.chars().next(), Some(']') | Some(',')).then(|| verbe.to_string())
            })
            .collect()
    }

    fn verbes_de_la_page() -> Vec<String> {
        // La page ET le volet des commandes qu'elle charge (voir
        // `cef_terminal::tests::verbes_de_la_page`).
        const SOURCE: &str = include_str!("../../assets/popups/ssh.html");
        [SOURCE, crate::win32::commandes::VOLET_JS, include_str!("../../assets/popups/sphere_source.js"), include_str!("../../assets/popups/sphere_vue.js")]
            .into_iter()
            .flat_map(|source| {
                source.match_indices("go(").filter_map(move |(i, _)| {
                    let reste = &source[i + 3..];
                    let guillemet = reste.chars().next().filter(|c| *c == '"' || *c == '\'')?;
                    reste[1..].split(guillemet).next().map(str::to_string)
                })
            })
            .collect()
    }

    #[test]
    fn la_page_et_le_dispatch_acceptent_les_memes_verbes() {
        let mut page = verbes_de_la_page();
        let mut rust = verbes_du_dispatch();
        page.sort();
        page.dedup();
        rust.sort();
        rust.dedup();
        assert_eq!(page, rust);
    }

    /// ⛔ `on_before_browse` retire les `/` en fin d'URL : un DERNIER segment
    /// vide disparaît, et l'action arrive amputée (la vue Fichiers restait sur
    /// « Lecture… »). La page ne doit donc jamais passer "" à `go(…)` ni à
    /// `lister(…)`.
    #[test]
    fn la_page_n_envoie_jamais_de_segment_vide() {
        const PAGE: &str = include_str!("../../assets/popups/ssh.html");
        let mut appels = 0;
        for debut in ["go(", "lister("] {
            for (i, _) in PAGE.match_indices(debut) {
                let arguments = PAGE[i + debut.len()..].split(')').next().unwrap_or_default();
                assert!(
                    !arguments.contains("\"\"") && !arguments.contains("''"),
                    "segment vide dans ssh.html : {debut}{arguments})"
                );
                appels += 1;
            }
        }
        assert!(appels > 20, "{appels} appels seulement : la lecture de la page a changé ?");
    }

    /// ⛔ Un clic de sélection qui RECONSTRUIT la table tue le double-clic : le
    /// 2ᵉ clic tombe sur une rangée neuve, Chromium n'émet pas `dblclick`, et
    /// aucun dossier ne s'ouvrait.
    #[test]
    fn choisir_une_rangee_ne_reconstruit_pas_la_table() {
        const PAGE: &str = include_str!("../../assets/popups/ssh.html");
        let debut = PAGE.find("function choisir(").expect("choisir absente de ssh.html");
        let corps = &PAGE[debut..];
        let corps = &corps[..corps.find("\n  }\n").expect("fin de choisir")];
        assert!(corps.contains("majChoix()"), "{corps}");
        for interdit in ["rendre()", "renderFichiers(", "renderLocal(", "renderTable("] {
            assert!(!corps.contains(interdit), "choisir appelle {interdit}");
        }
    }

    /// Les fonctions que Rust appelle dans la page (`appeler("…")`) doivent y
    /// exister — sinon la sortie d'un terminal se perdrait EN SILENCE.
    #[test]
    fn la_page_definit_chaque_fonction_que_rust_appelle() {
        const PAGE: &str = include_str!("../../assets/popups/ssh.html");
        let source = include_str!("cef_ssh.rs");
        let rust = source.split("#[cfg(test)]").next().unwrap_or(source);
        let appelees: Vec<&str> =
            rust.match_indices("appeler(\"").filter_map(|(i, _)| rust[i + 9..].split('"').next()).collect();
        assert!(appelees.len() >= 6, "{appelees:?}");
        for fonction in appelees {
            assert!(PAGE.contains(&format!("{fonction}(")), "window.ssh.{fonction} absente de ssh.html");
        }
    }
}
