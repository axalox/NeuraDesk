//! ⭐ LE CONVERTISSEUR DE LA CALCULATRICE — DEMANDE EXPLICITE (2026-10-24) :
//! « convertisseur d'unités et de devises […] sur la Calculatrice
//! (« 12 in en cm ») ».
//!
//! `<expression> <unité> en <unité>` : l'expression passe par l'évaluateur de
//! la calculatrice (`12*3 ft en m` marche), l'unité source se lit à la fin de la
//! partie gauche (ou en tête pour un symbole monétaire : `$100 en €`), et le
//! résultat s'écrit avec l'unité cible : `30,48 cm`. Séparateurs : `en`,
//! `vers`, `to`, `->`, `→`, `=>`. ⚠️ **PAS `in`** : c'est le POUCE, et
//! `12 in en cm` doit rester lisible.
//!
//! Une expression sans séparateur n'est PAS une conversion (`None`) : la
//! calculatrice l'évalue comme avant, rien ne change pour elle.
//!
//! LES DEVISES : les taux de référence de la BCE, par l'API publique et sans
//! clé de Frankfurter (`api.frankfurter.dev`), mis en cache dans
//! `devises.json` et rafraîchis en arrière-plan au-delà de `FRAICHEUR`.
//! ⚠️ **La première conversion de devises d'une machine n'a pas encore de
//! taux** : elle lance le téléchargement et répond une erreur qui le dit —
//! on n'attend JAMAIS le réseau sur le fil de l'interface.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::win32::calculatrice::{evaluer, formater, Angle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grandeur {
    Longueur,
    Masse,
    Volume,
    Surface,
    Vitesse,
    Duree,
    Donnees,
    Energie,
    Pression,
    Puissance,
    Angle,
    Temperature,
    Devise,
}

impl Grandeur {
    fn nom(self) -> &'static str {
        match self {
            Grandeur::Longueur => "une longueur",
            Grandeur::Masse => "une masse",
            Grandeur::Volume => "un volume",
            Grandeur::Surface => "une surface",
            Grandeur::Vitesse => "une vitesse",
            Grandeur::Duree => "une durée",
            Grandeur::Donnees => "une quantité de données",
            Grandeur::Energie => "une énergie",
            Grandeur::Pression => "une pression",
            Grandeur::Puissance => "une puissance",
            Grandeur::Angle => "un angle",
            Grandeur::Temperature => "une température",
            Grandeur::Devise => "une somme d'argent",
        }
    }
}

/// `(alias, affichage, grandeur, facteur vers l'unité de base)`. Le premier
/// alias en minuscules est celui qu'on tape ; `affichage` est ce qui suit le
/// résultat. Les températures ont un facteur fictif (voir `temperature`), et
/// les devises n'en ont pas ici (voir `taux_en_euros`).
type Unite = (&'static [&'static str], &'static str, Grandeur, f64);

