//! **LE PROXY** — onglet « Proxy » de l'applet « Modifier réglages réseau »
//! (voir `win32::cef_dns`).
//!
//! DEMANDE (2026-11-24) : « peux tu ajouter ces réglages dans la fenêtre de
//! l'applet ».
//!
//! Le proxy de Windows est un réglage DE L'UTILISATEUR (celui de Paramètres >
//! Réseau > Proxy) : WinINet le lit et l'écrit sans droit d'administrateur —
//! AUCUNE invite UAC, aucun PowerShell. `InternetSetOptionW` avec
//! `INTERNET_OPTION_PER_CONNECTION_OPTION` (connexion `NULL` = le réseau local),
//! puis `SETTINGS_CHANGED` et `REFRESH` pour que les applications le relisent.
//!
//! - Détection automatique, script de configuration (URL d'un PAC), serveur
//!   manuel `hôte:port`, exceptions séparées par `;`, « ne pas passer par le
//!   proxy pour les adresses locales » (`<local>` dans les exceptions).
//! - Rien n'atteint un interpréteur, mais tout est VALIDÉ quand même (la page
//!   n'est qu'une commodité) : un hôte, un port 1-65535, une URL http(s) sans
//!   espace, des exceptions faites de caractères d'adresse.

use windows::core::PWSTR;
use windows::Win32::Foundation::{GlobalFree, HGLOBAL};
use windows::Win32::Networking::WinInet::{
    InternetQueryOptionW, InternetSetOptionW, INTERNET_OPTION_PER_CONNECTION_OPTION, INTERNET_OPTION_REFRESH,
    INTERNET_OPTION_SETTINGS_CHANGED, INTERNET_PER_CONN, INTERNET_PER_CONN_AUTOCONFIG_URL, INTERNET_PER_CONN_FLAGS,
    INTERNET_PER_CONN_FLAGS_UI, INTERNET_PER_CONN_OPTIONW, INTERNET_PER_CONN_OPTION_LISTW, INTERNET_PER_CONN_PROXY_BYPASS,
    INTERNET_PER_CONN_PROXY_SERVER, PROXY_TYPE_AUTO_DETECT, PROXY_TYPE_AUTO_PROXY_URL, PROXY_TYPE_DIRECT, PROXY_TYPE_PROXY,
};

/// L'exception qui veut dire « les adresses locales ».
const LOCAL: &str = "<local>";
/// Les longueurs maximales acceptées (celles de la boîte de Windows).
const URL_MAX: usize = 2048;
const EXCEPTIONS_MAX: usize = 4096;

/// Les réglages du proxy de l'utilisateur.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Proxy {
    /// Détecter automatiquement les paramètres (WPAD).
    pub detection: bool,
    /// L'URL du script de configuration (vide = aucun).
    pub script: String,
    /// Utiliser le serveur `serveur` pour tout le reste.
    pub manuel: bool,
    /// `hôte:port`.
    pub serveur: String,
    /// Les exceptions, séparées par `;` (sans `<local>`).
    pub exceptions: String,
    /// Ne pas utiliser le proxy pour les adresses locales.
    pub local: bool,
}

impl Proxy {
    /// L'objet envoyé à la page (le projet n'a pas `serde` : `json!` à la main).
    pub fn en_json(&self) -> serde_json::Value {
        serde_json::json!({
            "detection": self.detection,
            "script": self.script,
            "manuel": self.manuel,
            "serveur": self.serveur,
            "exceptions": self.exceptions,
            "local": self.local,
        })
    }

    /// Relu depuis la page : les SIX champs doivent être là et du bon type.
    fn de_json(texte: &str) -> Option<Proxy> {
        let v: serde_json::Value = serde_json::from_str(texte).ok()?;
        let booleen = |k: &str| v.get(k)?.as_bool();
        let chaine = |k: &str| v.get(k)?.as_str().map(str::to_string);
        Some(Proxy {
            detection: booleen("detection")?,
            script: chaine("script")?,
            manuel: booleen("manuel")?,
            serveur: chaine("serveur")?,
            exceptions: chaine("exceptions")?,
            local: booleen("local")?,
        })
    }
}

