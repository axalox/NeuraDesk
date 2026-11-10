pub mod app_categories;
pub mod app_identity;
pub mod appbar;
pub mod appicon;
pub mod apps;
pub mod apps_cache;
pub mod file_dialog;
pub mod cef_sous_dock;
pub mod sous_dock;
pub mod audio;
pub mod automatisations;
pub mod bandeau;
pub mod bulle;
pub mod chien_de_garde;
pub mod cef_app;
pub mod cef_apropos;
pub mod cef_bar_size;
pub mod cef_calendar;
pub mod cef_calendar_applet;
pub mod cef_clipboard;
pub mod cef_data;
pub mod cef_image_editor;
pub mod cef_minuteur;
pub mod cef_calculatrice;
pub mod cef_couleurs;
pub mod cef_surveillance;
pub mod cef_confirm;
pub mod cef_dash;
pub mod cef_context;
pub mod cef_dossier_menu;
pub mod cef_network;
pub mod cef_notes;
pub mod cef_popup;
pub mod cef_settings;
pub mod cef_sound;
pub mod cef_spacer_size;
pub mod cef_test_window;
pub mod cef_voicemeeter;
pub mod capture_ecran;
pub mod poignees;
pub mod clipboard;
pub mod clipboard_fichiers;
pub mod actions_fichiers;
pub mod cef_visionneuse;
pub mod texte_fichier;
pub mod toast_garde;
pub mod toast_duree;
pub mod toast_natif;
pub mod toast_veille;
pub mod clock;
pub mod debug_capture;
pub mod decodeur_document;
pub mod enregistrement;
pub mod gst;
/// Le pilote de l'enregistreur (applet « Enregistrer »).
pub mod enregistreur;
/// La FENÊTRE de l'applet « Enregistrer ».
pub mod cef_enregistrer;
/// La FENÊTRE du jeu 2048 (fork de GNOME 2048).
pub mod cef_jeu2048;
/// La FENÊTRE du jeu « Cinq ou plus » (fork de GNOME Five or More).
pub mod cef_cinq_ou_plus;
/// La FENÊTRE du jeu « Quadrapassel » (fork du Tetris de GNOME).
pub mod cef_quadrapassel;
/// La FENÊTRE du jeu « Lights Off » (fork de GNOME Lights Off).
pub mod cef_lightsoff;
/// La FENÊTRE du jeu Reversi (Othello, « lagno » / iagno).
pub mod cef_reversi;
/// La FENÊTRE du jeu Bovo (Gomoku, cinq pierres alignées).
pub mod cef_bovo;
/// La FENÊTRE de la Patience (le Klondike de KDE, KPatience).
pub mod cef_patience;
/// La FENÊTRE de l'applet « Capture d'écran » — même présentation que
/// l'enregistreur, avec la webcam en plus (voir `cef_capture`).
pub mod cef_capture;
pub mod desktop_previews;
pub mod desktops;
pub mod display;
pub mod document_image;
pub mod dpi;
pub mod epic_apps;
pub mod envoi_image;
pub mod coins_ecran;
pub mod coins_secondaires;
pub mod centre_haut;
pub mod ombre_barre;
pub mod texture_terminaux;
pub mod ecran_accueil;
pub mod epingle;
pub mod exe_metadata;
pub mod fade;
pub mod fetes;
pub mod format_psd;
pub mod format_xcf;
pub mod loupe;
pub mod palette_gpl;
/// La bibliothèque des effets de lueurs (voir `gfx::lueur`).
pub mod lueurs_effets;
pub mod textures_bibliotheque;
pub mod fullscreen_clock;
pub mod plein_ecran_zone;
pub mod ouverture_fenetres;
pub mod jeux_fenetres;
pub mod gestionnaire_peripheriques;
pub mod observateur_evenements;
pub mod gog_apps;
pub mod media;
#[cfg(test)] // une sonde : son seul point d'entrée est le test ignoré `ou_va_la_memoire`
pub mod memoire;
pub mod menu;
pub mod minuteur;
pub mod calculatrice;
/// **OÙ L'ON ÉCRIT** — la dernière fenêtre étrangère retenue et la frappe qui s'y
/// adresse. Partagé par les DEUX fenêtres qui écrivent (caractères, kaomojis) : la
/// cible est une propriété du bureau, elle n'a qu'un traqueur.
pub mod cible_de_frappe;
/// Les caractères accentués : la TABLE et la frappe (applet « Caractères »).
pub mod caracteres;
/// Les kaomojis : la TABLE (applet « Kaomojis »). La fenêtre est `cef_kaomoji`.
pub mod kaomoji;
/// La FENÊTRE des kaomojis — une page CEF, sœur de celle des caractères.
pub mod cef_kaomoji;
/// La FENÊTRE des caractères — une page CEF, comme la visionneuse.
pub mod cef_caracteres;
/// **L'AMÉLIORATION DE LUMINOSITÉ ET DE CONTRASTE D'UNE PAGE SCANNÉE** — le calcul
/// pur (histogramme, niveaux automatiques, pixels, écriture d'une COPIE), partagé
/// par les deux hôtes de la vue agrandie. L'état du réglage vit dans `cef_page_scan`.
pub mod amelioration;
/// L'APPLET SCANNER et son panneau — NAPS2 piloté depuis le dock.
pub mod scanner;
pub mod cef_page_scan;
pub mod cef_scanner;
/// La FENÊTRE de l'éditeur de shaders — les motifs du dégradé animé (voir
/// `gfx::shader_utilisateur`).
pub mod cef_shaders;
/// Les symboles phonétiques : la TABLE (applet « Symboles phonétiques »).
pub mod phonetique;
pub mod polices;
pub mod maths;
/// La FENÊTRE des symboles phonétiques — deux onglets, sœur des kaomojis.
pub mod cef_phonetique;
pub mod cef_maths;
/// Le DASH qui édite les lueurs (voir `gfx::lueur`).
pub mod cef_lueurs;
/// Le DASH qui édite la texture personnalisée (voir `gfx::texture_perso`).
pub mod cef_textures;
/// Démarrer et arrêter le Narrateur de Windows.
pub mod narrateur;
/// Ouvrir la Loupe de Windows (`Magnify.exe`) — à ne pas confondre avec `loupe`,
/// le grossissement de la pipette du sélecteur de couleurs.
pub mod loupe_windows;
/// Le clavier visuel de Windows (applet « Clavier visuel »).
pub mod clavier_visuel;
pub mod ocr;
pub mod texte_capture;
pub mod outils_applets;
pub mod portables;
pub mod outils_reseau;
pub mod surveillance;
/// Le moniteur système : mesures et historique (voir `cef_moniteur`).
pub mod moniteur_systeme;
/// Les tâches qui consomment le plus de processeur (voir `cef_moniteur`).
pub mod moniteur_taches;
/// Un graphique par disque et par carte graphique (voir `cef_moniteur`).
pub mod moniteur_peripheriques;
/// Le DASH du moniteur système.
pub mod cef_moniteur;
pub mod cef_sphere;
pub mod cef_ssh;
pub mod cef_fenetres;
pub mod clic_molette_bureau;
pub mod cef_terminal;
pub mod ssh;
pub mod ssh_serveurs;
pub mod commandes;
pub mod monitors;
pub mod network;
pub mod notification;
pub mod notes;
pub mod power;
/// La VERSION de Windows, lue une fois — la lignée (10 ou 11) se décide au
/// BUILD, jamais au majeur : Windows 11 annonce un majeur de 10. L'application
/// tourne sur les DEUX avec un seul exécutable (plancher : Windows 10 22H2).
pub mod os_version;
pub mod registry_util;
/// L'éclairage nocturne de Windows — et il n'a aucune API publique (voir sa doc de
/// module : un blob CloudStore relu et réécrit octet par octet).
pub mod night_light;

