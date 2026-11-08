//! **LE CANAL DES GROSSES DONNÉES, DE LA PAGE VERS RUST** (lot 0 de
//! `docs/plans/editeur-image.md`).
//!
//! Le canal historique (`location.href = "neuradesk://…"`, voir
//! `cef_app::NeuraDeskRequestHandler`) passe par une URL : limité à ~2 Mo par
//! Chromium, et deux navigations d'une même tâche se télescopent. Une image
//! 4K en PNG pèse 5 à 15 Mo — elle ne passe pas.
//!
//! Ici la page fait un vrai `fetch(POST)` vers [`PREFIXE`] ; la requête
//! n'atteint jamais le réseau : `cef_app` la détourne vers [`gestionnaire`],
//! qui lit le corps, le dépose dans une boîte aux lettres, et répond
//! `{"taille":n,"empreinte":"xxxxxxxx"}` pour que la page vérifie que Rust a
//! reçu **exactement** ce qu'elle a envoyé.
//!
//! ⚠️ **TOUT CE FICHIER TOURNE HORS DU FIL UI** (fil IO de CEF pour le
//! choix du gestionnaire, fil de travail pour `open`/`read`). D'où une
//! boîte aux lettres **statique sous `Mutex`**, jamais le
//! `Rc<RefCell<RenderState>>` des popups.
//!
//! ⚠️ Côté page, le corps doit être un `ArrayBuffer`/`Uint8Array`, **jamais un
//! `Blob`** : Chromium transmet un `Blob` comme un flux de données que CEF
//! présente comme un élément vide.

use std::sync::{Arc, Mutex};

use cef::*;

/// Préfixe détourné. `.invalid` (RFC 2606) ne peut jamais être résolu : si le
/// détournement cessait de fonctionner, la requête échouerait au lieu de
/// partir sur le réseau.
pub const PREFIXE: &str = "http://neuradesk-data.invalid/";

/// Route réservée à la sonde du lot 0 : le verdict de la page, en texte, est
/// seulement journalisé.
const ROUTE_VERDICT: &str = "sonde/verdict";

/// Dernier corps reçu PAR ROUTE — un nouvel envoi sur la même route remplace
/// l'ancien, pour que la boîte ne grossisse jamais sans borne si personne ne
/// la vide.
static BOITE: Mutex<Vec<(String, Vec<u8>)>> = Mutex::new(Vec::new());

/// FNV-1a 32 bits — la même empreinte se calcule en quatre lignes de JS
/// (`Math.imul`), ce qui permet à la page de comparer sans dépendance.
pub fn fnv1a32(octets: &[u8]) -> u32 {
    octets.iter().fold(0x811c_9dc5u32, |h, &o| (h ^ o as u32).wrapping_mul(0x0100_0193))
}

fn deposer(route: &str, corps: Vec<u8>) {
    let mut boite = BOITE.lock().unwrap_or_else(|e| e.into_inner());
    boite.retain(|(r, _)| r != route);
    boite.push((route.to_string(), corps));
}

