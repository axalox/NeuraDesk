//! **LA LANGUE DE L'INTERFACE** — français (la langue d'écriture de l'app) ou
//! anglais.
//!
//! ⭐ **LE FRANÇAIS EST LA CLÉ.** Aucun texte du code n'est renommé : le
//! dictionnaire `assets/i18n/en.tsv` associe une phrase française à sa
//! traduction. Une phrase absente du dictionnaire s'affiche telle quelle, en
//! français — jamais un vide, jamais une clé technique.
//!
//! ⭐ **DEUX POINTS DE PASSAGE, ET SEULEMENT DEUX** :
//! - ce que Rust dessine passe par `gfx::text` (`rendre_rgba`, `measure_text`),
//!   qui appelle [`traduire`] ;
//! - les pages CEF reçoivent [`script_des_pages`] au chargement
//!   (`cef_app::NeuraDeskLoadHandler`) : il traduit le DOM, puis tout ce que la
//!   page y ajoute ensuite (`MutationObserver`).
//!
//! **LE CHOIX** : `langue.txt` dans le dossier de configuration — `auto`
//! (défaut), `fr` ou `en`. En `auto`, l'anglais est pris dès que la langue
//! d'AFFICHAGE de Windows n'est pas le français. Le choix est lu UNE fois au
//! démarrage : les pages préchargées sont déjà traduites, en changer demande un
//! redémarrage (la page des Réglages le dit).
//!
//! **LES MOTIFS** : une clé contenant `{}` reconnaît n'importe quel texte à cet
//! endroit (`Il reste {} minutes` → `{} minutes left`), dans l'ORDRE — pour les
//! phrases que les pages fabriquent par concaténation.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Le dictionnaire, embarqué : un exécutable copié seul reste traduit.
const DICTIONNAIRE_EN: &str = include_str!("../../assets/i18n/en.tsv");

/// Le fichier de configuration qui porte le choix.
const FICHIER_PREFERENCE: &str = "langue.txt";

/// `LANG_FRENCH` de `winnt.h` : la langue PRINCIPALE, toutes variantes
/// régionales confondues (France, Canada, Belgique, Suisse…).
const LANG_FRENCH: u16 = 0x0C;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Langue {
    Fr,
    En,
}

impl Langue {
    /// Le code que CEF et l'attribut `lang` attendent.
    pub fn code(self) -> &'static str {
        match self {
            Langue::Fr => "fr",
            Langue::En => "en",
        }
    }
}

/// Le choix enregistré : `"auto"`, `"fr"` ou `"en"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preference {
    Auto,
    Fr,
    En,
}

impl Preference {
    pub fn nom(self) -> &'static str {
        match self {
            Preference::Auto => "auto",
            Preference::Fr => "fr",
            Preference::En => "en",
        }
    }

    pub fn depuis_nom(nom: &str) -> Option<Self> {
        match nom.trim() {
            "auto" => Some(Preference::Auto),
            "fr" => Some(Preference::Fr),
            "en" => Some(Preference::En),
            _ => None,
        }
    }

    fn resoudre(self, windows_en_francais: bool) -> Langue {
        match self {
            Preference::Fr => Langue::Fr,
            Preference::En => Langue::En,
            Preference::Auto if windows_en_francais => Langue::Fr,
            Preference::Auto => Langue::En,
        }
    }
}

/// La préférence ENREGISTRÉE (relue à chaque appel : la page des Réglages doit
/// montrer le choix fait même s'il ne vaut qu'au prochain démarrage).
pub fn preference() -> Preference {
    crate::config::config_path(FICHIER_PREFERENCE)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| Preference::depuis_nom(&s))
        .unwrap_or(Preference::Auto)
}

/// Enregistre le choix. ⚠️ Un nom inconnu n'écrit RIEN.
pub fn set_preference(nom: &str) {
    if let Some(p) = Preference::depuis_nom(nom) {
        crate::config::write_config(FICHIER_PREFERENCE, p.nom().to_string());
    }
}

/// La langue d'AFFICHAGE de Windows est-elle le français ? `GetUserDefaultUILanguage`
/// — et non le format régional (`GetUserDefaultLocaleName`), qui gouverne les dates :
/// un Windows en anglais réglé au format français reste un Windows en anglais.
fn windows_en_francais() -> bool {
    let langid = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
    (langid & 0x3FF) == LANG_FRENCH
}

static LANGUE: OnceLock<Langue> = OnceLock::new();

