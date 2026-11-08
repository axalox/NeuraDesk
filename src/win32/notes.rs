//! Modèle de données des notes — dash "Notes" inspiré de
//! [usememos/memos](https://github.com/usememos/memos) (contenu texte libre,
//! étiquettes `#tag`, épinglage, liste anté-chronologique). Uniquement la
//! logique PURE (aucun Win32/CEF/disque ici — voir `win32::cef_notes` pour la
//! fenêtre et `config::load_notes`/`save_notes` pour la persistance), pour
//! rester testable sans dépendance externe, même discipline que
//! `win32::clipboard::ClipboardEntry`.

use std::time::{SystemTime, UNIX_EPOCH};

/// Longueur maximale (en caractères, pas en octets) d'une note — le contenu
/// traverse l'IPC JS→Rust encodé dans une URL `neuradesk://notes/...` (voir
/// `win32::cef_notes::dispatch_actions`) ; cette borne explicite garde ce
/// trajet raisonnable même si Chromium tolère des URL bien plus longues.
/// Appliquée aussi côté JS (`assets/popups/notes.html`, `maxlength`).
pub const MAX_NOTE_CHARS: usize = 8000;

/// Nombre maximal d'images attachées à UNE note — borne arbitraire mais
/// généreuse (voir `Note::push_image`) : évite qu'un collage répété fasse
/// grossir une note sans limite (chaque image occupe un fichier permanent
/// sur disque, voir `config::store_note_image`, sans éviction contrairement
/// à l'historique du presse-papiers).
pub const MAX_NOTE_IMAGES: usize = 8;

/// Grand côté maximal de la miniature persistée pour une image de note (voir
/// `config::store_note_image`) — volontairement plus PETIT que
/// `clipboard::CLIPBOARD_THUMB_MAX_SIDE` : une miniature de note est
/// affichée en permanence dans la liste (voir `assets/popups/notes.html`),
/// pas seulement au survol d'un menu, donc un format plus modeste suffit et
/// garde `push_state` (voir `win32::cef_notes`) léger même avec plusieurs
/// notes illustrées visibles à la fois.
pub const NOTE_THUMB_MAX_SIDE: u32 = 320;

/// Une image attachée à une note — persistée séparément du contenu texte
/// (voir `config::store_note_image`/`notes_images_dir`), référencée par
/// `file` (nom déterministe dérivé du contenu, voir `config::
/// hashed_png_name`). `thumb_data_uri` est précalculé ET conservé tel quel
/// (voir doc de `config::store_note_image` pour pourquoi une note ne
/// recalcule jamais sa miniature au rechargement, contrairement à
/// `clipboard::ClipboardEntry`).
#[derive(Clone, PartialEq)]
pub struct NoteImage {
    pub file: String,
    pub width: u32,
    pub height: u32,
    /// `data:image/png;base64,...` prêt à poser dans un `<img src>` — voir
    /// `win32::cef_notes::push_state`.
    pub thumb_data_uri: String,
}

/// Implémentation manuelle plutôt qu'un `#[derive(Debug)]` : ce dernier
/// afficherait le base64 ENTIER de `thumb_data_uri` (potentiellement des
/// dizaines de Ko) au moindre `{:?}` — même raison que `clipboard::
/// ClipboardEntry`, doc de son propre `impl Debug`.
impl std::fmt::Debug for NoteImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoteImage").field("file", &self.file).field("width", &self.width).field("height", &self.height).finish_non_exhaustive()
    }
}

/// Nom de fichier de la miniature associée à une image pleine résolution
/// (`<hash>.png` → `<hash>.thumb.png`) — convention PURE partagée entre
/// `config::store_note_image` (écriture des deux fichiers) et
/// `referenced_image_files` (ensemble à conserver lors du nettoyage), pour
/// ne jamais les laisser diverger.
pub fn thumb_file_name(file: &str) -> String {
    match file.strip_suffix(".png") {
        Some(stem) => format!("{stem}.thumb.png"),
        None => format!("{file}.thumb.png"),
    }
}

/// Une note. `content` est la SEULE source de vérité pour les étiquettes
/// (voir `extract_tags`) — aucune liste de tags séparée à maintenir en
/// synchronisation avec le texte. `content` ET `images` peuvent coexister
/// (une légende avec sa capture d'écran, par exemple) — PAS de variante
/// "note image" séparée : `extract_tags`/`sorted_for_display`/
/// `truncate_chars` n'ont besoin de connaître que `content`, un type à part
/// aurait fourché ces trois chemins pour aucun bénéfice réel.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// Identifiant stable, utilisé par l'IPC (`notes/update/<id>/...`,
    /// `notes/delete/<id>`, `notes/toggle-pin/<id>`) au lieu d'un index —
    /// l'ordre affiché change avec l'épinglage/le tri (voir
    /// `sorted_for_display`), un index de liste serait donc instable.
    pub id: u64,
    pub content: String,
    pub pinned: bool,
    /// Secondes Unix (UTC) — horodatage de création, ne change jamais après
    /// coup.
    pub created_at: i64,
    /// Secondes Unix (UTC) — mis à jour par `update` (contenu modifié), PAS
    /// par `toggle-pin` (épingler n'est pas une modification du contenu).
    pub updated_at: i64,
    /// Voir `NoteImage` — vide pour la quasi-totalité des notes (texte
    /// seul).
    pub images: Vec<NoteImage>,
    /// **L'ALARME DE CETTE NOTE** — `None` pour la quasi-totalité des notes.
    /// Voir `AlarmeNote` : demandé le 2026-09-24, « mettre une alarme sur les
    /// notes créées depuis le dash note ».
    pub alarme: Option<AlarmeNote>,
    /// **ARCHIVÉE** — sortie de la liste sans être supprimée ; elle ne se voit
    /// plus que dans le filtre « Archives » du panneau, et son rappel se tait
    /// (voir `fetes::rappels_des_notes`). DEMANDE EXPLICITE (2026-10-11), d'après
    /// memos. Archiver ne touche pas `updated_at` : ce n'est pas une édition.
    pub archived: bool,
}

