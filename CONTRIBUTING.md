# Contribuer à NeuraDesk

Merci de votre intérêt ! Le projet est écrit et documenté en **français** ;
les contributions en anglais sont bienvenues aussi.

## Signaler un bogue

Ouvrez une *Issue* avec :

- la version (menu de l'icône NeuraDesk → **À propos de NeuraDesk** : version,
  révision et date) ;
- votre Windows (11, ou 10 22H2) et vos écrans (nombre, définition, échelle) ;
- ce que vous avez fait, ce que vous attendiez, ce qui s'est passé ;
- si possible le journal `neuradesk-diag.log`, à côté de `neuradesk.exe`.

## Proposer une modification

1. Créez une branche à partir de `main`.
2. Gardez les changements ciblés : une fonctionnalité ou un correctif par
   *pull request*.
3. Ajoutez des tests pour la logique nouvelle, et vérifiez que la suite passe :

   ```powershell
   cargo test --release --bin neuradesk
   ```

4. Messages de commit au format *Conventional Commits* :
   `feat(notes): …`, `fix(dock): …`, `docs: …`.

### Quelques conventions du code

- Les panneaux sont des pages de `assets/popups/` ; chaque page parle à Rust
  par `go("verbe", …)` et un module `src/win32/cef_*.rs` traite ces verbes.
  Un verbe ajouté d'un côté doit l'être de l'autre : des tests de parité le
  vérifient.
- Pas de `<select>` dans les pages CEF (rendu hors-écran) : utiliser les menus
  de `menu_contextuel.js`.
- NeuraDesk ne doit jamais simuler d'entrée clavier ou souris dans la session
  de l'utilisateur.

## Licence des contributions

En soumettant une contribution, vous acceptez qu'elle soit distribuée sous la
licence du projet, **GPL-3.0-or-later** (voir [`LICENSE`](LICENSE)). Les
fichiers tiers que vous ajoutez (icônes, polices, sons) doivent avoir une
licence compatible, mentionnée dans [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md).