/// Le gestionnaire de FILE D'IMPRESSION de Windows — une seule commande, mais vérifiée
/// dans la documentation plutôt que devinée (voir sa doc de module).
pub mod printer;
pub mod reglages_ecran;
pub mod proprietes_systeme;
pub mod fonctionnalites_facultatives;
pub mod centre_notifications;
pub mod everything;
pub mod cef_recherche;
pub mod cef_automatisations;
/// **LA MÉTÉO** (voir sa doc de module : Open-Meteo, fil dédié, et la ville
/// réglable depuis le panneau).
pub mod meteo;
pub mod meteo_carte;
/// La météo actuelle sous le calendrier du menu de l'horloge.
pub mod meteo_horloge;
pub mod horloges_monde;
pub mod conversion;
pub mod qr;
pub mod debit_reseau;
pub mod cef_meteo;
pub mod cef_aide;
pub mod screen_dock;
pub mod show_desktop;
/// **JOUER UN SON** — le premier du dépôt (voir sa doc de module : l'application
/// n'avait jamais émis un seul son avant le 2026-09-24).
pub mod son;
pub mod steam_apps;
pub mod svg_raster;
pub mod firefox_profiles;
pub mod sysinfo;
pub mod taskbar_autohide;
pub mod taille_panneaux;
pub mod taskbar_pins;
pub mod taskbar_windows;
pub mod conpty;
pub mod lecteur_video;
pub mod pty;
pub mod theme;
// SONDE, et **uniquement une sonde** : ce module ne contient AUCUN code de
// production (voir sa doc), donc il n'existe que dans les binaires de test — il
// n'alourdit pas l'application, et rien de ce qu'il fabrique ne peut se
// déclencher en usage normal.
/// ⭐ **LA CAPTURE D'UNE FENÊTRE SANS `Windows.Graphics.Capture`** — lue par
/// `PrintWindow`, elle rend le contenu d'une fenêtre HORS DE L'ÉCRAN et sous
/// alpha nul, sans session ni mémoire GPU à rendre (voir sa doc, mesurée).
pub mod capture_fenetre;
/// ⭐ **L'INJECTION AUTOMATIQUE DU MODULE `window-hook`** dans les applications
/// qui ouvrent une fenêtre animable (voir sa doc) — désactivée par défaut.
pub mod injection;
#[cfg(test)]
pub mod thumb_probe;
// SONDE, elle aussi — la seule qui touche à la fenêtre d'un AUTRE processus
// (région, place, alpha, visibilité), toujours restaurée par un `Drop`. Elle
// mesure si `PrintWindow` lit une fenêtre que PERSONNE NE VOIT, c'est-à-dire si
// un effet peut s'afficher sans que la fenêtre clignote d'abord (voir sa doc).
#[cfg(test)]
pub mod capture_probe;
// SONDE, elle aussi — même statut que `thumb_probe` ci-dessus : elle mesure ce
// que Windows expose de sa ZONE DE NOTIFICATION (dont l'énumération n'a AUCUNE
// API publique) avant qu'on décide par quelle voie l'afficher. Voir sa doc.
#[cfg(test)]
pub mod tray_probe;
// SONDE, elle aussi — et **la seule du dépôt qui ne soit PAS en lecture seule** :
// pour recevoir le flux `WM_COPYDATA` des icônes, sa fenêtre doit gagner le
// `FindWindow` du shell. Elle est isolée dans son propre module à dessein, et sa
// doc dit exactement ce qu'elle intercepte, ce qu'elle relaie et ce qu'elle
// s'interdit. Voir sa doc avant toute réutilisation.
#[cfg(test)]
pub mod tray_spy_probe;
pub mod tray_icons;
// L'ONGLET de l'interface masquée : la place de l'applet de masquage, gardée à
// l'écran pendant que tout le reste est caché. Voir sa doc.
pub mod onglet_masque;
// Le FLUX des icônes : une fenêtre imposteur `Shell_TrayWnd` qui RELAIE tout à la
// vraie barre et garde l'icône exacte de chaque application. Voir sa doc.
pub mod tray_flux;
// **LE FIL DE TRAVAIL DE LA ZONE DE NOTIFICATION** — l'énumération UI Automation
// coûte ~64 ms de médiane (mesuré ; la doc annonçait 42 ms) et tournait à chaque
// seconde sur le fil qui peint : c'est la cause des saccades, mesurée. Voir sa
// doc, qui porte aussi l'appartement COM et la
// raison pour laquelle ce déplacement est possible sans état partagé.
pub mod tray_worker;
pub mod tooltip;
pub mod uwp_apps;
pub mod voicemeeter;
pub mod wallpaper_color;
pub mod material_you;
pub mod wgc;
pub mod window;
pub mod window_previews;
pub mod windows_update;
// **LA MISE À JOUR DEPUIS LA DERNIÈRE RELEASE GITHUB** — vérification, archive,
// préparation et assistant de remplacement. Voir sa doc de module pour l'ordre
// complet et pour la raison du processus séparé.
pub mod maj;
