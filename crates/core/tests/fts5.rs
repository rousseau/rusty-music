//! La recherche repose sur FTS5 et sur son repli des diacritiques. Ni l'un ni
//! l'autre n'est garanti par défaut : ils dépendent des options de compilation
//! du SQLite embarqué par `rusqlite`. Ce test nomme cette dépendance, pour que
//! sa disparition se signale ici plutôt que par une recherche silencieusement
//! dégradée.

#[test]
fn le_sqlite_embarque_fournit_fts5_sans_accents() {
    let c = rusqlite::Connection::open_in_memory().unwrap();
    c.execute_batch(
        "CREATE VIRTUAL TABLE t USING fts5(x, tokenize=\"unicode61 remove_diacritics 2\");
         INSERT INTO t(x) VALUES ('Björk chante Kanañ a ri');",
    )
    .expect("fts5 indisponible dans le SQLite embarqué");

    for requete in ["bjork", "björk", "kanan", "KANAÑ"] {
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM t WHERE t MATCH ?1", [requete], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(n, 1, "« {requete} » aurait dû trouver la ligne");
    }
}