/// Lit les réglages actuels. Une erreur WinINet rend les réglages par défaut.
pub fn lire() -> Proxy {
    let mut options = [
        option(INTERNET_PER_CONN(INTERNET_PER_CONN_FLAGS_UI)),
        option(INTERNET_PER_CONN_PROXY_SERVER),
        option(INTERNET_PER_CONN_PROXY_BYPASS),
        option(INTERNET_PER_CONN_AUTOCONFIG_URL),
    ];
    let mut liste = liste(&mut options);
    let mut taille = std::mem::size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32;
    let lu = unsafe {
        InternetQueryOptionW(None, INTERNET_OPTION_PER_CONNECTION_OPTION, Some(&mut liste as *mut _ as *mut _), &mut taille)
    };
    if lu.is_err() {
        // `FLAGS_UI` n'existe pas avant Windows 7 : jamais ici, mais sans lui
        // on ne lirait rien du tout.
        crate::diag!("[proxy] lecture : {lu:?}");
        return Proxy::default();
    }
    let drapeaux = unsafe { options[0].Value.dwValue };
    let [serveur, exceptions, script] = [1, 2, 3].map(|i| unsafe { prendre_texte(options[i].Value.pszValue) });
    let mut local = false;
    let exceptions: Vec<&str> = exceptions
        .split(';')
        .map(str::trim)
        .filter(|e| {
            if e.eq_ignore_ascii_case(LOCAL) {
                local = true;
            }
            !e.is_empty() && !e.eq_ignore_ascii_case(LOCAL)
        })
        .collect();
    Proxy {
        detection: drapeaux & PROXY_TYPE_AUTO_DETECT != 0,
        script: if drapeaux & PROXY_TYPE_AUTO_PROXY_URL != 0 { script } else { String::new() },
        manuel: drapeaux & PROXY_TYPE_PROXY != 0,
        serveur,
        exceptions: exceptions.join(";"),
        local,
    }
}

fn option(quoi: INTERNET_PER_CONN) -> INTERNET_PER_CONN_OPTIONW {
    INTERNET_PER_CONN_OPTIONW { dwOption: quoi, ..Default::default() }
}

fn liste(options: &mut [INTERNET_PER_CONN_OPTIONW]) -> INTERNET_PER_CONN_OPTION_LISTW {
    INTERNET_PER_CONN_OPTION_LISTW {
        dwSize: std::mem::size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
        pszConnection: PWSTR::null(),
        dwOptionCount: options.len() as u32,
        dwOptionError: 0,
        pOptions: options.as_mut_ptr(),
    }
}

/// Copie une chaîne rendue par WinINet puis la LIBÈRE (`GlobalFree`).
unsafe fn prendre_texte(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    let texte = unsafe { p.to_string() }.unwrap_or_default();
    let _ = unsafe { GlobalFree(Some(HGLOBAL(p.0 as *mut _))) };
    texte
}

