//! **LA CONFIGURATION INITIALE DU PORTABLE, ADAPTÉE À L'ÉCRAN QUI LA REÇOIT.**
//!
//! DEMANDE (2026-10-24) : « il faudrait que l'appli se règle toute seule pour avoir
//! des réglages équivalents à mes réglages actuels en tenant compte de la résolution
//! de l'écran de l'ordinateur sur lequel l'app va se lancer ». Précisé ensuite :
//! l'apparence ET l'arrangement voyagent, **aucun programme épinglé**, les écrans
//! secondaires repartent des réglages par défaut, et la copie est figée à
//! l'assemblage du portable.
//!
//! ⛔ **POURQUOI IL FAUT CONVERTIR** : presque toutes les longueurs de la
//! configuration sont en PIXELS PHYSIQUES de l'écran où elles ont été réglées, et
//! utilisées telles quelles (épaisseur des barres, cotes des zones, écarts, ombres,
//! police des zones…) ; le zoom des pages CEF est un ×3 fixe. Réglées sur un
//! 3840×2160, elles occupent le DOUBLE de la place, en proportion, sur un
//! 1920×1080. Rien ne retenait l'écran de référence : c'est `REFERENCE`.
//!
//! Trois temps :
//! 1. **`--config-initiale <dossier>`** (`exporter_si_demande`), lancé par le script
//!    d'assemblage : copie ce qui voyage, plus `REFERENCE`.
//!    ⭐ **L'APPARENCE, PLUS `zones.toml` ET `icon_order.txt`** (2026-10-29 :
//!    `est_emporte` délègue à `sauvegarde_config::est_apparence` et ajoute
//!    `DISPOSITION`). Ne voyagent pas : les épingles, `dossiers.toml`,
//!    `pin_capacity.txt`, les `*_integres.txt`, ni aucune donnée. La machine qui
//!    reçoit le portable garde donc SES épingles, SES dossiers de barre du bas et
//!    SA réserve, et emprunte le LOOK et la GÉOMÉTRIE des zones.
//! 2. **Au premier lancement** sur une machine sans configuration
//!    (`au_lancement`), la copie `DOSSIER` posée à côté de l'exe est semée dans
//!    `%APPDATA%\NeuraDesk`, puis adaptée.
//! 3. **L'adaptation** (`adapter_dossier`) multiplie chaque longueur par
//!    `Facteurs` — la HAUTEUR du nouvel écran sur celle de la référence — UNE FOIS,
//!    puis réécrit `REFERENCE` : les réglages suivants se font normalement.
//!
//! ⭐ **LA HAUTEUR, PAS LA LARGEUR** : barres, zones et icônes s'empilent dans la
//! hauteur ; un écran plus large (16:10, ultra-large) garde ainsi les proportions
//! verticales, et la place en plus va à la zone centrale.
//!
//! ⚠️ **UN CHANGEMENT D'ÉCRAN NE CONVERTIT RIEN** : seuls le semis et l'import d'une
//! archive (`sauvegarde_config`, qui pose `A_ADAPTER`) déclenchent l'adaptation.
//! Brancher un autre écran principal ne doit pas réécrire les réglages qu'on vient
//! de régler.

use std::fs;
use std::io;
use std::path::Path;

/// Le dossier de la configuration initiale, à côté de l'exe du portable.
pub const DOSSIER: &str = "config-initiale";

/// L'écran sur lequel la configuration a été réglée : `largeur,hauteur,dpi`.
pub const REFERENCE: &str = "reference_ecran.txt";

/// Posé par l'import d'une archive : « adapte au prochain lancement ». Son contenu
/// dit ce qui est venu de l'archive (`apparence` ou `complete`).
pub const A_ADAPTER: &str = "a_adapter_a_l_ecran.txt";

/// La ligne de commande du script d'assemblage.
const DRAPEAU: &str = "--config-initiale";



/// La clé de l'écran principal dans `fullscreen_clock_decalages.txt` (voir
/// `win32::fullscreen_clock::ECRAN_PRINCIPAL`).
const CLE_PRINCIPAL: &str = "principal";

/// Les deux fichiers d'ARRANGEMENT demandés NOMMÉMENT le 2026-10-29 — « mes
/// colonnes de la barre de droite ne s'appliquent pas ». `zones.toml` porte les
/// colonnes et les cotes (celles-ci sont converties, il est dans `CONVERTIS`) ;
/// `icon_order.txt`, l'ordre des icônes dans les barres. Rien d'autre de
/// l'arrangement ne suit : ni `dossiers.toml`, ni `pin_capacity.txt`, ni les
/// `*_integres.txt`.
/// ⛔ **`*_integres.txt` ET `pin_capacity.txt` SONT INDISPENSABLES À CES DEUX-LÀ**
/// (mesure du 2026-10-29, deuxième passe : « l'ordre des icônes de la barre de
/// droite est cassé, les colonnes ne sont pas respectées et les icônes ne sont
/// pas toutes dans les bonnes zones »). `zones.toml` ne décrit que la GÉOMÉTRIE ;
/// les fichiers `*_integres.txt` (16 chez l'utilisateur) disent QUEL applet est
/// dans quelle barre, et `icon_order.txt` n'est qu'une suite d'INDICES dans cet
/// espace — livré seul, il désigne d'autres icônes. `pin_capacity.txt` réserve
/// les places des épingles et décale le même espace.
/// ⚠️ **`icon_order.txt` EST REVENU** (2026-10-29, deuxième décision du jour).
///
/// Il a d'abord été retiré, parce qu'il référence la région des épingles, qui
/// diffère d'une machine à l'autre. Mesuré en VM SANS lui : le défaut met
/// presque tout à GAUCHE (tout sauf Réglages, thème, éclairage nocturne,
/// alimentation, terminal, son, réseau) — « des icônes de la barre de droite se
/// retrouvent après l'icône de neuradesk », et les zones Intégrés/Réglages
/// restent vides. Son fichier, lui, décrit une vraie répartition : ~35 icônes à
/// gauche, ~56 à droite, sur QUATRE lignes (gauche, droite, haut-gauche,
/// haut-droite).
///
/// Sans lui, sa disposition n'existe simplement plus. Avec lui, elle revient à
/// hauteur de ce que les index veulent bien dire — voir la note de
/// `icon_order.txt` ci-dessous.
///
/// ⛔ `pin_capacity.txt` reste DEHORS : la réserve de places ne remplissait rien
/// et laissait des trous (`capacite 38`, 0 épingle).
///
/// Demande de l'utilisateur : « je veux que les icônes de la zone Intégrés et de
/// la zone Réglages gardent leurs places et leur ordre même si le reste change,
/// que l'icône de neuradesk soit la première icône de la barre de gauche et que
/// les programmes épinglés suivis de ceux qui sont ouverts apparaissent après ».
///
/// **C'est déjà l'ordre NATUREL de la base** (`ui/dock.rs:1793` et `:2450`) :
/// logo (index 0) → applets intégrés → épingles → applications ouvertes ; les
/// épingles sont AJOUTÉES à la fin par `ensure_winpin_capacity`, précisément pour
/// ne pas décaler les index des applets.
///
/// Son `icon_order.txt` commençait par `361` — un index de la région
/// épingles/ouvertes, celle qui diffère d'une machine à l'autre (30 épingles
/// chez lui, 6 sur la VM). Livré, il déplaçait donc les applets ET les icônes de
/// droite. Sans lui, la base naturelle reprend, et tout ce qui PRÉCÈDE les
/// épingles est identique sur les deux machines.
const DISPOSITION: &[&str] = &["zones.toml", "icon_order.txt"];

