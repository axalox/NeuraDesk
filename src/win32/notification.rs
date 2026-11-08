//! Notifications Windows : quelles APPLICATIONS en ont une en attente.
//!
//! Sert à faire clignoter l'indicateur de l'icône d'une application qui vient
//! d'envoyer une notification (DEMANDE EXPLICITE : « il faudrait aussi que
//! l'indicateur clignote de plusieurs couleurs si l'app envoit une
//! notification »).
//!
//! **Au lancement, l'historique du centre de notifications est tenu pour DÉJÀ VU**
//! (voir `UnreadState::seed`) : seules les notifications qui arrivent pendant que
//! l'application tourne font clignoter. Sans cet amorçage, tout ce qui traîne
//! dans le centre se rallume au démarrage — DÉFAUT RÉEL SIGNALÉ, « au lancement
//! de l'app j'ai toujours une notification qui apparaît pour Chrome ».
//!
//! # ⛔ LA FRONTIÈRE DE CONFIDENTIALITÉ A ÉTÉ FRANCHIE — 2026-10-08, SUR DEMANDE
//!
//! **Ce module ne lisait QUE l'identifiant de l'application émettrice.** La
//! colonne `Payload` — qui porte le TEXTE des messages — **n'était nommée par
//! aucune requête de ce fichier**, et cette promesse se vérifiait en le lisant :
//! le contenu n'était pas seulement « non lu », il était **hors d'atteinte par
//! construction**.
//!
//! **ELLE EST LEVÉE, ET IL FAUT LE SAVOIR.** Sur demande explicite de
//! l'utilisateur (2026-10-08) : « j'aimerais faire s'afficher les
//! notifications en temps réel sous l'horloge de la barre principale au lieu
//! qu'elles s'affichent en bas à droite de l'écran ». Déplacer un toast du shell
//! est **impossible** — ils sont rendus par le shell, aucune API n'expose leur
//! position. La seule voie est de les **lire** et de les **réafficher**, donc de
//! lire ce texte.
//!
//! ⭐ **CE QUE LA MESURE A ÉTABLI AVANT D'ÉCRIRE** (sonde `regarde_le_payload`) :
//! le BLOB est du **XML EN CLAIR** qui commence par `<toast>` — lisible, non
//! compressé, avec le titre et le message dans des éléments `<text>`. Ce n'était
//! pas acquis : un BLOB opaque aurait fermé la question.
//!
//! ⚠️ **CE QUI RESTE VRAI** : la base est ouverte **en lecture seule**
//! (`SQLITE_OPEN_READONLY`), et c'est une garantie du drapeau, pas du code
//! au-dessus. Rien ici n'écrit, ni dans la base, ni dans le centre.
//!
//! ⚠️ **ET CE QU'UN APPELANT DOIT SAVOIR** : `Payload` porte tout ce que
//! l'application émettrice a voulu dire. Le lire, c'est lire tes messages — donc
//! **rien de tout cela ne doit sortir de la machine**, et rien ne doit être
//! journalisé. Les tests de ce fichier n'impriment que des LONGUEURS.
//!
//! # Pourquoi pas `UserNotificationListener`
//!
//! C'est l'API que la documentation Microsoft met en avant pour ça, et elle est
//! HORS DE PORTÉE : elle exige une identité de paquet (MSIX ou paquet sparse) et
//! une capacité déclarée, ce qu'un exécutable portable n'a pas. La voie de
//! l'identité changerait le modèle de distribution du projet (certificat,
//! installation) — écartée.
//!
//! # Ce qui a été MESURÉ avant d'écrire ce fichier (2026-09-16)
//!
//! Schéma relevé en lecture seule sur Windows 11 26200 :
//! - `NotificationHandler(RecordId INTEGER PRIMARY KEY, PrimaryId TEXT NOT NULL
//!   COLLATE NOCASE, HandlerType TEXT, …)`, index unique sur `PrimaryId` —
//!   **`PrimaryId` EST l'identifiant d'application** ;
//! - `Notification(Order INTEGER PRIMARY KEY, HandlerId, Type TEXT, ArrivalTime,
//!   Payload BLOB, …)`. ⛔ `Order` n'est PAS monotone dans le temps : clé
//!   entière sans `AUTOINCREMENT`, SQLite la RÉUTILISE après un effacement
//!   (mesuré le 2026-11-02 : 11752 en septembre, 11709 ce jour-là). Le repère
//!   « plus récente que » est `ArrivalTime` (FILETIME).
//!
//! Une notification postée DEPUIS UNE APPLICATION DE BUREAU y arrive bien, avec
//! `HandlerType = 'app:desktop'` et un `PrimaryId` qui est un CHEMIN complet
//! terminé par l'exécutable (`…\WindowsPowerShell\v1.0\powershell.exe`) — c'est
//! ce que [`exe_named_by`] exploite. Les applications du Store y arrivent avec
//! leur identité de paquet (`Microsoft.YourPhone_8wekyb3d8bbwe!App`).
//!
//! # Le piège du mode WAL, et pourquoi la connexion n'est PAS gardée
//!
//! La base est en mode WAL. Une connexion `SQLITE_OPEN_READONLY` de longue durée
//! peut cesser de voir les commits suivants, et surtout elle **échouerait
//! définitivement** si le service s'arrêtait un instant — alors que cet échec-là
//! est TRANSITOIRE, contrairement à celui de `win32::voicemeeter` (dont le
//! `OnceLock<Result<…>>` « jamais réessayé » est donc le mauvais modèle ici).
//! La connexion est donc **ouverte et fermée à chaque scrutation** : moins d'une
//! milliseconde, auto-réparateur, et cela supprime du même coup toute question
//! de durée de vie dans un `thread_local!`.
//!
//! **Ce que ce module met dans un `thread_local!` n'est que de la mémoire**
//! (`UnreadState`, un booléen) : aucun handle système, donc rien qui puisse
//! poser le problème d'ordre de destruction qui a fait mourir l'application à
//! chaque fermeture (voir la mémoire du projet sur `app::GpuCacheGuard`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{c_void, CStr, CString};
use std::sync::OnceLock;

use windows::core::{s, w, PCSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};

/// Identifiants dont aucune icône ne s'est réclamée pendant ce nombre de
/// scrutations consécutives avant d'être acquittés d'office.
///
/// Sans cette sortie de secours, une notification d'une application qui n'a
/// AUCUNE icône dans le dock serait impossible à acquitter : elle s'accumulerait
/// indéfiniment, et « clignote jusqu'au clic » échouerait en silence — sans
/// qu'aucun clic ne puisse jamais arriver. Sur la machine de développement, ce
/// n'est pas un cas de bord : les notifications relayées par Phone Link visent
/// une application (le téléphone) que l'utilisateur n'a pas forcément épinglée.
const UNCLAIMED_POLLS_BEFORE_ACK: u8 = 3;