/// **L'ALARME D'UNE NOTE** — ce qu'il faut pour qu'elle sonne tous les
/// 30 secondes jusqu'à ce que l'utilisateur acquitte (voir `win32::son` pour le
/// son, et `win32::fetes::rappels_du_jour` pour la cloche).
///
/// ⚠️ **ELLE NE PORTE PAS SON PROPRE SON** : le son est un réglage par FAMILLE
/// (`config::load_son_rappel(config::FamilleRappel::Notes)`, choisi par
/// l'utilisateur dans le panneau des Réglages) — une alarme par note aurait voulu
/// un sélecteur par note, pour aucun bénéfice ici.
///
/// ⚠️ **CE RÉGLAGE A DÉMÉNAGÉ** : il vivait au-dessus du panneau Notes, où il n'y
/// en avait qu'un pour les notes ET les dates (DEMANDE EXPLICITE du 2026-09-24 :
/// « choisir les alarmes pour les rappels de notes et les rappels de dates »).
///
/// ⚠️ **ELLE NE SE DÉSACTIVE PAS TOUTE SEULE.** Acquitter la cloche arrête la
/// sonnerie DU JOUR (l'empreinte d'acquittement contient la date, voir
/// `fetes::empreinte_des_rappels`), mais l'alarme reste posée : **elle resonne le
/// lendemain**, comme un rappel qui revient. Pour l'arrêter pour de bon, il faut
/// RETIRER l'alarme de la note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmeNote {
    /// **LA DATE CHOISIE** — `None` = « sans date », c'est-à-dire tout de suite.
    ///
    /// ⚠️ **C'EST UNE DATE, PAS UN INSTANT UNIX**, et ce n'est pas un détail : le
    /// mini calendrier doit PLACER l'alarme sur un JOUR (voir la demande
    /// « que les notes programmées apparaissent sous les prénoms »), et un
    /// instant ne dit pas son jour sans repasser par le fuseau local — le calcul
    /// qu'on avait justement évité.
    pub date: Option<(i32, u32, u32)>,
    /// **L'HEURE CHOISIE** — `None` = dès que le jour commence (ou tout de suite
    /// s'il n'y a pas de date non plus).
    pub heure: Option<(u32, u32)>,
    /// **LES JOURS DE RÉPÉTITION** — DEMANDE EXPLICITE (2026-09-25) : « pouvoir
    /// aussi rajouter des répétitions sur les notes ». Un bit par jour, bit 0 =
    /// lundi … bit 6 = dimanche (voir `JOURS_SEMAINE`).
    ///
    /// ⚠️ **`0` = UNE FOIS**, la forme de toutes les alarmes d'avant ; `TOUS_LES_JOURS`
    /// = chaque jour. Avec une répétition, la date devient le jour de DÉBUT : une
    /// alarme répétée ne sonne jamais « en retard » (voir `sonne_a`).
    pub jours: u8,
}

/// Les sept jours dans l'ordre des bits de `AlarmeNote::jours` — c'est aussi le
/// vocabulaire de `notes.toml` (`jours = ["lun", "mer"]`).
pub const JOURS_SEMAINE: [&str; 7] = ["lun", "mar", "mer", "jeu", "ven", "sam", "dim"];

/// Les sept bits de `AlarmeNote::jours` levés.
pub const TOUS_LES_JOURS: u8 = 0x7F;

/// **LE JOUR DE LA SEMAINE D'UNE DATE**, 0 = lundi … 6 = dimanche — PUR (méthode
/// de Sakamoto), pour que `sonne_a` reste testable sans passer par Windows.
pub fn jour_de_semaine(date: (i32, u32, u32)) -> u32 {
    const DECALAGES: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let (annee, mois, jour) = date;
    let a = if mois < 3 { annee - 1 } else { annee };
    // 0 = dimanche dans la formule ; on décale pour que 0 = lundi.
    let dimanche_zero = (a + a / 4 - a / 100 + a / 400 + DECALAGES[(mois - 1) as usize] + jour as i32).rem_euclid(7);
    ((dimanche_zero + 6) % 7) as u32
}

impl AlarmeNote {
    /// L'alarme qui sonne dès qu'elle est posée — ni date ni heure.
    #[cfg(test)]
    pub fn tout_de_suite() -> Self {
        Self { date: None, heure: None, jours: 0 }
    }

    /// L'alarme sonne-t-elle ce jour-là de la semaine ? Toujours vrai pour une
    /// alarme sans répétition.
    pub fn tombe_un(&self, date: (i32, u32, u32)) -> bool {
        self.jours == 0 || self.jours & (1 << jour_de_semaine(date)) != 0
    }