/// Les `*_integres.txt` : l'appartenance des applets aux barres. ⛔
/// `sauvegarde_config::est_apparence` les EXCLUT volontairement (« de
/// l'arrangement, pas de l'apparence ») — l'exception est ici, pour le portable
/// seulement, parce que sans eux la disposition livrée ne veut rien dire.
fn integres(relatif: &str) -> bool {
    relatif.ends_with("_integres.txt")
}

/// **CE CHEMIN RELATIF VOYAGE-T-IL AVEC LE PORTABLE ?**
///
/// ⭐ **L'APPARENCE, PLUS LA DISPOSITION** (demandes du 2026-10-29, dans cet
/// ordre : « je ne veux que les réglages d'apparence de l'app comme
/// préréglages », puis « mes colonnes de la barre de droite ne s'appliquent
/// pas »). L'apparence DÉLÈGUE à `sauvegarde_config::est_apparence`, la
/// définition qui sert déjà à l'archive d'apparence seule : une seule règle,
/// jamais deux qui divergent.
///
/// Ne voyagent donc pas : les épingles et leurs catégories, les dossiers de la
/// barre du bas, les chemins de cette machine, ni aucune donnée. Voyagent :
/// style, couleurs, thèmes, icônes, polices, épaisseurs et teintes de barres,
/// textures, palettes, lueurs, ombres, fondus, échelles par panneau — plus les
/// deux fichiers de `DISPOSITION`.
pub fn est_emporte(relatif: &str) -> bool {
    crate::sauvegarde_config::est_apparence(relatif)
        || DISPOSITION.contains(&relatif)
        // `est_exporte` même ici : un `_integres.txt` rangé dans un sous-dossier
        // ne doit pas entrer par la porte de service.
        || (integres(relatif) && crate::sauvegarde_config::est_exporte(relatif))
}

/// Un écran : sa taille en pixels physiques et son DPI effectif.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ecran {
    pub largeur: u32,
    pub hauteur: u32,
    pub dpi: u32,
}

impl Ecran {
    pub fn serialiser(&self) -> String {
        format!("{},{},{}", self.largeur, self.hauteur, self.dpi)
    }

    /// Relit `serialiser` ; `None` s'il manque un champ ou qu'un champ est nul.
    pub fn lire(contenu: &str) -> Option<Self> {
        let champs: Vec<u32> = contenu.trim().split(',').map(|c| c.trim().parse().ok()).collect::<Option<_>>()?;
        let [largeur, hauteur, dpi] = champs[..] else { return None };
        (largeur > 0 && hauteur > 0 && dpi > 0).then_some(Self { largeur, hauteur, dpi })
    }

    /// L'écran PRINCIPAL. ⚠️ Le processus doit être « DPI aware » (voir
    /// `win32::dpi::make_process_dpi_aware`) : sinon Windows rend une taille et un
    /// DPI VIRTUALISÉS (96).
    pub fn principal() -> Option<Self> {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
        let moniteur = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !unsafe { GetMonitorInfoW(moniteur, &mut info) }.as_bool() {
            return None;
        }
        let r = info.rcMonitor;
        let (largeur, hauteur) = (u32::try_from(r.right - r.left).ok()?, u32::try_from(r.bottom - r.top).ok()?);
        let dpi = crate::win32::dpi::get_dpi_for_monitor(moniteur);
        (largeur > 0 && hauteur > 0).then_some(Self { largeur, hauteur, dpi })
    }
}

/// Les deux facteurs de l'adaptation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Facteurs {
    /// Pour les longueurs en pixels PHYSIQUES — et le zoom des pages CEF.
    pub physique: f32,
    /// Pour les longueurs en pixels LOGIQUES, que l'app multiplie déjà par le DPI
    /// (rayon des coins d'écran, décalage des horloges plein écran).
    pub logique: f32,
}

impl Facteurs {
    pub fn entre(reference: Ecran, actuel: Ecran) -> Self {
        let physique = actuel.hauteur as f32 / reference.hauteur as f32;
        let logique = (actuel.hauteur as f32 / actuel.dpi as f32) / (reference.hauteur as f32 / reference.dpi as f32);
        Self { physique, logique }
    }

    fn neutres(&self) -> bool {
        (self.physique - 1.0).abs() < 0.005 && (self.logique - 1.0).abs() < 0.005
    }
}

/// **CE QU'ON EMPORTE EST PARFOIS FILTRÉ** — le contenu à écrire dans la copie.
/// ⚠️ Depuis que l'apparence seule voyage (2026-10-29), **aucune de ses deux
/// règles ne peut plus s'appliquer** : `dossiers.toml` et
/// `fullscreen_clock_decalages.txt` sont de l'ARRANGEMENT, donc ils ne sont même
/// plus copiés. Il reste le point unique où poser un filtrage si un fichier
/// d'apparence en réclamait un — les tests ci-dessous gardent sa mémoire.
pub fn filtrer(relatif: &str, contenu: &str) -> String {
    match relatif {
        // Les DOSSIERS épinglés de la barre du bas désignent des chemins de cette
        // machine ; les séparateurs, eux, sont de l'arrangement. Ils restent tous,
        // donc le compte de `[bars.B]` dans `zones.toml` reste juste.
        "dossiers.toml" => {
            let Ok(mut table) = contenu.parse::<toml::Table>() else { return String::new() };
            if let Some(toml::Value::Array(elements)) = table.get_mut("dossiers") {
                elements.retain(|e| e.get("type").and_then(toml::Value::as_str) == Some("separateur"));
            }
            table.to_string()
        }
        "fullscreen_clock_decalages.txt" => contenu
            .lines()
            .filter(|ligne| ligne.split_once('=').is_some_and(|(cle, _)| cle.trim() == CLE_PRINCIPAL))
            .map(|ligne| format!("{ligne}\n"))
            .collect(),
        _ => contenu.to_string(),
    }
}