/// Requête de scrutation — **la colonne `Payload` n'y figure pas, et ne doit
/// JAMAIS y figurer** (voir la frontière de confidentialité, doc de module).
///
/// `Type = 'toast'` est PORTEUR : la table contient aussi des `badge`, qui
/// vivent des semaines (une pastille Teams de vingt jours a été mesurée) et
/// marqueraient leur application comme notifiée pour toujours.
///
/// Le plus récent par identifiant : c'est le plus haut atteint qui compte (voir
/// `UnreadState::observe`, qui ne le fait jamais redescendre).
///
/// ⛔ **`MAX(ArrivalTime)`, PAS `MAX([Order])`** (corrigé le 2026-11-02) :
/// SQLite RÉUTILISE les `Order` après un effacement (voir `QUERY_CONTENU`). Un
/// toast neuf portait alors un `Order` plus bas que le plus haut atteint, et
/// son application ne clignotait jamais. `UnreadState` compare des `i64` sans
/// leur donner de sens : seule la colonne change.
const QUERY: &str = "SELECT h.PrimaryId, MAX(n.ArrivalTime) \
                     FROM Notification n JOIN NotificationHandler h ON h.RecordId = n.HandlerId \
                     WHERE n.Type = 'toast' GROUP BY h.PrimaryId";

/// ⛔ **LA SEULE REQUÊTE DU FICHIER QUI NOMME `Payload` — ET ELLE EST NOMMÉE
/// POUR ÇA.** La frontière de confidentialité a été franchie sur demande
/// explicite le 2026-10-08 (voir la doc de module) : la voisine ci-dessus,
/// elle, ne doit **toujours pas** la nommer.
///
/// ⚠️ **`ORDER BY [Order] ASC`** : ici l'ordre d'ARRIVÉE compte, on affiche dans
/// l'ordre où Windows les a reçues — là où `query_rows` ne garde que le plus
/// haut par application.
///
/// ⛔ **LE CURSEUR EST `ArrivalTime`, PAS `Order`** (corrigé le 2026-11-02).
/// `Order` est un `INTEGER PRIMARY KEY` SANS `AUTOINCREMENT` : SQLite le
/// REPREND à « plus grand + 1 » quand l'utilisateur efface ses notifications.
/// Mesuré : le plus récent valait 11752 en septembre, 11709 le 2026-11-02, avec
/// des trous. Un seuil sur `Order` rendait le bandeau AVEUGLE après chaque
/// effacement du centre, jusqu'à ce que les numéros dépassent l'ancien maximum.
const QUERY_CONTENU: &str = "SELECT h.PrimaryId, n.[Order], n.Payload, n.ArrivalTime \
                             FROM Notification n JOIN NotificationHandler h ON h.RecordId = n.HandlerId \
                             WHERE n.Type = 'toast' AND n.ArrivalTime > {apres} \
                             ORDER BY n.ArrivalTime ASC, n.[Order] ASC";

/// **LES TOASTS ARRIVÉS APRÈS `apres`, CONTENU COMPRIS.**
///
/// ⚠️ **LE SEUIL EST ÉCRIT DANS LA REQUÊTE, PAS LIÉ** : `sqlite3_bind_int64`
/// n'est pas résolu par ce fichier, et l'ajouter pour un `i64` qui vient de la
/// même base serait une cérémonie. Il n'y a rien à injecter — la valeur ne peut
/// pas venir d'ailleurs.
///
/// Rend `None` quand la base est illisible (mêmes causes que `query_rows`), et
/// une liste VIDE quand il n'y a rien de neuf — les deux se distinguent, et
/// l'appelant en a besoin pour ne pas confondre « rien » et « panne ».
pub fn toasts_depuis(apres: i64) -> Option<Vec<ToastWindows>> {
    let api = api()?;
    let path = database_path()?;
    let chemin = CString::new(path.to_string_lossy().as_bytes()).ok()?;
    let requete = CString::new(QUERY_CONTENU.replace("{apres}", &apres.to_string())).ok()?;

    unsafe {
        let mut db: *mut c_void = std::ptr::null_mut();
        if (api.open_v2)(chemin.as_ptr(), &mut db, SQLITE_OPEN_READONLY, std::ptr::null()) != SQLITE_OK {
            if !db.is_null() {
                (api.close)(db);
            }
            return None;
        }
        (api.busy_timeout)(db, 50);

        let mut stmt: *mut c_void = std::ptr::null_mut();
        if (api.prepare_v2)(db, requete.as_ptr(), -1, &mut stmt, std::ptr::null_mut()) != SQLITE_OK {
            (api.close)(db);
            return None;
        }

        let mut toasts = Vec::new();
        loop {
            match (api.step)(stmt) {
                SQLITE_ROW => {
                    // `column_text` est appelé UNE fois : un second appel peut
                    // invalider le pointeur rendu par le premier (même règle que
                    // `query_rows`).
                    let identifiant = (api.column_text)(stmt, 0);
                    if identifiant.is_null() {
                        continue;
                    }
                    let aumid = CStr::from_ptr(identifiant as *const i8).to_string_lossy().into_owned();
                    let order = (api.column_int64)(stmt, 1);
                    // ⚠️ **LE BLOB EST LU ENTRE LES DEUX APPELS QUI LE
                    // DÉCRIVENT** : `column_bytes` rend une longueur valable pour
                    // le pointeur rendu par `column_blob`, et rien ne garantit
                    // qu'un autre appel à la même ligne ne les invalide pas.
                    let longueur = (api.column_bytes)(stmt, 2) as usize;
                    let pointeur = (api.column_blob)(stmt, 2) as *const u8;
                    let (titre, message) = if pointeur.is_null() || longueur == 0 {
                        (String::new(), String::new())
                    } else {
                        titre_et_message(std::slice::from_raw_parts(pointeur, longueur))
                    };
                    let arrivee = (api.column_int64)(stmt, 3);
                    toasts.push(ToastWindows { aumid, order, arrivee, titre, message });
                }
                SQLITE_DONE => break,
                _ => break,
            }
        }
        (api.finalize)(stmt);
        (api.close)(db);
        Some(toasts)
    }
}

// ---------------------------------------------------------------------------
// La règle d'identité
// ---------------------------------------------------------------------------

/// Exécutable nommé par un identifiant d'application, s'il en nomme un —
/// `…\WindowsPowerShell\v1.0\powershell.exe` → `powershell.exe`.
///
/// MESURÉ (voir la doc de module) : l'identifiant d'une notification d'une
/// application de bureau est un chemin terminé par son exécutable. Même
/// discipline que `win32::taskbar_pins::process_name_from_icon_path`, et pour
/// la même raison : **ne rend un nom que pour un `.exe`**, et se TAIT sinon.
/// Un identifiant opaque (`FirefoxToast-308046B0AF4A39CB`, mesuré dans la table
/// d'enregistrement de cette machine) ou une identité de paquet du Store n'est
/// PAS deviné à partir de sa forme — ce serait exactement l'inférence que cette
/// fonctionnalité s'interdit.
pub fn exe_named_by(aumid: &str) -> Option<&str> {
    let name = aumid.rsplit(['\\', '/']).next()?;
    let (stem, extension) = name.rsplit_once('.')?;
    (!stem.is_empty() && extension.eq_ignore_ascii_case("exe")).then_some(name)
}

// ---------------------------------------------------------------------------
// LE CONTENU D'UN TOAST — la frontière franchie, isolée ici
// ---------------------------------------------------------------------------