    /// **DOIT-ELLE SONNER MAINTENANT ?** — pure, et c'est elle qui porte la seule
    /// règle de temps du modèle. On lui passe le jour et l'heure LOCAUX, que
    /// l'appelant lit une fois (`GetLocalTime`) plutôt que de les recalculer ici.
    ///
    /// Trois cas, et le troisième est celui qu'on oublie :
    /// une date FUTURE ne sonne pas ; une date PASSÉE sonne (on a du retard, et un
    /// rappel en retard doit se voir) ; sans date, c'est l'heure qui décide.
    ///
    /// ⚠️ **UNE ALARME RÉPÉTÉE N'EST JAMAIS EN RETARD** : sa date est un DÉBUT,
    /// et chaque jour choisi se juge seul — sinon, une fois la date de début
    /// passée, elle sonnerait tous les jours, choisis ou non.
    pub fn sonne_a(&self, aujourdhui: (i32, u32, u32), maintenant: (u32, u32)) -> bool {
        if self.jours != 0 {
            if self.date.is_some_and(|debut| debut > aujourdhui) || !self.tombe_un(aujourdhui) {
                return false;
            }
            return self.heure.is_none_or(|h| maintenant >= h);
        }
        match self.date {
            Some(jour) if jour > aujourdhui => return false,
            Some(jour) if jour < aujourdhui => return true,
            _ => {}
        }
        match self.heure {
            Some(h) => maintenant >= h,
            None => true,
        }
    }
}

#[cfg(test)]
mod alarme_tests {
    use super::*;

    /// **LA SEULE RÈGLE DE TEMPS DU MODÈLE**, et elle a QUATRE cas — pas deux.
    /// Le troisième est celui qu'on oublie : une date PASSÉE doit sonner, parce
    /// qu'un rappel en retard doit se voir.
    #[test]
    fn la_date_et_l_heure_decident_ensemble() {
        let aujourdhui = (2026, 9, 25);
        let alarme = |date, heure| AlarmeNote { date, heure, jours: 0 };
        let midi = (12, 0);

        // Sans date ni heure : tout de suite, même à l'instant zéro.
        assert!(alarme(None, None).sonne_a(aujourdhui, (0, 0)));

        // Une date FUTURE ne sonne pas, quelle que soit l'heure.
        assert!(!alarme(Some((2026, 9, 26)), None).sonne_a(aujourdhui, (23, 59)));
        assert!(!alarme(Some((2026, 9, 26)), Some((0, 0))).sonne_a(aujourdhui, (23, 59)));

        // Une date PASSÉE sonne — le rappel est en retard, il doit se voir.
        assert!(alarme(Some((2026, 9, 24)), Some((23, 59))).sonne_a(aujourdhui, (0, 0)));

        // Le jour même, c'est l'HEURE qui décide.
        assert!(!alarme(None, Some((15, 30))).sonne_a(aujourdhui, midi), "une heure et demie trop tôt");
        assert!(alarme(None, Some((15, 30))).sonne_a(aujourdhui, (15, 30)), "pile à l'heure");
        assert!(alarme(None, Some((15, 30))).sonne_a(aujourdhui, (16, 0)), "après");
        assert!(alarme(Some(aujourdhui), Some((11, 0))).sonne_a(aujourdhui, midi), "le jour dit, l'heure passée");
    }

    /// Le 2026-09-25 est un vendredi ; le 1ᵉʳ mars 2000 un mercredi (année
    /// bissextile séculaire, le cas où la formule se trompe le plus volontiers).
    #[test]
    fn le_jour_de_semaine_est_juste() {
        assert_eq!(JOURS_SEMAINE[jour_de_semaine((2026, 9, 25)) as usize], "ven");
        assert_eq!(JOURS_SEMAINE[jour_de_semaine((2026, 9, 27)) as usize], "dim");
        assert_eq!(JOURS_SEMAINE[jour_de_semaine((2026, 1, 1)) as usize], "jeu");
        assert_eq!(JOURS_SEMAINE[jour_de_semaine((2000, 3, 1)) as usize], "mer");
        assert_eq!(JOURS_SEMAINE[jour_de_semaine((2024, 2, 29)) as usize], "jeu");
    }

    /// **UNE ALARME RÉPÉTÉE** : les jours choisis seulement, à partir de sa date
    /// de début, et jamais « en retard ».
    #[test]
    fn une_alarme_repetee_suit_ses_jours() {
        let vendredi = (2026, 9, 25);
        let samedi = (2026, 9, 26);
        let lundi_vendredi = 0b0001_0001;
        let alarme = AlarmeNote { date: None, heure: Some((8, 0)), jours: lundi_vendredi };
        assert!(alarme.sonne_a(vendredi, (8, 0)));
        assert!(!alarme.sonne_a(vendredi, (7, 59)), "pas avant l'heure");
        assert!(!alarme.sonne_a(samedi, (9, 0)), "pas un jour non choisi");
        assert!(alarme.sonne_a((2026, 9, 28), (9, 0)), "le lundi suivant");

        // La date est un DÉBUT : rien avant, et une date passée ne rend pas
        // tous les jours sonnants.
        let depuis_lundi = AlarmeNote { date: Some((2026, 9, 28)), heure: None, jours: TOUS_LES_JOURS };
        assert!(!depuis_lundi.sonne_a(vendredi, (12, 0)));
        assert!(depuis_lundi.sonne_a((2026, 10, 3), (0, 0)));
        let passee = AlarmeNote { date: Some((2026, 9, 1)), heure: Some((23, 0)), jours: lundi_vendredi };
        assert!(!passee.sonne_a(samedi, (23, 30)), "une alarme répétée n'est jamais en retard");
    }