/// Une longueur à l'échelle, au pixel.
fn longueur(v: f32, k: f32) -> f32 {
    (v * k).round()
}

/// Un facteur de zoom à l'échelle, au centième.
fn zoom(v: f32, k: f32) -> f32 {
    (v * k * 100.0).round() / 100.0
}

/// Chaque champ (séparé par `,`) de chaque ligne que `garde(rang)` retient passe
/// par `f` ; le reste du texte est gardé à l'octet près.
fn champs(contenu: &str, garde: impl Fn(usize) -> bool, f: impl Fn(f32) -> f32) -> String {
    let lignes: Vec<String> = contenu
        .lines()
        .map(|ligne| {
            ligne
                .split(',')
                .enumerate()
                .map(|(i, champ)| match champ.trim().parse::<f32>() {
                    Ok(v) if garde(i) => f(v).to_string(),
                    _ => champ.to_string(),
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect();
    let mut sortie = lignes.join("\n");
    if contenu.ends_with('\n') {
        sortie.push('\n');
    }
    sortie
}

/// `zones.toml` par le modèle des zones lui-même (`config::zones_from_toml` /
/// `zones_to_toml`) : les cotes, les avancées et la taille des espaces. ⛔ Un
/// fichier dont une entrée est refusée n'est PAS réécrit — la réécriture la
/// perdrait.
fn zones(contenu: &str, k: f32) -> Option<String> {
    let table = contenu.parse::<toml::Table>().ok()?;
    let (mut zones, refuses) = crate::config::zones_from_toml(&table);
    if !refuses.is_empty() {
        return None;
    }
    for barre in zones.bars.values_mut() {
        for taille in &mut barre.sizes {
            *taille = longueur(*taille, k);
        }
    }
    for zone in zones.zones.values_mut() {
        // Une cote nulle EFFACE la zone (voir `toml-entier-contre-flottant`).
        zone.cote = longueur(zone.cote, k).max(1.0);
        zone.lead = longueur(zone.lead, k);
    }
    Some(crate::config::zones_to_toml(&zones).to_string())
}

/// Les longueurs qu'on n'a jamais réglées valent leur DÉFAUT — calibré, lui aussi,
/// sur l'écran de développement. Pour garder les proportions, l'adaptation l'écrit
/// d'abord, puis le convertit. Ceux dont le défaut se déduit déjà de l'écran
/// (épaisseur des barres, lue sur la barre des tâches ; texte de la barre du haut,
/// tiré de son épaisseur) ou qui veulent dire « éteint » (fondus) ne sont pas ici.
fn defaut(relatif: &str) -> Option<f32> {
    use crate::ui::dock;
    Some(match relatif {
        "icon_gap.txt" => dock::DEFAULT_ICON_GAP,
        "icon_margin.txt" => dock::DEFAULT_ICON_MARGIN,
        "zone_cell_margin.txt" => dock::Dock::DEFAULT_ZONE_CELL_MARGIN,
        "running_dot_size.txt" => dock::DEFAULT_RUNNING_DOT_SIZE,
        "separator_size.txt" => dock::SPACER_MARKER_DIAMETER,
        "desktop_dot_size.txt" => crate::ui::topbar::DEFAULT_DESKTOP_DOT_SIZE,
        "zone_label_font.txt" => dock::DEFAULT_ZONE_LABEL_FONT_PX as f32,
        "ombre_barre_haut.txt" => dock::DEFAULT_OMBRE_BARRE_HAUT,
        "preview_thumb_width.txt" => crate::win32::window_previews::DEFAULT_THUMB_WIDTH,
        "desktop_preview_width.txt" => crate::win32::desktop_previews::DEFAULT_PREVIEW_WIDTH,
        // Les facteurs globaux des pages CEF : absents, ils valent 1.
        "textes_content_scale.txt" | "notifications_content_scale.txt" | "tooltip_scale.txt" => 1.0,
        _ => return None,
    })
}

/// **LE CONTENU ADAPTÉ D'UN FICHIER** — `None` s'il ne porte aucune longueur.
pub fn convertir(relatif: &str, contenu: &str, f: Facteurs) -> Option<String> {
    let (p, l) = (f.physique, f.logique);
    let tout = |_| true;
    Some(match relatif {
        // Un nombre (ou une liste, une ligne par barre) en pixels physiques.
        "bar_thicknesses.txt" | "icon_gap.txt" | "icon_gap_barres.txt" | "icon_margin.txt" | "zone_cell_margin.txt"
        | "running_dot_size.txt" | "running_dot_offset.txt" | "separator_size.txt" | "desktop_dot_size.txt"
        | "topbar_text_size.txt" | "zone_label_font.txt" | "fondu_haut_gauche.txt" | "fondu_haut_droite.txt"
        | "ombre_barre_haut.txt" | "preview_thumb_width.txt" | "desktop_preview_width.txt" | "menu_corner_radius.txt" => {
            champs(contenu, tout, |v| longueur(v, p))
        }
        // « nom,largeur,largeur… » : tout sauf le nom.
        "top_slots.txt" => champs(contenu, |i| i > 0, |v| longueur(v, p)),
        // `ReglagesCotes` : la largeur (1) et celle du bas (7) ; le reste est des
        // cases et des forces.
        "ombre_cotes.txt" => champs(contenu, |i| i == 1 || i == 7, |v| longueur(v, p)),
        // `ReglagesCouleur` : « largeur_haut,force_haut;<ReglagesCotes> ».
        "ombre_couleur.txt" => {
            let (haut, cotes) = contenu.trim().split_once(';')?;
            format!("{};{}", champs(haut, |i| i == 0, |v| longueur(v, p)), champs(cotes, |i| i == 1 || i == 7, |v| longueur(v, p)))
        }
        "zones.toml" => zones(contenu, p)?,
        // Les zooms des pages CEF (×3 fixe) et de l'infobulle : le facteur global
        // multiplie TOUT ce que les curseurs par panneau règlent.
        "textes_content_scale.txt" | "notifications_content_scale.txt" | "tooltip_scale.txt" => {
            champs(contenu, tout, |v| zoom(v, p))
        }
        // Pixels LOGIQUES : l'app les multiplie déjà par le DPI de l'écran.
        // Le rayon des congés de la zone principale : même règle que les coins de
        // l'écran — des pixels LOGIQUES, ×0,45 de 4K@144 à 1280×648@96 (2026-10-29).
        "coins_ecran_rayon.txt" | "coins_zone_rayon.txt" => champs(contenu, tout, |v| longueur(v, l)),
        "fullscreen_clock_decalages.txt" => contenu
            .lines()
            .map(|ligne| match ligne.split_once('=') {
                Some((cle, valeurs)) => format!("{cle}={}\n", champs(valeurs, tout, |v| longueur(v, l))),
                None => format!("{ligne}\n"),
            })
            .collect(),
        _ => return None,
    })
}

/// Tous les fichiers que `convertir` connaît — `adapter_dossier` les parcourt.
const CONVERTIS: &[&str] = &[
    "bar_thicknesses.txt",
    "icon_gap.txt",
    "icon_gap_barres.txt",
    "icon_margin.txt",
    "zone_cell_margin.txt",
    "running_dot_size.txt",
    "running_dot_offset.txt",
    "separator_size.txt",
    "desktop_dot_size.txt",
    "topbar_text_size.txt",
    "zone_label_font.txt",
    "fondu_haut_gauche.txt",
    "fondu_haut_droite.txt",
    "ombre_barre_haut.txt",
    "preview_thumb_width.txt",
    "desktop_preview_width.txt",
    "menu_corner_radius.txt",
    "top_slots.txt",
    "ombre_cotes.txt",
    "ombre_couleur.txt",
    "zones.toml",
    "textes_content_scale.txt",
    "notifications_content_scale.txt",
    "tooltip_scale.txt",
    "coins_ecran_rayon.txt",
    "coins_zone_rayon.txt",
    "fullscreen_clock_decalages.txt",
];

/// **ADAPTE LA CONFIGURATION DE `dossier` À `actuel`** — rend le nombre de
/// fichiers réécrits. `garde` borne ce qu'on touche (l'import d'une archive
/// d'apparence ne convertit que l'apparence : le reste est déjà d'ici).
///
/// Sans `REFERENCE`, il n'y a rien à convertir : l'écran actuel devient la
/// référence. Dans tous les cas, `REFERENCE` vaut `actuel` à la fin.
pub fn adapter_dossier(dossier: &Path, actuel: Ecran, garde: impl Fn(&str) -> bool) -> io::Result<usize> {
    let reference = fs::read_to_string(dossier.join(REFERENCE)).ok().and_then(|c| Ecran::lire(&c));
    let mut reecrits = 0;
    if let Some(reference) = reference {
        let facteurs = Facteurs::entre(reference, actuel);
        if !facteurs.neutres() {
            for &relatif in CONVERTIS.iter().filter(|r| garde(r)) {
                let chemin = dossier.join(relatif);
                let contenu = match fs::read_to_string(&chemin) {
                    Ok(contenu) => contenu,
                    Err(_) => match defaut(relatif) {
                        Some(v) => v.to_string(),
                        None => continue,
                    },
                };
                if let Some(nouveau) = convertir(relatif, &contenu, facteurs) {
                    fs::write(&chemin, nouveau)?;
                    reecrits += 1;
                }
            }
        }
    }
    if elargir_les_barres(dossier, actuel.dpi, actuel.hauteur)? {
        reecrits += 1;
    }
    fs::write(dossier.join(REFERENCE), actuel.serialiser())?;
    Ok(reecrits)
}

/// **AUCUNE BARRE PLUS ÉTROITE QUE CE QU'ELLE DOIT PORTER** — demande du
/// 2026-10-29 : « dans la barre de droite les zones qui avaient plusieurs
/// colonnes n'en ont qu'une, si les barres calculées ne sont pas assez larges
/// fait en sorte qu'elles soient assez larges automatiquement ».
///
/// ⛔ **POURQUOI LA CONVERSION NE SUFFIT PAS** : les épaisseurs sont converties
/// par la HAUTEUR (sa barre de droite, 124 px, devient 37), mais DEUX planchers
/// restent des constantes en pixels physiques — `ui::dock::ICON_SIZE_MIN` (32) et
/// `ui::dock::MIN_ZONE_COLUMN_PX` (ajusté au DPI seulement). Mesuré sur la VM :
/// 37 / 21,33 = **1 colonne** là où `zones.toml` en demande 3, et à gauche
/// `29 − 2×9 = 11` relevé à **32** — l'icône remplit la barre et dépasse ses
/// marges.
///
/// La règle est celle-là même, retournée : la barre de DROITE s'ÉLARGIT jusqu'à
/// pouvoir ses colonnes. Rien n'est jamais rétréci.
///
/// ⭐ **LA GAUCHE PREND ENSUITE LA LARGEUR DE LA DROITE** (demande du même jour :
/// « mets une taille égale à la taille de la barre de droite pour la barre de
/// gauche automatiquement par défaut »). ⚠️ Conséquence assumée : à l'échelle
/// ×1,5, une icône de gauche remplit une barre de 64 px (base 46 → 69, plafonnée
/// à 64), alors que celle de droite, sans échelle, y fait 46. Si l'utilisateur
/// veut l'air autour, c'est l'échelle de gauche qu'il baisse, pas la barre.
pub fn elargir_les_barres(dossier: &Path, dpi: u32, hauteur: u32) -> io::Result<bool> {
    // ⚠️ **LA HAUTEUR EST UN PARAMÈTRE, PAS UNE LECTURE DU SYSTÈME** — l'adaptation tourne
    // avant `app::run`, et c'est ici que les planchers doivent prendre l'échelle de l'écran
    // qui REÇOIT. En faire un paramètre la rend aussi VÉRIFIABLE : un test qui lisait l'écran
    // de la machine aurait valu 43 px au lieu de 64 sur un poste non DPI-aware, et n'aurait
    // rien dit de la VM.
    crate::ui::dock::poser_hauteur_des_planchers(hauteur as f32);
    let chemin = dossier.join("bar_thicknesses.txt");
    let Ok(texte) = fs::read_to_string(&chemin) else { return Ok(false) };
    let mut valeurs: Vec<f32> = texte
        .split(',')
        .filter_map(|v| v.trim().parse::<f32>().ok())
        .collect();
    // `[haut, gauche, droite]` ou `[haut, gauche, droite, bas]` — même forme que
    // `config::load_bar_thicknesses`, qui accepte les deux.
    if valeurs.len() < 3 {
        return Ok(false);
    }
    // ⛔ **ON DEMANDE LE PLANCHER À SA SOURCE, ON NE LE RECALCULE PAS.** Cette ligne
    // recalculait `MIN_ZONE_COLUMN_PX × dpi / 144` à la main, donc elle IGNORAIT le facteur
    // de hauteur ajouté à `plancher_de_colonne()` — et la VM a produit `18,64,64,24` :
    // 64 = 3 × 21,33 = trois colonnes du plancher NU, la barre convertie à 37 étant écrasée.
    // Une seule définition, sinon la correction s'arrête à la porte qu'on a franchie.
    crate::ui::dock::poser_dpi_du_plancher_de_colonne(dpi);
    let plancher = crate::ui::dock::plancher_de_colonne();
    let colonnes = colonnes_de_la_barre_de_droite(dossier).max(1) as f32;

    // ⛔ SEULE LA BARRE DE DROITE : la GAUCHE n'est PAS élargie — une icône trop
    // grande dans une barre fine se règle par le plancher (`ui::dock::taille_de_base`),
    // pas en épaississant la barre, qui l'aurait fait grandir avec elle.
    let droite = valeurs[2].max(colonnes * plancher);
    // ⭐ **LA GAUCHE PREND LA LARGEUR DE LA DROITE** — demande du 2026-10-29 :
    // « mets une taille égale à la taille de la barre de droite pour la barre de
    // gauche automatiquement par défaut ». Deux barres de largeurs différentes
    // se lisaient mal une fois la droite élargie pour ses colonnes.
    let gauche = droite;
    if gauche == valeurs[1] && droite == valeurs[2] {
        return Ok(false);
    }
    valeurs[1] = gauche;
    valeurs[2] = droite;
    let ligne: Vec<String> = valeurs.iter().map(|v| v.round().to_string()).collect();
    fs::write(&chemin, ligne.join(","))?;
    Ok(true)
}

#[cfg(test)]
mod tests_elargir_les_barres {
    use super::*;
    use std::path::PathBuf;

    fn bac(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("neuradesk_barres_{nom}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// Les nombres de la VM du 2026-10-29 : barre de droite 37 px, marge 9,
    /// plancher 21,33 à 96 DPI, trois colonnes demandées en `D4`.
    #[test]
    fn la_barre_s_elargit_pour_les_colonnes_et_pour_l_icone() {
        let d = bac("elargit");
        fs::write(d.join("bar_thicknesses.txt"), "18,29,37,24").unwrap();
        fs::write(d.join("icon_margin.txt"), "9").unwrap();
        fs::write(
            d.join("zones.toml"),
            "[zones.D1]\ncolumns = 2\n\n[zones.D4]\ncolumns = 3\n\n[zones.G1]\ncolumns = 5\n",
        )
        .unwrap();
        assert!(elargir_les_barres(&d, 96, 648).unwrap());
        // ⭐ **SUR LA VM, LA BARRE N'EST PLUS ÉLARGIE DU TOUT** : le plancher de colonne
        // vaut 32 × 96/144 × (648/2160) = 6,4, donc trois colonnes tiennent dans 19,2 px —
        // moins que les 37 px convertis. C'était 64 avant le correctif du 2026-10-30
        // (« les barres de gauche et de droite sont toujours énormes »).
        assert_eq!(fs::read_to_string(d.join("bar_thicknesses.txt")).unwrap(), "18,37,37,24");
        let _ = fs::remove_dir_all(&d);
    }

    /// ⭐ SUR LA MACHINE DE L'UTILISATEUR, RIEN NE CHANGE : sa barre fait déjà
    /// 98/124 px à 144 DPI, et le plancher y vaut 32.
    #[test]
    fn une_barre_deja_assez_large_n_est_elargie_que_pour_egaliser() {
        let d = bac("intacte");
        fs::write(d.join("bar_thicknesses.txt"), "60,98,124,80").unwrap();
        fs::write(d.join("icon_margin.txt"), "29").unwrap();
        fs::write(d.join("zones.toml"), "[zones.D4]\ncolumns = 3\n").unwrap();
        // À 144 DPI le plancher vaut 32 : 3 × 32 = 96 < 124, la DROITE ne bouge
        // pas. Seule la GAUCHE change, pour prendre sa largeur (98 → 124).
        assert!(elargir_les_barres(&d, 144, 2160).unwrap());
        assert_eq!(fs::read_to_string(d.join("bar_thicknesses.txt")).unwrap(), "60,124,124,80");
        // Rejoué : plus rien à faire.
        assert!(!elargir_les_barres(&d, 144, 2160).unwrap());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn sans_fichier_d_epaisseurs_il_n_y_a_rien_a_elargir() {
        let d = bac("vide");
        assert!(!elargir_les_barres(&d, 96, 648).unwrap());
        assert!(!d.join("bar_thicknesses.txt").exists());
        let _ = fs::remove_dir_all(&d);
    }
}

/// Le nombre de colonnes le plus élevé demandé sur la barre de DROITE —
/// `[zones.D*]` de `zones.toml`. Zéro s'il n'y a pas de fichier.
fn colonnes_de_la_barre_de_droite(dossier: &Path) -> u16 {
    let Ok(texte) = fs::read_to_string(dossier.join("zones.toml")) else { return 0 };
    let mut dans_droite = false;
    let mut colonnes = 0u16;
    for ligne in texte.lines() {
        let ligne = ligne.trim();
        if ligne.starts_with('[') {
            dans_droite = ligne.starts_with("[zones.D");
            continue;
        }
        if dans_droite {
            if let Some(v) = ligne.strip_prefix("columns").and_then(|r| r.split_once('=')).and_then(|(_, v)| v.trim().parse::<u16>().ok()) {
                colonnes = colonnes.max(v);
            }
        }
    }
    colonnes
}

/// **ÉCRIT LA COPIE DE `source` DANS `cible`** — rend le nombre de fichiers.
/// `cible` est vidé de ses fichiers d'abord : une copie précédente ne doit rien y
/// laisser (une épingle, un écran secondaire). `REFERENCE` est celle de `source`,
/// ou `actuel` si elle n'en a pas.
pub fn exporter(source: &Path, cible: &Path, actuel: Ecran) -> io::Result<usize> {
    if cible.is_dir() {
        fs::remove_dir_all(cible)?;
    }
    fs::create_dir_all(cible)?;
    let mut copies = 0;
    let mut copier = |relatif: &str, de: &Path| -> io::Result<()> {
        let vers = relatif.split('/').fold(cible.to_path_buf(), |c, m| c.join(m));
        if let Some(parent) = vers.parent() {
            fs::create_dir_all(parent)?;
        }
        match fs::read_to_string(de) {
            Ok(texte) => fs::write(&vers, filtrer(relatif, &texte))?,
            // Une image, un binaire : tel quel.
            Err(_) => {
                fs::copy(de, &vers)?;
            }
        }
        copies += 1;
        Ok(())
    };
    for entree in fs::read_dir(source)? {
        let entree = entree?;
        let nom = entree.file_name().to_string_lossy().into_owned();
        if entree.file_type()?.is_file() {
            if est_emporte(&nom) && nom != REFERENCE {
                copier(&nom, &entree.path())?;
            }
        } else if crate::sauvegarde_config::SOUS_DOSSIERS_APPARENCE.contains(&nom.as_str()) {
            for sous in fs::read_dir(entree.path())? {
                let sous = sous?;
                let relatif = format!("{nom}/{}", sous.file_name().to_string_lossy());
                if sous.file_type()?.is_file() && est_emporte(&relatif) {
                    copier(&relatif, &sous.path())?;
                }
            }
        }
    }
    // ⭐ LE RAYON DES CONGÉS, RENDU EXPLICITE POUR ÊTRE CONVERTI (2026-10-29).
    // Tant qu'il n'a pas été réglé, il n'existe aucun fichier : c'est une
    // constante du STYLE (`ui::dock::rayon_conge_du_style`), qu'aucun `CONVERTIS`
    // ne toucherait — le portable emporterait donc un rayon de 4K sur un écran
    // plus petit (mesuré : 56 px contre une barre de 18 px sur une VM 1280×648).
    // On l'écrit ICI, AVANT la conversion : `adapter_dossier` le convertira.
    let rayon = cible.join("coins_zone_rayon.txt");
    if !rayon.is_file() {
        // `trouver` retombe sur le style de base pour un identifiant inconnu, et
        // sur lui aussi quand `style.txt` est absent : une seule règle de repli.
        let id = fs::read_to_string(source.join("style.txt")).unwrap_or_default();
        let style = crate::ui::style::trouver(id.trim());
        fs::write(&rayon, crate::ui::dock::rayon_conge_du_style(&style).to_string())?;
        copies += 1;
    }
    let reference = fs::read_to_string(source.join(REFERENCE)).ok().and_then(|c| Ecran::lire(&c)).unwrap_or(actuel);
    fs::write(cible.join(REFERENCE), reference.serialiser())?;
    Ok(copies)
}

/// Un dossier de configuration « vierge » : absent, ou sans aucun fichier.
fn est_vierge(dossier: &Path) -> bool {
    match fs::read_dir(dossier) {
        Err(_) => true,
        Ok(entrees) => !entrees.flatten().any(|e| e.file_type().is_ok_and(|t| t.is_file())),
    }
}

/// **SÈME `initiale` DANS `dossier` S'IL EST VIERGE, PUIS L'ADAPTE** — `Some` avec
/// le nombre de fichiers semés, `None` s'il n'y avait rien à faire.
pub fn semer(initiale: &Path, dossier: &Path, actuel: Ecran) -> io::Result<Option<usize>> {
    if !initiale.join(REFERENCE).is_file() || !est_vierge(dossier) {
        return Ok(None);
    }
    let mut semes = 0;
    let mut a_copier = vec![(initiale.to_path_buf(), dossier.to_path_buf())];
    while let Some((de, vers)) = a_copier.pop() {
        fs::create_dir_all(&vers)?;
        for entree in fs::read_dir(&de)? {
            let entree = entree?;
            let cible = vers.join(entree.file_name());
            if entree.file_type()?.is_dir() {
                a_copier.push((entree.path(), cible));
            } else {
                fs::copy(entree.path(), cible)?;
                semes += 1;
            }
        }
    }
    adapter_dossier(dossier, actuel, |_| true)?;
    Ok(Some(semes))
}

/// **LE POINT D'APPEL DE `main`, AU LANCEMENT** — après `make_process_dpi_aware`
/// (sinon l'écran mesuré est virtualisé) et avant toute lecture de réglage.
pub fn au_lancement() {
    let (Some(dossier), Some(actuel)) = (crate::config::config_dir(), Ecran::principal()) else { return };
    let initiale = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.join(DOSSIER)));
    if let Some(initiale) = initiale {
        match semer(&initiale, &dossier, actuel) {
            Ok(None) => {}
            Ok(Some(n)) => {
                crate::diag!("[config-initiale] {n} fichier(s) semés, adaptés à {}", actuel.serialiser());
                return;
            }
            Err(err) => crate::diag!("[config-initiale] semis interrompu : {err}"),
        }
    }
    let marque = dossier.join(A_ADAPTER);
    let resultat = match fs::read_to_string(&marque) {
        Ok(sorte) if sorte.trim() == "apparence" => adapter_dossier(&dossier, actuel, crate::sauvegarde_config::est_apparence),
        Ok(_) => adapter_dossier(&dossier, actuel, |_| true),
        // Ni semis ni import : la référence n'est écrite que si elle manque.
        Err(_) if !dossier.join(REFERENCE).is_file() && dossier.is_dir() => adapter_dossier(&dossier, actuel, |_| false),
        Err(_) => return,
    };
    match resultat {
        Ok(n) => crate::diag!("[config-initiale] adaptée à {} : {n} fichier(s) réécrits", actuel.serialiser()),
        Err(err) => crate::diag!("[config-initiale] adaptation interrompue : {err}"),
    }
    let _ = fs::remove_file(marque);
}

/// **`neuradesk.exe --config-initiale <dossier>`** — écrit la copie et SORT, sans
/// fenêtre ni CEF. Même discipline que `win32::decodeur_document`.
pub fn exporter_si_demande() {
    let arguments: Vec<_> = std::env::args_os().collect();
    if arguments.get(1).is_none_or(|a| a != DRAPEAU) {
        return;
    }
    let Some(cible) = arguments.get(2) else { std::process::exit(2) };
    crate::win32::dpi::make_process_dpi_aware();
    let (Some(source), Some(actuel)) = (crate::config::config_dir(), Ecran::principal()) else { std::process::exit(3) };
    match exporter(&source, Path::new(cible), actuel) {
        Ok(n) => {
            println!("{n} fichier(s) copies vers {}", Path::new(cible).display());
            std::process::exit(0);
        }
        Err(err) => {
            eprintln!("copie impossible : {err}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const QUATRE_K: Ecran = Ecran { largeur: 3840, hauteur: 2160, dpi: 144 };
    const FULL_HD: Ecran = Ecran { largeur: 1920, hauteur: 1080, dpi: 96 };

    fn dossier_de_test(nom: &str) -> PathBuf {
        let dossier = std::env::temp_dir().join(format!("neuradesk_config_initiale_{nom}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dossier);
        fs::create_dir_all(&dossier).unwrap();
        dossier
    }

    #[test]
    fn les_facteurs_suivent_la_hauteur_et_le_dpi() {
        let f = Facteurs::entre(QUATRE_K, FULL_HD);
        assert_eq!(f.physique, 0.5);
        // 1080 / 96 = 11,25 pouces de haut contre 2160 / 144 = 15.
        assert_eq!(f.logique, 0.75);
        // Un ultra-large de même hauteur : la largeur ne compte pas.
        let large = Facteurs::entre(QUATRE_K, Ecran { largeur: 5120, hauteur: 2160, dpi: 144 });
        assert!(large.neutres());
    }

    #[test]
    fn l_ecran_se_relit() {
        assert_eq!(Ecran::lire(&QUATRE_K.serialiser()), Some(QUATRE_K));
        for faux in ["", "3840,2160", "3840,0,144", "a,b,c", "1,2,3,4"] {
            assert_eq!(Ecran::lire(faux), None, "{faux:?}");
        }
    }

    #[test]
    fn l_apparence_et_la_disposition_partent() {
        // Demande du 2026-10-29 : « je ne veux que les réglages d'apparence »,
        // puis « mes colonnes de la barre de droite ne suivent pas » — la
        // DISPOSITION (zones et ordre des icônes) revient donc, mais pas les
        // épingles, ni les dossiers de la barre du bas, ni les données.
        for oui in [
            "zones.toml",
            "icon_order.txt",
            // ⭐ Sans eux, `zones.toml` ne veut rien dire : les zones sont des
            // plages CONTIGUËS de la barre, et c'est l'appartenance des applets
            // aux barres qui décide de ce qui tombe dans chacune (mesure du
            // 2026-10-29 en VM : colonnes non respectées, icônes hors zone).
            "terminal_integres.txt",
            "lueurs_integres.txt",
            "style.txt",
            "icones.txt",
            "bar_thicknesses.txt",
            "theme_famille.txt",
            "degrade_sauvegardes.txt",
            "ombre_barre.txt",
            "dash_accent_color.txt",
            "tooltip_fade_ms.txt",
            "palettes/a.gpl",
            "textures/x.png",
            "lueurs_images/a.png",
            "shaders/s.glsl",
        ] {
            assert!(est_emporte(oui), "{oui} part");
        }
        for non in [
            "dossiers.toml",
            "feature_clock.txt",
            "dock_pins.txt",
            // ⭐ La réserve de places sans les épingles ne faisait que des TROUS
            // dans la barre de gauche de la VM (2026-10-29).
            "pin_capacity.txt",
            "categories.toml",
            "notes.toml",
            "ssh.toml",
            "envoi_image.toml",
            "secondary_screens.txt",
            "dossiers_images/a.png",
            "notes_images/a.png",
            "clipboard_history.toml",
            "icon_order.txt.avant-reserve-epingles",
            A_ADAPTER,
        ] {
            assert!(!est_emporte(non), "{non} reste");
        }
    }

    #[test]
    fn les_dossiers_epingles_partent_les_separateurs_restent() {
        let contenu = "[[dossiers]]\nsorte = \"elastique\"\ntype = \"separateur\"\n\n[[dossiers]]\nchemin = 'E:\\Vrac'\nvignette = \"a.png\"\n\n[[dossiers]]\ntype = \"separateur\"\n";
        let filtre = filtrer("dossiers.toml", contenu);
        let table: toml::Table = filtre.parse().unwrap();
        let elements = table["dossiers"].as_array().unwrap();
        assert_eq!(elements.len(), 2);
        assert!(!filtre.contains("Vrac"));
        assert_eq!(elements[0].get("sorte").and_then(toml::Value::as_str), Some("elastique"));
    }

    #[test]
    fn l_horloge_plein_ecran_ne_garde_que_l_ecran_principal() {
        let filtre = filtrer("fullscreen_clock_decalages.txt", "\\\\.\\DISPLAY2=10,20\nprincipal=30,-40\n");
        assert_eq!(filtre, "principal=30,-40\n");
    }

    #[test]
    fn les_longueurs_passent_a_l_echelle_et_le_reste_ne_bouge_pas() {
        let f = Facteurs::entre(QUATRE_K, FULL_HD);
        assert_eq!(convertir("bar_thicknesses.txt", "60,124,124,80", f).unwrap(), "30,62,62,40");
        assert_eq!(convertir("icon_gap_barres.txt", "20\n12\n5\n10\n", f).unwrap(), "10\n6\n3\n5\n");
        assert_eq!(convertir("top_slots.txt", "apres-les-pastilles,40,40,34", f).unwrap(), "apres-les-pastilles,20,20,17");
        // Cases et forces gardées ; largeurs (1 et 7) divisées.
        assert_eq!(convertir("ombre_cotes.txt", "1,100,0.85,1,1,1,1,100,0.85", f).unwrap(), "1,50,0.85,1,1,1,1,50,0.85");
        assert_eq!(convertir("ombre_couleur.txt", "250,1;1,250,1,1,1,1,1,0,1", f).unwrap(), "125,1;1,125,1,1,1,1,1,0,1");
        assert_eq!(convertir("textes_content_scale.txt", "1", f).unwrap(), "0.5");
        // « auto » n'est pas un nombre : il reste.
        assert_eq!(convertir("topbar_text_size.txt", "auto", f).unwrap(), "auto");
        // Pixels logiques : ×0,75. Le rayon des congés de la zone suit la même
        // règle que les coins de l'écran (2026-10-29 : 56 px de rayon contre une
        // barre de 18 px sur une VM 1280×648, parce que RIEN ne le convertissait).
        assert_eq!(convertir("coins_ecran_rayon.txt", "40", f).unwrap(), "30");
        assert_eq!(convertir("coins_zone_rayon.txt", "56", f).unwrap(), "42");
        assert_eq!(convertir("fullscreen_clock_decalages.txt", "principal=40,-80\n", f).unwrap(), "principal=30,-60\n");
        // Un ratio ou une couleur : rien à convertir.
        assert_eq!(convertir("hover_scale.txt", "1.5", f), None);
        assert_eq!(convertir("menu_scale.txt", "0.7", f), None);
    }

    #[test]
    fn les_zones_passent_a_l_echelle_et_restent_flottantes() {
        let contenu = "[bars.D]\nsizes = [8.0, 18.0]\nspacers = 2\n\n[zones.D1]\ncolumns = 2\ncote = 889.0\nlabel = true\nlead = 64.0\nname = \"Intégrés\"\n";
        let f = Facteurs::entre(QUATRE_K, FULL_HD);
        let sortie = convertir("zones.toml", contenu, f).unwrap();
        let (zones, refuses) = crate::config::zones_from_toml(&sortie.parse().unwrap());
        assert!(refuses.is_empty());
        let zone = zones.zones.values().next().unwrap();
        assert_eq!((zone.cote, zone.lead, zone.columns), (445.0, 32.0, 2));
        assert_eq!(zone.name.as_deref(), Some("Intégrés"));
        assert_eq!(zones.bars.values().next().unwrap().sizes, vec![4.0, 9.0]);
        // ⛔ Une cote entière en TOML effaçait une zone : elle reste un flottant.
        assert!(sortie.contains("cote = 445.0"), "{sortie}");
    }

    #[test]
    fn l_adaptation_ecrit_les_defauts_puis_la_reference() {
        let dossier = dossier_de_test("defauts");
        fs::write(dossier.join(REFERENCE), QUATRE_K.serialiser()).unwrap();
        fs::write(dossier.join("bar_thicknesses.txt"), "60,124,124,80").unwrap();
        let n = adapter_dossier(&dossier, FULL_HD, |_| true).unwrap();
        let lire = |nom: &str| fs::read_to_string(dossier.join(nom)).unwrap();
        assert_eq!(lire("bar_thicknesses.txt"), "30,62,62,40");
        // Jamais réglé : le défaut (12), à l'échelle.
        assert_eq!(lire("icon_gap.txt"), "6");
        assert_eq!(lire("textes_content_scale.txt"), "0.5");
        // Un défaut qui veut dire « éteint » n'est pas écrit.
        assert!(!dossier.join("fondu_haut_gauche.txt").exists());
        assert!(n > 2);
        assert_eq!(Ecran::lire(&lire(REFERENCE)), Some(FULL_HD));
        // ⭐ UNE FOIS : relancer ne divise pas une seconde fois.
        assert_eq!(adapter_dossier(&dossier, FULL_HD, |_| true).unwrap(), 0);
        assert_eq!(lire("bar_thicknesses.txt"), "30,62,62,40");
        let _ = fs::remove_dir_all(&dossier);
    }

    #[test]
    fn sans_reference_rien_n_est_converti() {
        let dossier = dossier_de_test("sans_reference");
        fs::write(dossier.join("bar_thicknesses.txt"), "60,124,124,80").unwrap();
        assert_eq!(adapter_dossier(&dossier, FULL_HD, |_| true).unwrap(), 0);
        assert_eq!(fs::read_to_string(dossier.join("bar_thicknesses.txt")).unwrap(), "60,124,124,80");
        assert_eq!(fs::read_to_string(dossier.join(REFERENCE)).unwrap(), FULL_HD.serialiser());
        let _ = fs::remove_dir_all(&dossier);
    }

    #[test]
    fn l_apparence_seule_ne_convertit_pas_les_zones() {
        let dossier = dossier_de_test("apparence");
        fs::write(dossier.join(REFERENCE), QUATRE_K.serialiser()).unwrap();
        fs::write(dossier.join("bar_thicknesses.txt"), "60,124,124,80").unwrap();
        let zones = "[zones.D1]\ncolumns = 2\ncote = 889.0\n";
        fs::write(dossier.join("zones.toml"), zones).unwrap();
        adapter_dossier(&dossier, FULL_HD, crate::sauvegarde_config::est_apparence).unwrap();
        assert_eq!(fs::read_to_string(dossier.join("bar_thicknesses.txt")).unwrap(), "30,62,62,40");
        assert_eq!(fs::read_to_string(dossier.join("zones.toml")).unwrap(), zones);
        let _ = fs::remove_dir_all(&dossier);
    }

    #[test]
    fn la_copie_puis_le_semis_de_bout_en_bout() {
        let racine = dossier_de_test("bout_en_bout");
        let (ici, copie, ailleurs) = (racine.join("ici"), racine.join("copie"), racine.join("ailleurs"));
        fs::create_dir_all(ici.join("palettes")).unwrap();
        fs::create_dir_all(ici.join("notes_images")).unwrap();
        fs::write(ici.join("bar_thicknesses.txt"), "60,124,124,80").unwrap();
        fs::write(ici.join("dock_pins.txt"), "C:\\x.exe").unwrap();
        fs::write(ici.join("secondary_screens.txt"), "\\\\.\\DISPLAY2").unwrap();
        fs::write(ici.join("palettes/a.gpl"), "GIMP Palette").unwrap();
        fs::write(ici.join("notes_images/a.png"), [0u8, 1, 2]).unwrap();
        // Un reste d'une copie précédente qui ne voyage PLUS disparaît.
        fs::create_dir_all(&copie).unwrap();
        fs::write(copie.join("secondary_screens.txt"), "\\\\.\\DISPLAY2").unwrap();
        fs::write(copie.join("dock_pins.txt"), "ancien").unwrap();

        let n = exporter(&ici, &copie, QUATRE_K).unwrap();
        assert_eq!(n, 3);
        // ⭐ 2026-10-29 : l'APPARENCE SEULE. L'épingle est un réglage, mais pas
        // d'APPARENCE : elle ne part pas — même le reste d'une copie d'avant
        // (posé plus haut) est retiré. L'écran secondaire, lui, ne désigne rien
        // ailleurs.
        assert!(!copie.join("dock_pins.txt").exists());
        assert!(!copie.join("secondary_screens.txt").exists());
        assert!(!copie.join("notes_images").exists());
        // ⭐ Le rayon des congés est RENDU EXPLICITE à l'export (le style de base
        // donne 10, donc 10 × 2 = 20 px) : sans fichier, aucun `CONVERTIS` ne le
        // toucherait et le portable garderait un rayon de 4K.
        assert_eq!(fs::read_to_string(copie.join("coins_zone_rayon.txt")).unwrap(), "20");
        assert_eq!(Ecran::lire(&fs::read_to_string(copie.join(REFERENCE)).unwrap()), Some(QUATRE_K));

        assert_eq!(semer(&copie, &ailleurs, FULL_HD).unwrap(), Some(4));
        // ×0,75 (pixels logiques) : 20 → 15.
        assert_eq!(fs::read_to_string(ailleurs.join("coins_zone_rayon.txt")).unwrap(), "15");
        assert_eq!(fs::read_to_string(ailleurs.join("bar_thicknesses.txt")).unwrap(), "30,62,62,40");
        assert_eq!(fs::read_to_string(ailleurs.join("palettes/a.gpl")).unwrap(), "GIMP Palette");
        // ⛔ Une configuration qui existe n'est JAMAIS écrasée.
        fs::write(ailleurs.join("bar_thicknesses.txt"), "1,2,3,4").unwrap();
        assert_eq!(semer(&copie, &ailleurs, FULL_HD).unwrap(), None);
        assert_eq!(fs::read_to_string(ailleurs.join("bar_thicknesses.txt")).unwrap(), "1,2,3,4");
        let _ = fs::remove_dir_all(&racine);
    }
}