/// **UN TOAST, TEL QUE WINDOWS L'A ENREGISTRÉ.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToastWindows {
    /// L'identifiant de l'application émettrice — celui que `query_rows` rend
    /// déjà pour le clignotement.
    pub aumid: String,
    /// L'`Order` de la notification. ⛔ **Il NE DIT PAS « plus récente que »** :
    /// SQLite le réutilise après un effacement (voir `QUERY_CONTENU`).
    pub order: i64,
    /// `ArrivalTime` (FILETIME, centaines de ns) : c'est LUI le curseur.
    pub arrivee: i64,
    /// Le PREMIER `<text>` — le titre, par convention `ToastGeneric`.
    pub titre: String,
    /// Le DEUXIÈME `<text>` — le message. Vide quand il n'y en a qu'un.
    pub message: String,
}

/// **LES TEXTES D'UN `Payload`, DANS L'ORDRE OÙ WINDOWS LES AFFICHE.**
///
/// ⛔ **PAS DE PARSEUR XML ICI, ET C'EST DÉLIBÉRÉ.** Ce XML n'est pas une entrée
/// hostile : c'est celui que Windows vient d'enregistrer, toujours de la même
/// forme (`<toast><visual><binding template="ToastGeneric"><text>…`). Ajouter un
/// arbre XML entier pour lire deux balises serait payer très cher une
/// extraction — même raison que `winsqlite3.dll` résolu à la main plutôt qu'un
/// crate SQLite.
///
/// ⚠️ **LES BALISES PEUVENT PORTER DES ATTRIBUTS** (`<text id="3">`, rencontré
/// dans la vraie base) : on va donc jusqu'au premier `>`, jamais en supposant
/// `<text>` nu.
pub fn textes_du_payload(payload: &[u8]) -> Vec<String> {
    let source = String::from_utf8_lossy(payload);
    let mut trouves = Vec::new();
    let mut reste: &str = &source;
    while let Some(debut) = reste.find("<text") {
        let apres = &reste[debut..];
        let Some(fin_balise) = apres.find('>') else { break };
        let contenu = &apres[fin_balise + 1..];
        let Some(fin) = contenu.find("</text>") else { break };
        trouves.push(desecchapper(&contenu[..fin]));
        reste = &contenu[fin + "</text>".len()..];
    }
    trouves
}

/// Le titre et le message d'un `Payload` — **le PREMIER `<text>`, puis TOUS LES
/// AUTRES recollés**.
///
/// ⛔ **LA VERSION PRÉCÉDENTE PRENAIT `(premier, deuxième)`, ET C'ÉTAIT LE BUG**
/// (défaut signalé le 2026-10-09 : « les notifications n'ont que leur titre pas
/// leur contenu »). **MESURÉ sur la vraie base** : les notifications ont
/// **TROIS** `<text>`, pas deux —
///
/// ```text
/// [0] "Snapchat"                        <- l'émetteur
/// [1] "Louloute"                        <- l'expéditeur
/// [2] "vous a envoyé un Snap."          <- LE CONTENU
/// ```
///
/// Le deuxième `<text>` n'est donc pas le message, c'est **l'expéditeur** — et le
/// contenu, le seul texte qui compte, n'était **jamais** lu. Sur les cinq toasts
/// réels examinés (Telegram, Snapchat ×4, tous des miroirs du téléphone), le corps
/// affiché valait « Louloute », « La France insoumise »… jamais la phrase.
///
/// ⭐ **POURQUOI RECOLLER AU LIEU DE PRENDRE LE DERNIER** : un toast à plusieurs
/// textes les affiche EMPILÉS, et c'est cette pile que l'utilisateur reconnaît.
/// Prendre le dernier perdrait l'expéditeur (« vous a envoyé un Snap. » sans dire
/// qui) ; on les recoud donc d'un « — », qui se lit comme une ligne de liste.
///
/// Rend des chaînes VIDES plutôt que `None` quand le toast n'a pas de texte :
/// un toast sans texte est un état que Windows produit (une image seule), et
/// l'appelant décide de l'afficher ou non — pas cette fonction.
pub fn titre_et_message(payload: &[u8]) -> (String, String) {
    let mut textes = textes_du_payload(payload).into_iter();
    let titre = textes.next().unwrap_or_default();
    let message = textes.collect::<Vec<_>>().join(" — ");
    (titre, message)
}

/// **DÉFAIT LES ENTITÉS XML — EN UN SEUL PASSAGE, GAUCHE À DROITE.**
///
/// ⛔ **ET C'EST LA SEULE FAÇON JUSTE.** La première version enchaînait des
/// `replace()` — `&amp;` d'abord « puisque c'est lui qui produit les autres ».
/// **C'ÉTAIT LE BUG** : `&amp;lt;` doit rendre **`&lt;`** (l'émetteur a échappé
/// un « & », suivi du texte « lt; »), et la chaîne de `replace` rendait `<` —
/// elle re-décodait ce qu'elle venait de produire. Un décodeur d'entités
/// consomme sa source, il ne réécrit pas sa sortie.
///
/// Une entité inconnue ou trop longue est laissée **INTACTE** : le « & » est
/// recopié et l'analyse reprend après lui. Mieux vaut afficher `&#xZZ;` que de
/// perdre l'information sans le dire.
fn desecchapper(texte: &str) -> String {
    // Une entité tient en quelques caractères (`#x1F600` est la plus longue
    // plausible) : au-delà, c'est un « & » littéral suivi d'un « ; » lointain.
    const LONGUEUR_MAX: usize = 10;
    let mut sortie = String::with_capacity(texte.len());
    let mut reste = texte;
    while let Some(debut) = reste.find('&') {
        sortie.push_str(&reste[..debut]);
        let apres = &reste[debut + 1..];
        let decode = apres
            .find(';')
            .filter(|fin| *fin <= LONGUEUR_MAX)
            .and_then(|fin| decoder_entite(&apres[..fin]).map(|caractere| (caractere, fin)));
        match decode {
            Some((caractere, fin)) => {
                sortie.push(caractere);
                reste = &apres[fin + 1..];
            }
            None => {
                sortie.push('&');
                reste = apres;
            }
        }
    }
    sortie.push_str(reste);
    sortie
}

/// Le corps d'une entité (`amp`, `#233`, `#xE9`) → son caractère.
fn decoder_entite(corps: &str) -> Option<char> {
    match corps {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let nombre = corps.strip_prefix('#')?;
            let valeur = match nombre.strip_prefix(['x', 'X']) {
                Some(hexadecimal) => u32::from_str_radix(hexadecimal, 16).ok()?,
                None => nombre.parse::<u32>().ok()?,
            };
            char::from_u32(valeur)
        }
    }
}

// ---------------------------------------------------------------------------
// L'état, PUR et testable
// ---------------------------------------------------------------------------

/// Ce que l'on sait des notifications, sans aucune dépendance à Windows — c'est
/// cette partie qui porte toute la logique, et qui est donc testée.
#[derive(Default)]
pub struct UnreadState {
    /// Plus récente `ArrivalTime` JAMAIS VUE par identifiant d'application
    /// (⛔ pas `Order`, réutilisé par SQLite — voir `QUERY`).
    latest: HashMap<String, i64>,
    /// Plus récente `ArrivalTime` que l'utilisateur a acquittée (par un clic).
    acked: HashMap<String, i64>,
    /// Scutations consécutives où cet identifiant non lu n'a été réclamé par
    /// aucune icône (voir `UNCLAIMED_POLLS_BEFORE_ACK`).
    unclaimed: HashMap<String, u8>,
    /// Vrai dès que la base a été lue une première fois par ce processus — c'est
    /// `seed` qui le pose, et `poll` qui s'en sert pour choisir entre amorcer et
    /// observer (voir les deux).
    seeded: bool,
}