/// La langue de CETTE session — figée au premier appel.
///
/// `NEURADESK_LANGUE=fr|en` passe avant le réglage : pour photographier une
/// langue sans toucher au choix enregistré.
pub fn courante() -> Langue {
    *LANGUE.get_or_init(|| {
        let forcee = std::env::var("NEURADESK_LANGUE").ok().and_then(|v| Preference::depuis_nom(&v));
        forcee.unwrap_or_else(preference).resoudre(windows_en_francais())
    })
}

/// La langue que la préférence enregistrée donnerait — pour dire, dans les
/// Réglages, si un redémarrage est nécessaire.
pub fn au_prochain_demarrage() -> Langue {
    preference().resoudre(windows_en_francais())
}

/// Un motif : les morceaux fixes autour des `{}`, et la traduction.
struct Motif {
    morceaux: Vec<String>,
    traduction: String,
}

struct Dictionnaire {
    exacts: HashMap<String, String>,
    motifs: Vec<Motif>,
}

/// Les espaces d'une phrase ramenés à un seul, bords retirés — un nœud de texte
/// HTML porte l'indentation de sa source.
fn normaliser(texte: &str) -> String {
    texte.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn lire_dictionnaire(source: &str) -> Dictionnaire {
    let mut exacts = HashMap::new();
    let mut motifs = Vec::new();
    for ligne in source.lines() {
        let ligne = ligne.trim_end_matches('\r');
        if ligne.trim().is_empty() || ligne.starts_with('#') {
            continue;
        }
        let Some((fr, en)) = ligne.split_once('\t') else { continue };
        let (fr, en) = (normaliser(fr), en.trim().to_string());
        if fr.is_empty() || en.is_empty() {
            continue;
        }
        if fr.contains("{}") {
            let morceaux = fr.split("{}").map(str::to_string).collect();
            motifs.push(Motif { morceaux, traduction: en });
        } else {
            exacts.insert(fr, en);
        }
    }
    // ⭐ LE MOTIF LE PLUS PRÉCIS D'ABORD — celui qui a le plus de texte fixe :
    // « {} note créée le {} » doit passer avant « {} novembre », qui avalerait
    // « 1 note créée le 6 novembre ». L'ordre du fichier ne compte donc pas (tri
    // stable : à précision égale, il départage). Les pages reçoivent cet ordre.
    motifs.sort_by_key(|m: &Motif| std::cmp::Reverse(m.morceaux.iter().map(|p| p.chars().count()).sum::<usize>()));
    Dictionnaire { exacts, motifs }
}

fn dictionnaire() -> &'static Dictionnaire {
    static D: OnceLock<Dictionnaire> = OnceLock::new();
    D.get_or_init(|| lire_dictionnaire(DICTIONNAIRE_EN))
}

/// Les valeurs des `{}` si `texte` suit les morceaux de `motif` — le plus
/// court à chaque trou, comme `(.+?)` ancré.
fn reconnaitre<'a>(morceaux: &[String], texte: &'a str) -> Option<Vec<&'a str>> {
    let (premier, reste) = morceaux.split_first()?;
    let mut curseur = texte.strip_prefix(premier.as_str())?;
    let mut valeurs = Vec::new();
    for (i, morceau) in reste.iter().enumerate() {
        let dernier = i == reste.len() - 1;
        let fin = if dernier {
            curseur.strip_suffix(morceau.as_str())?.len()
        } else {
            // Au moins un caractère dans le trou.
            let debut = curseur.chars().next()?.len_utf8();
            debut + curseur[debut..].find(morceau.as_str())?
        };
        if fin == 0 {
            return None;
        }
        valeurs.push(&curseur[..fin]);
        curseur = &curseur[fin + morceau.len()..];
    }
    Some(valeurs)
}

fn traduire_dans<'a>(dico: &Dictionnaire, texte: &'a str) -> Cow<'a, str> {
    traduire_a_profondeur(dico, texte, 0)
}

/// Combien de motifs peuvent s'emboîter (« Dimanche {} » → « {} novembre {} »).
const PROFONDEUR_MAX: u8 = 3;