/// ⭐ **LES FICHIERS SERVIS À LA PAGE, DANS L'AUTRE SENS** — DÉFAUT SIGNALÉ
/// (2026-10-19) : « dans le dash dossier je ne peux pas afficher toutes les
/// images » — les GROSSES (11 Mo en médiane dans le dossier de l'utilisateur,
/// 297 Mo au plus). La visionneuse les poussait en `data:` dans son état, par
/// `execute_js`, soixante fois de suite : 15 Mo de texte par poussée. Une page lit
/// maintenant `GET <PREFIXE>fichier/<n>` (une balise `<img>` suffit), et seuls
/// les fichiers PUBLIÉS ici se lisent — par leur numéro, jamais par leur chemin.
/// Les `FICHIERS_MAX` derniers publiés restent lisibles.
static FICHIERS: Mutex<(u64, Vec<(u64, std::path::PathBuf, &'static str)>)> = Mutex::new((0, Vec::new()));

/// Assez pour les quatre visionneuses (voir `cef_visionneuse::CRENEAUX`), et une
/// page qui relit encore l'ancienne image pendant qu'on publie la suivante.
const FICHIERS_MAX: usize = 8;

/// Publie `chemin` (de type `mime`) pour la page : rend l'URL où elle le lira.
pub fn publier(chemin: &std::path::Path, mime: &'static str) -> String {
    let mut fichiers = FICHIERS.lock().unwrap_or_else(|e| e.into_inner());
    fichiers.0 += 1;
    let numero = fichiers.0;
    fichiers.1.push((numero, chemin.to_path_buf(), mime));
    let trop = fichiers.1.len().saturating_sub(FICHIERS_MAX);
    fichiers.1.drain(..trop);
    format!("{PREFIXE}fichier/{numero}")
}

/// Le fichier publié sous la route `fichier/<n>`, et son type.
fn publie(route: &str) -> Option<(std::path::PathBuf, &'static str)> {
    let numero: u64 = route.strip_prefix("fichier/")?.parse().ok()?;
    let fichiers = FICHIERS.lock().unwrap_or_else(|e| e.into_inner());
    fichiers.1.iter().find(|(n, _, _)| *n == numero).map(|(_, chemin, mime)| (chemin.clone(), *mime))
}

/// Retire le dernier corps reçu sur `route`, s'il y en a un.
pub fn prendre(route: &str) -> Option<Vec<u8>> {
    let mut boite = BOITE.lock().unwrap_or_else(|e| e.into_inner());
    let i = boite.iter().position(|(r, _)| r == route)?;
    Some(boite.remove(i).1)
}

/// Les routes `ssh/…` de l'applet SSH (voir `win32::cef_ssh`). `true` si la
/// route en était une.
///
/// - `ssh/ecrire/<onglet>` : des FRAPPES, remises tout de suite au client
///   (`ssh::ecrire` est sûr hors du fil UI) — jamais dans la boîte, qui
///   n'en garderait que la dernière ;
/// - les autres (`ssh/fiche`, `ssh/secret/<onglet>`) vont dans la boîte, où
///   `cef_ssh` les prend à l'action qui suit.
///
/// ⛔ **RIEN AU JOURNAL**, pas même la taille ou l'empreinte : une frappe
/// peut être un mot de passe tapé dans le terminal, et l'empreinte FNV d'un
/// secret court se retrouve par force brute.
///
/// Rend le corps (`Err`) quand la route n'est pas celle de l'applet.
fn route_ssh(route: &str, corps: Vec<u8>) -> Result<(), Vec<u8>> {
    let Some(reste) = route.strip_prefix("ssh/") else { return Err(corps) };
    match reste.strip_prefix("ecrire/") {
        Some(onglet) => {
            if let Ok(onglet) = onglet.parse() {
                crate::win32::ssh::ecrire(onglet, corps);
            }
        }
        None => deposer(route, corps),
    }
    Ok(())
}

/// Les routes `terminal/…` du panneau Terminal (voir `win32::cef_terminal`) —
/// même traitement que [`route_ssh`] : `terminal/ecrire/<onglet>` va tout de
/// suite au shell (`pty::ecrire`), le reste dans la boîte. ⛔ RIEN AU JOURNAL,
/// pour la même raison.
fn route_terminal(route: &str, corps: Vec<u8>) -> Result<(), Vec<u8>> {
    let Some(reste) = route.strip_prefix("terminal/") else { return Err(corps) };
    match reste.strip_prefix("ecrire/") {
        Some(onglet) => {
            if let Ok(onglet) = onglet.parse() {
                crate::win32::pty::ecrire(onglet, corps);
            }
        }
        None => deposer(route, corps),
    }
    Ok(())
}

/// Les routes qui ne passent PAS par le journal (voir [`route_ssh`]).
fn route_muette(url_sans_prefixe: &str) -> bool {
    url_sans_prefixe.starts_with("ssh/") || url_sans_prefixe.starts_with("terminal/") || url_sans_prefixe.starts_with("fenetres/")
}

/// Concatène les éléments OCTETS du corps d'une requête. Un élément d'un
/// autre type (fichier, vide) est compté à part : c'est le signe qu'on a
/// envoyé un `Blob` au lieu d'un `ArrayBuffer`.
fn lire_corps(request: &Request) -> (Vec<u8>, usize) {
    let Some(post) = request.post_data() else { return (Vec::new(), 0) };
    let mut elements = vec![None; post.element_count()];
    post.elements(Some(&mut elements));
    let mut corps = Vec::new();
    let mut ignores = 0;
    for element in elements.into_iter().flatten() {
        if element.get_type() != PostdataelementType::BYTES {
            ignores += 1;
            continue;
        }
        let debut = corps.len();
        corps.resize(debut + element.bytes_count(), 0);
        let lus = element.bytes(corps.len() - debut, corps[debut..].as_mut_ptr());
        corps.truncate(debut + lus);
    }
    (corps, ignores)
}

#[derive(Default)]
struct Reponse {
    statut: i32,
    corps: Vec<u8>,
    lu: usize,
    /// Le type du corps ; `None` : du JSON.
    mime: Option<&'static str>,
}

wrap_resource_handler! {
    struct ReponseCanal {
        reponse: Arc<Mutex<Reponse>>,
    }

    impl ResourceHandler {
        fn open(
            &self,
            request: Option<&mut Request>,
            handle_request: Option<&mut ::std::os::raw::c_int>,
            _callback: Option<&mut Callback>,
        ) -> ::std::os::raw::c_int {
            if let Some(handle_request) = handle_request {
                *handle_request = 1;
            }
            let Some(request) = request else { return 0 };
            let mut reponse = self.reponse.lock().unwrap_or_else(|e| e.into_inner());
            let methode = CefString::from(&request.method()).to_string();
            // Pré-vol CORS : la page est en `file://`, donc toute requête
            // non « simple » est précédée d'un OPTIONS.
            if methode == "OPTIONS" {
                crate::diag!("[canal] pré-vol OPTIONS");
                reponse.statut = 204;
                return 1;
            }
            let url = CefString::from(&request.url()).to_string();
            let route = url.strip_prefix(PREFIXE).unwrap_or(&url).trim_end_matches('/').to_string();
            // Un fichier publié (voir `publier`), lu ici, sur le fil de travail.
            if methode == "GET" {
                match publie(&route).map(|(chemin, mime)| (std::fs::read(&chemin), chemin, mime)) {
                    Some((Ok(octets), _, mime)) => {
                        crate::diag!("[canal] GET {route} : {} octets", octets.len());
                        reponse.statut = 200;
                        reponse.corps = octets;
                        reponse.mime = Some(mime);
                    }
                    Some((Err(erreur), chemin, _)) => {
                        crate::diag!("[canal] GET {route} : {} illisible : {erreur}", chemin.display());
                        reponse.statut = 404;
                    }
                    None => {
                        crate::diag!("[canal] GET {route} : rien de publié sous ce numéro");
                        reponse.statut = 404;
                    }
                }
                return 1;
            }
            let (corps, ignores) = lire_corps(request);
            // Applet SSH et Terminal : voir [`route_ssh`] — RIEN au journal.
            let corps = match route_ssh(&route, corps).or_else(|corps| route_terminal(&route, corps)) {
                Ok(()) => {
                    reponse.statut = 200;
                    reponse.corps = b"{}".to_vec();
                    return 1;
                }
                Err(corps) => corps,
            };
            let empreinte = fnv1a32(&corps);
            if route == ROUTE_VERDICT {
                crate::diag!("[canal] verdict de la page : {}", String::from_utf8_lossy(&corps));
            } else if !route_muette(&route) {
                // Les rectangles de la sphère des fenêtres partent à chaque
                // image d'un geste : pas une ligne par image.
                crate::diag!(
                    "[canal] {methode} {route} : {} octets, empreinte {empreinte:08x}, éléments non-octets {ignores}",
                    corps.len()
                );
            }
            let taille = corps.len();
            if route != ROUTE_VERDICT {
                deposer(&route, corps);
            }
            reponse.statut = 200;
            reponse.corps = format!("{{\"taille\":{taille},\"empreinte\":\"{empreinte:08x}\",\"ignores\":{ignores}}}").into_bytes();
            1
        }

        fn response_headers(
            &self,
            response: Option<&mut Response>,
            response_length: Option<&mut i64>,
            _redirect_url: Option<&mut CefString>,
        ) {
            let reponse = self.reponse.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(response) = response {
                response.set_status(reponse.statut);
                response.set_mime_type(Some(&CefString::from(reponse.mime.unwrap_or("application/json"))));
                let mut entetes = CefStringMultimap::new();
                entetes.append("Access-Control-Allow-Origin", "*");
                entetes.append("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
                entetes.append("Access-Control-Allow-Headers", "Content-Type");
                response.set_header_map(Some(&mut entetes));
            }
            if let Some(response_length) = response_length {
                *response_length = reponse.corps.len() as i64;
            }
        }

        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        fn read(
            &self,
            data_out: *mut u8,
            bytes_to_read: ::std::os::raw::c_int,
            bytes_read: Option<&mut ::std::os::raw::c_int>,
            _callback: Option<&mut ResourceReadCallback>,
        ) -> ::std::os::raw::c_int {
            let Some(bytes_read) = bytes_read else { return 0 };
            let mut reponse = self.reponse.lock().unwrap_or_else(|e| e.into_inner());
            let reste = reponse.corps.len() - reponse.lu;
            let n = reste.min(bytes_to_read.max(0) as usize);
            *bytes_read = n as _;
            if n == 0 {
                return 0;
            }
            // SAFETY : CEF garantit `data_out` valide pour `bytes_to_read` octets.
            unsafe { std::ptr::copy_nonoverlapping(reponse.corps[reponse.lu..].as_ptr(), data_out, n) };
            reponse.lu += n;
            1
        }
    }
}

wrap_resource_request_handler! {
    struct DetournementCanal {
        _rien: (),
    }

    impl ResourceRequestHandler {
        /// ⛔ **OBLIGATOIRE** : l'implémentation par défaut de la crate rend
        /// `ReturnValue::default()`, qui vaut **`RV_CANCEL`** — sans cette
        /// surcharge, chaque requête détournée est annulée avant `open`, et la
        /// page ne voit qu'un « Failed to fetch ». Mesuré.
        fn on_before_resource_load(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _callback: Option<&mut Callback>,
        ) -> ReturnValue {
            ReturnValue::CONTINUE
        }

        fn resource_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
        ) -> Option<ResourceHandler> {
            Some(ReponseCanal::new(Arc::new(Mutex::new(Reponse::default()))))
        }
    }
}

/// Le gestionnaire à rendre depuis `RequestHandler::resource_request_handler`
/// pour une URL du canal, `None` pour tout le reste (traitement normal).
pub fn gestionnaire(url: &str) -> Option<ResourceRequestHandler> {
    if !url.starts_with(PREFIXE) {
        return None;
    }
    // Pas une ligne par frappe d'un terminal (voir `route_ssh`).
    if !route_muette(&url[PREFIXE.len()..]) {
        crate::diag!("[canal] détournement de {url}");
    }
    Some(DetournementCanal::new(()))
}

/// **LEVIER DE VÉRIFICATION** du lot 0 — le script que le harnais exécute dans
/// une page (`NEURADESK_DEBUG_SONDE_CANAL=1`, lu dans `app::run`) : il envoie
/// 1 Ko, 2 Mo, 20 Mo et 60 Mo, compare taille et empreinte, et renvoie son
/// verdict par le canal lui-même (journalisé avec `NEURADESK_DIAG=1`).
pub const SONDE_JS: &str = include_str!("../../assets/popups/sonde_canal.js");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a32_suit_les_vecteurs_de_reference() {
        assert_eq!(fnv1a32(b""), 0x811c_9dc5);
        assert_eq!(fnv1a32(b"a"), 0xe40c_292c);
        assert_eq!(fnv1a32(b"foobar"), 0xbf9c_f968);
    }

    /// ⭐ Seuls les fichiers PUBLIÉS se lisent, par leur numéro ; les plus anciens
    /// tombent au-delà de `FICHIERS_MAX`.
    #[test]
    fn seuls_les_fichiers_publies_se_lisent() {
        let url = publier(std::path::Path::new("C:/images/grande.png"), "image/png");
        let route = url.strip_prefix(PREFIXE).unwrap();
        assert_eq!(publie(route), Some((std::path::PathBuf::from("C:/images/grande.png"), "image/png")));
        assert_eq!(publie("fichier/C:/Windows/win.ini"), None, "jamais par un chemin");
        for _ in 0..FICHIERS_MAX {
            publier(std::path::Path::new("C:/autre.png"), "image/png");
        }
        assert_eq!(publie(route), None, "trop ancien");
    }

    #[test]
    fn la_boite_garde_un_seul_corps_par_route() {
        deposer("test/boite", vec![1]);
        deposer("test/boite", vec![2, 3]);
        assert_eq!(prendre("test/boite"), Some(vec![2, 3]));
        assert_eq!(prendre("test/boite"), None);
    }

    #[test]
    fn les_routes_ssh_sont_prises_a_part() {
        assert_eq!(route_ssh("editeur/image", vec![1]), Err(vec![1]));
        assert!(route_ssh("ssh/fiche", b"{}".to_vec()).is_ok());
        assert_eq!(prendre("ssh/fiche"), Some(b"{}".to_vec()));
        // Une frappe vers un onglet sans connexion : consommée, jamais déposée.
        assert!(route_ssh("ssh/ecrire/999999", b"ls\r".to_vec()).is_ok());
        assert_eq!(prendre("ssh/ecrire/999999"), None);
        assert_eq!(route_terminal("ssh/fiche", vec![2]), Err(vec![2]));
        assert!(route_terminal("terminal/copie", b"x".to_vec()).is_ok());
        assert_eq!(prendre("terminal/copie"), Some(b"x".to_vec()));
        assert!(route_terminal("terminal/ecrire/999999", b"dir\r".to_vec()).is_ok());
        assert_eq!(prendre("terminal/ecrire/999999"), None);
        assert!(route_muette("terminal/ecrire/1") && route_muette("ssh/fiche") && !route_muette("editeur/image"));
    }

    #[test]
    fn seules_les_urls_du_canal_sont_detournees() {
        assert!(!PREFIXE.is_empty());
        assert!(gestionnaire("file:///C:/x/power.html").is_none());
        assert!(gestionnaire("neuradesk://app/quit").is_none());
    }
}
