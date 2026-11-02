// SPDX-License-Identifier: GPL-3.0-or-later
//! Le schéma de la base : migrations versionnées, fonctions SQL.

use super::*;

/// Enregistre `titre_normalise(t)` côté SQLite, pour que les jointures vers
/// `mb_release_groups.title_norm` appliquent exactement la même normalisation
/// que celle qui a rempli cette colonne ([`crate::musicbrainz::normaliser_titre`]).
/// Sans ça, `title_norm` retire les espaces (« temple of the dog » →
/// « templeofthedog ») mais une jointure en `lower(trim(...))` les garde : la
/// jointure ne matche jamais pour un titre à plusieurs mots, et l'année
/// bascule silencieusement sur celle du tag.
fn enregistrer_fonctions(conn: &Connection) -> rusqlite::Result<()> {
    conn.create_scalar_function(
        "titre_normalise",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let titre: String = ctx.get::<Option<String>>(0)?.unwrap_or_default();
            Ok(crate::musicbrainz::normaliser_titre(&titre))
        },
    )
}

/// Les migrations de la base, dans l'ordre ; `PRAGMA user_version` dit
/// combien sont déjà jouées (`rusqlite_migration`). Chacune est jouée une
/// seule fois, dans une transaction.
///
/// La n°1 reconnaît toutes les bases d'avant les migrations versionnées
/// (`user_version` à 0, quelle que soit leur génération) : le schéma, tout en
/// `IF NOT EXISTS`, puis [`migrate`], qui ajoute ce qui manque en comparant à
/// `pragma_table_info`. Ne plus la modifier : un changement de schéma
/// s'ajoute à la fin, en SQL simple.
///
/// Une base ouverte par une version plus ancienne que la dernière migration
/// qu'elle a reçue est refusée (`DatabaseTooFarAhead`), plutôt que lue avec
/// un schéma qu'on ne connaît pas.
pub(super) fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up_with_hook(include_str!("../../sql/schema.sql"), |tx: &Transaction| {
            Ok(migrate(tx)?)
        }),
        // N° 2 (1ᵉʳ nov. 2026) : l'édition MusicBrainz retenue à la main pour
        // un album, et ses identifiants piste à piste — réappliqués par
        // `upsert` à chaque scan, pour qu'un rescan ne les efface pas
        // (`Library::retenir_edition`, `docs/plan-ecouter-v0.3.md`, point 3).
        M::up(include_str!("../../sql/migration-002-editions.sql")),
        // N° 3 (2 nov. 2026) : le journal des passes « Compléter », pour les
        // annuler en bloc (`Library::ouvrir_passe`, `db/passes.rs`).
        M::up(include_str!("../../sql/migration-003-passes.sql")),
    ])
}

/// Ouvre une connexion : réglages, puis migrations en attente.
///
/// Les migrations prennent le verrou d'écriture dès leur `BEGIN`
/// (`IMMEDIATE`) : l'application, la CLI et les passes de fond ouvrent la
/// même base, et deux migrations lancées ensemble en transaction différée
/// se heurteraient à `SQLITE_BUSY` sans attendre `busy_timeout`. La seconde
/// attend donc, puis rejoue ce qu'elle croyait en retard (elle a lu
/// `user_version` avant le verrou) : sans effet pour la n°1, rejouable ; une
/// migration suivante qui ne l'est pas échouerait proprement, et la
/// réouverture suivante passerait.
pub(super) fn preparer(mut conn: Connection) -> Result<Connection> {
    conn.execute_batch(include_str!("../../sql/connexion.sql"))?;
    conn.set_transaction_behavior(TransactionBehavior::Immediate);
    let issue = migrations().to_latest(&mut conn);
    conn.set_transaction_behavior(TransactionBehavior::Deferred);
    issue?;
    enregistrer_fonctions(&conn)?;
    Ok(conn)
}