/// Un hôte : un nom (lettres, chiffres, `-`, `.`), une IPv4, ou `[IPv6]`.
fn est_un_hote(h: &str) -> bool {
    if let Some(v6) = h.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        return v6.parse::<std::net::Ipv6Addr>().is_ok();
    }
    !h.is_empty()
        && h.len() <= 253
        && h.split('.').all(|e| !e.is_empty() && e.len() <= 63 && !e.starts_with('-') && !e.ends_with('-') && e.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

/// `hôte:port`, port 1-65535.
pub fn serveur_valide(s: &str) -> bool {
    let Some((hote, port)) = s.rsplit_once(':') else { return false };
    est_un_hote(hote) && port.parse::<u16>().is_ok_and(|p| p > 0) && !port.starts_with('+')
}

/// Une URL `http(s)://…` sans espace ni caractère de contrôle.
pub fn script_valide(s: &str) -> bool {
    let reste = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://"));
    s.len() <= URL_MAX
        && reste.is_some_and(|r| {
            let hote = r.split(['/', '?', '#']).next().unwrap_or("");
            let hote = hote.rsplit_once(':').map_or(hote, |(h, p)| if p.chars().all(|c| c.is_ascii_digit()) { h } else { hote });
            est_un_hote(hote)
        })
        && s.chars().all(|c| c.is_ascii_graphic())
}

/// Des exceptions : des entrées faites de caractères d'adresse et de `*`.
pub fn exceptions_valides(s: &str) -> bool {
    s.len() <= EXCEPTIONS_MAX
        && s.split(';').map(str::trim).filter(|e| !e.is_empty()).all(|e| {
            e.len() <= 255 && e.chars().all(|c| c.is_ascii_alphanumeric() || ".-*:[]_".contains(c))
        })
}

/// **LIRE CE QUE LA PAGE DEMANDE** : un JSON de `Proxy`, revalidé ici.
/// Les espaces autour des valeurs sont retirés ; les exceptions sont remises
/// en `a;b;c`.
pub fn lire_demande(json: &str) -> Option<Proxy> {
    let mut p = Proxy::de_json(json)?;
    p.script = p.script.trim().to_string();
    p.serveur = p.serveur.trim().to_string();
    p.exceptions = p.exceptions.split([';', ',', '\n', ' ']).map(str::trim).filter(|e| !e.is_empty()).collect::<Vec<_>>().join(";");
    if !p.script.is_empty() && !script_valide(&p.script) {
        return None;
    }
    if p.manuel && !serveur_valide(&p.serveur) {
        return None;
    }
    if !p.serveur.is_empty() && !serveur_valide(&p.serveur) {
        return None;
    }
    exceptions_valides(&p.exceptions).then_some(p)
}

/// Les drapeaux WinINet d'un réglage.
fn drapeaux(p: &Proxy) -> u32 {
    let mut d = PROXY_TYPE_DIRECT;
    if p.manuel {
        d |= PROXY_TYPE_PROXY;
    }
    if !p.script.is_empty() {
        d |= PROXY_TYPE_AUTO_PROXY_URL;
    }
    if p.detection {
        d |= PROXY_TYPE_AUTO_DETECT;
    }
    d
}

/// Les exceptions telles que WinINet les range (`<local>` à la fin).
fn exceptions_wininet(p: &Proxy) -> String {
    let mut e = p.exceptions.clone();
    if p.local {
        if !e.is_empty() {
            e.push(';');
        }
        e.push_str(LOCAL);
    }
    e
}

/// **APPLIQUER** — sans droit, immédiat. `true` si WinINet a tout accepté.
pub fn appliquer(p: &Proxy) -> bool {
    let large = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (mut serveur, mut exceptions, mut script) = (large(&p.serveur), large(&exceptions_wininet(p)), large(&p.script));
    let mut options = [option(INTERNET_PER_CONN_FLAGS), option(INTERNET_PER_CONN_PROXY_SERVER), option(INTERNET_PER_CONN_PROXY_BYPASS), option(INTERNET_PER_CONN_AUTOCONFIG_URL)];
    options[0].Value.dwValue = drapeaux(p);
    options[1].Value.pszValue = PWSTR(serveur.as_mut_ptr());
    options[2].Value.pszValue = PWSTR(exceptions.as_mut_ptr());
    options[3].Value.pszValue = PWSTR(script.as_mut_ptr());
    let liste = liste(&mut options);
    let taille = std::mem::size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32;
    let pose = unsafe { InternetSetOptionW(None, INTERNET_OPTION_PER_CONNECTION_OPTION, Some(&liste as *const _ as *const _), taille) };
    if let Err(e) = pose {
        crate::diag!("[proxy] écriture : {e:?}");
        return false;
    }
    unsafe {
        let _ = InternetSetOptionW(None, INTERNET_OPTION_SETTINGS_CHANGED, None, 0);
        let _ = InternetSetOptionW(None, INTERNET_OPTION_REFRESH, None, 0);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_serveur_est_un_hote_et_un_port() {
        for bon in ["proxy.exemple.fr:8080", "10.0.0.1:3128", "[2001:db8::1]:8080", "localhost:1"] {
            assert!(serveur_valide(bon), "refusé à tort : {bon}");
        }
        for faux in ["proxy.exemple.fr", "proxy:0", "proxy:65536", "proxy:+80", "pro xy:80", "a..b:80", "-a.b:80", ":80", "[zz]:80", "proxy:80;calc"] {
            assert!(!serveur_valide(faux), "accepté à tort : {faux}");
        }
    }

    #[test]
    fn un_script_est_une_url_http() {
        assert!(script_valide("http://wpad.exemple.fr/proxy.pac"));
        assert!(script_valide("https://exemple.fr:8443/conf.pac?v=2"));
        for faux in ["ftp://exemple.fr/p.pac", "http://", "http://exem ple.fr/p.pac", "file:///C:/p.pac", "exemple.fr/p.pac"] {
            assert!(!script_valide(faux), "accepté à tort : {faux}");
        }
    }

    #[test]
    fn une_demande_est_relue_et_remise_en_forme() {
        let p = lire_demande(r#"{"detection":true,"script":"","manuel":true,"serveur":" 10.0.0.1:3128 ","exceptions":"*.local, intranet ;10.*","local":true}"#)
            .unwrap();
        assert_eq!(p.serveur, "10.0.0.1:3128");
        assert_eq!(p.exceptions, "*.local;intranet;10.*");
        assert_eq!(exceptions_wininet(&p), "*.local;intranet;10.*;<local>");
        assert_eq!(drapeaux(&p), PROXY_TYPE_DIRECT | PROXY_TYPE_PROXY | PROXY_TYPE_AUTO_DETECT);
        // Manuel sans serveur, serveur faux, exception suspecte, JSON incomplet.
        for faux in [
            r#"{"detection":false,"script":"","manuel":true,"serveur":"","exceptions":"","local":false}"#,
            r#"{"detection":false,"script":"","manuel":false,"serveur":"x:99999","exceptions":"","local":false}"#,
            r#"{"detection":false,"script":"","manuel":false,"serveur":"","exceptions":"a<b>","local":false}"#,
            r#"{"detection":false,"script":"javascript:x","manuel":false,"serveur":"","exceptions":"","local":false}"#,
            r#"{"detection":false}"#,
        ] {
            assert_eq!(lire_demande(faux), None, "accepté à tort : {faux}");
        }
        let direct = lire_demande(r#"{"detection":false,"script":"","manuel":false,"serveur":"","exceptions":"","local":false}"#).unwrap();
        assert_eq!(drapeaux(&direct), PROXY_TYPE_DIRECT);
    }
}

#[cfg(test)]
mod sonde {
    /// Sonde manuelle : `cargo test -- reseau_proxy::sonde --ignored --nocapture`.
    #[test]
    #[ignore]
    fn lire_le_proxy() {
        println!("{:?}", super::lire());
    }

    /// ALLER-RETOUR : reecrit les reglages LUS ; rien ne doit changer. Verifie
    /// qu'ecrire `FLAGS` garde la detection automatique, lue par `FLAGS_UI`.
    #[test]
    #[ignore]
    fn reecrire_ce_qui_est_lu_ne_change_rien() {
        let avant = super::lire();
        assert!(super::appliquer(&avant));
        let apres = super::lire();
        println!("avant {avant:?}
apres {apres:?}");
        assert_eq!(apres, avant);
    }
}
