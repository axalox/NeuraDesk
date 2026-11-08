//! La CALCULATRICE — le modèle de l'applet (voir `win32::cef_calculatrice`,
//! le panneau, et `assets/popups/calculatrice.html`).
//!
//! DEMANDE EXPLICITE (2026-09-26) : « une applet de calculatrice proposant les
//! fonctions principales avec un historique permettant de récupérer les
//! valeurs ou calculs écrits pour les réutiliser ».
//!
//! **L'ÉVALUATION VIT ICI, PAS DANS LA PAGE** : pas d'`eval` JavaScript sur une
//! saisie, et la grammaire se teste. La page envoie l'expression telle
//! qu'écrite ; Rust rend le résultat, ou une erreur en français.
//!
//! LA GRAMMAIRE (priorités croissantes) :
//! - `+ -` ; `a ± b%` vaut `a ± a×b/100` (la convention des calculatrices :
//!   `200 + 10%` = 220) ; un `%` ailleurs divise par 100 ;
//! - `* / × ÷` et la multiplication IMPLICITE (`2π`, `3(1+1)`, `2 sin 30`) ;
//! - `-` et `+` unaires — `-2^2` = −4, comme en mathématiques ;
//! - `^`, associatif à droite (`2^3^2` = 512) ;
//! - les suffixes `!`, `%`, `²` ;
//! - nombres (virgule OU point décimal, exposant `1,5e3`), parenthèses,
//!   `√`, fonctions `sqrt sin cos tan asin acos atan ln log abs exp`,
//!   constantes `π pi e` et `ans` (le dernier résultat).
//!
//! La VIRGULE est le séparateur décimal affiché (voir `formater`) : aucune
//! fonction ne prend deux arguments, elle n'a donc pas d'autre sens, et un
//! résultat repris de l'historique se relit tel quel.
//!
//! L'historique est rangé dans `%APPDATA%\NeuraDesk\calculatrice.toml`, le plus
//! récent en premier, borné à `HISTORIQUE_MAX` entrées.

use std::cell::RefCell;

use crate::config;

const FICHIER: &str = "calculatrice.toml";
/// Au-delà, les plus anciennes entrées tombent.
pub const HISTORIQUE_MAX: usize = 200;
/// Chiffres significatifs gardés à l'affichage — assez pour un calcul du
/// quotidien, assez peu pour que `0,1 + 0,2` rende `0,3`.
const CHIFFRES_SIGNIFICATIFS: usize = 12;
/// Le plus grand entier dont la factorielle tient dans un `f64`.
const FACTORIELLE_MAX: f64 = 170.0;

/// L'unité des fonctions trigonométriques.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Angle {
    #[default]
    Degres,
    Radians,
}