fn traduire_a_profondeur<'a>(dico: &Dictionnaire, texte: &'a str, profondeur: u8) -> Cow<'a, str> {
    let cle = normaliser(texte);
    if cle.is_empty() {
        return Cow::Borrowed(texte);
    }
    if let Some(en) = dico.exacts.get(&cle) {
        return Cow::Owned(en.clone());
    }
    for motif in &dico.motifs {
        if let Some(valeurs) = reconnaitre(&motif.morceaux, &cle) {
            let mut sortie = String::new();
            let mut valeurs = valeurs.into_iter();
            let mut parts = motif.traduction.split("{}").peekable();
            while let Some(part) = parts.next() {
                sortie.push_str(part);
                if parts.peek().is_some() {
                    // La valeur elle-même, traduite à son tour : « Papier clair »
                    // → « {} light » avec « Papier » → « Paper » ; « Dimanche 15
                    // novembre 2026 » → « Sunday {} » puis « {} November {} ».
                    let valeur = valeurs.next().unwrap_or("");
                    if profondeur < PROFONDEUR_MAX {
                        sortie.push_str(&traduire_a_profondeur(dico, valeur, profondeur + 1));
                    } else {
                        sortie.push_str(valeur);
                    }
                }
            }
            return Cow::Owned(sortie);
        }
    }
    Cow::Borrowed(texte)
}

thread_local! {
    static SANS_TRADUCTION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// ⭐ **DESSINER DU CONTENU, PAS DE L'INTERFACE** — un titre de vidéo ou de
/// fenêtre passe aussi par `gfx::text`, et un motif comme « {} novembre » le
/// déformerait (« Concert du 6 novembre »). Tout texte dessiné dans `f` reste
/// tel quel.
pub fn sans_traduction<T>(f: impl FnOnce() -> T) -> T {
    let avant = SANS_TRADUCTION.with(|c| c.replace(true));
    let resultat = f();
    SANS_TRADUCTION.with(|c| c.set(avant));
    resultat
}

/// ⭐ **LA TRADUCTION D'UN TEXTE AFFICHÉ** — inchangé en français, ou si la
/// phrase n'est pas au dictionnaire.
pub fn traduire(texte: &str) -> Cow<'_, str> {
    if courante() == Langue::Fr || SANS_TRADUCTION.with(std::cell::Cell::get) {
        return Cow::Borrowed(texte);
    }
    let traduit = traduire_dans(dictionnaire(), texte);
    if matches!(traduit, Cow::Borrowed(_)) {
        relever_texte_rust(texte);
    }
    traduit
}

/// Le RELEVÉ côté Rust (`NEURADESK_I18N_RELEVE`) : un texte dessiné, resté
/// en français, journalisé une seule fois — même format que les pages.
fn relever_texte_rust(texte: &str) {
    static ACTIF: OnceLock<bool> = OnceLock::new();
    if !*ACTIF.get_or_init(|| std::env::var_os("NEURADESK_I18N_RELEVE").is_some()) {
        return;
    }
    let francais = texte.chars().any(|c| "éèêàçùôîâûœÉÈÀÇ".contains(c))
        || texte.split_whitespace().any(|m| matches!(m.to_lowercase().as_str(), "le" | "la" | "les" | "des" | "du" | "une" | "et" | "pour" | "avec" | "sans" | "sur" | "dans" | "pas"));
    if !francais {
        return;
    }
    static VUS: std::sync::Mutex<Option<std::collections::HashSet<String>>> = std::sync::Mutex::new(None);
    if let Ok(mut vus) = VUS.lock() {
        if vus.get_or_insert_with(Default::default).insert(texte.to_string()) {
            crate::diag!("[i18n] rust\t{}", normaliser(texte));
        }
    }
}

/// Le dictionnaire en JSON, pour le script des pages : `[exacts, motifs]`.
fn dictionnaire_json(dico: &Dictionnaire) -> String {
    let motifs: Vec<_> = dico
        .motifs
        .iter()
        .map(|m| serde_json::json!([m.morceaux, m.traduction]))
        .collect();
    serde_json::json!([dico.exacts, motifs]).to_string()
}

/// ⭐ **UNE PAGE ÉCRITE DANS LA LANGUE, SI ELLE EXISTE** — `aide.html` →
/// `aide.en.html` en anglais. Pour les pages de TEXTE SUIVI (l'aide), qu'un
/// dictionnaire de phrases traduit mal : une phrase coupée par du gras y est
/// découpée en morceaux. Sinon, la page elle-même (et le dictionnaire).
pub fn page_traduite(page: &std::path::Path) -> std::path::PathBuf {
    if courante() == Langue::Fr {
        return page.to_path_buf();
    }
    let traduite = page.with_extension(format!("{}.html", courante().code()));
    if traduite.is_file() { traduite } else { page.to_path_buf() }
}

