//! **LA FENÊTRE DU DNS** — changer les serveurs DNS de Windows, dans la zone
//! Intégrés (voir `win32::dns` pour la lecture et l'application).
//!
//! DEMANDE (2026-11-22) : « dans intégrés je voudrais une applet pour modifier
//! le dns de windows ».
//!
//! ⭐ **LA JUMELLE DE `cef_patience`** pour la chrome (une `fenetre: true` au
//! centre de l'écran PRIMAIRE, punaise, croix, bande de titre). La page montre
//! les cartes actives et leurs serveurs, des préréglages (automatique,
//! Cloudflare, Google, Quad9…) et deux champs libres.
//!
//! DEMANDE (2026-11-23) : un onglet « Adresse IP » (automatique ou manuelle,
//! passerelle) et le nom « Modifier réglages réseau » — voir `win32::reseau_ip`.
//! Le verbe `ip/<carte>/<auto|adresse,masque,passerelle>` suit le même chemin
//! que `appliquer` (relecture, validation, fil de travail, UAC).
//!
//! DEMANDE (2026-11-24) : les autres réglages réseau de Windows, en onglets, et
//! l'adresse MAC. Verbes : `appliquer/<cartes>/<quoi>/<0|1 chiffré>`,
//! `ip6/<carte>/<auto|adresse,préfixe,passerelle>`,
//! `connexion/<carte>/<categorie/…|limitee/…|renouveler>`,
//! `mac/<carte>/<adresse|origine>` (élevés, même chemin que `appliquer`) et
//! `proxy/<json>` (réglage de l'utilisateur, SANS UAC, appliqué ici même).
//!
//! ⚠️ **L'APPLICATION BLOQUE** (invite UAC puis PowerShell) : elle part sur un
//! FIL DE TRAVAIL. `SLOT` est `thread_local` — le fil ne peut pas parler à la
//! page ; il dépose son issue dans `ISSUE`, que `refresh_if_visible` relève
//! (relecture des cartes, puis `push_state`).

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetSystemMetrics, GetWindowRect, SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SM_CXSCREEN,
    SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WM_CLOSE, WM_NCHITTEST,
};

use crate::gfx::GfxDevice;
use crate::win32::cef_popup::{self, PopupSlot, PopupSpec, PART_ECRAN};
use crate::win32::{dns, reseau_connexion, reseau_ip, reseau_mac, reseau_proxy};

thread_local! {
    static SLOT: PopupSlot = PopupSlot::new();
    static PAYLOAD: RefCell<Option<String>> = const { RefCell::new(None) };
    static RAFALE: Cell<u32> = const { Cell::new(0) };
    static CARTES: RefCell<Vec<dns::Carte>> = const { RefCell::new(Vec::new()) };
    static PROXY: RefCell<Option<reseau_proxy::Proxy>> = const { RefCell::new(None) };
    /// La dernière issue montrée par la page (`""` = aucune).
    static MESSAGE: RefCell<String> = const { RefCell::new(String::new()) };
    /// Relectures DIFFEREES apres une issue : une carte redemarree (adresse MAC)
    /// ou un bail renouvele n'a pas encore sa passerelle quand le script rend la
    /// main, et la premiere relecture la croirait deconnectee.
    static RELIRE_A: RefCell<Vec<std::time::Instant>> = const { RefCell::new(Vec::new()) };
}

/// Les delais des relectures differees, apres l'issue.
const RELECTURES_MS: [u64; 3] = [3_000, 8_000, 20_000];

/// Une application en cours sur le fil de travail.
static EN_COURS: AtomicBool = AtomicBool::new(false);
/// L'issue déposée par le fil, relevée par `refresh_if_visible`.
static ISSUE: Mutex<Option<dns::Issue>> = Mutex::new(None);

/// Le réglage « rester au premier plan » de CETTE fenêtre (voir `cef_jeu2048`).
const PREMIER_PLAN: &str = "dns_premier_plan";

const SPEC: PopupSpec = PopupSpec {
    class_name: w!("NeuraDeskDnsWindow"),
    window_title: w!("NeuraDesk — Modifier réglages réseau"),
    html_file: "dns.html",
    wndproc: Some(wndproc),
    above_a_magnified_dock: false,
    // `fades: false` est OBLIGATOIRE pour une fenêtre (voir `cef_maths::SPEC`).
    fades: false,
    fenetre: true,
};

