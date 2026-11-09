# rusqlite_migration 2.6.0, exigence rusqlite relevée

Copie du paquet `rusqlite_migration` 2.6.0 de crates.io (Clément Joly et
contributeurs, Apache-2.0, `LICENSE.txt`), sha256 du `.crate` :
`03451d4dc523c0a96d9e412c574670b3f5f184c3988b77a435876be25477bbd3`.

Seule modification : dans `Cargo.toml`, `rusqlite` passe de `0.40.0` à
`0.41.0` (première version à embarquer SQLite 3.54, corrigé de
RUSTSEC-2026-0459), et les dépendances de développement sont retirées
(les tests du paquet ne sont pas copiés). C'est le changement de la PR
amont cljoly/rusqlite_migration#370, hors un doctest.

Branchée par `[patch.crates-io]` dans le `Cargo.toml` racine. À supprimer
dès qu'une version > 2.6.0 est publiée.