    /// Une note neuve n'a pas d'alarme : elle doit se poser exprès, sinon chaque
    /// note créée se mettrait à sonner.
    #[test]
    fn une_note_neuve_n_a_pas_d_alarme() {
        let note = Note::new(1, "du texte".to_string());
        assert_eq!(note.alarme, None);
    }
}

impl Note {
    /// Construit une note fraîchement créée, sans image : `created_at ==
    /// updated_at`.
    pub fn new(id: u64, content: String) -> Self {
        Self::new_with_images(id, content, Vec::new())
    }

    /// Comme `new`, avec des images déjà prêtes (voir `config::
    /// store_note_image`, seul fournisseur en pratique) — utilisé par le
    /// collage d'image (voir `win32::cef_notes::handle_paste`), qui crée la
    /// note et son image en une seule fois plutôt qu'en deux étapes (pas
    /// d'état "en attente" à maintenir, voir doc de module de
    /// `win32::cef_notes`).
    pub fn new_with_images(id: u64, content: String, images: Vec<NoteImage>) -> Self {
        let now = unix_now();
        Self {
            id,
            content: truncate_chars(&content, MAX_NOTE_CHARS),
            pinned: false,
            created_at: now,
            updated_at: now,
            images,
            // Une note neuve n'a pas d'alarme : elle se pose exprès.
            alarme: None,
            archived: false,
        }
    }

    /// `true` si `content` (une fois `trim()`-é) et `images` sont TOUS LES
    /// DEUX vides — remplace le test `content.trim().is_empty()` utilisé
    /// seul par `win32::cef_notes` avant l'ajout des images : une note
    /// composée uniquement d'une image collée (`content` vide) n'est PAS
    /// vide. Prend `content`/`images` en paramètres plutôt que `&self` : le
    /// seul appelant (`win32::cef_notes::handle_update`) doit statuer sur un
    /// contenu CANDIDAT pas encore assigné à la note, avant d'écraser
    /// `note.content`. `pub(crate)` : appelé depuis `win32::cef_notes`.
    pub(crate) fn content_and_images_are_empty(content: &str, images: &[NoteImage]) -> bool {
        content.trim().is_empty() && images.is_empty()
    }

    /// Attache `image`, sauf si `MAX_NOTE_IMAGES` est déjà atteint (renvoie
    /// `false` sans rien modifier) — pour "coller dans une note existante"
    /// (voir `win32::cef_notes::handle_paste_into`).
    pub fn push_image(&mut self, image: NoteImage) -> bool {
        if self.images.len() >= MAX_NOTE_IMAGES {
            return false;
        }
        self.images.push(image);
        true
    }

    /// Retire l'image à `index` — `false` si hors bornes, sans rien
    /// modifier (index déjà obsolète : la note a changé entre l'affichage
    /// et le clic, ou message IPC corrompu).
    pub fn remove_image_at(&mut self, index: usize) -> bool {
        if index >= self.images.len() {
            return false;
        }
        self.images.remove(index);
        true
    }
}

/// Ensemble des noms de fichiers (pleine résolution ET miniature, voir
/// `thumb_file_name`) actuellement référencés par au moins une note — le
/// "keep set" du nettoyage de `config::save_notes` : tout fichier du dossier
/// `notes_images/` absent de cet ensemble est orphelin (image retirée d'une
/// note ou note supprimée) et peut être supprimé sans perte. PURE et
/// testable sans toucher au disque, contrairement au nettoyage lui-même
/// (voir `config::save_notes`) — une erreur ici serait une perte de données
/// silencieuse (fichier encore utilisé supprimé à tort), d'où des tests
/// dédiés.
pub fn referenced_image_files(notes: &[Note]) -> std::collections::HashSet<String> {
    let mut files = std::collections::HashSet::new();
    for note in notes {
        for image in &note.images {
            files.insert(thumb_file_name(&image.file));
            files.insert(image.file.clone());
        }
    }
    files
}

/// Horodatage courant en secondes Unix — `SystemTime::now()` peut en théorie
/// précéder `UNIX_EPOCH` (horloge système mal réglée) ; on retombe alors sur
/// `0` plutôt que de paniquer (même discipline best-effort que le reste de ce
/// module vis-à-vis d'un environnement anormal). `pub(crate)` : réutilisé par
/// `win32::cef_notes` (`handle_paste_into`/`handle_remove_image`) plutôt que
/// dupliquer la même recette.
pub(crate) fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Tronque `s` à `max_chars` CARACTÈRES (pas octets) — un contenu accentué/
/// Unicode ne doit jamais faire paniquer une coupe en plein milieu d'un
/// caractère multi-octets, même discipline que `app::truncate_with_ellipsis`.
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Prochain identifiant disponible : `max(id existant) + 1`, `1` si la liste
/// est vide. Préféré à un identifiant dérivé de l'horodatage : pur,
/// déterministe, sans risque de collision en cas de créations rapprochées
/// (résolution à la seconde) ou de décalage d'horloge.
pub fn next_id(notes: &[Note]) -> u64 {
    notes.iter().map(|n| n.id).max().map_or(1, |max| max + 1)
}