impl Angle {
    pub fn nom(self) -> &'static str {
        match self {
            Angle::Degres => "deg",
            Angle::Radians => "rad",
        }
    }

    pub fn depuis_nom(nom: &str) -> Option<Self> {
        match nom {
            "deg" => Some(Angle::Degres),
            "rad" => Some(Angle::Radians),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// L'ÉVALUATEUR
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Jeton {
    Nombre(f64),
    Nom(String),
    Op(char),
    Ouvrante,
    Fermante,
}

fn est_operateur(c: char) -> bool {
    matches!(c, '+' | '-' | '−' | '*' | '×' | '·' | '/' | '÷' | '^' | '!' | '%' | '²' | '√')
}

/// Découpe `texte` en jetons. Les opérateurs typographiques (`−`, `×`, `÷`)
/// sont ramenés à leur forme ASCII.
fn decouper(texte: &str) -> Result<Vec<Jeton>, String> {
    let chars: Vec<char> = texte.chars().collect();
    let mut jetons = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || ((c == ',' || c == '.') && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let debut = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == ',' || chars[i] == '.') {
                i += 1;
            }
            // Un exposant : `e` suivi d'un chiffre, ou d'un signe PUIS d'un
            // chiffre. Sinon `2e` est « 2 × e ».
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let signe = matches!(chars.get(i + 1), Some('+' | '-'));
                let premier = if signe { i + 2 } else { i + 1 };
                if chars.get(premier).is_some_and(char::is_ascii_digit) {
                    i = premier;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let brut: String = chars[debut..i].iter().collect::<String>().replace(',', ".");
            if brut.matches('.').count() > 1 {
                return Err(format!("nombre mal formé : {}", chars[debut..i].iter().collect::<String>()));
            }
            jetons.push(Jeton::Nombre(brut.parse().map_err(|_| format!("nombre mal formé : {brut}"))?));
        } else if c.is_alphabetic() && c != '²' {
            let debut = i;
            while i < chars.len() && chars[i].is_alphabetic() {
                i += 1;
            }
            jetons.push(Jeton::Nom(chars[debut..i].iter().collect::<String>().to_lowercase()));
        } else if c == '(' {
            jetons.push(Jeton::Ouvrante);
            i += 1;
        } else if c == ')' {
            jetons.push(Jeton::Fermante);
            i += 1;
        } else if est_operateur(c) {
            jetons.push(Jeton::Op(match c {
                '−' => '-',
                '×' | '·' => '*',
                '÷' => '/',
                autre => autre,
            }));
            i += 1;
        } else {
            return Err(format!("caractère inattendu : « {c} »"));
        }
    }
    Ok(jetons)
}

const FONCTIONS: &[&str] = &["sqrt", "racine", "sin", "cos", "tan", "asin", "acos", "atan", "ln", "log", "abs", "exp"];

struct Analyseur<'a> {
    jetons: &'a [Jeton],
    pos: usize,
    angle: Angle,
    ans: f64,
}

/// Un terme, et s'il s'agissait d'un POURCENTAGE nu (`10%`) — voir `somme`.
struct Terme {
    valeur: f64,
    pourcentage: bool,
}