/// Unités de page → pixels physiques : le zoom `CONTENT_ZOOM_FACTOR` (3×).
const PX_PAR_UNITE: i32 = 3;
/// La taille VISÉE, en unités de page : la liste des cartes, les préréglages,
/// les deux champs et les boutons. Plafonnée à `PART_ECRAN`. Cinq onglets
/// depuis le 2026-11-24 : la barre des onglets doit tenir en largeur.
const LARGEUR: i32 = 520;
const HAUTEUR: i32 = 720;

fn taille_pour_tout_le_contenu(largeur_ecran: i32, hauteur_ecran: i32) -> [i32; 2] {
    let plafond_l = (largeur_ecran * PART_ECRAN / 100) / PX_PAR_UNITE;
    let plafond_h = (hauteur_ecran * PART_ECRAN / 100) / PX_PAR_UNITE;
    [LARGEUR.min(plafond_l) * PX_PAR_UNITE, HAUTEUR.min(plafond_h) * PX_PAR_UNITE]
}

/// **LE RECTANGLE D'OUVERTURE — AU CENTRE DE L'ÉCRAN PRIMAIRE** (voir `cef_jeu2048`).
pub fn rect_pour_ecran_primaire() -> [i32; 4] {
    let (largeur, hauteur) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let [l, h] = cef_popup::taille_aux_textes(largeur, hauteur, taille_pour_tout_le_contenu);
    [((largeur - l) / 2).max(0), ((hauteur - h) / 2).max(0), l, h]
}

pub fn preload(gfx: &GfxDevice) {
    cef_popup::preload(&SLOT, &SPEC, gfx);
}

/// Voir `cef_maths::recaler_si_il_ne_tient_plus` — ouverte sur la PRIMAIRE.
pub fn recaler_si_il_ne_tient_plus() {
    cef_popup::recaler_si_il_ne_tient_plus(&SLOT);
}

/// Ouvre ou referme la fenêtre. Appelée par le bras de clic de l'applet.
pub fn toggle(rect: [i32; 4], gfx: &GfxDevice) {
    if cef_popup::basculer_ou_cacher_si_ouvert(&SLOT) {
        return;
    }
    crate::diag!("[dns] ouverture : rect {rect:?} (x,y,largeur,hauteur)");
    relire();
    MESSAGE.with(|m| m.borrow_mut().clear());
    PAYLOAD.with(|p| *p.borrow_mut() = None);
    cef_popup::regler_zoom_textes(&SLOT, crate::win32::taille_panneaux::global(crate::win32::taille_panneaux::TEXTES), rect);
    cef_popup::montrer_fenetre(&SLOT, &SPEC, rect, gfx);
    RAFALE.with(|r| r.set(6));
    push_state();
}

pub fn render_if_visible(tint_rgba: [f32; 4], accent_rgb: [f32; 3], dark_mode: bool) {
    cef_popup::render_if_visible(&SLOT, tint_rgba, cef_popup::rayon_d_applet(), cef_popup::PopupPalette::Window { accent_rgb, dark_mode });
}

pub fn debug_dump_backbuffer(path: &std::path::Path) -> bool {
    cef_popup::debug_dump_backbuffer(&SLOT, path)
}

fn relire() {
    let cartes = dns::cartes();
    crate::diag!("[dns] {} carte(s) active(s)", cartes.len());
    CARTES.with(|c| *c.borrow_mut() = cartes);
    PROXY.with(|p| *p.borrow_mut() = Some(reseau_proxy::lire()));
}

fn etat_json() -> String {
    let cartes: Vec<serde_json::Value> = CARTES.with(|c| {
        c.borrow()
            .iter()
            .map(|carte| {
                serde_json::json!({
                    "index": carte.index,
                    "nom": carte.nom,
                    "description": carte.description,
                    "serveurs": carte.serveurs,
                    "manuel": carte.manuel,
                    "dhcp": carte.dhcp,
                    "ipv4": carte.ipv4.map(|(ip, _)| ip.to_string()),
                    "masque": carte.ipv4.map(|(_, n)| crate::win32::reseau_ip::masque_du_prefixe(n).to_string()),
                    "passerelle": carte.passerelle.map(|ip| ip.to_string()),
                    "wifi": carte.type_if == reseau_connexion::IF_TYPE_WIFI,
                    "cable": carte.type_if == reseau_connexion::IF_TYPE_ETHERNET,
                    "ipv6": carte.ipv6.map(|(ip, _)| ip.to_string()),
                    "prefixe6": carte.ipv6.map(|(_, n)| n),
                    "passerelle6": carte.passerelle6.map(|ip| ip.to_string()),
                    "ipv6Manuel": carte.ipv6_manuel,
                    "doh": carte.doh,
                    "categorie": carte.connexion.and_then(|c| c.categorie).map(|c| c.code()),
                    "limitee": carte.connexion.map(|c| c.limitee),
                    "mac": carte.mac,
                    "macPosee": carte.mac_posee.is_some(),
                })
            })
            .collect()
    });
    serde_json::json!({
        "epingle": crate::config::load_bool_setting_or(PREMIER_PLAN, false),
        // La langue de la session : la page masque Quad9 en français.
        "langue": crate::ui::langue::courante().code(),
        "cartes": cartes,
        "proxy": PROXY.with(|p| p.borrow().as_ref().map(reseau_proxy::Proxy::en_json)),
        "enCours": EN_COURS.load(Ordering::SeqCst),
        "message": MESSAGE.with(|m| m.borrow().clone()),
    })
    .to_string()
}