impl UnreadState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enregistre ce que la base vient de rendre.
    ///
    /// **Le plus haut atteint ne redescend JAMAIS** (`max`) : `Order` est un
    /// compteur clairsemé dont les lignes sont réellement supprimées, donc
    /// `MAX` peut REDESCENDRE quand l'utilisateur efface la notification la plus
    /// récente depuis le centre. Écraser ferait alors cesser le clignotement
    /// sans qu'aucun clic n'ait eu lieu — contre la demande.
    ///
    /// Conséquence assumée : effacer une notification depuis le centre de
    /// notifications ne suffit pas à arrêter son clignotement, il faut cliquer
    /// l'icône. C'est le sens littéral de « jusqu'au clic ».
    pub fn observe(&mut self, rows: &[(String, i64)]) {
        for (aumid, order) in rows {
            let entry = self.latest.entry(aumid.clone()).or_insert(i64::MIN);
            *entry = (*entry).max(*order);
        }
    }

    /// **AMORÇAGE — la PREMIÈRE lecture d'un processus neuf tient pour DÉJÀ VU
    /// tout ce que le centre de notifications contient.**
    ///
    /// C'est la seule différence entre « une notification est ARRIVÉE » et « une
    /// notification est LÀ », et c'est toute la demande (DEMANDE EXPLICITE :
    /// « que doit signaler le clignotement au lancement ? — rien, sauf du
    /// nouveau »). Sans amorçage, la première scrutation d'un état vide tient
    /// l'HISTORIQUE ENTIER du centre pour nouveau. DÉFAUT RÉEL SIGNALÉ : « au
    /// lancement de l'app j'ai toujours une notification qui apparaît pour
    /// Chrome » — l'identifiant `Chrome` de la base, vieux d'une centaine de
    /// crans et jamais effacé du centre, se rallumait à chaque lancement, parce
    /// que l'acquittement d'un clic ne vit qu'en mémoire.
    ///
    /// **La raison de fond, qui rend l'amorçage préférable à un état persisté** :
    /// le centre de notifications EST déjà la mémoire de ce qui est vu ou non, et
    /// Windows l'efface quand l'utilisateur efface ses notifications. Un fichier
    /// d'état de plus aurait à suivre des identifiants qui, eux, ne se périment
    /// jamais (voir la doc de module).
    ///
    /// L'amorçage n'efface rien : il acquitte ce qu'il vient de voir, donc une
    /// notification PLUS RÉCENTE rallumera l'indicateur (voir `acknowledge`).
    pub fn seed(&mut self, rows: &[(String, i64)]) {
        self.observe(rows);
        let seen: Vec<String> = self.latest.keys().cloned().collect();
        self.acknowledge(&seen);
        self.seeded = true;
    }

    /// Identifiants ayant une notification que l'utilisateur n'a pas acquittée.
    pub fn unread(&self) -> Vec<String> {
        self.latest
            .iter()
            .filter(|(aumid, order)| self.acked.get(*aumid).copied().unwrap_or(i64::MIN) < **order)
            .map(|(aumid, _)| aumid.clone())
            .collect()
    }

    /// Acquitte ces identifiants jusqu'à ce qui est connu MAINTENANT : une
    /// notification PLUS RÉCENTE rallumera donc le clignotement, ce qui est
    /// voulu — l'utilisateur a acquitté ce qu'il a vu, pas l'avenir.
    pub fn acknowledge(&mut self, aumids: &[String]) {
        for aumid in aumids {
            if let Some(&order) = self.latest.get(aumid) {
                self.acked.insert(aumid.clone(), order);
            }
        }
    }

    /// Enregistre quelles icônes se sont réclamées, à cette scrutation, parmi
    /// les identifiants non lus — et acquitte d'office celles que PERSONNE ne
    /// réclame depuis assez longtemps (voir `UNCLAIMED_POLLS_BEFORE_ACK`).
    pub fn report_claimed(&mut self, claimed: &[String]) {
        for aumid in self.unread() {
            if claimed.iter().any(|c| c.eq_ignore_ascii_case(&aumid)) {
                self.unclaimed.remove(&aumid);
                continue;
            }
            let count = self.unclaimed.entry(aumid.clone()).or_insert(0);
            *count = count.saturating_add(1);
            if *count >= UNCLAIMED_POLLS_BEFORE_ACK {
                if let Some(&order) = self.latest.get(&aumid) {
                    self.acked.insert(aumid, order);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// L'accès à la base — le seul code qui touche Windows
// ---------------------------------------------------------------------------

type OpenV2Fn = unsafe extern "C" fn(*const i8, *mut *mut c_void, i32, *const i8) -> i32;
type BusyTimeoutFn = unsafe extern "C" fn(*mut c_void, i32) -> i32;
type PrepareV2Fn = unsafe extern "C" fn(*mut c_void, *const i8, i32, *mut *mut c_void, *mut *const i8) -> i32;
type StepFn = unsafe extern "C" fn(*mut c_void) -> i32;
type ColumnTextFn = unsafe extern "C" fn(*mut c_void, i32) -> *const u8;
type ColumnInt64Fn = unsafe extern "C" fn(*mut c_void, i32) -> i64;
/// ⛔ **RÉSOLUES POUR LA SONDE `regarde_le_payload`, ET POUR ELLE SEULE** — voir
/// la frontière de confidentialité (doc de module) : **aucune requête de
/// production ne nomme la colonne `Payload`**, et ces deux-là n'existent que
/// pour répondre à « ce BLOB contient-il du texte lisible ? ».
type ColumnBlobFn = unsafe extern "C" fn(*mut c_void, i32) -> *const c_void;
type ColumnBytesFn = unsafe extern "C" fn(*mut c_void, i32) -> i32;
type FinalizeFn = unsafe extern "C" fn(*mut c_void) -> i32;
type CloseFn = unsafe extern "C" fn(*mut c_void) -> i32;
type ErrmsgFn = unsafe extern "C" fn(*mut c_void) -> *const i8;

const SQLITE_OK: i32 = 0;
const SQLITE_ROW: i32 = 100;
const SQLITE_DONE: i32 = 101;
/// Le drapeau qui EST la garantie de confidentialité : la base ne peut pas être
/// écrite par cette connexion, quoi que fasse le code au-dessus.
const SQLITE_OPEN_READONLY: i32 = 0x0000_0001;

/// Table de fonctions résolues dans `winsqlite3.dll` — mêmes forme et raison
/// que `win32::voicemeeter::Api` : que des pointeurs de fonction, donc `Send` et
/// `Sync` sans cérémonie.
struct Api {
    open_v2: OpenV2Fn,
    busy_timeout: BusyTimeoutFn,
    prepare_v2: PrepareV2Fn,
    step: StepFn,
    column_text: ColumnTextFn,
    column_int64: ColumnInt64Fn,
    column_blob: ColumnBlobFn,
    column_bytes: ColumnBytesFn,
    finalize: FinalizeFn,
    close: CloseFn,
    errmsg: ErrmsgFn,
}

/// Charge `winsqlite3.dll` et résout ses symboles — UNE fois pour tout le
/// process, jamais réessayé : ces échecs-là SONT permanents (contrairement à
/// celui d'une base momentanément verrouillée, qui lui est retenté à chaque
/// scrutation).
///
/// `LOAD_LIBRARY_SEARCH_SYSTEM32` (comme `win32::theme`, et NON le
/// `LoadLibraryW` de `win32::voicemeeter`) : c'est une DLL du système, on ne
/// veut pas qu'un fichier homonyme traînant à côté de l'exécutable soit chargé
/// à sa place.
fn api() -> Option<&'static Api> {
    // `HMODULE` enveloppe un pointeur brut (ni `Send` ni `Sync`) : on garde sa
    // valeur en `usize` dans la statique et on reconstruit, même technique que
    // `win32::theme::uxtheme_module`. La DLL n'est JAMAIS libérée — Windows la
    // garde chargée pour la durée du process, exactement la durée voulue.
    static MODULE: OnceLock<usize> = OnceLock::new();
    let module = *MODULE.get_or_init(|| unsafe {
        LoadLibraryExW(w!("winsqlite3.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)
            .map(|m| m.0 as usize)
            .unwrap_or(0)
    });
    if module == 0 {
        return None;
    }
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        let module = HMODULE(module as *mut c_void);
        let resolve = |name: PCSTR| GetProcAddress(module, name);
        Some(Api {
            open_v2: std::mem::transmute_copy(&resolve(s!("sqlite3_open_v2"))?),
            busy_timeout: std::mem::transmute_copy(&resolve(s!("sqlite3_busy_timeout"))?),
            prepare_v2: std::mem::transmute_copy(&resolve(s!("sqlite3_prepare_v2"))?),
            step: std::mem::transmute_copy(&resolve(s!("sqlite3_step"))?),
            column_text: std::mem::transmute_copy(&resolve(s!("sqlite3_column_text"))?),
            column_int64: std::mem::transmute_copy(&resolve(s!("sqlite3_column_int64"))?),
            column_blob: std::mem::transmute_copy(&resolve(s!("sqlite3_column_blob"))?),
            column_bytes: std::mem::transmute_copy(&resolve(s!("sqlite3_column_bytes"))?),
            finalize: std::mem::transmute_copy(&resolve(s!("sqlite3_finalize"))?),
            close: std::mem::transmute_copy(&resolve(s!("sqlite3_close"))?),
            errmsg: std::mem::transmute_copy(&resolve(s!("sqlite3_errmsg"))?),
        })
    })
    .as_ref()
}

/// Chemin de la base de notifications de CET utilisateur, s'il existe.
fn database_path() -> Option<std::path::PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let path = std::path::PathBuf::from(local)
        .join("Microsoft")
        .join("Windows")
        .join("Notifications")
        .join("wpndatabase.db");
    path.exists().then_some(path)
}

/// Ouvre, interroge, referme. `None` en cas d'échec — la cause est signalée par
/// `warn_once` plutôt que noyée dans un `None` muet.
fn query_rows() -> Option<Vec<(String, i64)>> {
    let Some(api) = api() else {
        warn_once("winsqlite3.dll indisponible (symboles introuvables)");
        return None;
    };
    let Some(path) = database_path() else {
        warn_once("base de notifications absente");
        return None;
    };
    let path_c = CString::new(path.to_string_lossy().as_bytes()).ok()?;

    unsafe {
        let mut db: *mut c_void = std::ptr::null_mut();
        if (api.open_v2)(path_c.as_ptr(), &mut db, SQLITE_OPEN_READONLY, std::ptr::null()) != SQLITE_OK {
            let message = if db.is_null() { String::new() } else { CStr::from_ptr((api.errmsg)(db)).to_string_lossy().into_owned() };
            if !db.is_null() {
                (api.close)(db);
            }
            warn_once("ouverture en lecture seule refusée");
            eprintln!("[neuradesk][notification] ouverture refusée : {message}");
            return None;
        }
        // Une base verrouillée par le service ne doit pas figer la boucle de
        // rendu : on préfère manquer une scrutation.
        (api.busy_timeout)(db, 50);

        let query_c = CString::new(QUERY).ok()?;
        let mut stmt: *mut c_void = std::ptr::null_mut();
        if (api.prepare_v2)(db, query_c.as_ptr(), -1, &mut stmt, std::ptr::null_mut()) != SQLITE_OK {
            warn_once("préparation de la requête refusée");
            (api.close)(db);
            return None;
        }

        let mut rows = Vec::new();
        loop {
            match (api.step)(stmt) {
                SQLITE_ROW => {
                    // `sqlite3_column_text` est appelé UNE fois : un second appel
                    // peut invalider le pointeur rendu par le premier.
                    let text = (api.column_text)(stmt, 0);
                    if text.is_null() {
                        continue;
                    }
                    let aumid = CStr::from_ptr(text as *const i8).to_string_lossy().into_owned();
                    rows.push((aumid, (api.column_int64)(stmt, 1)));
                }
                SQLITE_DONE => break,
                _ => {
                    warn_once("lecture interrompue");
                    break;
                }
            }
        }
        (api.finalize)(stmt);
        (api.close)(db);
        Some(rows)
    }
}

/// **Une requête EN LECTURE SEULE sur une base SQLite quelconque**, par le même
/// `winsqlite3.dll` — rend les `columns` premières colonnes de chaque rangée en
/// texte (`NULL` → chaîne vide). `None` si la base ne s'ouvre pas ou si la
/// requête est refusée.
///
/// Même garantie que `query_rows` : `SQLITE_OPEN_READONLY`, et une base
/// verrouillée fait perdre la lecture plutôt que de figer l'appelant. Sert à
/// lire les profils de Firefox (voir `win32::firefox_profiles`).
pub(crate) fn read_only_rows(path: &std::path::Path, query: &str, columns: i32) -> Option<Vec<Vec<String>>> {
    let api = api()?;
    let path_c = CString::new(path.to_string_lossy().as_bytes()).ok()?;
    let query_c = CString::new(query).ok()?;
    unsafe {
        let mut db: *mut c_void = std::ptr::null_mut();
        if (api.open_v2)(path_c.as_ptr(), &mut db, SQLITE_OPEN_READONLY, std::ptr::null()) != SQLITE_OK {
            if !db.is_null() {
                (api.close)(db);
            }
            return None;
        }
        (api.busy_timeout)(db, 50);
        let mut stmt: *mut c_void = std::ptr::null_mut();
        if (api.prepare_v2)(db, query_c.as_ptr(), -1, &mut stmt, std::ptr::null_mut()) != SQLITE_OK {
            (api.close)(db);
            return None;
        }
        let mut rows = Vec::new();
        while (api.step)(stmt) == SQLITE_ROW {
            let row = (0..columns)
                .map(|column| {
                    let text = (api.column_text)(stmt, column);
                    if text.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(text as *const i8).to_string_lossy().into_owned()
                    }
                })
                .collect();
            rows.push(row);
        }
        (api.finalize)(stmt);
        (api.close)(db);
        Some(rows)
    }
}

/// N'imprime qu'une fois par cause DISTINCTE — cette fonction tourne une fois
/// par seconde, un message répété noierait tout le reste.
fn warn_once(reason: &'static str) {
    LAST_WARNING.with(|last| {
        if last.get() != Some(reason) {
            last.set(Some(reason));
            eprintln!("[neuradesk][notification] clignotement indisponible : {reason}");
        }
    });
}

thread_local! {
    /// Voir `warn_once` — `None` = aucune alerte en cours.
    static LAST_WARNING: Cell<Option<&'static str>> = const { Cell::new(None) };
    /// Réglage d'activation (voir `set_enabled`) — `false` par défaut : la
    /// fonctionnalité est OPT-IN, et désactivée elle ne lit RIEN.
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    /// L'état — de la mémoire seule, jamais un handle système (voir la doc de
    /// module sur la raison de ne PAS garder de connexion).
    static STATE: RefCell<UnreadState> = RefCell::new(UnreadState::new());
}

/// Active ou désactive la lecture — voir `app::run`, qui l'initialise depuis le
/// réglage enregistré, et `win32::cef_settings`, qui la bascule à chaud.
pub fn set_enabled(enabled: bool) {
    ENABLED.with(|e| e.set(enabled));
}

pub fn is_enabled() -> bool {
    ENABLED.with(Cell::get)
}

/// Lit la base et met l'état à jour. **Ne fait RIEN si la fonctionnalité est
/// désactivée** : la base n'est même pas ouverte, c'est la garantie de
/// confidentialité tenue à la lettre.
pub fn poll() {
    if !is_enabled() {
        return;
    }
    if let Some(rows) = query_rows() {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            // La PREMIÈRE lecture amorce au lieu d'observer (voir `seed`) : ce qui
            // est déjà dans le centre de notifications n'est pas une nouvelle.
            if state.seeded {
                state.observe(&rows);
            } else {
                state.seed(&rows);
            }
        });
    }
}