impl Analyseur<'_> {
    fn courant(&self) -> Option<&Jeton> {
        self.jetons.get(self.pos)
    }

    fn est_op(&self, op: char) -> bool {
        self.courant() == Some(&Jeton::Op(op))
    }

    fn somme(&mut self) -> Result<f64, String> {
        let mut valeur = self.produit()?.valeur;
        while let Some(Jeton::Op(op @ ('+' | '-'))) = self.courant().cloned() {
            self.pos += 1;
            let terme = self.produit()?;
            let droite = if terme.pourcentage { valeur * terme.valeur } else { terme.valeur };
            valeur = if op == '+' { valeur + droite } else { valeur - droite };
        }
        Ok(valeur)
    }

    /// Un jeton qui peut COMMENCER un facteur — la multiplication implicite
    /// (`2π`, `3(1+1)`) en dépend.
    fn commence_un_facteur(&self) -> bool {
        matches!(self.courant(), Some(Jeton::Nombre(_) | Jeton::Nom(_) | Jeton::Ouvrante | Jeton::Op('√')))
    }

    fn produit(&mut self) -> Result<Terme, String> {
        let mut terme = self.signe()?;
        loop {
            let op = match self.courant() {
                Some(Jeton::Op(op @ ('*' | '/'))) => {
                    let op = *op;
                    self.pos += 1;
                    op
                }
                _ if self.commence_un_facteur() => '*',
                _ => break,
            };
            let droite = self.signe()?.valeur;
            terme = Terme {
                valeur: if op == '*' {
                    terme.valeur * droite
                } else if droite == 0.0 {
                    return Err("division par zéro".into());
                } else {
                    terme.valeur / droite
                },
                pourcentage: false,
            };
        }
        Ok(terme)
    }

    fn signe(&mut self) -> Result<Terme, String> {
        if self.est_op('-') {
            self.pos += 1;
            let t = self.signe()?;
            return Ok(Terme { valeur: -t.valeur, pourcentage: t.pourcentage });
        }
        if self.est_op('+') {
            self.pos += 1;
            return self.signe();
        }
        self.puissance()
    }

    fn puissance(&mut self) -> Result<Terme, String> {
        let base = self.suffixes()?;
        if !self.est_op('^') {
            return Ok(base);
        }
        self.pos += 1;
        // À droite, `signe` : `2^-1` est permis, et `2^3^2` = 2^(3^2).
        let exposant = self.signe()?.valeur;
        Ok(Terme { valeur: base.valeur.powf(exposant), pourcentage: false })
    }

    fn suffixes(&mut self) -> Result<Terme, String> {
        let mut terme = Terme { valeur: self.primaire()?, pourcentage: false };
        loop {
            if self.est_op('!') {
                self.pos += 1;
                terme = Terme { valeur: factorielle(terme.valeur)?, pourcentage: false };
            } else if self.est_op('²') {
                self.pos += 1;
                terme = Terme { valeur: terme.valeur * terme.valeur, pourcentage: false };
            } else if self.est_op('%') {
                self.pos += 1;
                terme = Terme { valeur: terme.valeur / 100.0, pourcentage: true };
            } else {
                return Ok(terme);
            }
        }
    }

    /// L'argument NU d'une fonction ou de `√` (`sin 30`, `√2`) — un signe en
    /// tête s'y rattache (`√-4` doit être refusé, pas mal lu).
    fn argument(&mut self, suite: fn(&mut Self) -> Result<Terme, String>) -> Result<f64, String> {
        if self.est_op('-') {
            self.pos += 1;
            return Ok(-self.argument(suite)?);
        }
        Ok(suite(self)?.valeur)
    }

    fn primaire(&mut self) -> Result<f64, String> {
        match self.courant().cloned() {
            Some(Jeton::Nombre(n)) => {
                self.pos += 1;
                Ok(n)
            }
            Some(Jeton::Ouvrante) => {
                self.pos += 1;
                let v = self.somme()?;
                // Une parenthèse laissée ouverte en FIN d'expression est
                // fermée d'office, comme le fait toute calculatrice.
                match self.courant() {
                    Some(Jeton::Fermante) => self.pos += 1,
                    None => {}
                    Some(_) => return Err("parenthèse fermante attendue".into()),
                }
                Ok(v)
            }
            Some(Jeton::Op('√')) => {
                self.pos += 1;
                let x = self.argument(Self::suffixes)?;
                racine(x)
            }
            Some(Jeton::Nom(nom)) => {
                self.pos += 1;
                match nom.as_str() {
                    "pi" | "π" => Ok(std::f64::consts::PI),
                    "e" => Ok(std::f64::consts::E),
                    "ans" => Ok(self.ans),
                    f if FONCTIONS.contains(&f) => {
                        // L'argument : une parenthèse, ou un facteur nu (`sin 30`, `√2`).
                        let x = self.argument(Self::puissance)?;
                        appliquer(f, x, self.angle)
                    }
                    inconnu => Err(format!("nom inconnu : « {inconnu} »")),
                }
            }
            Some(Jeton::Fermante) => Err("parenthèse fermante en trop".into()),
            Some(Jeton::Op(op)) => Err(format!("« {op} » mal placé")),
            None => Err("expression incomplète".into()),
        }
    }
}

fn racine(x: f64) -> Result<f64, String> {
    if x < 0.0 {
        Err("racine d'un nombre négatif".into())
    } else {
        Ok(x.sqrt())
    }
}

fn factorielle(x: f64) -> Result<f64, String> {
    if x < 0.0 || x.fract() != 0.0 {
        return Err("factorielle d'un entier positif seulement".into());
    }
    if x > FACTORIELLE_MAX {
        return Err("nombre trop grand".into());
    }
    Ok((1..=x as u32).fold(1.0, |acc, k| acc * f64::from(k)))
}

/// `sin 180°` vaut 1,2e-16 en flottants, pas 0 : l'arrondi de `formater`
/// garde des chiffres SIGNIFICATIFS, il ne l'efface donc pas. Sous ce seuil,
/// une fonction trigonométrique rend 0.
fn sans_residu(v: f64) -> f64 {
    if v.abs() < 1e-12 {
        0.0
    } else {
        v
    }
}

