# Applet SSH / SFTP — plan

Demande : « une applet de gestion de serveurs par ssh et sftp avec un système
d'onglets quand plusieurs connexions sont ouvertes, sers-toi de ce dépôt comme
modèle : https://github.com/SinaXhpm/Submarine ».

## Ce qu'on reprend de Submarine

Submarine = Tauri 2 : cœur Rust (`russh` + `russh-sftp` sur `tokio`), interface
React + **xterm.js**. Un onglet par serveur, qui porte un terminal et un
navigateur SFTP. On reprend :

| Submarine | NeuraDesk |
|---|---|
| liste de serveurs enregistrés (dossier, couleur, hôte, port, identifiant) | liste à gauche du panneau, `ssh.toml` |
| onglets de session (`ScrollableTabs`, `SessionView`) | barre d'onglets en haut du panneau, une connexion par onglet |
| terminal xterm.js, copier à la sélection, coller au clic droit | xterm.js 6 embarqué (`assets/popups/vendor/`, MIT) |
| SFTP : liste, filtre, multi-sélection, téléverser/télécharger, renommer, supprimer, chmod | vue « Fichiers » de l'onglet |
| clés d'hôte TOFU | empreinte montrée et confirmée à la 1ʳᵉ connexion |
| coffre chiffré Argon2id + AES-GCM, synchro cloud | **DPAPI** (`CryptProtectData`, liée à la session Windows) — pas de mot de passe maître, pas de cloud |

**Hors périmètre** (reprenables plus tard sur demande) : redirection de ports,
miroir de dossiers, Docker, surveillance, synchro de profils, édition en direct.

## Décisions techniques

- **Crates** : `russh` 0.63 + `russh-sftp` 3.0 + `tokio` (runtime multi-fil sur
  son propre fil, jamais sur le fil UI). `russh` avec
  `default-features = false, features = ["ring", "flate2", "rsa"]` : la valeur
  par défaut `aws-lc-rs` exige NASM, absent de cette machine.
- **Page → Rust (frappes, envois de fichiers)** : le canal POST
  `neuradesk-data.invalid` (`win32::cef_data`). Les routes `ssh/…` ne vont PAS
  dans la boîte « un corps par route » (elle écraserait des frappes) : elles
  partent dans une file vers le runtime SSH. La page n'a **qu'un envoi en vol à
  la fois** (sinon l'ordre des frappes n'est pas garanti) et regroupe ce qui
  arrive entre-temps.
- **Page → Rust (commandes)** : `neuradesk://ssh/<verbe>/…` comme les autres
  panneaux (arguments encodés, décodés par bras).
- **Rust → page** : `dispatch_actions` (fil UI, chaque frame) vide une file
  d'évènements et appelle `execute_js` — sortie du terminal en base64 par
  onglet, états de connexion, listes SFTP.
- **Clés d'hôte** : fichier propre `%APPDATA%\NeuraDesk\ssh_hotes_connus`, et
  `~/.ssh/known_hosts` lu en lecture seule. Clé inconnue → la page demande
  confirmation ; clé CHANGÉE → refus, message explicite.
- **Authentification** : mot de passe, clé privée (fichier, phrase de passe
  facultative), agent Pageant/OpenSSH plus tard. Secrets chiffrés DPAPI, jamais
  renvoyés à la page (`motDePasseDefini: bool`), jamais journalisés.
- **Tests sans serveur réel** : un serveur `russh` EN PROCESSUS sur
  `127.0.0.1:0` (shell écho + sous-système SFTP de `russh-sftp`). Aucun
  essai contre les serveurs de l'utilisateur.

## Lots

0. **Socle** : dépendances, runtime, client connexion/authentification/shell/SFTP
   prouvé contre le serveur en processus (tests).
1. **Panneau + onglets + terminal** : applet dans Intégrés, liste de serveurs
   (ajouter / modifier / supprimer), connexion, onglets, xterm.js,
   redimensionnement du pty, TOFU, reconnexion.
2. **SFTP** : vue Fichiers par onglet — naviguer, filtrer, télécharger vers un
   dossier local, téléverser (dialogue de fichiers), créer un dossier,
   renommer, supprimer, droits.
   - ⛔ Rien n'écrase : un téléchargement prend « nom (1).ext » si le nom local
     est pris ; un téléversement est REFUSÉ si le fichier distant existe
     (`EXCLUDE`, atomique).
   - ⛔ Un nom venu du serveur est assaini avant d'écrire sur le disque local
     (`ssh::fichiers::nom_local` : `\`, `:`, `..`, `CON`…).
   - La suppression est récursive et ne SUIT jamais un lien (`lstat`) ; un
     téléchargement récursif ignore les liens internes (boucles possibles).
   - **Deux volets** (demande du 2026-09-26) : « ce PC » à gauche
     (`ssh::local`, listé par Rust hors du fil UI — `*` = Ce PC : dossiers
     usuels + lecteurs, `~` = dossier personnel ; fichiers cachés/système
     masqués), le serveur à droite, une séparation qu'on glisse et qui porte
     les transferts `→` / `←` vers le dossier ouvert EN FACE. Les dialogues de
     fichiers ont disparu. Un téléversement de dossier est récursif, refusé si
     le dossier distant existe.
3. **Confort** (sur demande) : commandes rapides par serveur, import de
   `~/.ssh/config`, agent SSH, redirection de ports.

Chaque lot est essayé par l'utilisateur avant le suivant : la capture prouve
qu'une page s'affiche, jamais qu'elle réagit.