pub fn push_state() {
    let json = etat_json();
    let change = PAYLOAD.with(|p| {
        let mut p = p.borrow_mut();
        if p.as_deref() == Some(json.as_str()) {
            false
        } else {
            *p = Some(json.clone());
            true
        }
    });
    if change {
        cef_popup::execute_js(&SLOT, &format!("if (window.setState) window.setState({json})"));
    }
}

pub fn refresh_if_visible() {
    // L'issue d'une application se relève même fenêtre cachée : elle est
    // montrée à la prochaine ouverture si l'utilisateur a fermé entre-temps.
    let issue = ISSUE.lock().ok().and_then(|mut i| i.take());
    if let Some(issue) = issue {
        EN_COURS.store(false, Ordering::SeqCst);
        MESSAGE.with(|m| *m.borrow_mut() = issue.code().to_string());
        relire();
        push_state();
        let maintenant = std::time::Instant::now();
        RELIRE_A.with(|r| {
            *r.borrow_mut() = RELECTURES_MS.iter().map(|ms| maintenant + std::time::Duration::from_millis(*ms)).collect()
        });
    }
    let echue = RELIRE_A.with(|r| {
        let mut r = r.borrow_mut();
        let avant = r.len();
        let maintenant = std::time::Instant::now();
        r.retain(|t| *t > maintenant);
        r.len() != avant
    });
    if echue {
        relire();
        push_state();
    }
    if !cef_popup::is_visible(&SLOT) {
        return;
    }
    appliquer_le_premier_plan();
    let restante = RAFALE.with(|r| r.get());
    if restante > 0 {
        RAFALE.with(|r| r.set(restante - 1));
        push_state();
    }
}