fn appliquer(fonction: &str, x: f64, angle: Angle) -> Result<f64, String> {
    let vers_radians = |v: f64| if angle == Angle::Degres { v.to_radians() } else { v };
    let depuis_radians = |v: f64| if angle == Angle::Degres { v.to_degrees() } else { v };
    let domaine = |ok: bool, v: f64, message: &str| if ok { Ok(v) } else { Err(message.to_string()) };
    match fonction {
        "sqrt" | "racine" => racine(x),
        // `tan 90°` doit être REFUSÉ, pas rendre 1,6e16.
        "sin" => Ok(sans_residu(vers_radians(x).sin())),
        "cos" => Ok(sans_residu(vers_radians(x).cos())),
        "tan" => {
            let multiple_impair_de_90 = angle == Angle::Degres && (x / 90.0).fract() == 0.0 && (x / 90.0) as i64 % 2 != 0;
            domaine(!multiple_impair_de_90, sans_residu(vers_radians(x).tan()), "tangente non définie")
        }
        "asin" => domaine((-1.0..=1.0).contains(&x), depuis_radians(x.asin()), "arc sinus hors de [−1 ; 1]"),
        "acos" => domaine((-1.0..=1.0).contains(&x), depuis_radians(x.acos()), "arc cosinus hors de [−1 ; 1]"),
        "atan" => Ok(depuis_radians(x.atan())),
        "ln" => domaine(x > 0.0, x.ln(), "logarithme d'un nombre ≤ 0"),
        "log" => domaine(x > 0.0, x.log10(), "logarithme d'un nombre ≤ 0"),
        "abs" => Ok(x.abs()),
        "exp" => Ok(x.exp()),
        autre => Err(format!("nom inconnu : « {autre} »")),
    }
}

/// Évalue `texte`. `ans` est la valeur de `ans` (le dernier résultat).
pub fn evaluer(texte: &str, angle: Angle, ans: f64) -> Result<f64, String> {
    let jetons = decouper(texte)?;
    if jetons.is_empty() {
        return Err("expression vide".into());
    }
    let mut analyseur = Analyseur { jetons: &jetons, pos: 0, angle, ans };
    let valeur = analyseur.somme()?;
    if analyseur.pos < jetons.len() {
        return Err(match &jetons[analyseur.pos] {
            Jeton::Fermante => "parenthèse fermante en trop".into(),
            _ => "expression mal formée".into(),
        });
    }
    if valeur.is_nan() {
        return Err("résultat indéfini".into());
    }
    if valeur.is_infinite() {
        return Err("nombre trop grand".into());
    }
    Ok(valeur)
}

/// Le résultat tel qu'il s'affiche ET se relit : `CHIFFRES_SIGNIFICATIFS`
/// chiffres, virgule décimale, notation scientifique aux extrêmes (`1,5e20`).
pub fn formater(valeur: f64) -> String {
    let arrondi: f64 = format!("{:.*e}", CHIFFRES_SIGNIFICATIFS - 1, valeur).parse().unwrap_or(valeur);
    // `-0` n'a rien à faire à l'écran.
    let arrondi = if arrondi == 0.0 { 0.0 } else { arrondi };
    let absolu = arrondi.abs();
    let texte = if absolu != 0.0 && !(1e-9..1e15).contains(&absolu) {
        let brut = format!("{arrondi:e}");
        // `{:e}` écrit `1.5e20` et `1e-10` : il suffit de changer le point.
        brut
    } else {
        format!("{arrondi}")
    };
    texte.replace('.', ",")
}

// ---------------------------------------------------------------------------
// L'HISTORIQUE
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Entree {
    pub expression: String,
    pub resultat: String,
}

/// Le dernier calcul demandé — la page le reconnaît à `numero`, qui change à
/// chaque demande, même répétée.
#[derive(Clone, Debug, PartialEq)]
pub struct Dernier {
    pub numero: u64,
    pub expression: String,
    pub reponse: Result<String, String>,
}