const UNITES: &[Unite] = &[
    // Longueur, en mètres.
    (&["nm", "nanomètre", "nanometre"], "nm", Grandeur::Longueur, 1e-9),
    (&["µm", "um", "micron", "micromètre", "micrometre"], "µm", Grandeur::Longueur, 1e-6),
    (&["mm", "millimètre", "millimetre"], "mm", Grandeur::Longueur, 1e-3),
    (&["cm", "centimètre", "centimetre"], "cm", Grandeur::Longueur, 1e-2),
    (&["dm", "décimètre", "decimetre"], "dm", Grandeur::Longueur, 0.1),
    (&["m", "mètre", "metre"], "m", Grandeur::Longueur, 1.0),
    (&["km", "kilomètre", "kilometre"], "km", Grandeur::Longueur, 1000.0),
    (&["in", "pouce", "\"", "inch", "inches"], "in", Grandeur::Longueur, 0.0254),
    (&["ft", "pied", "feet", "foot"], "ft", Grandeur::Longueur, 0.3048),
    (&["yd", "yard"], "yd", Grandeur::Longueur, 0.9144),
    (&["mi", "mile"], "mi", Grandeur::Longueur, 1609.344),
    (&["nmi", "mille marin", "milles marins"], "nmi", Grandeur::Longueur, 1852.0),
    // Masse, en kilogrammes.
    (&["mg", "milligramme"], "mg", Grandeur::Masse, 1e-6),
    (&["g", "gramme"], "g", Grandeur::Masse, 1e-3),
    (&["kg", "kilo", "kilogramme"], "kg", Grandeur::Masse, 1.0),
    (&["t", "tonne"], "t", Grandeur::Masse, 1000.0),
    (&["lb", "lbs", "livre", "pound"], "lb", Grandeur::Masse, 0.453_592_37),
    (&["oz", "once", "ounce"], "oz", Grandeur::Masse, 0.028_349_523_125),
    (&["st", "stone"], "st", Grandeur::Masse, 6.350_293_18),
    // Volume, en litres.
    (&["ml", "millilitre"], "ml", Grandeur::Volume, 1e-3),
    (&["cl", "centilitre"], "cl", Grandeur::Volume, 1e-2),
    (&["dl", "décilitre", "decilitre"], "dl", Grandeur::Volume, 0.1),
    (&["l", "litre"], "l", Grandeur::Volume, 1.0),
    (&["m3", "m³", "mètre cube", "metre cube"], "m³", Grandeur::Volume, 1000.0),
    (&["cm3", "cm³"], "cm³", Grandeur::Volume, 1e-3),
    (&["gal", "gallon"], "gal", Grandeur::Volume, 3.785_411_784),
    (&["floz", "fl oz"], "fl oz", Grandeur::Volume, 0.029_573_529_562_5),
    (&["cup", "tasse"], "tasse", Grandeur::Volume, 0.236_588_236_5),
    (&["pt", "pint", "pinte"], "pt", Grandeur::Volume, 0.473_176_473),
    // Surface, en mètres carrés.
    (&["mm2", "mm²"], "mm²", Grandeur::Surface, 1e-6),
    (&["cm2", "cm²"], "cm²", Grandeur::Surface, 1e-4),
    (&["m2", "m²"], "m²", Grandeur::Surface, 1.0),
    (&["km2", "km²"], "km²", Grandeur::Surface, 1e6),
    (&["a", "are"], "a", Grandeur::Surface, 100.0),
    (&["ha", "hectare"], "ha", Grandeur::Surface, 1e4),
    (&["acre", "ac"], "acre", Grandeur::Surface, 4046.856_422_4),
    (&["ft2", "ft²"], "ft²", Grandeur::Surface, 0.092_903_04),
    (&["in2", "in²"], "in²", Grandeur::Surface, 0.000_645_16),
    (&["mi2", "mi²"], "mi²", Grandeur::Surface, 2_589_988.110_336),
    // Vitesse, en mètres par seconde.
    (&["m/s"], "m/s", Grandeur::Vitesse, 1.0),
    (&["km/h", "kmh"], "km/h", Grandeur::Vitesse, 1.0 / 3.6),
    (&["mph"], "mph", Grandeur::Vitesse, 0.447_04),
    (&["kn", "nœud", "noeud", "kt"], "nœuds", Grandeur::Vitesse, 1852.0 / 3600.0),
    (&["ft/s"], "ft/s", Grandeur::Vitesse, 0.3048),
    // Durée, en secondes. L'année est julienne (365,25 j).
    (&["ms", "milliseconde"], "ms", Grandeur::Duree, 1e-3),
    (&["s", "sec", "seconde"], "s", Grandeur::Duree, 1.0),
    (&["min", "minute"], "min", Grandeur::Duree, 60.0),
    (&["h", "heure", "hr"], "h", Grandeur::Duree, 3600.0),
    (&["j", "jour", "d", "day"], "j", Grandeur::Duree, 86_400.0),
    (&["sem", "semaine", "week", "wk"], "semaines", Grandeur::Duree, 604_800.0),
    (&["an", "année", "annee", "year", "yr"], "ans", Grandeur::Duree, 31_557_600.0),
    // Données, en octets : préfixes décimaux (ko) ET binaires (Kio).
    (&["bit"], "bits", Grandeur::Donnees, 0.125),
    (&["o", "octet", "byte"], "o", Grandeur::Donnees, 1.0),
    (&["ko", "kb", "kilooctet"], "ko", Grandeur::Donnees, 1e3),
    (&["mo", "mb", "mégaoctet", "megaoctet"], "Mo", Grandeur::Donnees, 1e6),
    (&["go", "gb", "gigaoctet"], "Go", Grandeur::Donnees, 1e9),
    (&["to", "tb", "téraoctet", "teraoctet"], "To", Grandeur::Donnees, 1e12),
    (&["kio", "kib"], "Kio", Grandeur::Donnees, 1024.0),
    (&["mio", "mib"], "Mio", Grandeur::Donnees, 1_048_576.0),
    (&["gio", "gib"], "Gio", Grandeur::Donnees, 1_073_741_824.0),
    (&["tio", "tib"], "Tio", Grandeur::Donnees, 1_099_511_627_776.0),
    (&["kbit", "kbps"], "kbit", Grandeur::Donnees, 125.0),
    (&["mbit", "mbps"], "Mbit", Grandeur::Donnees, 125_000.0),
    (&["gbit", "gbps"], "Gbit", Grandeur::Donnees, 125_000_000.0),
    // Énergie, en joules. ⚠️ Pas de « j » nu : c'est le JOUR.
    (&["joule", "joules"], "J", Grandeur::Energie, 1.0),
    (&["kj", "kilojoule"], "kJ", Grandeur::Energie, 1e3),
    (&["cal", "calorie"], "cal", Grandeur::Energie, 4.184),
    (&["kcal", "kilocalorie"], "kcal", Grandeur::Energie, 4184.0),
    (&["wh"], "Wh", Grandeur::Energie, 3600.0),
    (&["kwh"], "kWh", Grandeur::Energie, 3.6e6),
    // Pression, en pascals.
    (&["pa", "pascal"], "Pa", Grandeur::Pression, 1.0),
    (&["hpa"], "hPa", Grandeur::Pression, 100.0),
    (&["kpa"], "kPa", Grandeur::Pression, 1000.0),
    (&["mbar"], "mbar", Grandeur::Pression, 100.0),
    (&["bar"], "bar", Grandeur::Pression, 1e5),
    (&["atm"], "atm", Grandeur::Pression, 101_325.0),
    (&["psi"], "psi", Grandeur::Pression, 6894.757_293_168),
    (&["mmhg"], "mmHg", Grandeur::Pression, 133.322_387_415),
    // Puissance, en watts.
    (&["w", "watt"], "W", Grandeur::Puissance, 1.0),
    (&["kw", "kilowatt"], "kW", Grandeur::Puissance, 1000.0),
    (&["ch", "cv", "cheval"], "ch", Grandeur::Puissance, 735.498_75),
    (&["hp"], "hp", Grandeur::Puissance, 745.699_871_582_270_2),
    // Angle, en degrés.
    (&["deg", "°", "degré", "degre"], "°", Grandeur::Angle, 1.0),
    (&["rad", "radian"], "rad", Grandeur::Angle, 180.0 / std::f64::consts::PI),
    (&["grad", "gon"], "grad", Grandeur::Angle, 0.9),
    (&["tour", "tr"], "tours", Grandeur::Angle, 360.0),
    // Température : le facteur dit QUELLE échelle (voir `temperature`).
    (&["°c", "c", "celsius"], "°C", Grandeur::Temperature, 0.0),
    (&["°f", "f", "fahrenheit"], "°F", Grandeur::Temperature, 1.0),
    (&["k", "kelvin"], "K", Grandeur::Temperature, 2.0),
];

