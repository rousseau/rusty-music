// SPDX-License-Identifier: GPL-3.0-or-later
//! Les sources externes : MusicBrainz, popularité, biographies, Discogs, critiques, tags Last.fm.

use super::*;

/// Les artistes MusicBrainz des morceaux d'un artiste lié, et les paires
/// d'artistes liés ([`Library::liens_entre_artistes`]).
pub type LiensArtistes = (HashMap<i64, Vec<String>>, HashSet<(String, String)>);

impl Library {
    /* ------------------------------------------------ genres MusicBrainz */

    /// Les artistes qu'il reste à interroger pour un échelon donné.
    ///
    /// `echelon` vaut `"artist"` (les genres de l'artiste) ou `"albums"` (le
    /// parcours de ses disques). La trace est gardée **même quand la réponse
    /// est vide** : sans elle, les artistes sans genre — un quart d'entre eux —
    /// seraient réinterrogés à chaque passe, à une requête par seconde.
    ///
    /// Les plus représentés d'abord : la couverture en morceaux monte alors
    /// bien plus vite que la couverture en artistes, et une passe interrompue
    /// laisse déjà un résultat exploitable.
    pub fn mb_artistes_en_attente(&self, echelon: &str, limite: usize) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.mb_artist_id, COUNT(*) n FROM tracks t
              WHERE t.mb_artist_id IS NOT NULL AND t.mb_artist_id <> ''
                AND NOT EXISTS (SELECT 1 FROM mb_fetched f
                                 WHERE f.mbid = t.mb_artist_id AND f.kind = ?1)
              GROUP BY t.mb_artist_id ORDER BY n DESC LIMIT ?2",
        )?;
        let brut: Vec<String> = stmt
            .query_map(params![echelon, limite as i64], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(brut)
    }

    /// Les genres d'un artiste, et la trace du passage.
    ///
    /// Les deux dans une transaction : une passe interrompue entre l'écriture
    /// et la marque réinterrogerait l'artiste, celle-ci interrompue dans
    /// l'autre ordre le tiendrait pour fait sans l'avoir enregistré.
    pub fn mb_poser_genres(
        &mut self,
        mbid: &str,
        echelon: &str,
        genres: &[(String, i64)],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        for (nom, votes) in genres {
            tx.execute(
                "INSERT INTO mb_genres (mbid, kind, genre, votes) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(mbid, kind, genre) DO UPDATE SET votes = excluded.votes",
                params![mbid, echelon, nom, votes],
            )?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO mb_fetched (mbid, kind) VALUES (?1, ?2)",
            params![
                mbid,
                if echelon == "artist" {
                    "artist"
                } else {
                    echelon
                }
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Les albums d'un artiste et leurs genres, en une transaction.
    ///
    /// `albums` porte, pour chaque disque, son identifiant, son titre, le titre
    /// normalisé qui servira au rapprochement, ses genres, et la date de
    /// parution du release-group avec ses types secondaires — sans ces deux
    /// derniers, [`Library::ordre_darrivee`] et [`Library::albums`] n'ont
    /// jamais rien à corriger et retombent silencieusement sur le seul tag.
    pub fn mb_poser_albums(&mut self, artiste: &str, albums: &[AlbumRange]) -> Result<()> {
        let tx = self.conn.transaction()?;
        for (mbid, titre, norme, genres, date_sortie, types_secondaires) in albums {
            tx.execute(
                "INSERT INTO mb_release_groups
                   (mbid, artist_mbid, title, title_norm, first_release_date, secondary_types)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(mbid) DO UPDATE SET
                   artist_mbid = excluded.artist_mbid,
                   title = excluded.title,
                   title_norm = excluded.title_norm,
                   first_release_date = excluded.first_release_date,
                   secondary_types = excluded.secondary_types",
                params![mbid, artiste, titre, norme, date_sortie, types_secondaires],
            )?;
            for (nom, votes) in genres {
                tx.execute(
                    "INSERT INTO mb_genres (mbid, kind, genre, votes) VALUES (?1, 'release-group', ?2, ?3)
                     ON CONFLICT(mbid, kind, genre) DO UPDATE SET votes = excluded.votes",
                    params![mbid, nom, votes],
                )?;
            }
        }
        tx.execute(
            "INSERT OR REPLACE INTO mb_fetched (mbid, kind) VALUES (?1, 'albums')",
            params![artiste],
        )?;
        tx.commit()?;
        Ok(())
    }

    /* -------------------------------------- collaborations (mode Découvrir) */

    /// A-t-on déjà demandé les relations de cet artiste — même si la
    /// réponse était une liste vide. Sans cette trace, un artiste sans
    /// collaboration connue serait réinterrogé à chaque visite du mode
    /// Découvrir, une requête par seconde.
    pub fn liens_artiste_en_cache(&self, mbid: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM mb_fetched WHERE mbid = ?1 AND kind = 'relations')",
            params![mbid],
            |r| r.get(0),
        )?)
    }

    /// Les relations d'un artiste et la trace du passage, dans une
    /// transaction — même raison que [`Self::mb_poser_genres`] : une passe
    /// interrompue entre les deux écritures ne doit ni perdre le résultat,
    /// ni le tenir pour acquis sans l'avoir noté.
    pub fn enregistrer_liens_artiste(
        &mut self,
        mbid: &str,
        liens: &[crate::musicbrainz::Relation],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        for lien in liens {
            tx.execute(
                "INSERT INTO artist_links (src_mbid, dst_mbid, dst_name, relation)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(src_mbid, dst_mbid, relation) DO UPDATE SET dst_name = excluded.dst_name",
                params![mbid, lien.dst_mbid, lien.dst_name, lien.relation],
            )?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO mb_fetched (mbid, kind) VALUES (?1, 'relations')",
            params![mbid],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Les liens déjà connus d'un artiste — toujours depuis le cache,
    /// jamais le réseau ; c'est [`Self::liens_artiste_en_cache`] qui décide
    /// s'il faut d'abord interroger MusicBrainz.
    pub fn liens_artiste(&self, mbid: &str) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT dst_mbid, COALESCE(dst_name, '?'), relation FROM artist_links
              WHERE src_mbid = ?1 ORDER BY relation, dst_name",
        )?;
        let rows = stmt
            .query_map(params![mbid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Les artistes de la bibliothèque liés entre eux par une relation
    /// MusicBrainz (membre, collaboration, fondateur…), pour le bonus de
    /// relation de la balade (`docs/plan-balade.md`, P3) : les identifiants
    /// MusicBrainz des artistes de chaque morceau concerné, et les paires
    /// liées, dans les deux sens. Un hommage ou un nom emprunté (« tribute »,
    /// « named after ») n'est pas un lien de musique. Seuls les morceaux d'un
    /// artiste lié figurent dans la table.
    pub fn liens_entre_artistes(&self) -> Result<LiensArtistes> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT src_mbid, dst_mbid FROM artist_links
              WHERE relation NOT LIKE 'tribute%' AND relation NOT LIKE 'named after%'
                AND src_mbid <> dst_mbid",
        )?;
        let mut paires: HashSet<(String, String)> = HashSet::new();
        for paire in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (a, b) = paire?;
            paires.insert((b.clone(), a.clone()));
            paires.insert((a, b));
        }
        let lies: HashSet<&str> = paires.iter().map(|(a, _)| a.as_str()).collect();
        let mut stmt = self
            .conn
            .prepare("SELECT id, mb_artist_id FROM tracks WHERE mb_artist_id IS NOT NULL")?;
        let mut artistes: HashMap<i64, Vec<String>> = HashMap::new();
        for ligne in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (id, mbids) = ligne?;
            // Plusieurs artistes : « a/b » (tags de Picard) ou « a; b ».
            let v: Vec<String> = mbids
                .split(['/', ';'])
                .map(str::trim)
                .filter(|m| lies.contains(m))
                .map(str::to_string)
                .collect();
            if !v.is_empty() {
                artistes.insert(id, v);
            }
        }
        Ok((artistes, paires))
    }

    /// Genres par identifiant, du plus sûr au moins sûr.
    ///
    /// **Le classement compte plus que le seuil**, et l'avoir cru l'inverse a
    /// coûté un bogue. Chez Yann Tiersen, dix genres portent une seule voix —
    /// `modern classical`, `minimalism`, `instrumental`, tous justes, et
    /// `amapiano`, qui ne l'est pas. Départager par ordre alphabétique mettait
    /// `amapiano` en tête et nommait ainsi la famille de piano néoclassique.
    ///
    /// À votes égaux, on départage donc par **le nombre d'artistes de la
    /// bibliothèque qui portent ce genre** : un genre que personne d'autre ne
    /// porte est un accident de contributeur, un genre partagé par dix-sept
    /// artistes est une catégorie. `amapiano` tombe alors dernier, et
    /// `instrumental` remonte.
    ///
    /// La popularité se mesure toujours sur l'échelon artiste, même quand on
    /// interroge les albums : c'est le plus large échantillon dont on dispose.
    pub fn mb_genres(&self, echelon: &str, plancher: i64) -> Result<HashMap<String, Vec<String>>> {
        let mut stmt = self.conn.prepare(
            "WITH portee AS (
                 SELECT genre, COUNT(DISTINCT mbid) n FROM mb_genres
                  WHERE kind = 'artist' GROUP BY genre)
             SELECT g.mbid, g.genre FROM mb_genres g
               LEFT JOIN portee p ON p.genre = g.genre
              WHERE g.kind = ?1 AND g.votes >= ?2
              ORDER BY g.mbid, g.votes DESC, COALESCE(p.n, 0) DESC, g.genre",
        )?;
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        let lignes = stmt.query_map(params![echelon, plancher], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for l in lignes {
            let (mbid, genre) = l?;
            out.entry(mbid).or_default().push(genre);
        }
        Ok(out)
    }

    /// Les albums connus, indexés par `(artiste, titre normalisé)`.
    ///
    /// Nos fichiers ne portent pas d'identifiant d'album : c'est ce couple qui
    /// fait le lien avec un release-group.
    pub fn mb_albums(&self) -> Result<HashMap<(String, String), String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT artist_mbid, title_norm, mbid FROM mb_release_groups")?;
        let lignes = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut out = HashMap::new();
        for l in lignes {
            let (artiste, norme, mbid) = l?;
            // Un même titre normalisé peut désigner deux release-groups (album
            // et sa compilation homonyme) : le premier suffit, ils portent des
            // genres voisins.
            out.entry((artiste, norme)).or_insert(mbid);
        }
        Ok(out)
    }

    /// Les release-groups MusicBrainz dont un album de la bibliothèque relève
    /// réellement — par opposition à `mb_release_groups`, qui porte tout le
    /// catalogue d'un artiste (nécessaire à [`Self::mb_albums`] pour
    /// désambiguïser un titre parmi toute la discographie, mais bien plus
    /// large que ce qu'on possède : sur une grosse bibliothèque, la table
    /// entière peut compter plusieurs dizaines de milliers de release-groups
    /// pour quelques milliers d'albums réels). Sert à ne pas interroger une
    /// source par release-group — CritiqueBrainz en premier lieu — pour des
    /// albums qu'on n'a jamais et n'affichera jamais.
    pub fn release_groups_possedes(&self) -> Result<HashSet<String>> {
        let albums = self.mb_albums()?;
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT mb_artist_id, album FROM tracks
              WHERE mb_artist_id IS NOT NULL AND album IS NOT NULL",
        )?;
        let paires = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(paires
            .into_iter()
            .filter_map(|(artiste, album)| {
                let norme = crate::musicbrainz::normaliser_titre(&album);
                albums.get(&(artiste, norme)).cloned()
            })
            .collect())
    }

    /* --------------------------------------------- popularité générale */

    /// Les enregistrements dont la popularité reste à récupérer sur au moins
    /// une source (ListenBrainz ou Deezer). `depuis` est l'instant à partir
    /// duquel un enregistrement déjà interrogé compte comme « frais » :
    /// `depuis = 0` ne rafraîchit rien, `now − 90 j` réinterroge le périmé.
    /// Les plus représentés d'abord, comme [`Self::mb_artistes_en_attente`].
    pub fn pop_recordings_candidats(
        &self,
        depuis: i64,
        limite: usize,
    ) -> Result<Vec<PisteAPopulariser>> {
        let limite = if limite == usize::MAX {
            i64::MAX
        } else {
            limite as i64
        };
        let mut stmt = self.conn.prepare(
            "SELECT t.mb_recording_id, MIN(t.artist), MIN(t.title), COUNT(*) n
               FROM tracks t
              WHERE t.mb_recording_id IS NOT NULL AND t.mb_recording_id <> ''
                AND (SELECT COUNT(*) FROM popularite_fetched f
                      WHERE f.mbid = t.mb_recording_id AND f.kind = 'recording'
                        AND f.source IN ('listenbrainz', 'deezer') AND f.at >= ?1) < 2
              GROUP BY t.mb_recording_id
              ORDER BY n DESC
              LIMIT ?2",
        )?;
        let out = stmt
            .query_map(params![depuis, limite], |r| {
                Ok(PisteAPopulariser {
                    recording_mbid: r.get(0)?,
                    artiste: r.get(1)?,
                    titre: r.get(2)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(out)
    }

    /// Les release-groups dont la popularité ListenBrainz reste à récupérer.
    /// Le lien morceau → release-group se fait en Rust, par
    /// `(mb_artist_id, titre normalisé)` — exactement comme [`genres_du_morceau`].
    pub fn pop_rg_candidats(&self, depuis: i64, limite: usize) -> Result<Vec<String>> {
        let albums = self.mb_albums()?;
        let deja = self.pop_deja_fait("listenbrainz", "release-group", depuis)?;

        let mut stmt = self.conn.prepare(
            "SELECT mb_artist_id, album, COUNT(*) n FROM tracks
              WHERE mb_artist_id IS NOT NULL AND mb_artist_id <> '' AND album IS NOT NULL
              GROUP BY mb_artist_id, album
              ORDER BY n DESC",
        )?;
        let lignes: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;

        let mut vus = HashSet::new();
        let mut out = Vec::new();
        for (artiste, album) in lignes {
            let cle = (artiste, crate::musicbrainz::normaliser_titre(&album));
            if let Some(rg) = albums.get(&cle) {
                if !deja.contains(rg) && vus.insert(rg.clone()) {
                    out.push(rg.clone());
                    if out.len() >= limite {
                        break;
                    }
                }
            }
        }
        Ok(out)
    }

    /// Les MBID déjà interrogés (et encore frais) pour une source et un
    /// échelon donnés. `depuis` : voir [`Self::pop_recordings_candidats`].
    pub fn pop_deja_fait(&self, source: &str, kind: &str, depuis: i64) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT mbid FROM popularite_fetched
              WHERE source = ?1 AND kind = ?2 AND at >= ?3",
        )?;
        let s = stmt
            .query_map(params![source, kind, depuis], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(s)
    }

    /// Range les popularités d'un lot et marque tous les MBID demandés comme
    /// interrogés — les deux dans une transaction, comme
    /// [`Self::mb_poser_genres`] : une passe coupée ne perd ni ne refait.
    /// `demandes` couvre tout le lot (y compris les inconnus, pour ne pas y
    /// revenir) ; `trouves` n'en est que le sous-ensemble ayant rendu un
    /// chiffre.
    pub fn pop_poser(
        &mut self,
        source: &str,
        kind: &str,
        demandes: &[String],
        trouves: &[PopulariteBrute<'_>],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        for p in trouves {
            tx.execute(
                "INSERT INTO popularite (mbid, kind, source, ecoutes, auditeurs, at)
                 VALUES (?1, ?2, ?3, ?4, ?5, strftime('%s','now'))
                 ON CONFLICT(mbid, kind, source)
                 DO UPDATE SET ecoutes = excluded.ecoutes,
                               auditeurs = excluded.auditeurs,
                               at = excluded.at",
                params![p.mbid, kind, source, p.ecoutes, p.auditeurs],
            )?;
        }
        for mbid in demandes {
            tx.execute(
                "INSERT OR REPLACE INTO popularite_fetched (mbid, kind, source)
                 VALUES (?1, ?2, ?3)",
                params![mbid, kind, source],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// État de fraîcheur de la popularité, pour la ligne d'alerte du mode
    /// Bibliothèque : combien de morceaux ont une popularité, l'instant de la
    /// plus ancienne interrogation, et combien d'entités datent de plus de
    /// `peremption_jours`.
    pub fn popularite_fraicheur(&self, peremption_jours: i64) -> Result<(i64, Option<i64>, i64)> {
        let couverts: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM track_popularite", [], |r| r.get(0))?;
        let plus_ancienne: Option<i64> =
            self.conn
                .query_row("SELECT MIN(at) FROM popularite_fetched", [], |r| r.get(0))?;
        let perimes: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM popularite_fetched
              WHERE at < strftime('%s','now') - ?1",
            params![peremption_jours * 86_400],
            |r| r.get(0),
        )?;
        Ok((couverts, plus_ancienne, perimes))
    }

    /// `mbid → métrique brute` pour une source et un échelon (écoutes
    /// ListenBrainz, `rank` Deezer).
    fn pop_valeurs(&self, source: &str, kind: &str) -> Result<HashMap<String, f64>> {
        let mut stmt = self.conn.prepare(
            "SELECT mbid, ecoutes FROM popularite
              WHERE source = ?1 AND kind = ?2 AND ecoutes IS NOT NULL",
        )?;
        let m = stmt
            .query_map(params![source, kind], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as f64))
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(m)
    }

    /// Recalcule `track_popularite` en entier depuis `popularite` et la
    /// distribution de la bibliothèque — voir « la valeur affichée » de
    /// `docs/popularite.md`. Rend le nombre de morceaux couverts.
    ///
    /// Pour chaque morceau : sa métrique par source à son meilleur échelon
    /// (enregistrement → release-group), chaque métrique convertie en **rang
    /// percentile** dans la bibliothèque, puis la **médiane** des rangs des
    /// sources disponibles. Un morceau sans aucune source n'a pas de ligne.
    pub fn recalculer_track_popularite(&mut self) -> Result<usize> {
        let albums = self.mb_albums()?;
        let lb_rec = self.pop_valeurs("listenbrainz", "recording")?;
        let lb_rg = self.pop_valeurs("listenbrainz", "release-group")?;
        let dz_rec = self.pop_valeurs("deezer", "recording")?;

        let pistes: Vec<PistePop> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, mb_recording_id, mb_artist_id, album FROM tracks")?;
            let v = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect::<std::result::Result<_, _>>()?;
            v
        };

        // Valeur brute par source et échelon de chaque morceau.
        struct Brut {
            id: i64,
            lb: Option<f64>,
            dz: Option<f64>,
            echelon: &'static str,
        }
        let mut bruts = Vec::new();
        for (id, rec, artiste, album) in &pistes {
            let rg = match (artiste, album) {
                (Some(a), Some(al)) => albums
                    .get(&(a.clone(), crate::musicbrainz::normaliser_titre(al)))
                    .cloned(),
                _ => None,
            };
            let lb_r = rec.as_deref().and_then(|m| lb_rec.get(m)).copied();
            let dz_r = rec.as_deref().and_then(|m| dz_rec.get(m)).copied();
            let lb_g = rg.as_deref().and_then(|m| lb_rg.get(m)).copied();

            let lb = lb_r.or(lb_g);
            let echelon = if lb_r.is_some() || dz_r.is_some() {
                "recording"
            } else if lb_g.is_some() {
                "release-group"
            } else {
                continue;
            };
            bruts.push(Brut {
                id: *id,
                lb,
                dz: dz_r,
                echelon,
            });
        }

        let rang_lb = rangs_percentiles(bruts.iter().filter_map(|b| b.lb.map(|v| (b.id, v))));
        let rang_dz = rangs_percentiles(bruts.iter().filter_map(|b| b.dz.map(|v| (b.id, v))));

        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM track_popularite", [])?;
        let mut n = 0usize;
        for b in &bruts {
            let mut rangs: Vec<f64> = Vec::new();
            if let Some(r) = rang_lb.get(&b.id) {
                rangs.push(*r);
            }
            if let Some(r) = rang_dz.get(&b.id) {
                rangs.push(*r);
            }
            if rangs.is_empty() {
                continue;
            }
            rangs.sort_by(|x, y| x.partial_cmp(y).unwrap());
            let relative = if rangs.len() % 2 == 1 {
                rangs[rangs.len() / 2]
            } else {
                (rangs[rangs.len() / 2 - 1] + rangs[rangs.len() / 2]) / 2.0
            };
            tx.execute(
                "INSERT INTO track_popularite (track_id, relative, echelon, calcule_le)
                 VALUES (?1, ?2, ?3, strftime('%s','now'))",
                params![b.id, relative, b.echelon],
            )?;
            n += 1;
        }
        tx.commit()?;
        Ok(n)
    }

    /* -------------------------------------------- biographies (TheAudioDB) */

    /// Les MBID d'artiste dont la biographie reste à récupérer. **Aucun repli
    /// par nom** : un artiste sans `mb_artist_id` n'entre jamais ici — voir
    /// `docs/enrichissement-lecteur.md`. `depuis` : voir
    /// [`Self::pop_recordings_candidats`].
    pub fn theaudiodb_candidats(&self, depuis: i64, limite: usize) -> Result<Vec<String>> {
        let limite = if limite == usize::MAX {
            i64::MAX
        } else {
            limite as i64
        };
        let mut stmt = self.conn.prepare(
            "SELECT t.mb_artist_id, COUNT(*) n FROM tracks t
              WHERE t.mb_artist_id IS NOT NULL AND t.mb_artist_id <> ''
                AND NOT EXISTS (SELECT 1 FROM theaudiodb_fetched f
                                 WHERE f.mb_artist_id = t.mb_artist_id AND f.at >= ?1)
              GROUP BY t.mb_artist_id
              ORDER BY n DESC
              LIMIT ?2",
        )?;
        let out = stmt
            .query_map(params![depuis, limite], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(out)
    }

    /// Range les biographies d'un lot et marque tous les MBID demandés comme
    /// interrogés — même transaction, patron [`Self::pop_poser`].
    pub fn theaudiodb_poser(&mut self, demandes: &[String], trouvees: &[BioBrute]) -> Result<()> {
        let tx = self.conn.transaction()?;
        for b in trouvees {
            tx.execute(
                "INSERT INTO theaudiodb_artistes
                   (mb_artist_id, id_theaudiodb, biographie_en, biographie_fr, recupere_le)
                 VALUES (?1, ?2, ?3, ?4, strftime('%s','now'))
                 ON CONFLICT(mb_artist_id) DO UPDATE SET
                   id_theaudiodb=excluded.id_theaudiodb,
                   biographie_en=excluded.biographie_en,
                   biographie_fr=excluded.biographie_fr,
                   recupere_le=excluded.recupere_le",
                params![
                    b.mb_artist_id,
                    b.id_theaudiodb,
                    b.biographie_en,
                    b.biographie_fr
                ],
            )?;
        }
        for mbid in demandes {
            tx.execute(
                "INSERT OR REPLACE INTO theaudiodb_fetched (mb_artist_id) VALUES (?1)",
                params![mbid],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Les biographies connues pour un morceau. `mb_artist_id` peut porter
    /// plusieurs MBID `/`-séparés (« X feat. Y ») : une entrée par MBID connu.
    pub fn bio_pour_piste(&self, track_id: i64) -> Result<Vec<BioArtiste>> {
        let brut: Option<String> = self
            .conn
            .query_row(
                "SELECT mb_artist_id FROM tracks WHERE id = ?1",
                params![track_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let Some(brut) = brut else {
            return Ok(Vec::new());
        };

        let mut stmt = self.conn.prepare(
            "SELECT mb_artist_id, biographie_en, biographie_fr
               FROM theaudiodb_artistes WHERE mb_artist_id = ?1",
        )?;
        let mut out = Vec::new();
        for mbid in brut.split('/').map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(row) = stmt
                .query_row(params![mbid], |r| {
                    Ok(BioArtiste {
                        mb_artist_id: r.get(0)?,
                        biographie_en: r.get(1)?,
                        biographie_fr: r.get(2)?,
                    })
                })
                .optional()?
            {
                out.push(row);
            }
        }
        Ok(out)
    }

    /// La biographie connue pour un artiste, par MBID direct — pour la vue
    /// centrale « albums de cet artiste » (mode Écoute), indépendante de
    /// toute piste sélectionnée. `bio_pour_piste` reste la voie de
    /// l'inspecteur, qui part d'une piste et résout son/ses MBID d'artiste.
    pub fn bio_pour_artiste(&self, mbid: &str) -> Result<Option<BioArtiste>> {
        let row = self
            .conn
            .query_row(
                "SELECT mb_artist_id, biographie_en, biographie_fr
               FROM theaudiodb_artistes WHERE mb_artist_id = ?1",
                params![mbid],
                |r| {
                    Ok(BioArtiste {
                        mb_artist_id: r.get(0)?,
                        biographie_en: r.get(1)?,
                        biographie_fr: r.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /* --------------------------------------- crédits Discogs (dumps CC0) */

    /// Les MBID d'édition (`tracks.mb_release_id`) dont le lien Discogs reste
    /// à vérifier — jamais interrogés via `editions_discogs`.
    pub fn discogs_liaison_candidats(&self, limite: usize) -> Result<Vec<String>> {
        let limite = if limite == usize::MAX {
            i64::MAX
        } else {
            limite as i64
        };
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT t.mb_release_id FROM tracks t
              WHERE t.mb_release_id IS NOT NULL AND t.mb_release_id <> ''
                AND NOT EXISTS (SELECT 1 FROM editions_discogs e
                                 WHERE e.mb_release_id = t.mb_release_id)
              LIMIT ?1",
        )?;
        let out = stmt
            .query_map(params![limite], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(out)
    }

    /// Range le lien (ou son absence — `discogs_release_id = NULL`) entre une
    /// édition MusicBrainz et son édition Discogs.
    pub fn discogs_lier_edition(
        &self,
        mb_release_id: &str,
        discogs_release_id: Option<i64>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO editions_discogs (mb_release_id, discogs_release_id, at)
             VALUES (?1, ?2, strftime('%s','now'))
             ON CONFLICT(mb_release_id) DO UPDATE SET
               discogs_release_id=excluded.discogs_release_id, at=excluded.at",
            params![mb_release_id, discogs_release_id],
        )?;
        Ok(())
    }

    /// Tous les identifiants d'édition Discogs à rechercher dans le prochain
    /// dump — ceux qu'on a réussi à relier depuis MusicBrainz.
    pub fn discogs_wanted_ids(&self) -> Result<HashSet<u64>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT discogs_release_id FROM editions_discogs
              WHERE discogs_release_id IS NOT NULL",
        )?;
        let out = stmt
            .query_map([], |r| r.get::<_, i64>(0).map(|v| v as u64))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(out)
    }

    /// Range les crédits d'une édition Discogs — remplace ce qui existait déjà
    /// pour cette édition (un import réécrit, il ne fusionne pas).
    pub fn credits_poser(
        &mut self,
        discogs_release_id: u64,
        credits: &[CreditDiscogs],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM credits_discogs WHERE discogs_release_id = ?1",
            params![discogs_release_id as i64],
        )?;
        for c in credits {
            tx.execute(
                "INSERT OR IGNORE INTO credits_discogs
                   (discogs_release_id, personne, role, pistes, discogs_artist_id, source)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'discogs')",
                params![
                    discogs_release_id as i64,
                    c.personne,
                    c.role,
                    c.pistes,
                    c.discogs_artist_id
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Les crédits Discogs d'un morceau, via son édition MusicBrainz.
    pub fn credits_pour_piste(&self, track_id: i64) -> Result<Vec<CreditDiscogs>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.personne, c.role, c.pistes, c.discogs_artist_id
               FROM tracks t
               JOIN editions_discogs e ON e.mb_release_id = t.mb_release_id
               JOIN credits_discogs c ON c.discogs_release_id = e.discogs_release_id
              WHERE t.id = ?1
              ORDER BY c.personne",
        )?;
        let out = stmt
            .query_map(params![track_id], |r| {
                Ok(CreditDiscogs {
                    personne: r.get(0)?,
                    role: r.get(1)?,
                    pistes: r.get(2)?,
                    discogs_artist_id: r.get(3)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(out)
    }

    /// Range les labels d'une édition Discogs — remplace ce qui existait déjà
    /// pour cette édition (même principe que `credits_poser` : un import
    /// réécrit, il ne fusionne pas).
    pub fn labels_poser(&mut self, discogs_release_id: u64, labels: &[LabelDiscogs]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM labels_discogs WHERE discogs_release_id = ?1",
            params![discogs_release_id as i64],
        )?;
        for l in labels {
            tx.execute(
                "INSERT OR IGNORE INTO labels_discogs (discogs_release_id, nom, catno, discogs_label_id)
                 VALUES (?1, ?2, ?3, ?4)",
                params![discogs_release_id as i64, l.nom, l.catno, l.discogs_label_id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Les labels (nom + numéro de catalogue) Discogs d'un morceau, via son
    /// édition MusicBrainz — même jointure que `credits_pour_piste`. Vide
    /// tant que la liaison et l'import mensuel n'ont pas couvert cette
    /// édition, jamais une valeur inventée.
    pub fn labels_pour_piste(&self, track_id: i64) -> Result<Vec<LabelDiscogs>> {
        let mut stmt = self.conn.prepare(
            "SELECT l.nom, l.catno, l.discogs_label_id
               FROM tracks t
               JOIN editions_discogs e ON e.mb_release_id = t.mb_release_id
               JOIN labels_discogs l ON l.discogs_release_id = e.discogs_release_id
              WHERE t.id = ?1
              ORDER BY l.nom",
        )?;
        let out = stmt
            .query_map(params![track_id], |r| {
                Ok(LabelDiscogs {
                    nom: r.get(0)?,
                    catno: r.get(1)?,
                    discogs_label_id: r.get(2)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(out)
    }

    /// Marque une édition comme traitée par un import du dump — voir
    /// `discogs_importes` du schéma. Appelée pour toute édition réellement
    /// rencontrée dans le dump, même sans crédit ni label trouvé : c'est ce
    /// qui distingue « importé, rien trouvé » de « jamais importé ».
    pub fn discogs_marquer_importe(&self, discogs_release_id: u64) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO discogs_importes (discogs_release_id) VALUES (?1)",
            params![discogs_release_id as i64],
        )?;
        Ok(())
    }

    /// Y a-t-il une édition reliée dont l'import n'a encore jamais été
    /// tenté ? Sert à décider si relire le dump (~10 min sur 11 Go) vaut le
    /// coût après une liaison — y compris quand cette liaison-ci n'a rien
    /// relié de neuf, mais qu'un import précédent avait été interrompu ou
    /// n'avait jamais eu lieu pour des éditions déjà reliées avant elle.
    pub fn discogs_import_utile(&self) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM editions_discogs e
                  WHERE e.discogs_release_id IS NOT NULL
                    AND NOT EXISTS (SELECT 1 FROM discogs_importes i
                                     WHERE i.discogs_release_id = e.discogs_release_id)
             )",
            [],
            |r| r.get(0),
        )?)
    }

    /* --------------------------------------- critiques (CritiqueBrainz) */

    /// Les release-groups dont les critiques restent à récupérer. Réutilise
    /// `mb_release_groups`, déjà rempli par l'enrichissement des genres — pas
    /// de nouvelle résolution nom → release-group.
    pub fn critiques_candidats(&self, depuis: i64, limite: usize) -> Result<Vec<String>> {
        let limite = if limite == usize::MAX {
            i64::MAX
        } else {
            limite as i64
        };
        let mut stmt = self.conn.prepare(
            "SELECT mbid FROM mb_release_groups g
              WHERE NOT EXISTS (SELECT 1 FROM critiques_fetched f
                                  WHERE f.mbid_release_group = g.mbid AND f.at >= ?1)
              LIMIT ?2",
        )?;
        let out = stmt
            .query_map(params![depuis, limite], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(out)
    }

    /// Range les critiques d'un release-group et marque celui-ci comme
    /// interrogé — « aucune critique » est une réponse valable, marquée comme
    /// les autres.
    pub fn critiques_poser(&mut self, mbid_rg: &str, critiques: &[CritiqueBrute]) -> Result<()> {
        let tx = self.conn.transaction()?;
        for c in critiques {
            tx.execute(
                "INSERT INTO critiques
                   (id, mbid_release_group, source, auteur, licence_id, licence_nom,
                    langue, texte, url_originale, recupere_le)
                 VALUES (?1, ?2, 'critiquebrainz', ?3, ?4, ?5, ?6, ?7, ?8, strftime('%s','now'))
                 ON CONFLICT(id) DO UPDATE SET
                   auteur=excluded.auteur, licence_id=excluded.licence_id,
                   licence_nom=excluded.licence_nom, langue=excluded.langue,
                   texte=excluded.texte, url_originale=excluded.url_originale,
                   recupere_le=excluded.recupere_le",
                params![
                    c.id,
                    mbid_rg,
                    c.auteur,
                    c.licence_id,
                    c.licence_nom,
                    c.langue,
                    c.texte,
                    c.url_originale
                ],
            )?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO critiques_fetched (mbid_release_group) VALUES (?1)",
            params![mbid_rg],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Les critiques connues pour un morceau, via son release-group
    /// MusicBrainz (même résolution `(mb_artist_id, titre normalisé)` que
    /// [`Self::pop_rg_candidats`]).
    pub fn critiques_pour_piste(&self, track_id: i64) -> Result<Vec<Critique>> {
        let Some(rg) = self.release_group_pour_piste(track_id)? else {
            return Ok(Vec::new());
        };
        self.critiques_pour_release_group(&rg)
    }

    /// Le release-group MusicBrainz d'un morceau, s'il est connu — même
    /// résolution que la popularité et les genres à l'échelon album.
    pub fn release_group_pour_piste(&self, track_id: i64) -> Result<Option<String>> {
        let paire = self
            .conn
            .query_row(
                "SELECT mb_artist_id, album FROM tracks WHERE id = ?1",
                params![track_id],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<String>>(1)?,
                    ))
                },
            )
            .optional()?;
        self.release_group_depuis_paire(paire)
    }

    /// Le release-group MusicBrainz du morceau au chemin `path`, s'il est
    /// connu — même résolution que [`Self::release_group_pour_piste`], pour
    /// les appelants qui n'ont qu'un chemin de fichier (le repli pochette).
    pub fn release_group_pour_path(&self, path: &Path) -> Result<Option<String>> {
        let chemin = path.to_string_lossy();
        let paire = self
            .conn
            .query_row(
                "SELECT mb_artist_id, album FROM tracks WHERE path = ?1",
                params![chemin],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<String>>(1)?,
                    ))
                },
            )
            .optional()?;
        self.release_group_depuis_paire(paire)
    }

    /// La release MusicBrainz (`MUSICBRAINZ_ALBUMID`) du morceau au chemin
    /// `path`, telle que portée par ses tags — sans jointure de titre, donc
    /// exacte là où [`Self::release_group_pour_path`] peut rater une édition.
    pub fn release_pour_path(&self, path: &Path) -> Result<Option<String>> {
        let chemin = path.to_string_lossy();
        Ok(self
            .conn
            .query_row(
                "SELECT mb_release_id FROM tracks WHERE path = ?1",
                params![chemin],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten()
            .filter(|id| !id.is_empty()))
    }

    /// Chemins de `limite` autres pistes du même album que `path`, dans l'ordre
    /// des chemins. Même regroupement que [`Self::albums`] (album + artiste
    /// d'album, à défaut artiste de la piste).
    ///
    /// La grille d'albums ne retient que la première piste pour la pochette :
    /// si c'est justement celle dont l'image manque (retaguée à part), les
    /// autres peuvent la porter.
    pub fn pistes_soeurs(&self, path: &Path, limite: usize) -> Result<Vec<PathBuf>> {
        let chemin = path.to_string_lossy();
        let mut stmt = self.conn.prepare(
            "SELECT t2.path FROM tracks t1
               JOIN tracks t2
                 ON t2.album = t1.album
                AND COALESCE(t2.album_artist, t2.artist) IS COALESCE(t1.album_artist, t1.artist)
                AND t2.path <> t1.path
              WHERE t1.path = ?1
              ORDER BY t2.path
              LIMIT ?2",
        )?;
        let out = stmt
            .query_map(params![chemin, limite as i64], |r| r.get::<_, String>(0))?
            .map(|p| p.map(PathBuf::from))
            .collect::<std::result::Result<_, _>>()?;
        Ok(out)
    }

    /// `(artiste de l'album, album)` du morceau au chemin `path` — la clé de
    /// recherche des sources sans identifiant MusicBrainz (Deezer).
    pub fn artiste_album_pour_path(&self, path: &Path) -> Result<Option<(String, String)>> {
        let chemin = path.to_string_lossy();
        let paire = self
            .conn
            .query_row(
                "SELECT COALESCE(NULLIF(album_artist, ''), artist), album
                   FROM tracks WHERE path = ?1",
                params![chemin],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<String>>(1)?,
                    ))
                },
            )
            .optional()?;
        Ok(paire
            .and_then(|(a, b)| Some((a?, b?)))
            .filter(|(a, b)| !a.is_empty() && !b.is_empty()))
    }

    fn release_group_depuis_paire(
        &self,
        paire: Option<(Option<String>, Option<String>)>,
    ) -> Result<Option<String>> {
        let Some((artiste, album)) = paire.and_then(|(a, b)| Some((a?, b?))) else {
            return Ok(None);
        };
        let albums = self.mb_albums()?;
        Ok(albums
            .get(&(artiste, crate::musicbrainz::normaliser_titre(&album)))
            .cloned())
    }

    pub(super) fn critiques_pour_release_group(&self, mbid_rg: &str) -> Result<Vec<Critique>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, auteur, licence_id, licence_nom, langue, texte, url_originale
               FROM critiques WHERE mbid_release_group = ?1
              ORDER BY recupere_le DESC",
        )?;
        let out = stmt
            .query_map(params![mbid_rg], |r| {
                Ok(Critique {
                    id: r.get(0)?,
                    auteur: r.get(1)?,
                    licence_id: r.get(2)?,
                    licence_nom: r.get(3)?,
                    langue: r.get(4)?,
                    texte: r.get(5)?,
                    url_originale: r.get(6)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(out)
    }

    /* --------------------------------------------------- tags (Last.fm) */

    /// Les artistes dont les tags Last.fm restent à récupérer. Même forme que
    /// `mb_fetched` (kind = 'artist') mais avec péremption, comme
    /// `popularite_fetched` : `depuis = 0` ne rafraîchit rien.
    pub fn lastfm_candidats(&self, depuis: i64, limite: usize) -> Result<Vec<String>> {
        let limite = if limite == usize::MAX {
            i64::MAX
        } else {
            limite as i64
        };
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT t.mb_artist_id FROM tracks t
              WHERE t.mb_artist_id IS NOT NULL AND t.mb_artist_id <> ''
                AND NOT EXISTS (SELECT 1 FROM lastfm_fetched f
                                 WHERE f.mb_artist_id = t.mb_artist_id AND f.at >= ?1)
              LIMIT ?2",
        )?;
        let out = stmt
            .query_map(params![depuis, limite], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(out)
    }

    /// Range les tags Last.fm d'un artiste et le marque comme interrogé —
    /// « aucun tag » est une réponse valable, marquée comme pour les autres
    /// sources.
    pub fn lastfm_tags_poser(&mut self, mbid: &str, tags: &[crate::lastfm::Tag]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM lastfm_tags WHERE mb_artist_id = ?1",
            params![mbid],
        )?;
        for t in tags {
            tx.execute(
                "INSERT INTO lastfm_tags (mb_artist_id, tag, poids) VALUES (?1, ?2, ?3)
                 ON CONFLICT(mb_artist_id, tag) DO UPDATE SET poids=excluded.poids",
                params![mbid, t.nom, t.poids],
            )?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO lastfm_fetched (mb_artist_id) VALUES (?1)",
            params![mbid],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Les tags Last.fm connus, par artiste, triés par poids décroissant —
    /// miroir de [`Self::mb_genres`] pour le vote de nommage des familles.
    pub fn lastfm_tags(&self, plancher: u32) -> Result<HashMap<String, Vec<String>>> {
        let mut stmt = self.conn.prepare(
            "SELECT mb_artist_id, tag FROM lastfm_tags
              WHERE poids >= ?1
              ORDER BY mb_artist_id, poids DESC, tag",
        )?;
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        let lignes = stmt.query_map(params![plancher], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for l in lignes {
            let (mbid, tag) = l?;
            out.entry(mbid).or_default().push(tag);
        }
        Ok(out)
    }

    /// Les genres MusicBrainz assez représentés dans la bibliothèque pour
    /// entrer dans le vocabulaire CLAP-texte — voir `docs/nommage-
    /// familles.md`. `seuil` compte les entités MusicBrainz **distinctes**
    /// (artiste ou release-group, sans distinction) qui portent le genre,
    /// pas les votes internes à `mb_genres` : un genre porté par beaucoup
    /// d'entités pèse réellement sur la bibliothèque, même si chacune ne l'a
    /// reçu qu'une fois. Du plus représenté au moins représenté.
    pub fn genres_candidats(&self, seuil: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT genre, COUNT(DISTINCT mbid) n FROM mb_genres
              GROUP BY genre HAVING n >= ?1
              ORDER BY n DESC, genre",
        )?;
        let out = stmt
            .query_map(params![seuil], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(out)
    }

    /// Combien d'artistes ont été interrogés, et combien ont rendu un genre.
    pub fn mb_avancement(&self) -> Result<(i64, i64, i64)> {
        let total: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT mb_artist_id) FROM tracks
              WHERE mb_artist_id IS NOT NULL AND mb_artist_id <> ''",
            [],
            |r| r.get(0),
        )?;
        let faits: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM mb_fetched WHERE kind = 'artist'",
            [],
            |r| r.get(0),
        )?;
        let avec: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT mbid) FROM mb_genres WHERE kind = 'artist'",
            [],
            |r| r.get(0),
        )?;
        Ok((faits, total, avec))
    }

    /// Combien de morceaux ont un genre MusicBrainz utilisable.
    pub fn mb_couverture(&self, model: &str, plancher: i64) -> Result<(i64, i64)> {
        let total: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM features WHERE model = ?1",
            params![model],
            |r| r.get(0),
        )?;
        let couverts: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM features f JOIN tracks t ON t.id = f.track_id
              WHERE f.model = ?1 AND EXISTS (
                    SELECT 1 FROM mb_genres g
                     WHERE g.kind = 'artist' AND g.votes >= ?2
                       AND g.mbid = t.mb_artist_id)",
            params![model, plancher],
            |r| r.get(0),
        )?;
        Ok((couverts, total))
    }
}

/// Un album MusicBrainz prêt à ranger : identifiant, titre tel qu'il est
/// publié, titre normalisé pour le rapprochement, genres avec leurs votes,
/// date de première parution du release-group (`first-release-date`,
/// souvent partielle) et types secondaires joints par une virgule
/// (« Live,Compilation ») — c'est de ces deux derniers champs que dépendent
/// [`Library::ordre_darrivee`] et [`Library::albums`] pour préférer la date
/// de l'œuvre à celle, moins sûre, du tag.
pub type AlbumRange = (
    String,
    String,
    String,
    Vec<(String, i64)>,
    Option<String>,
    Option<String>,
);

/// Une piste vue par le recalcul de popularité : son id, son MBID
/// d'enregistrement, son MBID d'artiste, son album.
type PistePop = (i64, Option<String>, Option<String>, Option<String>);

/// Un enregistrement dont la popularité reste à récupérer. `artiste` et
/// `titre` servent à la recherche Deezer, qui n'a pas de MBID.
#[derive(Debug, Clone)]
pub struct PisteAPopulariser {
    pub recording_mbid: String,
    pub artiste: Option<String>,
    pub titre: Option<String>,
}

/// Une popularité brute à ranger : `ecoutes` porte la métrique principale de
/// la source (écoutes ListenBrainz, `rank` Deezer), `auditeurs` le compte
/// d'auditeurs distincts quand la source le donne.
#[derive(Debug, Clone, Copy)]
pub struct PopulariteBrute<'a> {
    pub mbid: &'a str,
    pub ecoutes: i64,
    pub auditeurs: Option<i64>,
}

/// Une biographie TheAudioDB à ranger, résolue par MBID d'artiste.
#[derive(Debug, Clone)]
pub struct BioBrute {
    pub mb_artist_id: String,
    pub id_theaudiodb: Option<String>,
    pub biographie_en: Option<String>,
    pub biographie_fr: Option<String>,
}

/// Une biographie telle que servie à l'inspecteur.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BioArtiste {
    pub mb_artist_id: String,
    pub biographie_en: Option<String>,
    pub biographie_fr: Option<String>,
}

/// Un crédit Discogs à ranger — voir `credits_discogs` du schéma.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CreditDiscogs {
    pub personne: String,
    pub role: String,
    pub pistes: String,
    pub discogs_artist_id: Option<i64>,
}

/// Un label Discogs à ranger — voir `labels_discogs` du schéma.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LabelDiscogs {
    pub nom: String,
    pub catno: String,
    pub discogs_label_id: Option<i64>,
}

/// Une critique CritiqueBrainz à ranger.
#[derive(Debug, Clone)]
pub struct CritiqueBrute {
    pub id: String,
    pub auteur: Option<String>,
    pub licence_id: String,
    pub licence_nom: Option<String>,
    pub langue: Option<String>,
    pub texte: String,
    pub url_originale: Option<String>,
}

/// Une critique telle que servie à l'inspecteur — l'attribution est
/// obligatoire partout où `texte` apparaît (licence CC).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Critique {
    pub id: String,
    pub auteur: Option<String>,
    pub licence_id: String,
    pub licence_nom: Option<String>,
    pub langue: Option<String>,
    pub texte: String,
    pub url_originale: Option<String>,
}

/// Convertit une liste `(id, valeur)` en `id → rang percentile` dans `[0, 1]` :
/// la part des valeurs strictement inférieures. Insensible à l'échelle et aux
/// distributions à longue traîne — c'est pourquoi on mélange des rangs, pas
/// des comptes bruts (`docs/popularite.md`).
pub(super) fn rangs_percentiles(paires: impl Iterator<Item = (i64, f64)>) -> HashMap<i64, f64> {
    let paires: Vec<(i64, f64)> = paires.collect();
    let mut triees: Vec<f64> = paires.iter().map(|(_, v)| *v).collect();
    triees.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = triees.len();
    paires
        .into_iter()
        .map(|(id, v)| {
            let moins = triees.partition_point(|x| *x < v);
            let rang = if n > 1 {
                moins as f64 / (n - 1) as f64
            } else {
                0.5
            };
            (id, rang)
        })
        .collect()
}