/// Met à niveau une base créée avant les migrations versionnées — corps de
/// la migration n°1, après le schéma.
///
/// `CREATE TABLE IF NOT EXISTS` ne touche pas à une table existante : sans
/// cela, une base déjà peuplée n'obtiendrait jamais les colonnes ajoutées
/// depuis. Tout est conditionnel (`pragma_table_info`, ligne sentinelle),
/// donc rejouable sans effet sur une base déjà à jour.
pub(super) fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('tracks')")?;
    let existantes: std::collections::HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;

    for (nom, decl) in [
        ("mb_artist_id", "TEXT"),
        ("mb_album_artist_id", "TEXT"),
        // Débit en kb/s et format, lus par `tags::read` en même temps que
        // `sample_rate`/`channels` — aucun décodage, juste les propriétés du
        // fichier. Absents des morceaux scannés avant cette version : un
        // rescan (« Scanner », case « relire même les fichiers inchangés »)
        // les remplit sans autre passe.
        ("bitrate", "INTEGER"),
        ("codec", "TEXT"),
        // Profondeur de bits des formats sans perte (« 16 bit »), lue par
        // `tags::read` comme `bitrate`/`codec`. `NULL` tant qu'un rescan
        // « relire même les fichiers inchangés » ne l'a pas remplie.
        ("bit_depth", "INTEGER"),
        // MBID d'édition (release, pas release-group) — lu par `tags::read`
        // via `ItemKey::MusicBrainzReleaseId`. Sert à retrouver la relation
        // d'URL Discogs de cette édition précise (`docs/enrichissement-lecteur.md`).
        ("mb_release_id", "TEXT"),
    ] {
        if !existantes.contains(nom) {
            conn.execute_batch(&format!("ALTER TABLE tracks ADD COLUMN {nom} {decl}"))?;
        }
    }

    // Les dates d'œuvre, pour le placement chronologique du peuplement.
    // `first_release_date` est **déjà** dans la réponse MusicBrainz que
    // `mb_poser_albums` reçoit : c'est la date de l'œuvre et non celle du
    // pressage, donc ce qui corrige les rééditions. `secondary_types` porte
    // « Compilation », qui signale une date à ne pas croire.
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('mb_release_groups')")?;
    let colonnes: std::collections::HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;
    drop(stmt);
    for (nom, decl) in [("first_release_date", "TEXT"), ("secondary_types", "TEXT")] {
        if !colonnes.contains(nom) {
            conn.execute_batch(&format!(
                "ALTER TABLE mb_release_groups ADD COLUMN {nom} {decl}"
            ))?;
        }
    }

    // Les deux colonnes ci-dessus ont longtemps existé sans jamais être
    // écrites : `mb_poser_albums` ignorait `first_release_date` et
    // `secondary_types` avant ce correctif. Toute base déjà enrichie porte
    // donc des artistes marqués « déjà faits » dans `mb_fetched` sans
    // aucune date sur leurs release-groups — et `mb_artistes_en_attente` ne
    // revisite jamais un artiste déjà marqué. Sans cette purge, une
    // bibliothèque déjà enrichie resterait pour toujours sans dates,
    // correctif ou pas. La ligne sentinelle (même idiome que
    // `decouvrir_marquer_passe`) garantit que ça ne se fait qu'une fois :
    // sans elle, un artiste dont MusicBrainz ne connaît vraiment aucune
    // date serait purgé à chaque démarrage et sa repasse rejouée sans fin.
    let deja_purge: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM mb_fetched WHERE mbid = '@migration' AND kind = 'purge_dates_2026_09')",
        [],
        |r| r.get(0),
    )?;
    if !deja_purge {
        conn.execute_batch(
            "DELETE FROM mb_fetched WHERE kind IN ('artist', 'albums');
             INSERT INTO mb_fetched (mbid, kind) VALUES ('@migration', 'purge_dates_2026_09');",
        )?;
    }

    // Créé ici, et non dans le schéma : la colonne visée peut venir d'être
    // ajoutée juste au-dessus.
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_tracks_mb_aa ON tracks(mb_album_artist_id)",
    )?;

    // `artist_links` existait déjà, vide, avant le mode Découvrir — sans
    // cette migration, une base créée par une version antérieure n'aurait
    // jamais `dst_name` : `CREATE TABLE IF NOT EXISTS` du schéma ne touche
    // pas une table déjà là.
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('artist_links')")?;
    let colonnes: std::collections::HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;
    if !colonnes.contains("dst_name") {
        conn.execute_batch("ALTER TABLE artist_links ADD COLUMN dst_name TEXT")?;
    }

    // Index de recherche. `remove_diacritics 2` est ce qui fait que « bjork »
    // trouve « Björk » — `LIKE` en est incapable sans ICU. `content='tracks'`
    // évite de dupliquer les textes : l'index ne stocke que ses termes.
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS tracks_fts USING fts5(
             title, artist, album,
             content='tracks', content_rowid='id',
             tokenize=\"unicode61 remove_diacritics 2\"
         );

         -- Table externe : c'est à nous de tenir l'index à jour.
         CREATE TRIGGER IF NOT EXISTS tracks_fts_ai AFTER INSERT ON tracks BEGIN
           INSERT INTO tracks_fts(rowid, title, artist, album)
           VALUES (new.id, new.title, new.artist, new.album);
         END;
         CREATE TRIGGER IF NOT EXISTS tracks_fts_ad AFTER DELETE ON tracks BEGIN
           INSERT INTO tracks_fts(tracks_fts, rowid, title, artist, album)
           VALUES ('delete', old.id, old.title, old.artist, old.album);
         END;
         CREATE TRIGGER IF NOT EXISTS tracks_fts_au AFTER UPDATE ON tracks BEGIN
           INSERT INTO tracks_fts(tracks_fts, rowid, title, artist, album)
           VALUES ('delete', old.id, old.title, old.artist, old.album);
           INSERT INTO tracks_fts(rowid, title, artist, album)
           VALUES (new.id, new.title, new.artist, new.album);
         END;",
    )?;

    // Base déjà peuplée dont l'index vient d'apparaître : on le remplit. Les
    // déclencheurs suffisent ensuite.
    //
    // On compte dans `tracks_fts_docsize`, pas dans `tracks_fts` : sur une
    // table à contenu externe, `COUNT(*)` est délégué à la table de contenu et
    // renvoie donc le nombre de morceaux même quand l'index est vide. La
    // condition serait toujours fausse et la recherche resterait muette.
    let a_reconstruire: bool = conn.query_row(
        "SELECT (SELECT COUNT(*) FROM tracks) > 0
            AND (SELECT COUNT(*) FROM tracks_fts_docsize) = 0",
        [],
        |r| r.get(0),
    )?;
    if a_reconstruire {
        conn.execute_batch("INSERT INTO tracks_fts(tracks_fts) VALUES('rebuild')")?;
    }

    // `0` (« 0000 ») est un espace réservé de tagueur pour « année absente »,
    // jamais une vraie date — `tags::read` ne l'écrit plus depuis cette
    // version, mais une base déjà scannée en porte encore. Sans ce nettoyage,
    // elle continuerait de fausser les bornes de tout ce qui classe ou
    // colore par année, jusqu'à un rescan complet des fichiers concernés.
    conn.execute_batch("UPDATE tracks SET year = NULL WHERE year = 0")?;

    // Descripteurs timbraux (ZCR, centroïde/rolloff/aplatissement spectraux) —
    // ajoutés après la table `descriptors` d'origine, `CREATE TABLE IF NOT
    // EXISTS` ne les apporte donc pas à une base déjà peuplée.
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('descriptors')")?;
    let colonnes: std::collections::HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;
    for nom in [
        "zcr",
        "centroid_mean",
        "centroid_std",
        "rolloff_mean",
        "rolloff_std",
        "flatness_mean",
        "flatness_std",
    ] {
        if !colonnes.contains(nom) {
            conn.execute_batch(&format!("ALTER TABLE descriptors ADD COLUMN {nom} REAL"))?;
        }
    }

    // Version de l'algorithme tempo/tonalité/énergie ayant produit chaque
    // ligne — voir `rusty_music_analysis::passe::VERSION_DESCRIPTEURS`. `0`
    // pour toute ligne écrite avant cette colonne : plus bas qu'aucune
    // version réelle, donc `pending_descripteurs` les considère à remesurer,
    // comme il se doit pour des mesures dont on ne sait pas si elles datent
    // d'avant ou d'après un correctif (l'inversion d'octave du tempo, par
    // exemple). Remplace le bouton « refaire ce qui est déjà mesuré » comme
    // seul moyen de rattraper un correctif d'algorithme : celui-ci reste,
    // pour une repasse volontaire, mais n'est plus le seul recours.
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('descriptors')")?;
    let colonnes: std::collections::HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;
    if !colonnes.contains("algo_version") {
        conn.execute_batch(
            "ALTER TABLE descriptors ADD COLUMN algo_version INTEGER NOT NULL DEFAULT 0",
        )?;
    }

    // Le genre gagnant du vote CLAP-texte, par morceau — calculé en lot par
    // `crates/analysis` (le calibrage a besoin du score de tous les morceaux
    // contre tout le vocabulaire avant de prendre l'argmax) et persisté ici
    // au même endroit que `cluster`, pour ne pas refaire le produit matriciel
    // à chaque lecture des familles. Voir `docs/nommage-familles.md`.
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('features')")?;
    let colonnes: std::collections::HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;
    if !colonnes.contains("texte_label") {
        conn.execute_batch("ALTER TABLE features ADD COLUMN texte_label TEXT")?;
    }

    Ok(())
}