/// Identifiants d'application ayant une notification non acquittée.
///
/// Rend une liste VIDE quand la fonctionnalité est désactivée : sans cela,
/// décocher la case en plein clignotement laisserait les pastilles clignoter
/// jusqu'au redémarrage.
pub fn unread_aumids() -> Vec<String> {
    if !is_enabled() {
        return Vec::new();
    }
    STATE.with(|state| state.borrow().unread())
}

/// Acquitte ces identifiants — appelé quand l'utilisateur clique une icône.
pub fn acknowledge(aumids: &[String]) {
    STATE.with(|state| state.borrow_mut().acknowledge(aumids));
}

/// Déclare quelles icônes se sont réclamées à cette scrutation (voir
/// `UnreadState::report_claimed`, qui en déduit les acquittements d'office).
pub fn report_claimed(claimed: &[String]) {
    STATE.with(|state| state.borrow_mut().report_claimed(claimed));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(entries: &[(&str, i64)]) -> Vec<(String, i64)> {
        entries.iter().map(|(a, o)| (a.to_string(), *o)).collect()
    }

    /// Un identifiant n'est « non lu » que si sa notification est plus récente
    /// que ce qui a été acquitté — et l'acquittement porte sur CE QUI ÉTAIT
    /// CONNU à cet instant, pas sur l'avenir.
    #[test]
    fn a_newer_notification_relights_an_acknowledged_app() {
        let mut state = UnreadState::new();
        state.observe(&rows(&[("Chrome", 10)]));
        assert_eq!(state.unread(), vec!["Chrome".to_string()]);

        state.acknowledge(&["Chrome".to_string()]);
        assert!(state.unread().is_empty(), "acquittée : éteinte");

        state.observe(&rows(&[("Chrome", 11)]));
        assert_eq!(state.unread(), vec!["Chrome".to_string()], "une plus récente rallume");

        // Réobserver le MÊME maximum ne rallume rien : l'acquittement porte sur
        // la notification, pas sur l'instant de la scrutation.
        state.acknowledge(&["Chrome".to_string()]);
        state.observe(&rows(&[("Chrome", 11)]));
        assert!(state.unread().is_empty());
    }

    /// **La première lecture n'allume RIEN** — DEMANDE EXPLICITE : « que doit
    /// signaler le clignotement au lancement ? — rien, sauf du nouveau ».
    /// DÉFAUT RÉEL SIGNALÉ, et c'est ce test qui le fixe : « au lancement de
    /// l'app j'ai toujours une notification qui apparaît pour Chrome ». L'état
    /// naît vide, donc sans amorçage la première scrutation tient TOUT le centre
    /// de notifications pour nouveau — y compris une notification vieille d'une
    /// centaine de crans, jamais effacée du centre.
    #[test]
    fn the_first_reading_seeds_the_state_and_lights_nothing() {
        let mut state = UnreadState::new();
        // L'état réel de la machine : l'identifiant `Chrome` à 11 652, quand le
        // plus récent de la base est à 11 752 (voir la doc de `seed`).
        state.seed(&rows(&[("Chrome", 11652), ("Discord", 11752)]));
        assert!(state.unread().is_empty(), "ce qui était déjà là ne clignote pas");
        assert!(state.seeded, "et l'état se sait amorcé : la lecture suivante observera");

        // L'amorçage n'éteint rien POUR TOUJOURS : il date ce qu'il a vu. Ce qui
        // arrive ensuite s'allume, et rien d'autre.
        state.observe(&rows(&[("Chrome", 11652), ("Discord", 11753)]));
        assert_eq!(state.unread(), vec!["Discord".to_string()], "seule la nouvelle s'allume");
    }

    /// **Le plus haut atteint ne redescend jamais.** `Order` est clairsemé et
    /// ses lignes sont réellement supprimées : sans ce `max`, effacer la
    /// notification la plus récente depuis le centre de notifications ferait
    /// cesser le clignotement sans aucun clic — contre la demande.
    #[test]
    fn the_high_water_mark_never_descends() {
        let mut state = UnreadState::new();
        state.observe(&rows(&[("Discord", 10)]));
        state.acknowledge(&["Discord".to_string()]);
        assert!(state.unread().is_empty());

        // La notification la plus récente est effacée : la base ne rend plus que
        // l'ancienne, plus basse.
        state.observe(&rows(&[("Discord", 5)]));
        assert!(state.unread().is_empty(), "un maximum qui redescend ne rallume pas");

        // Et une vraie nouveauté, elle, rallume.
        state.observe(&rows(&[("Discord", 11)]));
        assert_eq!(state.unread(), vec!["Discord".to_string()]);
    }

    /// Un identifiant qu'AUCUNE icône ne réclame finit par être acquitté d'office
    /// (voir `UNCLAIMED_POLLS_BEFORE_ACK`) — sinon il resterait non lu pour
    /// toujours, sans qu'aucun clic ne puisse jamais l'acquitter.
    #[test]
    fn an_identifier_no_icon_claims_is_acknowledged_after_a_while() {
        let mut state = UnreadState::new();
        state.observe(&rows(&[("Application.Sans.Icone", 10)]));
        for _ in 0..UNCLAIMED_POLLS_BEFORE_ACK - 1 {
            state.report_claimed(&[]);
            assert!(!state.unread().is_empty(), "pas encore acquitté d'office");
        }
        state.report_claimed(&[]);
        assert!(state.unread().is_empty(), "acquitté après assez de scrutations sans réclamation");
    }

    /// Une icône qui SE RÉCLAME ne se fait jamais acquitter d'office, même après
    /// autant de scrutations — c'est l'utilisateur qui décide, par un clic.
    #[test]
    fn a_claimed_identifier_is_never_auto_acknowledged() {
        let mut state = UnreadState::new();
        state.observe(&rows(&[("Traitement.De.Texte", 10)]));
        for _ in 0..UNCLAIMED_POLLS_BEFORE_ACK * 4 {
            state.report_claimed(&["Traitement.De.Texte".to_string()]);
        }
        assert_eq!(state.unread(), vec!["Traitement.De.Texte".to_string()]);
    }

    /// L'identité d'une application de bureau vient du CHEMIN qu'elle déclare —
    /// et rien n'est deviné à partir d'un identifiant opaque.
    #[test]
    fn an_executable_is_only_read_from_an_exe_path() {        // MESURÉ : ce que la base rend pour une notification de PowerShell.
        assert_eq!(
            exe_named_by(r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe"),
            Some("powershell.exe")
        );
        assert_eq!(exe_named_by("C:/Users/x/AppData/Local/PowerToys/PowerToys.PowerLauncher.exe"), Some("PowerToys.PowerLauncher.exe"));
        // Mesurés dans la table d'enregistrement de cette machine : opaques, donc
        // MUETS. Deviner l'application à partir de leur forme serait exactement
        // l'inférence que cette fonctionnalité s'interdit.
        assert_eq!(exe_named_by("FirefoxToast-308046B0AF4A39CB"), None);
        assert_eq!(exe_named_by("NotifyIconGeneratedAumid_6688409891507931140"), None);
        assert_eq!(exe_named_by("Microsoft.YourPhone_8wekyb3d8bbwe!App"), None);
        assert_eq!(exe_named_by("DeepL.Apps.Windows"), None);
        assert_eq!(exe_named_by("Chrome"), None);
    }

    /// Éprouve la couche SQL et la FFI sur la VRAIE base de cette machine —
    /// **ignoré par défaut**, parce qu'il dépend de l'état de la session (le
    /// service de notifications doit tourner, la base exister). À lancer
    /// explicitement :
    ///
    /// ```text
    /// cargo test -- --ignored --nocapture reads_the_real_database
    /// ```
    ///
    /// C'est la seule façon d'éprouver ce que les tests purs ne touchent pas :
    /// que `winsqlite3.dll` se charge, que la base s'ouvre en LECTURE SEULE
    /// pendant que le service la tient, et que la requête rend des identifiants
    /// bien formés. Il ne prouve PAS qu'une notification donnée serait détectée
    /// — il faudrait en poster une, et l'affichage d'une notification est un
    /// effet de bord qui ne regarde pas un test.
    #[test]
    #[ignore = "dépend de la base de notifications de la machine"]
    fn reads_the_real_database() {
        let rows = query_rows().expect("la base de notifications devrait être lisible en lecture seule");
        println!("[notification] {} identifiant(s) d'application lu(s)", rows.len());
        for (aumid, order) in rows.iter().take(8) {
            println!("[notification]   arrivee {order}  {aumid}");
        }
        for (aumid, order) in &rows {
            assert!(!aumid.is_empty(), "un identifiant vide ne désigne aucune application");
            assert!(*order > 0, "l'arrivée d'une notification est un FILETIME positif");
        }

        // CE QUE FERAIT LE LANCEMENT, sur ces données-là : amorcer, puis observer
        // la scrutation suivante. C'est le contrôle à la main du défaut signalé
        // (« au lancement, une notification pour Chrome ») — sur la vraie base,
        // l'historique entier doit rester ÉTEINT.
        let mut state = UnreadState::new();
        state.seed(&rows);
        state.observe(&rows);
        let lit = state.unread();
        println!("[notification] apres amorcage, {} identifiant(s) allume(s) : {lit:?}", lit.len());
        assert!(lit.is_empty(), "l'historique du centre ne doit rien allumer au lancement : {lit:?}");
    }

    /// ⛔ **SONDE — ELLE FRANCHIT LA FRONTIÈRE DE CONFIDENTIALITÉ, ET C'EST TOUT
    /// SON OBJET.** Elle lit la colonne `Payload`, que la requête de production
    /// ne nomme **jamais** (voir la doc de module), pour répondre à une seule
    /// question : **le texte d'une notification est-il lisible dans ce BLOB ?**
    ///
    /// Sans cette réponse, « réafficher les notifications sous l'horloge » reste
    /// une intention, pas un plan. Si le BLOB est opaque ou compressé, la
    /// fonctionnalité n'existe pas — et il vaut mieux le savoir avant d'écrire
    /// une ligne de plus.
    ///
    /// ⚠️ **La base est ouverte en LECTURE SEULE**, comme partout ailleurs : la
    /// garantie ne dépend pas de ce que la sonde fait ensuite.
    ///
    /// `cargo test --bins -- --ignored --nocapture regarde_le_payload`
    #[test]
    #[ignore]
    fn regarde_le_payload() {
        let Some(api) = api() else { panic!("winsqlite3.dll indisponible") };
        let Some(path) = database_path() else { panic!("base de notifications absente") };
        let Ok(chemin) = CString::new(path.to_string_lossy().as_bytes()) else { return };

        const REQUETE: &str = "SELECT Payload FROM Notification \
                               WHERE Type = 'toast' AND Payload IS NOT NULL \
                               ORDER BY [Order] DESC LIMIT 5";
        unsafe {
            let mut db: *mut c_void = std::ptr::null_mut();
            assert_eq!(
                (api.open_v2)(chemin.as_ptr(), &mut db, SQLITE_OPEN_READONLY, std::ptr::null()),
                SQLITE_OK
            );
            (api.busy_timeout)(db, 200);
            let Ok(requete) = CString::new(REQUETE) else { return };
            let mut stmt: *mut c_void = std::ptr::null_mut();
            assert_eq!(
                (api.prepare_v2)(db, requete.as_ptr(), -1, &mut stmt, std::ptr::null_mut()),
                SQLITE_OK
            );

            let mut n = 0;
            while (api.step)(stmt) == SQLITE_ROW {
                n += 1;
                let taille = (api.column_bytes)(stmt, 0) as usize;
                let pointeur = (api.column_blob)(stmt, 0) as *const u8;
                println!("\n--- toast {n} : {taille} octets ---");
                if pointeur.is_null() || taille == 0 {
                    println!("  (vide)");
                    continue;
                }
                let octets = std::slice::from_raw_parts(pointeur, taille);
                let hexa: Vec<String> =
                    octets.iter().take(48).map(|b| format!("{b:02x}")).collect();
                println!("  hexa : {}", hexa.join(" "));
                // ⭐ **LES PLAGES IMPRIMABLES, ET C'EST ÇA QUI TRANCHE** : du XML
                // ou du JSON apparaîtraient ici EN CLAIR ; un BLOB compressé ou
                // chiffré ne montrerait que du bruit.
                let mut suites: Vec<String> = Vec::new();
                let mut courante = String::new();
                for &b in octets {
                    if (0x20..0x7f).contains(&b) {
                        courante.push(b as char);
                    } else {
                        if courante.len() >= 8 {
                            suites.push(std::mem::take(&mut courante));
                        }
                        courante.clear();
                    }
                    if suites.len() >= 12 {
                        break;
                    }
                }
                if courante.len() >= 8 {
                    suites.push(courante);
                }
                println!("  {} plage(s) imprimable(s) d'au moins 8 caracteres :", suites.len());
                for suite in suites.iter().take(12) {
                    println!("    {:?}", &suite[..suite.len().min(160)]);
                }
                // ⭐ **CE QUE LE CODE EN EXTRAIT REELLEMENT** (2026-10-09). Les
                // plages ci-dessus montrent le XML ; elles ne disent PAS ce que
                // `titre_et_message` en tire — or c'est cela seul que l'utilisateur
                // voit. Défaut signalé : « les notifications n'ont que leur titre
                // pas leur contenu ».
                let textes = textes_du_payload(octets);
                println!("  {} <text> trouve(s) :", textes.len());
                for (i, t) in textes.iter().enumerate() {
                    println!("    [{i}] {:?}", &t[..t.len().min(120)]);
                }
                let (titre, message) = titre_et_message(octets);
                println!("  => TITRE {:?}", &titre[..titre.len().min(60)]);
                println!("  => MESSAGE {:?} ({} car.)", &message[..message.len().min(60)], message.chars().count());
            }
            (api.finalize)(stmt);
            (api.close)(db);
            println!("\n{n} toast(s) examine(s).");
        }

        // ⭐ **ET LA FONCTION DE PRODUCTION, SUR LA MÊME BASE** — c'est elle qui
        // sera appelée par le panneau. `0` en seuil : tout l'historique.
        println!("\n=== toasts_depuis(0) — ce que le panneau afficherait ===");
        match toasts_depuis(0) {
            None => println!("  base illisible"),
            Some(toasts) => {
                println!("  {} toast(s)", toasts.len());
                for toast in toasts.iter().rev().take(6) {
                    println!(
                        "  ordre {}  {}\n     titre   ({} car.) {:?}\n     message ({} car.) {:?}",
                        toast.order,
                        toast.aumid,
                        toast.titre.chars().count(),
                        toast.titre,
                        toast.message.chars().count(),
                        toast.message
                    );
                }
            }
        }
    }

    // --- Le contenu d'un toast ----------------------------------------------

    /// **LE TOAST RÉEL, RECOPIÉ DE LA BASE DE CETTE MACHINE** (sonde
    /// `regarde_le_payload`, 2026-10-08) — c'est notre propre toast d'essai.
    const TOAST_ESSAI: &str = "<toast><visual><binding template=\"ToastGeneric\">\
        <text>NeuraDesk - essai</text>\
        <text>Un toast WinRT, avec le logo de l'application.</text>\
        <image placement=\"appLogoOverride\" hint-crop=\"circle\" src=\"file:///C:/x.png\"/>\
        </binding></visual></toast>";

    #[test]
    fn le_titre_est_le_premier_texte_et_le_message_tout_le_reste() {
        let (titre, message) = titre_et_message(TOAST_ESSAI.as_bytes());
        assert_eq!(titre, "NeuraDesk - essai");
        assert_eq!(message, "Un toast WinRT, avec le logo de l'application.");
    }

    /// ⭐⭐ **LE TEST DU DÉFAUT RÉEL** (2026-10-09) — et la charge utile est
    /// **recopiée de la vraie base**, pas inventée : les cinq toasts examinés ont
    /// tous cette forme à trois textes.
    ///
    /// ⛔ Sur l'ancien code il rendait `("Snapchat", "Louloute")` — l'application et
    /// l'expéditeur. **Le contenu n'apparaissait nulle part**, ce qui est
    /// exactement le défaut signalé.
    #[test]
    fn le_contenu_d_un_toast_a_trois_textes_est_vraiment_affiche() {
        let payload = r#"<toast><visual><binding template="ToastGeneric">
            <text>Snapchat</text><text>Louloute</text><text>vous a envoyé un Snap.</text>
            </binding></visual></toast>"#;
        let (titre, message) = titre_et_message(payload.as_bytes());
        assert_eq!(titre, "Snapchat");
        assert!(
            message.contains("vous a envoyé un Snap."),
            "le CONTENU doit apparaitre, pas seulement l'expediteur ; message = {message:?}"
        );
        assert!(message.contains("Louloute"), "l'expediteur ne doit pas etre perdu pour autant");
    }

    /// Et au-delà de trois, on ne perd rien : la couture prend tout le reste.
    #[test]
    fn un_toast_a_plusieurs_textes_les_recoud_tous() {
        let payload = br#"<toast><text>A</text><text>B</text><text>C</text><text>D</text></toast>"#;
        assert_eq!(titre_et_message(payload), ("A".to_string(), "B — C — D".to_string()));
    }

    /// ⚠️ **LES BALISES PORTENT DES ATTRIBUTS DANS LA VRAIE BASE** — `<text
    /// id="3">` y a été relevé. Une extraction qui supposerait `<text>` nu
    /// rendrait le contenu AVEC son attribut, ou rien du tout.
    #[test]
    fn une_balise_text_a_attribut_rend_quand_meme_son_contenu() {
        let (titre, _) = titre_et_message(b"<toast><text id=\"3\">Avec attribut</text></toast>");
        assert_eq!(titre, "Avec attribut");
    }

    /// **UN TOAST SANS SECOND TEXTE N'EST PAS UNE ERREUR** — Windows en produit
    /// (une image seule). Le message est vide, le titre reste.
    #[test]
    fn un_toast_a_un_seul_texte_laisse_le_message_vide() {
        let (titre, message) = titre_et_message(b"<toast><text>Titre seul</text></toast>");
        assert_eq!(titre, "Titre seul");
        assert!(message.is_empty(), "message = {message:?}");
    }

    /// ⛔ **`&amp;` SE DÉFAIT EN PREMIER.** Dans l'autre ordre, `&amp;lt;`
    /// deviendrait `<` — l'inverse exact de ce qu'il faut. Le piège est le même
    /// que celui de l'échappement, côté `win32::bulle`.
    #[test]
    fn les_entites_se_defont_dans_le_bon_ordre() {
        let (titre, _) = titre_et_message(b"<toast><text>a &amp;lt; b</text></toast>");
        assert_eq!(titre, "a &lt; b", "« &amp;lt; » doit rendre « &lt; », pas « < »");
        let (titre2, _) = titre_et_message(b"<toast><text>Jean &amp; Fils &lt;pro&gt;</text></toast>");
        assert_eq!(titre2, "Jean & Fils <pro>");
    }

    /// Les entités **numériques** — un « é » écrit `&#233;` qui s'afficherait tel
    /// quel se remarque tout de suite.
    #[test]
    fn les_entites_numeriques_sont_defaites() {
        let (titre, _) = titre_et_message(b"<toast><text>caf&#233; &#xE9;t&#xE9;</text></toast>");
        assert_eq!(titre, "café été");
    }

    /// ⚠️ **UNE ENTITÉ INVALIDE EST LAISSÉE INTACTE** — mieux vaut afficher
    /// `&#xZZ;` que de perdre l'information sans le dire.
    #[test]
    fn une_entite_invalide_est_laissee_telle_quelle() {
        let (titre, _) = titre_et_message(b"<toast><text>a &#xZZ; b</text></toast>");
        assert_eq!(titre, "a &#xZZ; b");
    }
}