#[derive(Debug, Default)]
pub struct Calculatrice {
    /// Le plus récent en premier.
    pub historique: Vec<Entree>,
    pub angle: Angle,
    pub dernier: Option<Dernier>,
    /// La valeur de `ans` — le dernier résultat RÉUSSI, de cette session ou,
    /// au démarrage, le plus récent de l'historique.
    pub ans: f64,
}

impl Calculatrice {
    /// Calcule `expression` ; un succès entre dans l'historique (sauf s'il
    /// répète exactement l'entrée la plus récente) et devient `ans`.
    pub fn calculer(&mut self, expression: &str) {
        let expression = expression.trim().to_string();
        let numero = self.dernier.as_ref().map_or(1, |d| d.numero + 1);
        // ⭐ UNE CONVERSION (« 12 in en cm », voir `win32::conversion`) rend un
        // résultat SUIVI DE SON UNITÉ : `ans` en prend la valeur, pas le texte,
        // qui ne se relirait pas.
        let (reponse, valeur) = match crate::win32::conversion::convertir(&expression, self.angle, self.ans) {
            Some(Ok((valeur, texte))) => (Ok(texte), Some(valeur)),
            Some(Err(e)) => (Err(e), None),
            None => (evaluer(&expression, self.angle, self.ans).map(formater), None),
        };
        if let Ok(resultat) = &reponse {
            self.ans = valeur.or_else(|| evaluer(resultat, self.angle, 0.0).ok()).unwrap_or(self.ans);
            let entree = Entree { expression: expression.clone(), resultat: resultat.clone() };
            if self.historique.first() != Some(&entree) {
                self.historique.insert(0, entree);
                self.historique.truncate(HISTORIQUE_MAX);
            }
        }
        self.dernier = Some(Dernier { numero, expression, reponse });
    }

    pub fn supprimer(&mut self, index: usize) {
        if index < self.historique.len() {
            self.historique.remove(index);
        }
    }

    pub fn vider(&mut self) {
        self.historique.clear();
    }
}

fn depuis_toml(texte: &str) -> Calculatrice {
    let Ok(table) = texte.parse::<toml::Table>() else { return Calculatrice::default() };
    let angle = table.get("angle").and_then(|v| v.as_str()).and_then(Angle::depuis_nom).unwrap_or_default();
    let historique: Vec<Entree> = table
        .get("historique")
        .and_then(|v| v.as_array())
        .map(|entrees| {
            entrees
                .iter()
                .filter_map(|e| {
                    Some(Entree {
                        expression: e.get("expression")?.as_str()?.to_string(),
                        resultat: e.get("resultat")?.as_str()?.to_string(),
                    })
                })
                .take(HISTORIQUE_MAX)
                .collect()
        })
        .unwrap_or_default();
    let ans = historique.first().and_then(|e| evaluer(&e.resultat, angle, 0.0).ok()).unwrap_or(0.0);
    Calculatrice { historique, angle, dernier: None, ans }
}

