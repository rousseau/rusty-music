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
| `outils.js` | sert `../ui/` depuis le disque (`page.route`, pas de serveur), coupe le réseau, injecte faux Tauri et réponses |
| `fumee.spec.js` | démarrage, lecture d'un album, chaque mode, Pratiquer et Créer avec quatre stems factices, palette ⌘K, filtres qui estompent, thème |
| `captures.spec.js` | **pas un test** : captures d'écran des deux thèmes sur `collection-fictive.js` (artistes et albums inventés, pochettes engendrées), vers `docs/images/` ; seulement par `npm run captures` (`CAPTURES=1`) |
| `rails.spec.js`, `rails.py` | **pas un test** : la hauteur du rail de chaque écran, bloc par bloc (Règle 13, la mesure de G8), sur une bibliothèque fictive à douze familles, établi compris ; captures assemblées côte à côte par `rails.py` (`RAILS=avant npm run rails`, puis `python3 rails.py DOSSIER sortie.png avant apres`) |
| `ecritures.spec.js` | noms en arabe et en japonais sur une bibliothèque à part : `lang`, isolats bidi (Règle 11) |

Un test échoue sur **toute** erreur JavaScript : exception non rattrapée,
`console.error`, ou erreur que `ui/js/` remonte lui-même au moteur
(`js_error`). Une commande sans réponse figée rend `null` : l'interface doit
le supporter, et `SONDE=1` dit lesquelles manquent.

**Quand une commande Tauri change de forme**, mettre à jour `fixtures.js` :
ces tests ne voient que ce que les réponses figées leur montrent. Ils ne
vérifient ni le moteur ni la mise en page (pour celle-ci :
`scripts/audit-interface.sh`).