fn appliquer_le_premier_plan() {
    let Some(hwnd) = cef_popup::hwnd(&SLOT) else { return };
    let rang = if crate::config::load_bool_setting_or(PREMIER_PLAN, false) { Some(HWND_TOPMOST) } else { Some(HWND_NOTOPMOST) };
    unsafe {
        let _ = SetWindowPos(hwnd, rang, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Lance une demande sur un fil de travail (une seule à la fois).
fn lancer(demande: dns::Demande) {
    if EN_COURS.swap(true, Ordering::SeqCst) {
        crate::diag!("[dns] une application est déjà en cours");
        return;
    }
    crate::diag!("[dns] demande : {demande:?}");
    MESSAGE.with(|m| m.borrow_mut().clear());
    push_state();
    let lance = std::thread::Builder::new().name("dns-appliquer".into()).spawn(move || {
        let issue = dns::appliquer(&demande);
        if let Ok(mut i) = ISSUE.lock() {
            *i = Some(issue);
        }
    });
    if lance.is_err() {
        EN_COURS.store(false, Ordering::SeqCst);
        MESSAGE.with(|m| *m.borrow_mut() = dns::Issue::Echec.code().to_string());
        push_state();
    }
}

/// Une demande refusée par la validation : rien ne part, la page le dit.
fn refuser(quoi: &str) {
    crate::diag!("[dns] demande refusée : {quoi}");
    MESSAGE.with(|m| *m.borrow_mut() = "invalide".to_string());
    push_state();
}

/// L'`id` de la section d'`aide.html` (et `aide.en.html`) que « Voir l'aide » ouvre.
const AIDE_ANCRE: &str = "dns-local";

/// **CE QUE LA PAGE DEMANDE** : les trois gestes de la fenêtre, plus
/// `appliquer/<cartes>/<auto|adresses>/<0|1>`, `ip/…`, `ip6/…`, `connexion/…`,
/// `mac/…`, `proxy/<json>`, `vider` et `actualiser`.
pub fn dispatch_actions() {
    for action in cef_popup::drain_actions(&SLOT) {
        let Some(verbe) = action.strip_prefix("dns/") else { continue };
        let mut morceaux = verbe.splitn(3, '/');
        match morceaux.next().unwrap_or("") {
            "deplacer" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::deplacer_par_le_systeme(hwnd, "dns");
                }
            }
            "fermer" => {
                if let Some(hwnd) = cef_popup::hwnd(&SLOT) {
                    cef_popup::cacher_fenetre(hwnd);
                }
            }
            "epingler" => {
                let actif = !crate::config::load_bool_setting_or(PREMIER_PLAN, false);
                crate::config::save_bool_setting(PREMIER_PLAN, actif);
                appliquer_le_premier_plan();
                push_state();
            }
            // La page est prête : l'état poussé AVANT son script (à
            // l'ouverture) a été perdu — oublier le dernier envoi pour le
            // renvoyer en entier.
            "ready" => {
                PAYLOAD.with(|p| *p.borrow_mut() = None);
                push_state();
            }
            "actualiser" => {
                relire();
                MESSAGE.with(|m| m.borrow_mut().clear());
                push_state();
            }
            "vider" => lancer(dns::Demande::ViderCache),
            // « Voir l'aide » : la section d'`aide.html` sur Unbound et AdGuard.
            "aide" => crate::win32::cef_aide::demander(AIDE_ANCRE),
            "appliquer" => {
                let cartes = morceaux.next().unwrap_or("");
                // `<quoi>/<0|1>` : l'adresse encodée ne contient jamais de `/`.
                let (quoi, chiffrer) = morceaux.next().unwrap_or("").split_once('/').unwrap_or(("", ""));
                // Relue AVANT de valider : une carte débranchée entre-temps est refusée.
                relire();
                let connues: Vec<dns::Cible> =
                    CARTES.with(|c| c.borrow().iter().map(|carte| dns::Cible { index: carte.index, guid: carte.guid.clone() }).collect());
                match dns::lire_demande(cartes, &decoder(quoi), chiffrer, &connues) {
                    Some(demande) => lancer(demande),
                    None => refuser(&format!("{cartes:?} {quoi:?} {chiffrer:?}")),
                }
            }
            "ip6" => {
                let (carte, quoi) = (morceaux.next().unwrap_or(""), morceaux.next().unwrap_or(""));
                relire();
                let demande = CARTES.with(|c| {
                    let cartes = c.borrow();
                    let connues: Vec<reseau_ip::CarteConnue> = cartes
                        .iter()
                        .map(|carte| reseau_ip::CarteConnue { index: carte.index, serveurs: &carte.serveurs, dns_manuel: carte.manuel })
                        .collect();
                    reseau_ip::lire_demande6(carte, &decoder(quoi), &connues)
                });
                match demande {
                    Some(demande) => lancer(dns::Demande::Ip(demande)),
                    None => refuser(&format!("ip6 {carte:?} {quoi:?}")),
                }
            }
            "connexion" => {
                let (carte, quoi) = (morceaux.next().unwrap_or(""), morceaux.next().unwrap_or(""));
                relire();
                let demande = CARTES.with(|c| {
                    let cartes = c.borrow();
                    let connues: Vec<reseau_connexion::CarteConnue> = cartes
                        .iter()
                        .map(|carte| reseau_connexion::CarteConnue {
                            index: carte.index,
                            guid: &carte.guid,
                            type_if: carte.type_if,
                            categorie: carte.connexion.and_then(|e| e.categorie),
                            dhcp: carte.dhcp,
                        })
                        .collect();
                    reseau_connexion::lire_demande(carte, quoi, &connues)
                });
                match demande {
                    Some(demande) => lancer(dns::Demande::Connexion(demande)),
                    None => refuser(&format!("connexion {carte:?} {quoi:?}")),
                }
            }
            "mac" => {
                let (carte, quoi) = (morceaux.next().unwrap_or(""), morceaux.next().unwrap_or(""));
                relire();
                let guid = CARTES.with(|c| {
                    let index: Option<u32> = carte.trim().parse().ok();
                    c.borrow().iter().find(|x| Some(x.index) == index).map(|x| x.guid.clone())
                });
                match guid.and_then(|g| reseau_mac::lire_demande(&g, &decoder(quoi))) {
                    Some(demande) => lancer(dns::Demande::Mac(demande)),
                    None => refuser(&format!("mac {carte:?} {quoi:?}")),
                }
            }
            "proxy" => {
                // Le reste ENTIER : le JSON encodé peut contenir des `/` encodés.
                let reste = [morceaux.next().unwrap_or(""), morceaux.next().unwrap_or("")].iter().filter(|m| !m.is_empty()).copied().collect::<Vec<_>>().join("/");
                match reseau_proxy::lire_demande(&decoder(&reste)) {
                    Some(proxy) => {
                        // Vider d'abord : deux succes de suite donneraient le
                        // MEME etat, que `push_state` ne renverrait pas.
                        MESSAGE.with(|m| m.borrow_mut().clear());
                        push_state();
                        let reussi = reseau_proxy::appliquer(&proxy);
                        relire();
                        MESSAGE.with(|m| *m.borrow_mut() = if reussi { "reussi" } else { "echec" }.to_string());
                        push_state();
                    }
                    None => refuser("proxy"),
                }
            }
            "ip" => {
                let (carte, quoi) = (morceaux.next().unwrap_or(""), morceaux.next().unwrap_or(""));
                relire();
                let demande = CARTES.with(|c| {
                    let cartes = c.borrow();
                    let connues: Vec<reseau_ip::CarteConnue> = cartes
                        .iter()
                        .map(|carte| reseau_ip::CarteConnue { index: carte.index, serveurs: &carte.serveurs, dns_manuel: carte.manuel })
                        .collect();
                    reseau_ip::lire_demande(carte, &decoder(quoi), &connues)
                });
                match demande {
                    Some(demande) => lancer(dns::Demande::Ip(demande)),
                    None => {
                        crate::diag!("[dns] demande d'adresse refusée : {carte:?} {quoi:?}");
                        MESSAGE.with(|m| *m.borrow_mut() = "invalide".to_string());
                        push_state();
                    }
                }
            }
            autre => crate::diag!("[dns] verbe inconnu : {autre:?}"),
        }
    }
}

/// La page encode la liste (`encodeURIComponent`) : `:` et `,` y passent en
/// `%3A` et `%2C`. Un `%` mal formé donne une chaîne que `lire_demande` refuse.
fn decoder(s: &str) -> String {
    let octets = s.as_bytes();
    let mut sortie = Vec::with_capacity(octets.len());
    let mut i = 0;
    while i < octets.len() {
        if octets[i] == b'%' && i + 2 < octets.len() {
            // Lu en OCTETS : un caractère non ASCII après `%` ne coupe pas une chaîne.
            let hex = std::str::from_utf8(&octets[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                sortie.push(v);
                i += 3;
                continue;
            }
        }
        sortie.push(octets[i]);
        i += 1;
    }
    String::from_utf8_lossy(&sortie).into_owned()
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCHITTEST {
        let mut cadre = windows::Win32::Foundation::RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut cadre) }.is_ok() {
            let rect = [cadre.left, cadre.top, cadre.right - cadre.left, cadre.bottom - cadre.top];
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            return LRESULT(code_pour_point(rect, (x, y)) as isize);
        }
    }
    if msg == WM_CLOSE {
        cef_popup::cacher_fenetre(hwnd);
        return LRESULT(0);
    }
    if let Some(resultat) = cef_popup::dispatch_input(&SLOT, msg, wparam, lparam) {
        return resultat;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn code_pour_point(rect: [i32; 4], (x, y): (i32, i32)) -> i32 {
    const BORD: i32 = 12;
    let (gauche, haut) = (x < rect[0] + BORD, y < rect[1] + BORD);
    let (droite, bas) = (x >= rect[0] + rect[2] - BORD, y >= rect[1] + rect[3] - BORD);
    match (gauche, droite, haut, bas) {
        (true, _, true, _) => 13,
        (_, true, true, _) => 14,
        (true, _, _, true) => 16,
        (_, true, _, true) => 17,
        (true, _, _, _) => 10,
        (_, true, _, _) => 11,
        (_, _, true, _) => 12,
        (_, _, _, true) => 15,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verbes_connus() -> [&'static str; 13] {
        ["deplacer", "epingler", "fermer", "ready", "actualiser", "vider", "aide", "appliquer", "ip", "ip6", "connexion", "mac", "proxy"]
    }

    #[test]
    fn les_verbes_de_la_page_sont_exactement_ceux_du_dispatch() {
        let page = include_str!("../../assets/popups/dns.html");
        let mut vus: Vec<&str> = Vec::new();
        for morceau in page.split("go(\"").skip(1) {
            let verbe = morceau.split(['"', '/']).next().unwrap_or("");
            assert!(verbes_connus().contains(&verbe), "verbe inconnu passé à `go` : {verbe:?}");
            vus.push(verbe);
        }
        for attendu in verbes_connus() {
            assert!(vus.contains(&attendu), "la page n'appelle jamais `{attendu}`");
        }
        assert!(page.contains("neuradesk://dns/"), "la page ne parle pas à `dns/`");
    }

    /// Le renvoi de la page DNS tombe sur une section qui existe, dans les DEUX langues.
    #[test]
    fn le_renvoi_vers_l_aide_vise_une_section_existante() {
        let cible = format!("id=\"{AIDE_ANCRE}\"");
        for (nom, aide) in [
            ("aide.html", include_str!("../../assets/popups/aide.html")),
            ("aide.en.html", include_str!("../../assets/popups/aide.en.html")),
        ] {
            assert!(aide.contains(&cible), "{nom} n'a pas de section {cible}");
            for mot in ["Unbound", "AdGuard"] {
                assert!(aide.contains(mot), "{nom} ne parle pas de {mot}");
            }
        }
    }

    /// Quad9 n'est plus disponible en France : la page le masque en français, et
    /// ne peut le faire que si Rust lui envoie la langue.
    #[test]
    fn quad9_est_masque_en_francais() {
        let page = include_str!("../../assets/popups/dns.html");
        assert!(page.contains(r#"p.id === "quad9" && etat.langue === "fr""#), "le filtre de Quad9 a disparu de la page");
        assert!(include_str!("cef_dns.rs").contains(r#""langue": crate::ui::langue::courante().code()"#), "Rust n'envoie plus la langue");
        // Les adresses restent connues de Rust : une saisie à la main reste chiffrable.
        assert!(page.contains("id: \"quad9\""));
    }

    #[test]
    fn la_liste_encodee_se_decode() {
        assert_eq!(decoder("1.1.1.1%2C2606%3A4700%3A4700%3A%3A1111"), "1.1.1.1,2606:4700:4700::1111");
        assert_eq!(decoder("auto"), "auto");
        assert_eq!(decoder("1.1.1.1%"), "1.1.1.1%");
        assert_eq!(decoder("%zz"), "%zz");
        assert_eq!(decoder("%é1"), "%é1");
    }

    #[test]
    fn la_fenetre_tient_sur_l_ecran() {
        for (l_ecran, h_ecran) in [(3840, 2160), (2560, 1440), (1920, 1080), (1366, 768)] {
            let [l, h] = taille_pour_tout_le_contenu(l_ecran, h_ecran);
            assert!(l <= l_ecran * PART_ECRAN / 100, "déborde en largeur : {l} sur {l_ecran}");
            assert!(h <= h_ecran * PART_ECRAN / 100, "déborde en hauteur : {h} sur {h_ecran}");
            assert!(l > 0 && h > 0, "une taille positive");
        }
    }

    #[test]
    fn le_facteur_de_la_page_est_bien_celui_du_zoom() {
        assert_eq!(PX_PAR_UNITE as f64, cef_popup::CONTENT_ZOOM_FACTOR);
    }

    #[test]
    fn les_preglages_de_la_page_sont_des_adresses_valides() {
        let page = include_str!("../../assets/popups/dns.html");
        let debut = page.find("const PREREGLAGES = [").expect("aucun préréglage");
        let fin = debut + page[debut..].find("];").unwrap();
        let mut n = 0;
        for ligne in page[debut..fin].lines().filter(|l| l.contains("adresses:")) {
            let liste = ligne.split("adresses: [").nth(1).unwrap().split(']').next().unwrap();
            let liste = liste.replace(['"', ' '], "");
            let connue = [dns::Cible { index: 1, guid: "{01234567-89AB-CDEF-0123-456789ABCDEF}".into() }];
            assert!(dns::lire_demande("1", &liste, "0", &connue).is_some(), "préréglage invalide : {ligne}");
            // Chaque préréglage se chiffre : son modèle DoH est connu.
            assert!(dns::lire_demande("1", &liste, "1", &connue).is_some(), "préréglage sans modèle DoH : {ligne}");
            n += 1;
        }
        assert!(n >= 4, "au moins quatre préréglages, vu {n}");
    }
}