fn vers_toml(c: &Calculatrice) -> String {
    use toml::Value;
    let mut table = toml::Table::new();
    table.insert("angle".into(), Value::String(c.angle.nom().into()));
    let entrees = c
        .historique
        .iter()
        .map(|e| {
            let mut t = toml::Table::new();
            t.insert("expression".into(), Value::String(e.expression.clone()));
            t.insert("resultat".into(), Value::String(e.resultat.clone()));
            Value::Table(t)
        })
        .collect();
    table.insert("historique".into(), Value::Array(entrees));
    toml::to_string(&table).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// L'ÉTAT DE L'APPLICATION
// ---------------------------------------------------------------------------

thread_local! {
    static ETAT: RefCell<Calculatrice> = RefCell::new(Calculatrice::default());
}

/// Relit `calculatrice.toml` — une fois, au démarrage (voir `app::run`).
pub fn charger() {
    let texte = config::config_path(FICHIER).and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    ETAT.with(|e| *e.borrow_mut() = depuis_toml(&texte));
}

fn enregistrer(c: &Calculatrice) {
    if !config::write_config_atomique(FICHIER, vers_toml(c)) {
        crate::diag!("[calculatrice] calculatrice.toml n'a pas pu être écrit");
    }
}

pub fn lire<R>(f: impl FnOnce(&Calculatrice) -> R) -> R {
    ETAT.with(|e| f(&e.borrow()))
}

/// Modifie l'état PUIS l'enregistre — le seul chemin d'écriture.
pub fn modifier<R>(f: impl FnOnce(&mut Calculatrice) -> R) -> R {
    ETAT.with(|e| {
        let mut c = e.borrow_mut();
        let r = f(&mut c);
        enregistrer(&c);
        r
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calc(texte: &str) -> Result<String, String> {
        evaluer(texte, Angle::Degres, 0.0).map(formater)
    }

    #[test]
    fn les_quatre_operations_respectent_les_priorites() {
        assert_eq!(calc("2+3*4"), Ok("14".into()));
        assert_eq!(calc("(2+3)*4"), Ok("20".into()));
        assert_eq!(calc("10-4-3"), Ok("3".into()));
        assert_eq!(calc("100/10/5"), Ok("2".into()));
        assert_eq!(calc("7 × 6 ÷ 2 − 1"), Ok("20".into()));
    }

    #[test]
    fn la_virgule_et_le_point_sont_decimaux_et_l_affichage_rend_la_virgule() {
        assert_eq!(calc("1,5+1.25"), Ok("2,75".into()));
        assert_eq!(calc("0,1+0,2"), Ok("0,3".into()), "l'arrondi efface l'erreur des flottants");
        assert_eq!(calc(",5*2"), Ok("1".into()));
        assert!(calc("1,2,3").is_err());
    }

    #[test]
    fn puissance_signe_et_suffixes() {
        assert_eq!(calc("-2^2"), Ok("-4".into()), "le signe unaire passe APRÈS la puissance");
        assert_eq!(calc("2^3^2"), Ok("512".into()), "la puissance est associative à droite");
        assert_eq!(calc("2^-1"), Ok("0,5".into()));
        assert_eq!(calc("5!"), Ok("120".into()));
        assert_eq!(calc("3²+4²"), Ok("25".into()));
        assert_eq!(calc("√16+√(9)"), Ok("7".into()));
    }

    #[test]
    fn le_pourcentage_suit_la_convention_des_calculatrices() {
        assert_eq!(calc("200+10%"), Ok("220".into()));
        assert_eq!(calc("200-25%"), Ok("150".into()));
        assert_eq!(calc("50%"), Ok("0,5".into()));
        assert_eq!(calc("200*10%"), Ok("20".into()));
    }

    #[test]
    fn multiplication_implicite_constantes_et_fonctions() {
        assert_eq!(calc("2π"), Ok(formater(2.0 * std::f64::consts::PI)));
        assert_eq!(calc("3(1+1)"), Ok("6".into()));
        assert_eq!(calc("(1+1)(2+2)"), Ok("8".into()));
        assert_eq!(calc("sin(30)"), Ok("0,5".into()), "degrés par défaut");
        assert_eq!(calc("2 cos 60"), Ok("1".into()));
        assert_eq!(calc("sin 180"), Ok("0".into()));
        assert_eq!(calc("sin -30"), Ok("-0,5".into()), "un signe en tête appartient à l'argument");
        assert_eq!(evaluer("sin(pi)", Angle::Radians, 0.0).map(formater), Ok("0".into()));
        assert_eq!(calc("log(1000)+ln(e)"), Ok("4".into()));
        assert_eq!(calc("abs(-3)"), Ok("3".into()));
        assert_eq!(calc("acos(0)"), Ok("90".into()));
        assert_eq!(evaluer("sin(pi/2)", Angle::Radians, 0.0).map(formater), Ok("1".into()));
    }

    #[test]
    fn un_exposant_se_distingue_de_la_constante_e() {
        assert_eq!(calc("1,5e3"), Ok("1500".into()));
        assert_eq!(calc("2e-3"), Ok("0,002".into()));
        assert_eq!(calc("2e"), Ok(formater(2.0 * std::f64::consts::E)));
    }

    #[test]
    fn ans_vaut_le_dernier_resultat() {
        assert_eq!(evaluer("ans*2", Angle::Degres, 21.0), Ok(42.0));
    }

    #[test]
    fn une_parenthese_ouverte_en_fin_est_fermee_d_office() {
        assert_eq!(calc("2*(3+4"), Ok("14".into()));
        assert_eq!(calc("√(2+2"), Ok("2".into()));
    }

    #[test]
    fn les_erreurs_sont_nommees() {
        assert_eq!(calc("1/0"), Err("division par zéro".into()));
        assert_eq!(calc("√-4"), Err("racine d'un nombre négatif".into()));
        assert_eq!(calc("ln(0)"), Err("logarithme d'un nombre ≤ 0".into()));
        assert_eq!(calc("tan(90)"), Err("tangente non définie".into()));
        assert_eq!(calc("2,5!"), Err("factorielle d'un entier positif seulement".into()));
        assert_eq!(calc("171!"), Err("nombre trop grand".into()));
        assert_eq!(calc("2+"), Err("expression incomplète".into()));
        assert_eq!(calc("2)"), Err("parenthèse fermante en trop".into()));
        assert_eq!(calc(""), Err("expression vide".into()));
        assert_eq!(calc("2$3"), Err("caractère inattendu : « $ »".into()));
        assert_eq!(calc("truc(2)"), Err("nom inconnu : « truc »".into()));
    }

    #[test]
    fn le_format_se_relit_tel_quel() {
        for valeur in [0.0, -0.0, 1.0 / 3.0, -2.5, 1e20, 1.5e-12, 123_456_789.125, 1e15] {
            let texte = formater(valeur);
            let relu = evaluer(&texte, Angle::Degres, 0.0).unwrap();
            assert_eq!(formater(relu), texte, "{valeur} → {texte}");
        }
        assert_eq!(formater(-0.0), "0");
        assert_eq!(formater(1e20), "1e20");
        assert_eq!(formater(1.5e-12), "1,5e-12");
        assert_eq!(formater(1.0 / 3.0), "0,333333333333");
    }

    #[test]
    fn l_historique_garde_les_succes_sans_repeter_le_plus_recent() {
        let mut c = Calculatrice::default();
        c.calculer("2+2");
        c.calculer("2+2");
        c.calculer("1/0");
        c.calculer("ans*10");
        assert_eq!(
            c.historique,
            vec![
                Entree { expression: "ans*10".into(), resultat: "40".into() },
                Entree { expression: "2+2".into(), resultat: "4".into() },
            ]
        );
        let dernier = c.dernier.as_ref().unwrap();
        assert_eq!(dernier.numero, 4, "chaque demande a son numéro, même répétée");
        assert_eq!(dernier.reponse, Ok("40".into()));
        c.supprimer(0);
        c.supprimer(9);
        assert_eq!(c.historique.len(), 1);
        c.vider();
        assert!(c.historique.is_empty());
    }

    #[test]
    fn l_historique_est_borne() {
        let mut c = Calculatrice::default();
        for k in 0..HISTORIQUE_MAX + 5 {
            c.calculer(&format!("{k}+1"));
        }
        assert_eq!(c.historique.len(), HISTORIQUE_MAX);
        assert_eq!(c.historique[0].expression, format!("{}+1", HISTORIQUE_MAX + 4));
    }

    #[test]
    fn le_toml_fait_l_aller_retour() {
        let mut c = Calculatrice { angle: Angle::Radians, ..Default::default() };
        c.calculer("6*7");
        c.calculer("1+1");
        let relu = depuis_toml(&vers_toml(&c));
        assert_eq!(relu.historique, c.historique);
        assert_eq!(relu.angle, Angle::Radians);
        assert_eq!(relu.ans, 2.0, "ans repart du plus récent");
        assert!(depuis_toml("n'importe quoi").historique.is_empty());
    }
}