/// Les devises : `(alias, code ISO)`. Le code ISO lui-même est aussi reconnu
/// (`usd`, `jpy`…), s'il figure dans les taux.
const DEVISES: &[(&[&str], &str)] = &[
    (&["€", "euro"], "EUR"),
    (&["$", "dollar", "us$"], "USD"),
    (&["£", "sterling"], "GBP"),
    (&["¥", "yen"], "JPY"),
    (&["₹", "roupie"], "INR"),
    (&["franc suisse", "franc"], "CHF"),
    (&["yuan", "renminbi"], "CNY"),
    (&["c$", "dollar canadien"], "CAD"),
    (&["a$", "dollar australien"], "AUD"),
    (&["₩", "won"], "KRW"),
    (&["zł", "zloty"], "PLN"),
    (&["réal", "real"], "BRL"),
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Sorte {
    Unite(&'static Unite),
    Devise(&'static str),
}

/// Reconnaît une unité tapée : minuscules, espaces bordiers retirés, et un
/// pluriel en « s » toléré sur les noms longs (« pieds », « miles »).
fn reconnaitre(texte: &str, codes_connus: &dyn Fn(&str) -> bool) -> Option<Sorte> {
    let t = texte.trim().to_lowercase();
    if t.is_empty() {
        return None;
    }
    let essais = [t.clone(), t.strip_suffix('s').filter(|r| r.chars().count() >= 3).unwrap_or("").to_string()];
    for essai in essais.iter().filter(|e| !e.is_empty()) {
        if let Some(u) = UNITES.iter().find(|u| u.0.contains(&essai.as_str())) {
            return Some(Sorte::Unite(u));
        }
        if let Some((_, code)) = DEVISES.iter().find(|(alias, _)| alias.contains(&essai.as_str())) {
            return Some(Sorte::Devise(code));
        }
        let code = essai.to_uppercase();
        if code.len() == 3 && code.chars().all(|c| c.is_ascii_alphabetic()) && (code == "EUR" || codes_connus(&code)) {
            return Some(Sorte::Devise(DEVISES_ISO.iter().find(|c| **c == code).copied().unwrap_or("EUR")));
        }
    }
    None
}

/// Les codes ISO que la BCE publie (plus l'euro), en `&'static str` pour que
/// `Sorte` reste `Copy`.
const DEVISES_ISO: &[&str] = &[
    "EUR", "AUD", "BRL", "CAD", "CHF", "CNY", "CZK", "DKK", "GBP", "HKD", "HUF", "IDR", "ILS", "INR", "ISK", "JPY", "KRW",
    "MXN", "MYR", "NOK", "NZD", "PHP", "PLN", "RON", "SEK", "SGD", "THB", "TRY", "USD", "ZAR",
];

const SEPARATEURS: &[&str] = &[" en ", " vers ", " to ", "->", "→", "=>"];

/// Le DERNIER séparateur du texte — `(début, fin)` en octets.
fn separateur(texte: &str) -> Option<(usize, usize)> {
    let bas = texte.to_lowercase();
    // `to_lowercase` peut changer la longueur en octets de caractères exotiques :
    // on ne cherche dans la version minuscule que si elle a la même longueur.
    let cible = if bas.len() == texte.len() { bas.as_str() } else { texte };
    SEPARATEURS
        .iter()
        .filter_map(|s| cible.rfind(s).map(|i| (i, i + s.len())))
        .max_by_key(|(i, _)| *i)
}

/// Celsius ↔ Fahrenheit ↔ Kelvin, par le Celsius.
fn temperature(valeur: f64, de: f64, vers: f64) -> f64 {
    let celsius = match de as u8 {
        1 => (valeur - 32.0) * 5.0 / 9.0,
        2 => valeur - 273.15,
        _ => valeur,
    };
    match vers as u8 {
        1 => celsius * 9.0 / 5.0 + 32.0,
        2 => celsius + 273.15,
        _ => celsius,
    }
}

/// Une conversion, si `texte` en est une : la valeur et le texte affiché.
pub fn convertir(texte: &str, angle: Angle, ans: f64) -> Option<Result<(f64, String), String>> {
    convertir_avec(texte, angle, ans, &|code| taux_en_euros(code))
}

/// `taux(code)` : combien vaut UNE unité de cette devise, en euros — ou
/// l'erreur à montrer (taux pas encore téléchargés, devise inconnue).
fn convertir_avec(
    texte: &str,
    angle: Angle,
    ans: f64,
    taux: &dyn Fn(&str) -> Result<f64, String>,
) -> Option<Result<(f64, String), String>> {
    let (debut, fin) = separateur(texte)?;
    let gauche = texte[..debut].trim();
    let droite = texte[fin..].trim();
    let connus = |code: &str| DEVISES_ISO.contains(&code);
    let cible = reconnaitre(droite, &connus)?;
    let (quantite, source) = lire_gauche(gauche, angle, ans, &connus)?;
    let quantite = match quantite {
        Ok(v) => v,
        Err(e) => return Some(Err(e)),
    };
    Some(appliquer(quantite, source, cible, taux))
}

/// La partie gauche : `(valeur, unité)`. Le symbole monétaire peut PRÉCÉDER
/// (`$100`) ; sinon l'unité est le plus long suffixe reconnu dont le reste
/// s'évalue. Sans nombre (`km en m`), la valeur est 1.
fn lire_gauche(
    gauche: &str,
    angle: Angle,
    ans: f64,
    connus: &dyn Fn(&str) -> bool,
) -> Option<(Result<f64, String>, Sorte)> {
    if let Some(sorte) = reconnaitre(gauche, connus) {
        return Some((Ok(1.0), sorte));
    }
    let bornes: Vec<usize> = gauche.char_indices().map(|(i, _)| i).skip(1).collect();
    for &i in &bornes {
        let (avant, apres) = gauche.split_at(i);
        // Le symbole en tête : `$100`, `€ 12,5`.
        if let Some(sorte @ Sorte::Devise(_)) = reconnaitre(avant, connus) {
            if let Ok(v) = evaluer(apres, angle, ans) {
                return Some((Ok(v), sorte));
            }
        }
    }
    // Le plus long suffixe d'abord : `5 km/h` avant `h`.
    for &i in &bornes {
        let (avant, apres) = gauche.split_at(i);
        if avant.trim().is_empty() {
            continue;
        }
        if let Some(sorte) = reconnaitre(apres, connus) {
            if let Ok(v) = evaluer(avant, angle, ans) {
                return Some((Ok(v), sorte));
            }
        }
    }
    // Une unité reconnue à droite mais une gauche qu'on ne lit pas : l'erreur
    // de l'évaluateur sur le texte entier en dit plus que « pas une conversion ».
    None
}

fn appliquer(
    valeur: f64,
    source: Sorte,
    cible: Sorte,
    taux: &dyn Fn(&str) -> Result<f64, String>,
) -> Result<(f64, String), String> {
    match (source, cible) {
        (Sorte::Devise(de), Sorte::Devise(vers)) => {
            let resultat = valeur * taux(de)? / taux(vers)?;
            Ok((resultat, format!("{} {vers}", formater_monnaie(resultat))))
        }
        (Sorte::Unite(de), Sorte::Unite(vers)) if de.2 == vers.2 => {
            let resultat = if de.2 == Grandeur::Temperature {
                temperature(valeur, de.3, vers.3)
            } else {
                valeur * de.3 / vers.3
            };
            Ok((resultat, format!("{} {}", formater(resultat), vers.1)))
        }
        (de, vers) => Err(format!("on ne convertit pas {} en {}", grandeur(de).nom(), grandeur(vers).nom())),
    }
}

fn grandeur(sorte: Sorte) -> Grandeur {
    match sorte {
        Sorte::Unite(u) => u.2,
        Sorte::Devise(_) => Grandeur::Devise,
    }
}

/// Une somme : deux décimales, sauf si elle est minuscule.
fn formater_monnaie(valeur: f64) -> String {
    if valeur.abs() >= 0.01 {
        formater((valeur * 100.0).round() / 100.0)
    } else {
        formater(valeur)
    }
}

// ---------------------------------------------------------------------------
// LES TAUX DE CHANGE
// ---------------------------------------------------------------------------

const FICHIER_TAUX: &str = "devises.json";
const URL_TAUX: &str = "https://api.frankfurter.dev/v1/latest?base=EUR";
/// Au-delà, on retélécharge EN ARRIÈRE-PLAN — en gardant les anciens taux
/// pour répondre tout de suite. La BCE ne publie qu'une fois par jour ouvré.
const FRAICHEUR: Duration = Duration::from_secs(12 * 3600);

struct Taux {
    /// La date de publication BCE (« 2026-10-23 »).
    date: String,
    /// Unités de devise pour UN euro, comme l'API les donne.
    par_euro: HashMap<String, f64>,
    recu: SystemTime,
}

static TAUX: Mutex<Option<Taux>> = Mutex::new(None);
static CACHE_LU: AtomicBool = AtomicBool::new(false);
static EN_COURS: AtomicBool = AtomicBool::new(false);

fn depuis_json(texte: &str, recu: SystemTime) -> Option<Taux> {
    let v: serde_json::Value = serde_json::from_str(texte).ok()?;
    let par_euro: HashMap<String, f64> = v
        .get("rates")?
        .as_object()?
        .iter()
        .filter_map(|(k, x)| Some((k.clone(), x.as_f64().filter(|t| *t > 0.0 && t.is_finite())?)))
        .collect();
    if par_euro.is_empty() {
        return None;
    }
    let recu = v.get("recu").and_then(serde_json::Value::as_u64).map_or(recu, |s| UNIX_EPOCH + Duration::from_secs(s));
    Some(Taux { date: v.get("date")?.as_str()?.to_string(), par_euro, recu })
}

fn lire_le_cache() {
    if CACHE_LU.swap(true, Ordering::SeqCst) {
        return;
    }
    let lu = crate::config::config_path(FICHIER_TAUX)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| depuis_json(&t, UNIX_EPOCH));
    if let (Some(lu), Ok(mut taux)) = (lu, TAUX.lock()) {
        if taux.is_none() {
            *taux = Some(lu);
        }
    }
}

/// Télécharge les taux sur un fil à part — une seule fois à la fois.
fn rafraichir() {
    if EN_COURS.swap(true, Ordering::SeqCst) {
        return;
    }
    let lance = std::thread::Builder::new().name("neuradesk-devises".into()).spawn(|| {
        match crate::win32::meteo::get_json(URL_TAUX) {
            Ok(mut v) => {
                let recu = SystemTime::now();
                let secondes = recu.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                if let Some(objet) = v.as_object_mut() {
                    objet.insert("recu".into(), secondes.into());
                }
                if let Some(taux) = depuis_json(&v.to_string(), recu) {
                    crate::config::write_config(FICHIER_TAUX, v.to_string());
                    if let Ok(mut t) = TAUX.lock() {
                        *t = Some(taux);
                    }
                } else {
                    crate::diag!("[devises] réponse sans taux lisibles");
                }
            }
            Err(e) => crate::diag!("[devises] téléchargement des taux impossible : {e}"),
        }
        EN_COURS.store(false, Ordering::SeqCst);
    });
    if lance.is_err() {
        EN_COURS.store(false, Ordering::SeqCst);
    }
}

/// UNE unité de `code`, en euros.
fn taux_en_euros(code: &str) -> Result<f64, String> {
    if code == "EUR" {
        return Ok(1.0);
    }
    lire_le_cache();
    let guard = TAUX.lock().map_err(|_| "taux de change indisponibles".to_string())?;
    let perime = guard.as_ref().is_none_or(|t| t.recu.elapsed().map_or(true, |d| d > FRAICHEUR));
    let reponse = match guard.as_ref() {
        None => Err("taux de change en cours de téléchargement — réessayez dans un instant".to_string()),
        Some(t) => t
            .par_euro
            .get(code)
            .map(|par_euro| 1.0 / par_euro)
            .ok_or_else(|| format!("pas de taux pour {code} (taux BCE du {})", t.date)),
    };
    drop(guard);
    if perime {
        rafraichir();
    }
    reponse
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taux_fixes(code: &str) -> Result<f64, String> {
        match code {
            "EUR" => Ok(1.0),
            "USD" => Ok(1.0 / 1.25),
            "JPY" => Ok(1.0 / 160.0),
            _ => Err(format!("pas de taux pour {code}")),
        }
    }

    fn conv(texte: &str) -> Option<Result<String, String>> {
        convertir_avec(texte, Angle::Degres, 0.0, &taux_fixes).map(|r| r.map(|(_, s)| s))
    }

    #[test]
    fn les_longueurs_et_le_pouce_qui_n_est_pas_un_separateur() {
        assert_eq!(conv("12 in en cm"), Some(Ok("30,48 cm".into())));
        assert_eq!(conv("1 mi en km"), Some(Ok("1,609344 km".into())));
        assert_eq!(conv("6 pieds en m"), Some(Ok("1,8288 m".into())), "pluriel toléré");
        assert_eq!(conv("2*3 ft -> in"), Some(Ok("72 in".into())), "l'expression est évaluée");
        assert_eq!(conv("km en m"), Some(Ok("1000 m".into())), "sans nombre : 1");
    }

    #[test]
    fn le_plus_long_suffixe_gagne() {
        assert_eq!(conv("90 km/h en m/s"), Some(Ok("25 m/s".into())));
        assert_eq!(conv("1 kwh en kj"), Some(Ok("3600 kJ".into())));
        assert_eq!(conv("2 m² en cm²"), Some(Ok("20000 cm²".into())));
    }

    #[test]
    fn les_temperatures_sont_affines() {
        assert_eq!(conv("100 °C en °F"), Some(Ok("212 °F".into())));
        assert_eq!(conv("32 f en c"), Some(Ok("0 °C".into())));
        assert_eq!(conv("0 k en celsius"), Some(Ok("-273,15 °C".into())));
    }

    #[test]
    fn les_donnees_decimales_et_binaires() {
        assert_eq!(conv("1 gio en mo"), Some(Ok("1073,741824 Mo".into())));
        assert_eq!(conv("100 mbit en mo"), Some(Ok("12,5 Mo".into())));
    }

    #[test]
    fn les_devises_avant_ou_apres_le_nombre() {
        assert_eq!(conv("100 € en $"), Some(Ok("125 USD".into())));
        assert_eq!(conv("$100 en eur"), Some(Ok("80 EUR".into())));
        assert_eq!(conv("10 usd to jpy"), Some(Ok("1280 JPY".into())));
        assert_eq!(conv("1 € en chf"), Some(Err("pas de taux pour CHF".into())));
    }

    #[test]
    fn deux_grandeurs_differentes_sont_refusees() {
        assert_eq!(conv("3 kg en m"), Some(Err("on ne convertit pas une masse en une longueur".into())));
    }

    #[test]
    fn sans_separateur_ce_n_est_pas_une_conversion() {
        assert_eq!(conv("2+2"), None);
        assert_eq!(conv("12 in"), None);
        // Un séparateur, mais rien de convertible à droite : la calculatrice
        // garde la main (et son message d'erreur).
        assert_eq!(conv("2 en 3"), None);
    }

    /// SONDE RÉSEAU : le vrai téléchargement, puis une vraie conversion.
    /// `cargo test --bins sonde_taux_reels -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn sonde_taux_reels() {
        let premiere = convertir("100 € en $", Angle::Degres, 0.0);
        println!("première demande : {premiere:?}");
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(200));
            if !EN_COURS.load(Ordering::SeqCst) {
                break;
            }
        }
        let seconde = convertir("100 € en $", Angle::Degres, 0.0);
        println!("seconde demande : {seconde:?}");
        assert!(matches!(seconde, Some(Ok(_))));
    }

    #[test]
    fn le_cache_des_taux_se_relit() {
        let texte = r#"{"amount":1.0,"base":"EUR","date":"2026-10-23","rates":{"USD":1.1,"JPY":175.5,"BAD":-1},"recu":1000}"#;
        let t = depuis_json(texte, UNIX_EPOCH).expect("cache lisible");
        assert_eq!(t.date, "2026-10-23");
        assert_eq!(t.par_euro.get("USD"), Some(&1.1));
        assert!(!t.par_euro.contains_key("BAD"), "un taux négatif est écarté");
        assert_eq!(t.recu, UNIX_EPOCH + Duration::from_secs(1000));
    }
}
