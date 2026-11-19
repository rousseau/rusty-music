# Tests de fumée de l'interface

L'interface réelle (`../ui/`) dans Chrome headless, avec un `window.__TAURI__`
simulé : aucune compilation Rust, aucun modèle, ~12 s.

```bash
npm ci                    # une fois (Playwright, épinglé)
npx playwright test       # ou ./scripts/verifier.sh fumee depuis la racine
SONDE=1 npx playwright test   # liste, par test, les commandes sans réponse figée
```

| Fichier | Rôle |
|---|---|
| `faux-tauri.js` | `invoke`, `event.listen`, `dialog`, `opener` simulés ; note chaque appel dans `window.__APPELS__` |
| `fixtures.js` | réponses figées, sur une bibliothèque fictive (deux artistes, trois albums) ; formes recopiées des types Rust |
| `types/` | les types des commandes contrôlées, exportés de Rust (`commandes.ts`), et leur confrontation à `fixtures.js` (`controle.ts`, `npm run types` ; `donnees.d.ts` type `window.__DONNEES__`) |
| `outils.js` | sert `../ui/` depuis le disque (`page.route`, pas de serveur), coupe le réseau, injecte faux Tauri et réponses |
| `fumee.spec.js` | démarrage, lecture d'un album, chaque mode, Pratiquer et Créer avec quatre stems factices, palette ⌘K, filtres qui estompent, thème |
| `captures.spec.js` | **pas un test** : captures d'écran des deux thèmes sur `collection-fictive.js` (artistes et albums inventés, pochettes engendrées), vers `docs/images/` ; seulement par `npm run captures` (`CAPTURES=1`) |
| `rails.spec.js`, `rails.py` | **pas un test** : la hauteur du rail de chaque écran, bloc par bloc (Règle 13, la mesure de G8), sur une bibliothèque fictive à douze familles, établi compris ; captures assemblées côte à côte par `rails.py` (`RAILS=avant npm run rails`, puis `python3 rails.py DOSSIER sortie.png avant apres`) |
| `pratiquer-hauteur.spec.js`, `pratique-fictive.js` | **pas un test** : la hauteur de chaque bloc du centre de Pratiquer, coach de basse démarré, sur le portable et à 1 400 × 900, avec captures (V43 ; `HAUTEUR=avant npx playwright test pratiquer-hauteur`, `HAUTEUR_DOSSIER`) ; `pratique-fictive.js` : ouvrir un morceau en Pratiquer, une basse transcrite, partagé avec `fumee.spec.js` |
| `ecritures.spec.js` | noms en arabe et en japonais sur une bibliothèque à part : `lang`, isolats bidi (Règle 11) |

Un test échoue sur **toute** erreur JavaScript : exception non rattrapée,
`console.error`, ou erreur que `ui/js/` remonte lui-même au moteur
(`js_error`). Une commande sans réponse figée rend `null` : l'interface doit
le supporter, et `SONDE=1` dit lesquelles manquent.

**Quand une commande Tauri change de forme**, mettre à jour `fixtures.js` :
ces tests ne voient que ce que les réponses figées leur montrent. Pour les
commandes **contrôlées**, c'est vérifié : le test Rust `types_commandes`
(`apps/desktop/src/types_commandes.rs`, par `tauri-specta`) écrit leurs
types dans `types/commandes.ts`, à commiter avec le changement ; `npm run
types` confronte `fixtures.js` à ces types : champ renommé ou de mauvais type,
réponse absente, et champ en trop (que Rust a retiré mais que `fixtures.js`
garde : l'erreur nomme son chemin, `"courbe.le_long[].rang"`). Une réponse
calculée qui rend l'état partagé (`e.balade`) est `any` et échappe au
contrôle : rendre plutôt le littéral (voir `balade_demarrer`). 189 commandes
contrôlées sur 263 `#[tauri::command]` (V52 ; avant V46, les décomptes
sous-comptaient : 67 au lieu de 44). Un générique aplati (`Passe<A>`,
`#[serde(flatten)]`) ne s'exporte qu'intégré en ligne
(`#[specta(inline)]` sur le conteneur) : sinon `A` reste non défini. Pour contrôler une commande de plus :
`#[cfg_attr(test, specta::specta)]` sous son `#[tauri::command]`,
`#[cfg_attr(test, derive(specta::Type))]` sur les types qu'elle reçoit et
renvoie (`#[cfg_attr(feature = "specta", …)]` dans `core`, `carto` et
`player`, dont le bureau active la feature `specta` en test), et son nom dans
`collect_commands!` ; une commande réexportée par son nom plutôt que par `*`
doit aussi réexporter `__specta__fn__<nom>` (voir `explorer/mod.rs`). Ils ne
vérifient ni le moteur ni la mise en page (pour celle-ci :
`scripts/audit-interface.sh`).