/// Le moteur des pages — voir `assets/i18n/pages.js`.
const MOTEUR_DES_PAGES: &str = include_str!("../../assets/i18n/pages.js");

/// ⭐ **LE SCRIPT À INJECTER DANS CHAQUE PAGE**, ou `None` en français (rien à
/// faire, rien à payer). Construit une fois.
pub fn script_des_pages() -> Option<&'static str> {
    if courante() == Langue::Fr {
        return None;
    }
    static S: OnceLock<String> = OnceLock::new();
    Some(S.get_or_init(|| {
        // `NEURADESK_I18N_RELEVE=1` : la page journalise ce qu'elle n'a pas su
        // traduire (voir `on_console_message` dans `cef_app`).
        let releve = if std::env::var_os("NEURADESK_I18N_RELEVE").is_some() { "window.__ndI18nReleve = true;\n" } else { "" };
        format!("{releve}{}\n__ndI18n({});", MOTEUR_DES_PAGES, dictionnaire_json(dictionnaire()))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dico() -> Dictionnaire {
        lire_dictionnaire("# commentaire\nOuvrir\tOpen\nIl reste {} minutes\t{} minutes left\n{} sur {}\t{} of {}\n")
    }

    #[test]
    fn une_phrase_connue_est_traduite_et_une_inconnue_reste_en_francais() {
        let d = dico();
        assert_eq!(traduire_dans(&d, "Ouvrir"), "Open");
        assert_eq!(traduire_dans(&d, "  Ouvrir\n  "), "Open");
        assert_eq!(traduire_dans(&d, "Fermer"), "Fermer");
    }

    #[test]
    fn un_motif_reporte_ses_valeurs_dans_l_ordre() {
        let d = dico();
        assert_eq!(traduire_dans(&d, "Il reste 12 minutes"), "12 minutes left");
        assert_eq!(traduire_dans(&d, "3 sur 10"), "3 of 10");
        // La valeur traduite si elle est connue.
        assert_eq!(traduire_dans(&d, "Ouvrir sur 10"), "Open of 10");
        // Les motifs s'emboîtent.
        assert_eq!(traduire_dans(&d, "Il reste 3 sur 10 minutes"), "3 of 10 minutes left");
        // Un trou ne peut pas être vide.
        assert_eq!(traduire_dans(&d, "Il reste  minutes"), "Il reste  minutes");
    }

    #[test]
    fn le_motif_le_plus_precis_gagne_quel_que_soit_l_ordre_du_fichier() {
        let d = lire_dictionnaire("{} novembre\tNovember {}\n{} note créée le {}\t{} note created on {}\n");
        assert_eq!(traduire_dans(&d, "1 note créée le 6 novembre"), "1 note created on November 6");
    }

    #[test]
    fn le_choix_auto_suit_windows() {
        assert_eq!(Preference::Auto.resoudre(true), Langue::Fr);
        assert_eq!(Preference::Auto.resoudre(false), Langue::En);
        assert_eq!(Preference::Fr.resoudre(false), Langue::Fr);
        assert_eq!(Preference::En.resoudre(true), Langue::En);
        assert_eq!(Preference::depuis_nom("klingon"), None);
    }

    /// ⛔ Le dictionnaire LIVRÉ : chaque ligne a ses deux colonnes, aucune clé
    /// n'est en double (la seconde écraserait la première en silence), et un
    /// motif garde autant de `{}` des deux côtés.
    #[test]
    fn le_dictionnaire_livre_est_bien_forme() {
        let mut vues = std::collections::HashSet::new();
        let mut fautes = Vec::new();
        for (n, ligne) in DICTIONNAIRE_EN.lines().enumerate() {
            let ligne = ligne.trim_end_matches('\r');
            if ligne.trim().is_empty() || ligne.starts_with('#') {
                continue;
            }
            let Some((fr, en)) = ligne.split_once('\t') else {
                fautes.push(format!("ligne {} sans tabulation", n + 1));
                continue;
            };
            if en.contains('\t') || en.trim().is_empty() {
                fautes.push(format!("ligne {} : traduction vide ou 3 colonnes", n + 1));
            }
            if fr.matches("{}").count() != en.matches("{}").count() {
                fautes.push(format!("ligne {} : nombre de {{}} différent", n + 1));
            }
            if !vues.insert(normaliser(fr)) {
                fautes.push(format!("ligne {} : « {} » en double", n + 1, fr));
            }
        }
        assert!(vues.len() > 0, "dictionnaire vide");
        assert!(fautes.is_empty(), "{fautes:#?}");
    }
}