/// Extrait les étiquettes `#tag` du contenu, dans l'ordre de première
/// apparition, dédupliquées sans tenir compte de la casse (la casse
/// conservée est celle de la PREMIÈRE occurrence). Règles (voir tests) :
/// - un `#` ne démarre une étiquette que s'il est en début de chaîne ou
///   précédé d'un caractère d'espacement — un `#` au milieu d'un mot
///   (`a#b`) n'en est pas une ;
/// - l'étiquette doit contenir au moins un caractère alphanumérique
///   (Unicode, donc `#idée` est capturé en entier) ou `_`/`-` après le `#` —
///   un `#` seul, ou suivi d'un espace, n'est pas une étiquette ;
/// - elle s'arrête au premier caractère hors de cet ensemble (ponctuation,
///   espace, fin de chaîne) ;
/// - conséquence volontaire : `# Titre` et `## Titre` (titres Markdown, `#`
///   suivi d'un espace) ne produisent AUCUNE étiquette ;
/// - ⭐ **LES ÉTIQUETTES S'IMBRIQUENT** : `#projets/neuradesk` est l'étiquette
///   `projets/neuradesk`, rangée sous `projets` dans l'arbre du panneau —
///   DEMANDE EXPLICITE (2026-10-11), d'après memos. Les `/` en trop disparaissent
///   (`#a//b/` → `a/b`) : un niveau vide ne se range nulle part ;
/// - ⚠️ **RIEN DANS DU CODE** (voir `masque_de_code`) : un `#include` dans un bloc
///   ```` ``` ```` est du code, pas une étiquette.
pub fn extract_tags(content: &str) -> Vec<String> {
    let chars: Vec<char> = content.chars().collect();
    let en_code = masque_de_code(&chars);
    let mut tags: Vec<String> = Vec::new();
    let mut seen_lower: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut i = 0;
    while i < chars.len() {
        let at_boundary = i == 0 || chars[i - 1].is_whitespace();
        if chars[i] == '#' && at_boundary && !en_code[i] {
            let mut j = i + 1;
            while j < chars.len() && is_tag_char(chars[j]) {
                j += 1;
            }
            let brut: String = chars[i + 1..j].iter().collect();
            let tag = brut.split('/').filter(|niveau| !niveau.is_empty()).collect::<Vec<_>>().join("/");
            if !tag.is_empty() {
                let lower = tag.to_lowercase();
                if seen_lower.insert(lower) {
                    tags.push(tag);
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    tags
}

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || c == '/'
}

/// Une ligne qui ouvre ou ferme un bloc de code : ```` ``` ```` en tête, espaces
/// permis devant.
fn est_une_cloture(ligne: &[char]) -> bool {
    let debut = ligne.iter().take_while(|c| **c == ' ' || **c == '\t').count();
    ligne[debut..].starts_with(&['`', '`', '`'])
}

/// **CE QUI EST DU CODE, CARACTÈRE PAR CARACTÈRE** — les blocs ```` ``` ````
/// (clôtures comprises ; un bloc non refermé court jusqu'à la fin) et les
/// `` `code` `` d'une ligne.
///
/// ⛔ **LA MÊME RÈGLE QUE LA PAGE** (`zonesProtegees` dans `notes.html`) : les
/// deux décident où sont les tâches à cocher, et `basculer_tache` désigne une
/// tâche par son RANG — si l'une comptait une tâche que l'autre saute, cocher
/// la troisième en cocherait une autre. Un `` ` `` se ferme sur le PROCHAIN de
/// sa ligne, et deux collés ne font rien (`` /`([^`\n]+)`/g ``).
fn masque_de_code(chars: &[char]) -> Vec<bool> {
    let mut masque = vec![false; chars.len()];
    let mut dans_un_bloc = false;
    let mut debut = 0;
    while debut < chars.len() {
        let fin = chars[debut..].iter().position(|c| *c == '\n').map_or(chars.len(), |p| debut + p);
        let ligne = &chars[debut..fin];
        if est_une_cloture(ligne) {
            dans_un_bloc = !dans_un_bloc;
            masque[debut..fin].fill(true);
        } else if dans_un_bloc {
            masque[debut..fin].fill(true);
        } else {
            let mut i = 0;
            while i < ligne.len() {
                if ligne[i] == '`' {
                    if let Some(k) = ligne[i + 1..].iter().position(|c| *c == '`').map(|p| i + 1 + p) {
                        if k > i + 1 {
                            masque[debut + i..=debut + k].fill(true);
                            i = k + 1;
                            continue;
                        }
                    }
                }
                i += 1;
            }
        }
        debut = fin + 1;
    }
    masque
}

/// **LA TÂCHE D'UNE LIGNE** — la position du caractère entre crochets
/// (` `, `x` ou `X`) d'une ligne `- [ ] …` ou `* [x] …`, ou `None`. Le crochet
/// fermant doit être suivi d'une espace ou de la fin de ligne : `- [x]truc`
/// n'est pas une tâche.
fn case_de_tache(ligne: &[char]) -> Option<usize> {
    let retrait = ligne.iter().take_while(|c| **c == ' ' || **c == '\t').count();
    let reste = &ligne[retrait..];
    let tache = reste.len() >= 5
        && (reste[0] == '-' || reste[0] == '*')
        && reste[1] == ' '
        && reste[2] == '['
        && matches!(reste[3], ' ' | 'x' | 'X')
        && reste[4] == ']'
        && reste.get(5).is_none_or(|c| *c == ' ');
    tache.then_some(retrait + 3)
}

/// **COCHE OU DÉCOCHE LA `rang`-IÈME TÂCHE** (à partir de 0) d'une note —
/// DEMANDE EXPLICITE (2026-10-11), d'après memos : `- [ ] acheter du pain`
/// s'affiche en case à cocher, et un clic l'écrit DANS le texte (`[x]`). `None`
/// si la note n'a pas autant de tâches. Les lignes d'un bloc de code ne
/// comptent pas (voir `masque_de_code`).
pub fn basculer_tache(content: &str, rang: usize) -> Option<String> {
    let mut chars: Vec<char> = content.chars().collect();
    let en_code = masque_de_code(&chars);
    let mut vues = 0;
    let mut debut = 0;
    while debut <= chars.len() {
        let fin = chars[debut..].iter().position(|c| *c == '\n').map_or(chars.len(), |p| debut + p);
        if !en_code.get(debut).copied().unwrap_or(false) {
            if let Some(case) = case_de_tache(&chars[debut..fin]) {
                if vues == rang {
                    let i = debut + case;
                    chars[i] = if chars[i] == ' ' { 'x' } else { ' ' };
                    return Some(chars.into_iter().collect());
                }
                vues += 1;
            }
        }
        debut = fin + 1;
    }
    None
}

/// Nombre maximal de vues enregistrées — voir `VueNotes`.
pub const MAX_VUES_NOTES: usize = 30;
/// Longueur maximale (en caractères) du nom d'une vue.
const MAX_NOM_VUE: usize = 40;

/// **UNE VUE ENREGISTRÉE DU PANNEAU NOTES** — un filtre retenu sous un nom
/// (voir `config::load_vues_notes`). `etiquette` est le chemin en minuscules,
/// vide pour « aucune » — c'est la forme que la page compare (`activeTag`).
/// Le jour du calendrier n'en fait pas partie : un filtre de date se périme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueNotes {
    pub nom: String,
    pub recherche: String,
    pub etiquette: String,
    pub epinglees: bool,
    pub archivees: bool,
}

impl VueNotes {
    /// `None` pour un nom vide une fois rogné — une vue sans nom ne se
    /// retrouverait pas dans la liste.
    pub fn nouvelle(nom: &str, recherche: &str, etiquette: &str, epinglees: bool, archivees: bool) -> Option<Self> {
        let nom = truncate_chars(nom.trim(), MAX_NOM_VUE);
        (!nom.is_empty()).then(|| Self {
            nom,
            recherche: truncate_chars(recherche, MAX_NOTE_CHARS),
            etiquette: truncate_chars(&etiquette.to_lowercase(), MAX_NOTE_CHARS),
            epinglees,
            archivees,
        })
    }
}

/// Range `vue` dans `vues` : une vue du MÊME nom (sans tenir compte de la casse)
/// est remplacée à sa place, sinon elle s'ajoute à la fin — tant qu'il reste de
/// la place. Rend `false` si la liste est pleine.
pub fn enregistrer_vue(vues: &mut Vec<VueNotes>, vue: VueNotes) -> bool {
    if let Some(existante) = vues.iter_mut().find(|v| v.nom.to_lowercase() == vue.nom.to_lowercase()) {
        *existante = vue;
        return true;
    }
    if vues.len() >= MAX_VUES_NOTES {
        return false;
    }
    vues.push(vue);
    true
}

/// Ordre d'affichage : épinglées d'abord, puis les plus récemment modifiées
/// en tête ; à égalité de `updated_at` (créations rapprochées, résolution à
/// la seconde), départage par `id` décroissant pour un résultat déterministe
/// plutôt que dépendant de l'ordre de stockage.
pub fn sorted_for_display(notes: &[Note]) -> Vec<&Note> {
    let mut sorted: Vec<&Note> = notes.iter().collect();
    sorted.sort_by_key(|n| (!n.pinned, std::cmp::Reverse(n.updated_at), std::cmp::Reverse(n.id)));
    sorted
}

#[cfg(test)]
mod tag_extraction_tests {
    use super::extract_tags;

    #[test]
    fn extracts_several_tags_in_order_of_first_appearance() {
        let tags = extract_tags("Idée pour #neuradesk et #ui, revoir #neuradesk plus tard");
        assert_eq!(tags, vec!["neuradesk".to_string(), "ui".to_string()]);
    }

    #[test]
    fn deduplicates_case_insensitively_keeping_first_casing() {
        let tags = extract_tags("#Projet puis #projet puis #PROJET");
        assert_eq!(tags, vec!["Projet".to_string()]);
    }

    #[test]
    fn markdown_headings_produce_no_tag() {
        assert!(extract_tags("# Titre").is_empty());
        assert!(extract_tags("## Sous-titre").is_empty());
    }

    #[test]
    fn hash_mid_word_is_not_a_tag() {
        assert!(extract_tags("a#b").is_empty());
    }

    #[test]
    fn unicode_tag_is_captured_whole() {
        assert_eq!(extract_tags("une #idée à creuser"), vec!["idée".to_string()]);
    }

    #[test]
    fn trailing_punctuation_is_excluded() {
        assert_eq!(extract_tags("voir #projet, demain"), vec!["projet".to_string()]);
    }

    #[test]
    fn empty_content_yields_no_tags() {
        assert!(extract_tags("").is_empty());
    }

    #[test]
    fn underscore_and_dash_are_valid_tag_characters() {
        assert_eq!(extract_tags("#mon_tag-1 ici"), vec!["mon_tag-1".to_string()]);
    }

    #[test]
    fn une_etiquette_s_imbrique_et_perd_ses_niveaux_vides() {
        assert_eq!(extract_tags("#projets/neuradesk et #a//b/ puis #/"), vec!["projets/neuradesk".to_string(), "a/b".to_string()]);
    }

    #[test]
    fn rien_dans_du_code_n_est_une_etiquette() {
        let texte = "#vrai `#faux` ici\n```c\n#include <stdio.h>\n```\n#apres";
        assert_eq!(extract_tags(texte), vec!["vrai".to_string(), "apres".to_string()]);
    }
}

#[cfg(test)]
mod vues_tests {
    use super::{enregistrer_vue, VueNotes, MAX_VUES_NOTES};

    #[test]
    fn une_vue_du_meme_nom_est_remplacee_a_sa_place() {
        let mut vues = Vec::new();
        assert!(enregistrer_vue(&mut vues, VueNotes::nouvelle("Travail", "", "Projets", false, false).unwrap()));
        assert!(enregistrer_vue(&mut vues, VueNotes::nouvelle("Perso", "x", "", true, false).unwrap()));
        assert!(enregistrer_vue(&mut vues, VueNotes::nouvelle(" travail ", "réunion", "", false, true).unwrap()));
        assert_eq!(vues.len(), 2);
        assert_eq!(vues[0].nom, "travail");
        assert_eq!(vues[0].recherche, "réunion");
        assert!(vues[0].archivees);
        assert_eq!(VueNotes::nouvelle("A", "", "Projets/NeuraDesk", false, false).unwrap().etiquette, "projets/neuradesk");
    }

    #[test]
    fn une_vue_sans_nom_ou_une_liste_pleine_est_refusee() {
        assert!(VueNotes::nouvelle("   ", "", "", false, false).is_none());
        let mut vues: Vec<VueNotes> =
            (0..MAX_VUES_NOTES).map(|i| VueNotes::nouvelle(&format!("v{i}"), "", "", false, false).unwrap()).collect();
        assert!(!enregistrer_vue(&mut vues, VueNotes::nouvelle("en trop", "", "", false, false).unwrap()));
        assert_eq!(vues.len(), MAX_VUES_NOTES);
    }
}

#[cfg(test)]
mod taches_tests {
    use super::basculer_tache;

    #[test]
    fn cocher_puis_decocher_la_bonne_tache() {
        let texte = "courses\n- [ ] pain\n  * [x] lait\n- [ ] œufs";
        assert_eq!(basculer_tache(texte, 0).as_deref(), Some("courses\n- [x] pain\n  * [x] lait\n- [ ] œufs"));
        assert_eq!(basculer_tache(texte, 1).as_deref(), Some("courses\n- [ ] pain\n  * [ ] lait\n- [ ] œufs"));
        assert_eq!(basculer_tache(texte, 2).as_deref(), Some("courses\n- [ ] pain\n  * [x] lait\n- [x] œufs"));
        assert_eq!(basculer_tache(texte, 3), None);
    }

    /// Une tâche dans un bloc de code ne compte pas — sinon le rang de la page
    /// (qui ne l'affiche pas en case) et celui-ci divergeraient.
    #[test]
    fn une_tache_dans_du_code_ne_compte_pas() {
        let texte = "```\n- [ ] pas une tâche\n```\n- [ ] la vraie";
        assert_eq!(basculer_tache(texte, 0).as_deref(), Some("```\n- [ ] pas une tâche\n```\n- [x] la vraie"));
    }

    #[test]
    fn ce_qui_n_est_pas_une_tache() {
        assert_eq!(basculer_tache("-[ ] a\n- [x]b\n- [y] c\n[ ] d", 0), None);
        assert_eq!(basculer_tache("- [ ]", 0).as_deref(), Some("- [x]"));
    }
}

#[cfg(test)]
mod next_id_tests {
    use super::{next_id, Note};

    fn note_with_id(id: u64) -> Note {
        Note {
            id,
            content: String::new(),
            pinned: false,
            created_at: 0,
            updated_at: 0,
            images: Vec::new(),
            alarme: None,
            archived: false,
        }
    }

    #[test]
    fn empty_list_starts_at_one() {
        assert_eq!(next_id(&[]), 1);
    }

    #[test]
    fn returns_max_plus_one_even_when_unsorted_or_non_contiguous() {
        let notes = vec![note_with_id(5), note_with_id(1), note_with_id(9)];
        assert_eq!(next_id(&notes), 10);
    }
}

#[cfg(test)]
mod sorted_for_display_tests {
    use super::{sorted_for_display, Note};

    fn note(id: u64, pinned: bool, updated_at: i64) -> Note {
        Note {
            id,
            content: String::new(),
            pinned,
            created_at: updated_at,
            updated_at,
            images: Vec::new(),
            alarme: None,
            archived: false,
        }
    }

    #[test]
    fn pinned_notes_come_first_regardless_of_timestamp() {
        let notes = vec![note(1, false, 100), note(2, true, 10)];
        let ids: Vec<u64> = sorted_for_display(&notes).iter().map(|n| n.id).collect();
        assert_eq!(ids, vec![2, 1]);
    }

    #[test]
    fn within_a_group_most_recently_updated_comes_first() {
        let notes = vec![note(1, false, 10), note(2, false, 30), note(3, false, 20)];
        let ids: Vec<u64> = sorted_for_display(&notes).iter().map(|n| n.id).collect();
        assert_eq!(ids, vec![2, 3, 1]);
    }

    #[test]
    fn equal_updated_at_falls_back_to_descending_id() {
        let notes = vec![note(1, false, 10), note(2, false, 10)];
        let ids: Vec<u64> = sorted_for_display(&notes).iter().map(|n| n.id).collect();
        assert_eq!(ids, vec![2, 1]);
    }

    #[test]
    fn empty_input_yields_empty_output() {
        assert!(sorted_for_display(&[]).is_empty());
    }
}

#[cfg(test)]
mod truncate_tests {
    use super::{truncate_chars, MAX_NOTE_CHARS};

    #[test]
    fn truncates_on_characters_not_bytes() {
        // "é" est un seul caractère mais plusieurs octets en UTF-8 — une
        // coupe naïve sur les octets pourrait paniquer ici.
        let content = "é".repeat(MAX_NOTE_CHARS + 10);
        let truncated = truncate_chars(&content, MAX_NOTE_CHARS);
        assert_eq!(truncated.chars().count(), MAX_NOTE_CHARS);
    }
}

#[cfg(test)]
mod image_tests {
    use super::{Note, NoteImage, MAX_NOTE_IMAGES};

    fn image(file: &str) -> NoteImage {
        NoteImage { file: file.to_string(), width: 10, height: 10, thumb_data_uri: "data:image/png;base64,AAAA".to_string() }
    }

    #[test]
    fn image_only_note_is_not_empty() {
        let note = Note::new_with_images(1, String::new(), vec![image("a.png")]);
        assert!(!Note::content_and_images_are_empty(&note.content, &note.images));
    }

    #[test]
    fn blank_content_and_no_image_is_empty() {
        let note = Note::new_with_images(1, "   ".to_string(), Vec::new());
        assert!(Note::content_and_images_are_empty(&note.content, &note.images));
    }

    #[test]
    fn push_image_refuses_beyond_max_note_images() {
        let mut note = Note::new(1, String::new());
        for i in 0..MAX_NOTE_IMAGES {
            assert!(note.push_image(image(&format!("{i}.png"))));
        }
        assert!(!note.push_image(image("one-too-many.png")));
        assert_eq!(note.images.len(), MAX_NOTE_IMAGES);
    }

    #[test]
    fn remove_image_at_out_of_bounds_returns_false() {
        let mut note = Note::new_with_images(1, String::new(), vec![image("a.png")]);
        assert!(!note.remove_image_at(1));
        assert_eq!(note.images.len(), 1);
    }

    #[test]
    fn remove_image_at_shifts_remaining_indices() {
        let mut note = Note::new_with_images(1, String::new(), vec![image("a.png"), image("b.png"), image("c.png")]);
        assert!(note.remove_image_at(0));
        assert_eq!(note.images.iter().map(|i| i.file.as_str()).collect::<Vec<_>>(), vec!["b.png", "c.png"]);
    }

    #[test]
    fn new_with_images_truncates_content_but_keeps_images() {
        let content = "é".repeat(super::MAX_NOTE_CHARS + 10);
        let note = Note::new_with_images(1, content, vec![image("a.png")]);
        assert_eq!(note.content.chars().count(), super::MAX_NOTE_CHARS);
        assert_eq!(note.images.len(), 1);
    }
}

#[cfg(test)]
mod referenced_image_files_tests {
    use super::{referenced_image_files, Note, NoteImage};

    fn image(file: &str) -> NoteImage {
        NoteImage { file: file.to_string(), width: 10, height: 10, thumb_data_uri: "data:image/png;base64,AAAA".to_string() }
    }

    #[test]
    fn two_notes_sharing_one_file_yield_a_single_entry() {
        let notes = vec![
            Note::new_with_images(1, String::new(), vec![image("shared.png")]),
            Note::new_with_images(2, String::new(), vec![image("shared.png")]),
        ];
        let files = referenced_image_files(&notes);
        assert!(files.contains("shared.png"));
        assert!(files.contains("shared.thumb.png"));
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn includes_both_full_and_thumb_names_for_each_image() {
        let notes = vec![Note::new_with_images(1, String::new(), vec![image("a.png"), image("b.png")])];
        let files = referenced_image_files(&notes);
        assert_eq!(files.len(), 4);
        for name in ["a.png", "a.thumb.png", "b.png", "b.thumb.png"] {
            assert!(files.contains(name), "manquant : {name}");
        }
    }

    #[test]
    fn notes_without_images_yield_an_empty_set() {
        let notes = vec![Note::new(1, "sans image".to_string())];
        assert!(referenced_image_files(&notes).is_empty());
    }
}
