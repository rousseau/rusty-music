// SPDX-License-Identifier: GPL-3.0-or-later
//! La carte d'Explorer : familles, vote des genres, paramètres, positions.

use super::*;

impl Library {
    /// La famille (`features.cluster`) d'un morceau, si elle est déjà
    /// calculée — `None` sur un morceau pas encore projeté. Sert l'inspecteur
    /// (`docs/nommage-familles.md`) : la plupart des vues de piste ne
    /// portent pas déjà ce numéro (seuls les points de la carte l'ont), un
    /// lookup ponctuel évite d'en refaire porter le champ partout.
    pub fn cluster_de_piste(&self, model: &str, track_id: i64) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT cluster FROM features WHERE model = ?1 AND track_id = ?2",
                params![model, track_id],
                |r| r.get(0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(Into::into)
    }

    /// Les empreintes d'une seule famille — sert l'affinage local suggéré
    /// dans l'inspecteur (`docs/nommage-familles.md`) : `crates/analysis`
    /// n'y fait tourner un k-means que sur ce sous-ensemble, jamais sur la
    /// bibliothèque entière, et rien de ce qui en sort n'est écrit dans
    /// `features.cluster`.
    pub fn embeddings_du_cluster(&self, model: &str, cluster: i64) -> Result<Vec<(i64, Vec<f32>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT track_id, vector FROM features
              WHERE model = ?1 AND cluster = ?2
              ORDER BY track_id",
        )?;
        let rows = stmt
            .query_map(params![model, cluster], |r| {
                let id: i64 = r.get(0)?;
                let blob: Vec<u8> = r.get(1)?;
                let v = blob
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                Ok((id, v))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Les familles de la carte, nommées par leurs genres.
    ///
    /// Mince projeté de [`Self::familles_votees`] — la carte, l'anneau et la
    /// frise n'ont besoin que d'un nom par famille, jamais du détail du vote.
    /// Signature et forme du triplet inchangées, pour ne rien casser côté
    /// interface (`docs/nommage-familles.md`).
    pub fn familles(&self, model: &str) -> Result<Vec<(i64, String, i64)>> {
        Ok(self
            .familles_votees(model)?
            .into_iter()
            .map(|v| (v.cluster, v.nom_affiche, v.effectif))
            .collect())
    }

    /// Les familles de la carte, nommées par un vote entre trois sources
    /// indépendantes — MusicBrainz, le vocabulaire CLAP-texte et Last.fm.
    /// Voir `docs/nommage-familles.md`.
    ///
    /// **Trois sources, par ordre de précision décroissante** pour
    /// MusicBrainz — l'album, puis l'artiste, puis le tag du fichier. Le
    /// détail de l'arbitrage est dans [`genres_du_morceau`], le nommage par
    /// source dans [`nommer_les_familles`] (appelée trois fois, une par
    /// source, sans y toucher) ; la combinaison est dans [`arbitrer_vote`].
    pub fn familles_votees(&self, model: &str) -> Result<Vec<VoteFamille>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.cluster, t.mb_artist_id, t.album, t.genre, f.texte_label
               FROM features f JOIN tracks t ON t.id = f.track_id
              WHERE f.model = ?1",
        )?;
        let pistes: Vec<PisteVote> = stmt
            .query_map(params![model], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<std::result::Result<_, _>>()?;
        self.nommer_par_groupe(&pistes)
    }

    /// Affinage local d'une famille — nomme, par le même vote, des
    /// sous-groupes calculés à la volée (typiquement un k-means restreint
    /// aux empreintes de la famille, côté `crates/analysis`) au lieu des
    /// familles de `features.cluster`. `assignations` donne, pour chaque
    /// morceau considéré, son sous-groupe (0, 1, 2…) — un identifiant local
    /// à cet appel, **jamais** écrit dans `features.cluster` ni réutilisé
    /// par la carte, l'anneau ou la frise. Voir `docs/nommage-familles.md`.
    pub fn sous_groupes_votes(
        &self,
        model: &str,
        assignations: &[(i64, i64)],
    ) -> Result<Vec<VoteFamille>> {
        if assignations.is_empty() {
            return Ok(Vec::new());
        }
        let groupe_de: HashMap<i64, i64> = assignations.iter().cloned().collect();
        let ids: Vec<i64> = assignations.iter().map(|(id, _)| *id).collect();
        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT t.id, t.mb_artist_id, t.album, t.genre, f.texte_label
               FROM tracks t JOIN features f ON f.track_id = t.id
              WHERE f.model = ? AND t.id IN ({placeholders})"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut params: Vec<&dyn rusqlite::ToSql> = vec![&model];
        params.extend(ids.iter().map(|id| id as &dyn rusqlite::ToSql));
        let pistes: Vec<PisteVote> = stmt
            .query_map(params.as_slice(), |r| {
                let id: i64 = r.get(0)?;
                Ok((id, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .filter_map(|res| {
                res.ok().and_then(|(id, artiste, album, tag, texte_label)| {
                    groupe_de
                        .get(&id)
                        .map(|g| (*g, artiste, album, tag, texte_label))
                })
            })
            .collect();
        self.nommer_par_groupe(&pistes)
    }

    /// Le cœur du vote à trois sources, partagé par [`Self::familles_votees`]
    /// et [`Self::sous_groupes_votes`] : `pistes` donne, par morceau,
    /// `(groupe, artiste MusicBrainz, album, tag de fichier, genre gagnant
    /// CLAP-texte)` — un identifiant de groupe générique, famille de la
    /// carte ou sous-groupe d'un affinage local, cette fonction n'en sait
    /// rien et ne le réécrit nulle part.
    fn nommer_par_groupe(&self, pistes: &[PisteVote]) -> Result<Vec<VoteFamille>> {
        let par_artiste = self.mb_genres("artist", VOTES_MINIMUM)?;
        let par_album = self.mb_genres("release-group", VOTES_MINIMUM)?;
        let albums = self.mb_albums()?;
        let par_artiste_lastfm = self.lastfm_tags(LASTFM_POIDS_MINIMUM)?;

        let mut cumul_mb: HashMap<(i64, String), i64> = HashMap::new();
        let mut cumul_clap: HashMap<(i64, String), i64> = HashMap::new();
        let mut cumul_lastfm: HashMap<(i64, String), i64> = HashMap::new();
        let mut rg_par_cluster: HashMap<i64, HashSet<String>> = HashMap::new();
        for (cluster, artiste, album, tag, texte_label) in pistes {
            for genre in genres_du_morceau(
                artiste.as_deref(),
                album.as_deref(),
                tag.as_deref(),
                &albums,
                &par_album,
                &par_artiste,
            ) {
                *cumul_mb.entry((*cluster, genre)).or_default() += 1;
            }
            if let Some(label) = texte_label {
                *cumul_clap.entry((*cluster, label.clone())).or_default() += 1;
            }
            if let Some(tag) = artiste
                .as_deref()
                .and_then(|a| par_artiste_lastfm.get(a))
                .and_then(|tags| tags.first())
            {
                *cumul_lastfm.entry((*cluster, tag.clone())).or_default() += 1;
            }
            if let (Some(artiste), Some(album)) = (artiste, album) {
                let cle = (artiste.clone(), crate::texte::normaliser_titre(album));
                if let Some(rg) = albums.get(&cle) {
                    rg_par_cluster
                        .entry(*cluster)
                        .or_default()
                        .insert(rg.clone());
                }
            }
        }
        let versifier = |cumul: HashMap<(i64, String), i64>| -> Vec<(i64, String, i64)> {
            cumul.into_iter().map(|((c, g), n)| (c, g, n)).collect()
        };
        let comptes_mb = versifier(cumul_mb);
        let comptes_clap = versifier(cumul_clap);
        let comptes_lastfm = versifier(cumul_lastfm);

        // L'effectif compte tous les morceaux du groupe, y compris ceux sans
        // genre : c'est la taille de la tache sur la carte (ou du
        // sous-groupe, pour l'affinage local) — directement depuis `pistes`,
        // qui porte déjà une ligne par morceau considéré.
        let mut compte: HashMap<i64, i64> = HashMap::new();
        for (groupe, _, _, _, _) in pistes {
            *compte.entry(*groupe).or_default() += 1;
        }
        let mut effectifs: Vec<(i64, i64)> = compte.into_iter().collect();
        effectifs.sort_by_key(|(_, n)| std::cmp::Reverse(*n));

        // `nom_mb` garde son repli « Famille N » : c'est le filet de sécurité
        // de la légende quand les trois sources divergent (ou manquent).
        // CLAP-texte et Last.fm, eux, ne doivent jamais recevoir ce repli
        // pour le vote — voir [`nommer_les_familles_option`].
        let noms_mb = nommer_les_familles(&effectifs, &comptes_mb);
        let carte_clap: HashMap<i64, String> =
            nommer_les_familles_option(&effectifs, &comptes_clap)
                .into_iter()
                .filter_map(|(c, n, _)| n.map(|n| (c, n)))
                .collect();
        let carte_lastfm: HashMap<i64, String> =
            nommer_les_familles_option(&effectifs, &comptes_lastfm)
                .into_iter()
                .filter_map(|(c, n, _)| n.map(|n| (c, n)))
                .collect();

        let mut out = Vec::with_capacity(noms_mb.len());
        for (cluster, nom_mb, effectif) in noms_mb {
            let nom_clap = carte_clap.get(&cluster).cloned().unwrap_or_default();
            let nom_lastfm = carte_lastfm.get(&cluster).cloned().unwrap_or_default();
            let critique_confirme = |candidat: &str| -> bool {
                let Some(rgs) = rg_par_cluster.get(&cluster) else {
                    return false;
                };
                self.critique_contient(rgs, candidat).unwrap_or(false)
            };
            let (fiable, nom_affiche) =
                arbitrer_vote(&nom_mb, &nom_clap, &nom_lastfm, critique_confirme);
            out.push(VoteFamille {
                cluster,
                effectif,
                fiable,
                nom_affiche,
                musicbrainz: nom_mb,
                clap_texte: nom_clap,
                lastfm: nom_lastfm,
            });
        }
        Ok(out)
    }

    /// Une critique disponible pour un des release-groups `rgs` contient-elle
    /// (en sous-chaîne, insensible à la casse) le libellé de tête de
    /// `candidat` ? Signal de confirmation de dernier recours quand les trois
    /// sources principales divergent — voir [`arbitrer_vote`]. Borné à 200
    /// release-groups par appel : une famille de plusieurs milliers de
    /// morceaux n'a besoin que d'un indice, pas d'un balayage complet.
    fn critique_contient(&self, rgs: &HashSet<String>, candidat: &str) -> Result<bool> {
        let tete = tete_normalisee(candidat);
        if tete.is_empty() {
            return Ok(false);
        }
        let rgs: Vec<&String> = rgs.iter().take(200).collect();
        if rgs.is_empty() {
            return Ok(false);
        }
        let placeholders = std::iter::repeat_n("?", rgs.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql =
            format!("SELECT texte FROM critiques WHERE mbid_release_group IN ({placeholders})");
        let mut stmt = self.conn.prepare(&sql)?;
        let textes: Vec<String> = stmt
            .query_map(rusqlite::params_from_iter(rgs.iter()), |r| {
                r.get::<_, String>(0)
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(textes.iter().any(|t| t.to_lowercase().contains(&tete)))
    }

    /// Pour chaque album, sa famille sonique dominante — le cluster de la carte
    /// le plus représenté parmi ses morceaux déjà projetés.
    ///
    /// Sert le filtre par famille de la grille de pochettes du mode Écoute, qui
    /// réutilise la légende des familles du mode Explorer. La clé d'album est
    /// celle de [`Library::albums`] : nom + `COALESCE(album_artist, artist)`.
    /// Les albums dont aucun morceau n'est encore sur la carte n'apparaissent
    /// pas — le filtre les laisse alors visibles par défaut.
    pub fn familles_des_albums(&self, model: &str) -> Result<Vec<(String, Option<String>, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.album, COALESCE(t.album_artist, t.artist), f.cluster, COUNT(*)
               FROM features f JOIN tracks t ON t.id = f.track_id
              WHERE f.model = ?1 AND f.cluster IS NOT NULL AND t.album IS NOT NULL
              GROUP BY t.album, COALESCE(t.album_artist, t.artist), f.cluster",
        )?;
        let lignes: Vec<(String, Option<String>, i64, i64)> = stmt
            .query_map(params![model], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<std::result::Result<_, _>>()?;

        // Cluster majoritaire par album ; à égalité, le plus petit numéro
        // tranche, pour que le filtre range un album au même endroit d'une
        // session à l'autre.
        let mut par_album: HashMap<(String, Option<String>), (i64, i64)> = HashMap::new();
        for (album, artiste, cluster, n) in lignes {
            let e = par_album.entry((album, artiste)).or_insert((cluster, n));
            if n > e.1 || (n == e.1 && cluster < e.0) {
                *e = (cluster, n);
            }
        }
        Ok(par_album
            .into_iter()
            .map(|((album, artiste), (cluster, _))| (album, artiste, cluster))
            .collect())
    }

    /// Famille secondaire d'un album « à cheval » sur deux familles — la
    /// deuxième famille la plus représentée parmi ses morceaux, retenue
    /// seulement si elle atteint au moins deux morceaux (sans ce seuil, un
    /// seul morceau mal étiqueté suffirait à fabriquer une fausse
    /// hybridation). Sert uniquement à colorer l'arc entrant depuis cette
    /// famille-là sur la frise des filiations (`docs/frise-filiations.md`
    /// § 3) ; ne détermine jamais la bande ni la position d'un album, qui
    /// restent celles de la famille majoritaire ([`Self::familles_des_albums`]).
    ///
    /// Clé de retour : [`hacher`]`(artiste, album) as i64`, la même que
    /// [`Self::albums_annotes`] — pas de recalcul de similarité, seulement un
    /// second passage sur le même comptage par (album, famille).
    pub fn familles_secondaires_albums(&self, model: &str) -> Result<HashMap<i64, i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.album, COALESCE(t.album_artist, t.artist), f.cluster, COUNT(*)
               FROM features f JOIN tracks t ON t.id = f.track_id
              WHERE f.model = ?1 AND f.cluster IS NOT NULL AND t.album IS NOT NULL
              GROUP BY t.album, COALESCE(t.album_artist, t.artist), f.cluster",
        )?;
        let lignes: Vec<(String, Option<String>, i64, i64)> = stmt
            .query_map(params![model], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<std::result::Result<_, _>>()?;

        type ComptesFamillesParAlbum = HashMap<(String, Option<String>), Vec<(i64, i64)>>;
        let mut par_album: ComptesFamillesParAlbum = HashMap::new();
        for (album, artiste, cluster, n) in lignes {
            par_album
                .entry((album, artiste))
                .or_default()
                .push((cluster, n));
        }

        let mut secondaires = HashMap::new();
        for ((album, artiste), mut comptes) in par_album {
            // Effectif décroissant, plus petit numéro de famille en cas
            // d'égalité — même tri que la dominante, pour que la deuxième
            // place soit stable d'une session à l'autre.
            comptes.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            if let Some(&(famille, n)) = comptes.get(1) {
                if n >= 2 {
                    let id = hacher(&artiste.unwrap_or_default(), &album) as i64;
                    secondaires.insert(id, famille);
                }
            }
        }
        Ok(secondaires)
    }

    /// Centroïde d'empreinte de chaque album — la moyenne des vecteurs de ses
    /// morceaux déjà analysés, clé [`hacher`]`(artiste, album) as i64`.
    ///
    /// Sert le mode Explorer → Anneau : trouver les albums sonorement proches
    /// d'un album donné revient à chercher parmi ces centroïdes avec
    /// [`rusty_music_analysis::chemin::voisins`] — pas de nouveau calcul de
    /// distance, pas de graphe à construire (un balayage linéaire sur
    /// quelques milliers d'albums coûte quelques millisecondes).
    pub fn album_embeddings(&self, model: &str) -> Result<Vec<(i64, Vec<f32>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.album, COALESCE(t.album_artist, t.artist), f.vector
               FROM features f JOIN tracks t ON t.id = f.track_id
              WHERE f.model = ?1 AND t.album IS NOT NULL",
        )?;
        struct Ligne {
            album: String,
            artiste: Option<String>,
            vector: Vec<u8>,
        }
        let lignes: Vec<Ligne> = stmt
            .query_map(params![model], |r| {
                Ok(Ligne {
                    album: r.get(0)?,
                    artiste: r.get(1)?,
                    vector: r.get(2)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;

        let mut cumul: HashMap<i64, (Vec<f32>, u32)> = HashMap::new();
        for l in lignes {
            let artiste = l.artiste.unwrap_or_default();
            let id = hacher(&artiste, &l.album) as i64;
            let v: Vec<f32> = l
                .vector
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            let e = cumul.entry(id).or_insert_with(|| (vec![0.0; v.len()], 0));
            for (a, b) in e.0.iter_mut().zip(&v) {
                *a += b;
            }
            e.1 += 1;
        }

        Ok(cumul
            .into_iter()
            .map(|(id, (somme, n))| {
                let n = (n.max(1)) as f32;
                (id, somme.into_iter().map(|x| x / n).collect())
            })
            .collect())
    }

    /// Chaque album connu, annoté de sa famille dominante et de sa date de
    /// sortie — voir [`AlbumNoeud`]. Croise [`Library::albums`] (identité et
    /// pochette), [`Library::familles_des_albums`] (famille) et
    /// [`Library::ordre_darrivee`] (date, groupée par le même hachage
    /// d'album) : les trois partagent déjà la même clé `(album,
    /// COALESCE(album_artist, artist))`, il n'y a qu'à les assembler.
    pub fn albums_annotes(&self, model: &str) -> Result<Vec<AlbumNoeud>> {
        let mut noeuds: HashMap<i64, AlbumNoeud> = HashMap::new();

        for row in self.albums(None)? {
            let artist = row.artist.unwrap_or_default();
            let id = hacher(&artist, &row.name) as i64;
            noeuds.insert(
                id,
                AlbumNoeud {
                    id,
                    name: row.name,
                    artist,
                    famille: None,
                    date: 0,
                    path: Some(row.path),
                    duree_ms: 0,
                },
            );
        }

        let mut stmt = self.conn.prepare(
            "SELECT album, COALESCE(album_artist, artist), SUM(COALESCE(duration_ms, 0))
               FROM tracks
              WHERE album IS NOT NULL
              GROUP BY album, COALESCE(album_artist, artist)",
        )?;
        let durees: Vec<(String, Option<String>, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<std::result::Result<_, _>>()?;
        for (album, artiste, duree) in durees {
            let artiste = artiste.unwrap_or_default();
            let id = hacher(&artiste, &album) as i64;
            if let Some(n) = noeuds.get_mut(&id) {
                n.duree_ms = duree;
            }
        }

        for (album, artiste, cluster) in self.familles_des_albums(model)? {
            let artiste = artiste.unwrap_or_default();
            let id = hacher(&artiste, &album) as i64;
            if let Some(n) = noeuds.get_mut(&id) {
                n.famille = Some(cluster);
            }
        }

        // La date la plus ancienne parmi les morceaux de l'album — même
        // arbitrage que l'arrivée sur la carte : voir `ArriveeBrute.album`,
        // qui utilise déjà exactement ce hachage pour grouper un disque.
        let mut dates: HashMap<i64, u32> = HashMap::new();
        for a in self.ordre_darrivee()? {
            let id = a.album as i64;
            dates
                .entry(id)
                .and_modify(|d| *d = (*d).min(a.date))
                .or_insert(a.date);
        }
        for (id, date) in dates {
            if let Some(n) = noeuds.get_mut(&id) {
                n.date = date;
            }
        }

        Ok(noeuds.into_values().collect())
    }

    /// Recherche d'albums pour la barre de recherche en mode Explorer →
    /// Anneau — même index plein texte que [`Library::search`], mais
    /// regroupé par album ; l'identité (`id`) est calculée ici, une fois pour
    /// toutes, jamais recalculée côté interface.
    pub fn search_albums(&self, q: &str, limit: i64) -> Result<Vec<(i64, String, String)>> {
        let requete = requete_fts(q);
        if requete.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT t.album, COALESCE(t.album_artist, t.artist)
               FROM tracks t
              WHERE t.album IS NOT NULL
                AND t.id IN (SELECT rowid FROM tracks_fts WHERE tracks_fts MATCH ?1)
              ORDER BY t.album COLLATE NOCASE
              LIMIT ?2",
        )?;
        let lignes: Vec<(String, Option<String>)> = stmt
            .query_map(params![requete, limit], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(lignes
            .into_iter()
            .map(|(album, artiste)| {
                let artiste = artiste.unwrap_or_default();
                let id = hacher(&artiste, &album) as i64;
                (id, album, artiste)
            })
            .collect())
    }

    /// Pour chaque artiste (`mb_album_artist_id`), sa famille sonique dominante
    /// — le cluster de la carte le plus représenté parmi ses morceaux projetés.
    ///
    /// Sert le filtre par famille du fil du mode Découvrir (`ui/js/`), qui
    /// réutilise la légende des familles du mode Explorer. Même arbitrage que
    /// [`Library::familles_des_albums`] : majorité, et le plus petit numéro à
    /// égalité pour que le classement soit stable d'une session à l'autre.
    pub fn familles_des_artistes(&self, model: &str) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.mb_album_artist_id, f.cluster, COUNT(*)
               FROM features f JOIN tracks t ON t.id = f.track_id
              WHERE f.model = ?1 AND f.cluster IS NOT NULL
                AND t.mb_album_artist_id IS NOT NULL AND t.mb_album_artist_id <> ''
              GROUP BY t.mb_album_artist_id, f.cluster",
        )?;
        let lignes: Vec<(String, i64, i64)> = stmt
            .query_map(params![model], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<std::result::Result<_, _>>()?;

        let mut par_artiste: HashMap<String, (i64, i64)> = HashMap::new();
        for (mbid, cluster, n) in lignes {
            let e = par_artiste.entry(mbid).or_insert((cluster, n));
            if n > e.1 || (n == e.1 && cluster < e.0) {
                *e = (cluster, n);
            }
        }
        Ok(par_artiste
            .into_iter()
            .map(|(mbid, (cluster, _))| (mbid, cluster))
            .collect())
    }

    /// Le genre le plus précis de chaque morceau analysé, résolu par la même
    /// hiérarchie que [`Library::familles`] (album MusicBrainz, puis artiste,
    /// puis tag du fichier) — mais **par morceau**, pas agrégé par cluster.
    ///
    /// Sert à ancrer un regroupement par genre plutôt que par k-means : un
    /// morceau dont le genre est connu n'a pas besoin d'être placé par
    /// distance, il appartient déjà à sa famille.
    pub fn genres_resolus(&self, model: &str) -> Result<HashMap<i64, String>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.track_id, t.mb_artist_id, t.album, t.genre
               FROM features f JOIN tracks t ON t.id = f.track_id
              WHERE f.model = ?1",
        )?;
        let pistes: Vec<PisteNommage> = stmt
            .query_map(params![model], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<std::result::Result<_, _>>()?;

        let par_artiste = self.mb_genres("artist", VOTES_MINIMUM)?;
        let par_album = self.mb_genres("release-group", VOTES_MINIMUM)?;
        let albums = self.mb_albums()?;

        let mut resolus = HashMap::new();
        for (track_id, artiste, album, tag) in &pistes {
            if let Some(genre) = genres_du_morceau(
                artiste.as_deref(),
                album.as_deref(),
                tag.as_deref(),
                &albums,
                &par_album,
                &par_artiste,
            )
            .into_iter()
            .next()
            {
                // En minuscules : MusicBrainz les rend déjà ainsi, mais le
                // repli sur le tag du fichier ne le garantit pas, et
                // `cluster::VOCABULAIRE` compare sans normaliser la casse.
                resolus.insert(*track_id, genre.to_lowercase());
            }
        }
        Ok(resolus)
    }

    /// Les artistes les mieux représentés d'une famille. Diagnostic : c'est en
    /// les lisant qu'on juge si l'étiquette dit vrai.
    pub fn artistes_de_famille(&self, model: &str, cluster: i64, n: usize) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.artist FROM features f JOIN tracks t ON t.id = f.track_id
              WHERE f.model = ?1 AND f.cluster = ?2 AND t.artist IS NOT NULL AND t.artist <> ''
              GROUP BY t.artist ORDER BY COUNT(*) DESC LIMIT ?3",
        )?;
        let noms: Vec<String> = stmt
            .query_map(params![model, cluster, n as i64], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(noms)
    }

    /// Morceaux dont le genre résolu ([`genres_du_morceau`]) ne figure pas
    /// parmi les [`GENRES_DOMINANTS`] les plus représentés de leur famille
    /// sonique (cluster de la carte) — le tag dit une chose, l'empreinte en
    /// dit une autre. Un signal à vérifier, pas une certitude : deux genres
    /// voisins (« folk » / « folk-pop ») divergent tout en étant tous deux
    /// justes.
    ///
    /// Les familles trop petites pour qu'un « genre dominant » veuille dire
    /// quelque chose sont ignorées ([`PLANCHER_ABSOLU`], déjà utilisé pour
    /// nommer les familles).
    pub fn genres_suspects(&self, model: &str) -> Result<Vec<(i64, String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.cluster, f.track_id, t.mb_artist_id, t.album, t.genre,
                    COALESCE(t.title, t.path), COALESCE(t.artist, '?')
               FROM features f JOIN tracks t ON t.id = f.track_id
              WHERE f.model = ?1",
        )?;
        struct Ligne {
            cluster: i64,
            track_id: i64,
            artiste_mbid: Option<String>,
            album: Option<String>,
            tag: Option<String>,
            titre: String,
            artiste: String,
        }
        let pistes: Vec<Ligne> = stmt
            .query_map(params![model], |r| {
                Ok(Ligne {
                    cluster: r.get(0)?,
                    track_id: r.get(1)?,
                    artiste_mbid: r.get(2)?,
                    album: r.get(3)?,
                    tag: r.get(4)?,
                    titre: r.get(5)?,
                    artiste: r.get(6)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;

        let par_artiste = self.mb_genres("artist", VOTES_MINIMUM)?;
        let par_album = self.mb_genres("release-group", VOTES_MINIMUM)?;
        let albums = self.mb_albums()?;

        // Genre résolu par morceau, et effectifs par (cluster, genre) — même
        // arbitrage que `familles`, gardé par morceau ici au lieu d'être
        // sommé tout de suite : il faut le comparer au dominant de son
        // cluster, pas seulement le compter dedans.
        let mut genre_du_morceau: HashMap<i64, String> = HashMap::new();
        let mut cumul: HashMap<(i64, String), i64> = HashMap::new();
        let mut taille_cluster: HashMap<i64, i64> = HashMap::new();
        for p in &pistes {
            *taille_cluster.entry(p.cluster).or_default() += 1;
            let genres = genres_du_morceau(
                p.artiste_mbid.as_deref(),
                p.album.as_deref(),
                p.tag.as_deref(),
                &albums,
                &par_album,
                &par_artiste,
            );
            // `GENRES_PAR_ENTITE = 1` : au plus un genre par morceau déjà —
            // pas d'ambiguïté à choisir lequel comparer.
            if let Some(g) = genres.into_iter().next() {
                genre_du_morceau.insert(p.track_id, g.clone());
                *cumul.entry((p.cluster, g)).or_default() += 1;
            }
        }

        // Les `GENRES_DOMINANTS` premiers de chaque cluster, par effectif.
        let mut par_cluster: HashMap<i64, Vec<(String, i64)>> = HashMap::new();
        for ((cluster, genre), n) in cumul {
            par_cluster.entry(cluster).or_default().push((genre, n));
        }
        for v in par_cluster.values_mut() {
            v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            v.truncate(GENRES_DOMINANTS);
        }

        let mut suspects = Vec::new();
        for p in &pistes {
            if taille_cluster[&p.cluster] < PLANCHER_ABSOLU {
                continue;
            }
            let Some(genre) = genre_du_morceau.get(&p.track_id) else {
                continue;
            };
            let dominants = par_cluster
                .get(&p.cluster)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            if dominants.iter().any(|(g, _)| g == genre) {
                continue;
            }
            let etiquette = dominants
                .iter()
                .map(|(g, _)| g.as_str())
                .collect::<Vec<_>>()
                .join(" · ");
            suspects.push((
                p.track_id,
                format!("{} — {}", p.artiste, p.titre),
                genre.clone(),
                etiquette,
            ));
        }
        Ok(suspects)
    }

    /// Ce que la bibliothèque sait de chaque morceau pour filtrer une playlist
    /// demandée en texte libre (`rusty_music_lama::filtres_playlist`) : année, durée,
    /// tempo, énergie, popularité (rang percentile) et genres résolus — même
    /// arbitrage que [`Self::stats_genres`], donc mêmes noms que ceux qu'on
    /// donne au LLM ([`Self::vocabulaire_genres`]). Une ligne par morceau, y
    /// compris ceux sans descripteurs (valeurs `None`).
    pub fn caracteristiques_pistes(&self) -> Result<Vec<CaracteristiquesPiste>> {
        let par_artiste = self.mb_genres("artist", VOTES_MINIMUM)?;
        let par_album = self.mb_genres("release-group", VOTES_MINIMUM)?;
        let albums = self.mb_albums()?;

        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.artist, t.mb_artist_id, t.album, t.genre, t.year, t.duration_ms,
                    d.bpm, d.energy, tp.relative,
                    CAST((strftime('%s','now') - e.dernier) / 86400 AS INTEGER)
               FROM tracks t
               LEFT JOIN descriptors d ON d.track_id = t.id
               LEFT JOIN track_popularite tp ON tp.track_id = t.id
               LEFT JOIN (SELECT chemin, MAX(le) AS dernier FROM ecoute GROUP BY chemin) e
                      ON e.chemin = t.path",
        )?;
        let lignes = stmt
            .query_map([], |r| {
                let mbid: Option<String> = r.get(2)?;
                let album: Option<String> = r.get(3)?;
                let tag: Option<String> = r.get(4)?;
                let annee: Option<i64> = r.get(5)?;
                Ok((
                    CaracteristiquesPiste {
                        id: r.get(0)?,
                        artiste: r.get(1)?,
                        annee: annee.and_then(|a| i32::try_from(a).ok()),
                        duree_ms: r.get(6)?,
                        bpm: r.get(7)?,
                        energie: r.get(8)?,
                        popularite: r.get(9)?,
                        // Jamais négatif : une horloge reculée ne fait pas
                        // une écoute future.
                        jours_depuis_ecoute: r
                            .get::<_, Option<i64>>(10)?
                            .map(|j| u32::try_from(j.max(0)).unwrap_or(u32::MAX)),
                        genres: Vec::new(),
                    },
                    mbid,
                    album,
                    tag,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(lignes
            .into_iter()
            .map(|(mut p, mbid, album, tag)| {
                p.genres = genres_du_morceau(
                    mbid.as_deref(),
                    album.as_deref(),
                    tag.as_deref(),
                    &albums,
                    &par_album,
                    &par_artiste,
                )
                .into_iter()
                .map(|g| g.to_lowercase())
                .collect();
                p
            })
            .collect())
    }

    /// Les `n` genres les plus représentés (minuscules, sans le fourre-tout
    /// « — ») : le vocabulaire **réel** qu'on donne au LLM pour qu'« un truc
    /// jazzy » devienne un genre qui existe dans cette bibliothèque.
    pub fn vocabulaire_genres(&self, n: usize) -> Result<Vec<String>> {
        let mut vus = std::collections::HashSet::new();
        Ok(self
            .stats_genres()?
            .into_iter()
            .map(|(g, _)| g.to_lowercase())
            .filter(|g| g != "—" && !g.is_empty() && vus.insert(g.clone()))
            .take(n)
            .collect())
    }

    /// Les paramètres du calcul de la carte, une clé absente valant sa
    /// valeur par défaut ([`ParametresCarte::default`]) — voir la table
    /// `parametres_carte`.
    pub fn parametres_carte(&self) -> Result<ParametresCarte> {
        let mut stmt = self
            .conn
            .prepare("SELECT cle, valeur FROM parametres_carte")?;
        let lignes: HashMap<String, f64> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))?
            .collect::<std::result::Result<_, _>>()?;

        let d = ParametresCarte::default();
        let get = |cle: &str, defaut: f64| lignes.get(cle).copied().unwrap_or(defaut);
        Ok(ParametresCarte {
            perplexite: get("perplexite", d.perplexite as f64) as f32,
            epoques: get("epoques", d.epoques as f64) as usize,
            familles: get("familles", d.familles as f64) as usize,
            iterations_kmeans: get("iterations_kmeans", d.iterations_kmeans as f64) as usize,
            densite_noyau: get("densite_noyau", d.densite_noyau),
            densite_resolution: get("densite_resolution", d.densite_resolution as f64) as usize,
            densite_bandes: get("densite_bandes", d.densite_bandes as f64) as usize,
        })
    }

    /// Change un paramètre de la carte. `cle` doit être un des sept champs
    /// de [`ParametresCarte`] — la validation du nom reste à l'appelant
    /// (commande Tauri), cette méthode ne fait qu'écrire.
    pub fn set_parametre_carte(&self, cle: &str, valeur: f64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO parametres_carte(cle, valeur) VALUES (?1, ?2)
             ON CONFLICT(cle) DO UPDATE SET valeur = excluded.valeur",
            params![cle, valeur],
        )?;
        Ok(())
    }

    /// Le vocabulaire des familles par genre, dans l'ordre où il a été
    /// écrit. Table vide = [`VOCABULAIRE_DEFAUT`] — même convention que
    /// [`Library::parametres_carte`].
    pub fn vocabulaire_familles(&self) -> Result<Vec<(String, Vec<String>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT nom, genre FROM vocabulaire_familles ORDER BY rowid")?;
        let lignes: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;

        if lignes.is_empty() {
            return Ok(VOCABULAIRE_DEFAUT
                .iter()
                .map(|(nom, genres)| {
                    (
                        nom.to_string(),
                        genres.iter().map(|g| g.to_string()).collect(),
                    )
                })
                .collect());
        }

        // Regroupe par nom en gardant l'ordre de première apparition — celui
        // des `rowid`, donc celui de la dernière écriture.
        let mut ordre: Vec<String> = Vec::new();
        let mut par_nom: HashMap<String, Vec<String>> = HashMap::new();
        for (nom, genre) in lignes {
            if !par_nom.contains_key(&nom) {
                ordre.push(nom.clone());
            }
            par_nom.entry(nom).or_default().push(genre);
        }
        Ok(ordre
            .into_iter()
            .map(|nom| {
                let genres = par_nom.remove(&nom).expect("clé vue à l'instant");
                (nom, genres)
            })
            .collect())
    }

    /// Remplace le vocabulaire des familles en base, en bloc.
    ///
    /// Une famille sans aucun genre est écartée : elle ne pourrait jamais
    /// ancrer un morceau, ce serait une entrée morte dans le réglage. Passer
    /// une liste vide restaure les valeurs par défaut — la table vidée,
    /// [`Library::vocabulaire_familles`] retombe sur [`VOCABULAIRE_DEFAUT`].
    pub fn definir_vocabulaire_familles(
        &mut self,
        vocabulaire: &[(String, Vec<String>)],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM vocabulaire_familles", [])?;
        {
            let mut inserer =
                tx.prepare("INSERT INTO vocabulaire_familles (nom, genre) VALUES (?1, ?2)")?;
            for (nom, genres) in vocabulaire {
                if genres.is_empty() {
                    continue;
                }
                for genre in genres {
                    let genre = genre.trim().to_lowercase();
                    if !genre.is_empty() {
                        inserer.execute(params![nom.trim(), genre])?;
                    }
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Écrit les positions et les familles d'un coup.
    pub fn update_map(&self, model: &str, points: &[(i64, f32, f32, i64)]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "UPDATE features SET x=?3, y=?4, cluster=?5 WHERE track_id=?1 AND model=?2",
            )?;
            for (id, x, y, c) in points {
                stmt.execute(params![id, model, x, y, c])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Écrit le genre gagnant du vote CLAP-texte, un par morceau — calculé en
    /// lot par `crates/analysis` (voir `features.texte_label` dans
    /// `migrate`). `None` efface une entrée sans phrase gagnante exploitable.
    pub fn update_texte_labels(&self, model: &str, labels: &[(i64, Option<String>)]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt =
                tx.prepare("UPDATE features SET texte_label=?3 WHERE track_id=?1 AND model=?2")?;
            for (id, label) in labels {
                stmt.execute(params![id, model, label])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// La carte complète, prête à dessiner : positions et étiquettes.
    ///
    /// Une seule requête plutôt qu'un aller-retour par point — à 27 000
    /// morceaux, l'interface les charge tous d'un coup et n'y revient qu'à la
    /// demande de l'utilisateur.
    ///
    /// `year` ne vient **pas** de la seule colonne `tracks.year` : c'est la
    /// même date, à la même fiabilité, que [`Self::ordre_darrivee`] utilise
    /// pour le placement (MusicBrainz avant tag avant médiane album/artiste).
    /// Sans ce repli, l'étiquette d'un morceau le disait sans année alors que
    /// sa position sur la carte avait bien été fondée sur une date connue —
    /// mesuré : 643 morceaux sans tag contre 314 vraiment sans aucune date.
    /// `None` reste réservé à ces 314-là (source `ingestion`).
    pub fn map_view(&self, model: &str) -> Result<Vec<MapPoint>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.path, f.x, f.y, COALESCE(f.cluster, -1),
                    t.title, t.artist, t.album_artist, t.album, t.track_no, t.year, t.duration_ms,
                    d.bpm, d.energy, tp.relative
               FROM features f
               JOIN tracks t ON t.id = f.track_id
               LEFT JOIN descriptors d ON d.track_id = t.id
               LEFT JOIN track_popularite tp ON tp.track_id = t.id
              WHERE f.model = ?1 AND f.x IS NOT NULL
              ORDER BY t.id",
        )?;
        let mut rows = stmt
            .query_map(params![model], |r| {
                Ok(MapPoint {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    x: r.get(2)?,
                    y: r.get(3)?,
                    cluster: r.get(4)?,
                    title: r.get(5)?,
                    artist: r.get(6)?,
                    album_artist: r.get(7)?,
                    album: r.get(8)?,
                    track_no: r.get(9)?,
                    year: r.get(10)?,
                    duration_ms: r.get(11)?,
                    bpm: r.get(12)?,
                    energy: r.get(13)?,
                    popularite: r.get(14)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let annees_fiables: HashMap<i64, Option<i64>> = self
            .ordre_darrivee()?
            .into_iter()
            .map(|a| {
                let annee = (a.source != "ingestion").then_some((a.date / 10_000) as i64);
                (a.track_id, annee)
            })
            .collect();
        for p in &mut rows {
            if let Some(&annee) = annees_fiables.get(&p.id) {
                p.year = annee;
            }
        }
        Ok(rows)
    }

    /// Positions sur la carte pour un modèle donné — ce que lira le module 2.
    /// L'ordre d'arrivée des morceaux, pour le placement chronologique.
    ///
    /// Rend `(track_id, date AAAAMMJJ, source, clé d'album, disque, piste)`,
    /// **déjà trié** : l'ordre lexicographique de ce n-uplet est l'ordre du
    /// peuplement.
    ///
    /// L'échelle de datation, du plus fiable au moins :
    ///
    /// | source | d'où | ce qu'elle vaut |
    /// |---|---|---|
    /// | `musicbrainz` | `first_release_date` du release-group | la date de l'œuvre, pas du pressage — corrige les rééditions |
    /// | `tag` | `tracks.year` | date de l'édition ; 26 493 morceaux sur 27 044 |
    /// | `album` | médiane des frères datés du même album | +56 mesurés |
    /// | `artiste` | médiane des morceaux datés de l'artiste | +23 mesurés |
    /// | `ingestion` | `tracks.added_at` | 472 morceaux, dont 504 n'ont aucun MBID : rien ne les sauvera |
    ///
    /// Les morceaux sans date ne sont **ni écartés ni maquillés** : ils portent
    /// leur date d'entrée dans la bibliothèque et `date_source = 'ingestion'`,
    /// pour que la carte puisse les rendre autrement et qu'une correction
    /// ultérieure sache lesquels rejouer.
    pub fn ordre_darrivee(&self) -> Result<Vec<ArriveeBrute>> {
        // 1. Ce que les tags disent.
        let mut stmt = self.conn.prepare(
            // `GROUP BY t.id` n'est pas une précaution : un artiste peut avoir
            // plusieurs release-groups au même titre normalisé — un album et sa
            // réédition, une version live homonyme — et la jointure rendait
            // alors **plusieurs lignes par morceau**. Mesuré : 28 363 arrivées
            // pour 27 044 morceaux. `MIN` retient la plus ancienne, ce qui est
            // précisément la date d'œuvre qu'on cherche.
            "SELECT t.id, t.year, t.album, t.album_artist, t.artist, t.track_no, t.added_at,
                    MIN(r.first_release_date), MIN(r.secondary_types)
               FROM tracks t
               LEFT JOIN mb_release_groups r
                      ON r.artist_mbid = t.mb_album_artist_id
                     AND r.title_norm = titre_normalise(t.album)
              GROUP BY t.id
              ORDER BY t.id",
        )?;
        struct Ligne {
            id: i64,
            annee: Option<i64>,
            album: String,
            artiste: String,
            piste: i64,
            ajoute: i64,
            mb_date: Option<String>,
            compilation: bool,
        }
        let lignes: Vec<Ligne> = stmt
            .query_map([], |r| {
                let album: Option<String> = r.get(2)?;
                let album_artiste: Option<String> = r.get(3)?;
                let artiste: Option<String> = r.get(4)?;
                let types: Option<String> = r.get(8)?;
                Ok(Ligne {
                    id: r.get(0)?,
                    annee: r.get(1)?,
                    album: album.unwrap_or_default(),
                    artiste: album_artiste.or(artiste).unwrap_or_default(),
                    piste: r.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    ajoute: r.get(6)?,
                    mb_date: r.get(7)?,
                    compilation: types
                        .as_deref()
                        .is_some_and(|t| t.to_lowercase().contains("compilation")),
                })
            })?
            .collect::<std::result::Result<_, _>>()?;

        // 2. Les médianes de secours, par album puis par artiste.
        let mut par_album: HashMap<&str, Vec<i64>> = HashMap::new();
        let mut par_artiste: HashMap<&str, Vec<i64>> = HashMap::new();
        for l in &lignes {
            if let Some(a) = l.annee.filter(|a| (1900..=2100).contains(a)) {
                if !l.album.is_empty() {
                    par_album.entry(l.album.as_str()).or_default().push(a);
                }
                if !l.artiste.is_empty() {
                    par_artiste.entry(l.artiste.as_str()).or_default().push(a);
                }
            }
        }
        let mediane = |v: &mut Vec<i64>| -> i64 {
            v.sort_unstable();
            v[v.len() / 2]
        };
        let med_album: HashMap<&str, i64> = par_album
            .into_iter()
            .map(|(k, mut v)| (k, mediane(&mut v)))
            .collect();
        let med_artiste: HashMap<&str, i64> = par_artiste
            .into_iter()
            .map(|(k, mut v)| (k, mediane(&mut v)))
            .collect();

        let mut sortie: Vec<ArriveeBrute> = lignes
            .iter()
            .map(|l| {
                // Une compilation date d'elle-même, pas des œuvres qu'elle
                // rassemble : sa date de release-group ne vaut rien ici.
                let mb = if l.compilation {
                    None
                } else {
                    l.mb_date.as_deref()
                };
                let (date, source) = if let Some(d) = mb.and_then(date_iso_vers_cle) {
                    (d, "musicbrainz")
                } else if let Some(a) = l.annee.filter(|a| (1900..=2100).contains(a)) {
                    (a as u32 * 10_000, "tag")
                } else if let Some(&a) = med_album.get(l.album.as_str()) {
                    (a as u32 * 10_000, "album")
                } else if let Some(&a) = med_artiste.get(l.artiste.as_str()) {
                    (a as u32 * 10_000, "artiste")
                } else {
                    (epoch_vers_cle(l.ajoute), "ingestion")
                };
                ArriveeBrute {
                    track_id: l.id,
                    date,
                    source: source.to_string(),
                    // Un album arrive **en bloc** : sans ce regroupement, ses
                    // pistes s'éparpilleraient parmi les 1 341 arrivées d'une
                    // même année et chacune irait fonder ailleurs.
                    album: hacher(&l.artiste, &l.album),
                    piste: l.piste.clamp(0, 9999) as u16,
                }
            })
            .collect();

        sortie.sort_by(|a, b| {
            a.date
                .cmp(&b.date)
                .then_with(|| a.album.cmp(&b.album))
                .then_with(|| a.piste.cmp(&b.piste))
                .then_with(|| a.track_id.cmp(&b.track_id))
        });
        Ok(sortie)
    }

    /// Quand la projection a été calculée pour la dernière fois, en epoch.
    ///
    /// Sert à savoir si un dérivé de la carte — les tuiles vectorielles, par
    /// exemple — a été fabriqué avant ou après le dernier recalcul. `None`
    /// quand rien n'est encore projeté.
    pub fn derniere_projection(&self, model: &str) -> Result<Option<i64>> {
        let v: Option<i64> = self.conn.query_row(
            "SELECT MAX(computed_at) FROM features WHERE model = ?1 AND x IS NOT NULL",
            params![model],
            |r| r.get(0),
        )?;
        Ok(v)
    }

    pub fn map_points(&self, model: &str) -> Result<Vec<(i64, f32, f32, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT track_id, x, y, cluster FROM features
              WHERE model = ?1 AND x IS NOT NULL
              ORDER BY track_id",
        )?;
        let rows = stmt
            .query_map(params![model], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

/// Paramètres du calcul de la carte — projection t-SNE et clustering
/// k-means. Rien pour les empreintes elles-mêmes : le modèle CLAP importé
/// est figé au moment du build (`crates/analysis/src/encodeur.rs`), il n'y a
/// pas de bouton à tourner de ce côté-là.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ParametresCarte {
    /// Voisinage que t-SNE cherche à préserver. Doit rester bien en dessous
    /// du nombre de morceaux — `projeter` s'en charge, ce n'est pas à
    /// l'appelant d'y penser.
    pub perplexite: f32,
    /// Passes de descente de gradient. Plus haut stabilise le dessin, plus
    /// lentement.
    pub epoques: usize,
    /// Nombre de familles (k de k-means), sur la carte comme dans le rail
    /// « Familles ».
    pub familles: usize,
    /// Passes de k-means++. Au-delà d'une poignée de dizaines, les centres
    /// ont déjà cessé de bouger sur une bibliothèque de cette taille.
    pub iterations_kmeans: usize,
    /// Écart-type du noyau gaussien de la nappe de densité — voir
    /// [`crate::density::ParametresDensite`], que ce champ (et les deux
    /// suivants) alimente directement.
    pub densite_noyau: f64,
    /// Cellules par côté de la grille de densité.
    pub densite_resolution: usize,
    /// Bandes par nappe de densité.
    pub densite_bandes: usize,
}

impl Default for ParametresCarte {
    /// Les valeurs mesurées jusqu'ici, gardées à l'identique : changer de
    /// défaut ici changerait le dessin de la carte de quiconque n'a jamais
    /// ouvert ce réglage.
    fn default() -> Self {
        let d = crate::density::ParametresDensite::default();
        Self {
            perplexite: 30.0,
            epoques: 1000,
            familles: 12,
            iterations_kmeans: 50,
            densite_noyau: d.noyau,
            densite_resolution: d.resolution,
            densite_bandes: d.bandes,
        }
    }
}

impl ParametresCarte {
    /// Les trois champs de densité, sous la forme qu'attend
    /// [`crate::density::calculer`].
    pub fn parametres_densite(&self) -> crate::density::ParametresDensite {
        crate::density::ParametresDensite {
            noyau: self.densite_noyau,
            resolution: self.densite_resolution,
            bandes: self.densite_bandes,
        }
    }
}

/// Vocabulaire par défaut des familles par genre — voir
/// [`Library::vocabulaire_familles`]. Vit ici, à côté de `ParametresCarte`,
/// et non dans `rusty-music-analysis` : c'est une donnée de configuration de
/// la bibliothèque (réglable, persistée), pas un algorithme ; `analysis` en
/// dépend, pas l'inverse.
///
/// **Mesuré sur une bibliothèque réelle avant d'être écrit** — les genres
/// les mieux votés d'au moins cinq artistes (`mb_genres`) : `hip hop` (130
/// artistes), `jazz` (81), `alternative rock` (67), `electronic` (52),
/// `rock` (50), `folk` (40), `reggae` (21), `pop` (21), `chanson française`
/// (19), `funk` (17), `soul` (15), `blues` (15), `nu metal` (14), `dub`
/// (14), etc. Une bibliothèque différente peut vouloir une autre liste —
/// c'est justement pour ça qu'elle est réglable plutôt que figée dans le
/// code.
const VOCABULAIRE_DEFAUT: &[(&str, &[&str])] = &[
    (
        "Rock",
        &[
            "rock",
            "alternative rock",
            "hard rock",
            "psychedelic rock",
            "folk rock",
            "blues rock",
            "pop rock",
            "progressive rock",
            "punk",
            "grunge",
            "indie rock",
            "post-punk",
            "garage rock",
        ],
    ),
    (
        "Metal",
        &[
            "metal",
            "nu metal",
            "alternative metal",
            "thrash metal",
            "industrial metal",
            "groove metal",
            "heavy metal",
            "death metal",
            "black metal",
            "doom metal",
        ],
    ),
    ("Hip Hop", &["hip hop", "boom bap", "rap", "trap"]),
    (
        "Électronique",
        &[
            "electronic",
            "drum and bass",
            "big beat",
            "ambient",
            "downtempo",
            "trip hop",
            "house",
            "techno",
            "idm",
            "electronica",
            "synthwave",
        ],
    ),
    ("Jazz", &["jazz", "acid jazz", "fusion", "bebop", "swing"]),
    ("Reggae", &["reggae", "dub", "dancehall", "ska"]),
    ("Soul · Funk", &["funk", "soul", "r&b", "disco", "motown"]),
    (
        "Folk",
        &[
            "folk",
            "celtic",
            "bluegrass",
            "country",
            "americana",
            "singer-songwriter",
        ],
    ),
    (
        "Chanson",
        &["chanson française", "chanson", "variété française"],
    ),
    (
        "Classique",
        &["classical", "baroque", "opera", "contemporary classical"],
    ),
    ("Pop", &["pop", "synthpop", "dance pop", "indie pop"]),
    ("Monde", &["afrobeat", "world", "latin", "flamenco"]),
];

/// Un album annoté pour le mode Explorer → Anneau : identité, famille
/// dominante, date de sortie, chemin d'une piste pour la pochette.
///
/// `id` est [`hacher`]`(artiste, album)`, casté en `i64` — le même hachage
/// que celui qui regroupe déjà les pistes d'un disque dans
/// [`Library::ordre_darrivee`] ; aucune autre identité d'album n'existe dans
/// le schéma (voir [`Library::albums`], une simple agrégation SQL sans id
/// stable). Voir [`Library::albums_annotes`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct AlbumNoeud {
    pub id: i64,
    pub name: String,
    pub artist: String,
    /// Cluster dominant, `None` tant qu'aucun morceau de l'album n'est
    /// projeté (voir [`Library::familles_des_albums`]).
    pub famille: Option<i64>,
    /// AAAAMMJJ, le plus ancien parmi les morceaux de l'album — voir la table
    /// de fiabilité de [`Library::ordre_darrivee`].
    pub date: u32,
    pub path: Option<String>,
    /// Somme des durées de ses morceaux — sert l'épaisseur de l'arc d'album
    /// sur le second anneau du mode Explorer → Anneau. Choisie plutôt que la
    /// popularité générale : toujours disponible, même sans passe
    /// popularité, voir `docs/popularite.md`.
    pub duree_ms: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MapPoint {
    pub id: i64,
    pub path: String,
    pub x: f32,
    pub y: f32,
    pub cluster: i64,
    pub title: Option<String>,
    pub artist: Option<String>,
    /// Artiste de regroupement (`tracks.album_artist`), distinct de `artist`
    /// qui peut lister un featuring entier (« X feat. Y »). C'est ce champ
    /// qu'il faut utiliser pour identifier *un* artiste, pas `artist` — voir
    /// le commentaire sur `mb_album_artist_id` dans le schéma.
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub track_no: Option<i64>,
    /// La meilleure date connue, pas la seule colonne `tracks.year` — voir
    /// le repli documenté sur [`Library::map_view`]. `None` seulement quand
    /// aucune source ne donne de date, pas seulement le tag.
    pub year: Option<i64>,
    pub duration_ms: Option<i64>,
    /// Descripteurs mesurés. Absents tant que `rusty-music descripteurs` n'est
    /// pas passé — la carte doit savoir colorer sans eux.
    pub bpm: Option<f32>,
    pub energy: Option<f32>,
    /// Popularité générale, rang percentile `0..1` dans la bibliothèque
    /// (`track_popularite.relative`). Absente tant que les passes `enrich` +
    /// `popularité` ne sont pas passées, ou si l'entité est inconnue de
    /// ListenBrainz et Deezer. La carte s'en sert pour ancrer les artistes les
    /// plus connus sur les monuments (`crate::ancrage`, dans `rusty_music_carto`).
    pub popularite: Option<f64>,
}

/// Un morceau et sa place dans l'ordre du peuplement.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ArriveeBrute {
    pub track_id: i64,
    /// AAAAMMJJ. Le mois et le jour valent 00 quand on ne les connaît pas.
    pub date: u32,
    pub source: String,
    /// Hachage stable de (artiste, album) : regroupe les pistes d'un disque.
    pub album: u64,
    pub piste: u16,
}

/// Un instant epoch vers la même clé AAAAMMJJ, sans dépendance de date : on
/// n'a besoin que d'un ordre, et l'arithmétique civile grégorienne tient en
/// quinze lignes (algorithme de Howard Hinnant).
fn epoch_vers_cle(secondes: i64) -> u32 {
    let jours = secondes.div_euclid(86_400) + 719_468;
    let ere = jours.div_euclid(146_097);
    let doe = jours - ere * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + ere * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y.clamp(1900, 2100) as u32) * 10_000 + (m as u32) * 100 + (d as u32)
}

/// Une piste vue par le nommage des familles : sa famille, son artiste
/// MusicBrainz, son album, et le genre inscrit dans le fichier.
type PisteNommage = (i64, Option<String>, Option<String>, Option<String>);

/// `(groupe, artiste MusicBrainz, album, tag de fichier, genre gagnant
/// CLAP-texte)` — l'entrée du vote à trois sources, voir
/// [`Library::nommer_par_groupe`].
type PisteVote = (
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Une famille nommée par vote entre trois sources indépendantes —
/// MusicBrainz, le vocabulaire CLAP-texte et Last.fm. Voir
/// `docs/nommage-familles.md`.
///
/// `fiable` distingue un nom sur lequel au moins deux sources s'accordent
/// (ou qu'une critique CritiqueBrainz est venue confirmer) d'un simple repli
/// sur MusicBrainz seul quand les trois divergent — c'est ce dernier cas que
/// l'inspecteur affiche comme « nom incertain », avec les trois propositions
/// visibles plutôt qu'un choix silencieux.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct VoteFamille {
    pub cluster: i64,
    pub effectif: i64,
    pub fiable: bool,
    /// Ce que la carte, l'anneau et la frise affichent — toujours renseigné,
    /// même quand `fiable` est faux (repli sur le nom MusicBrainz).
    pub nom_affiche: String,
    pub musicbrainz: String,
    pub clap_texte: String,
    pub lastfm: String,
}

/// Combien des genres les mieux représentés d'une famille comptent comme
/// « dominants », pour [`Library::genres_suspects`]. Plus d'un seul : une
/// famille mêle légitimement des genres voisins (« Reggae · Afrobeat ») sans
/// qu'aucun morceau n'y soit pour autant suspect.
const GENRES_DOMINANTS: usize = 3;

/// Sous ce poids, un tag Last.fm est trop marginal pour compter — l'échelle
/// (0-100) n'est pas celle des votes MusicBrainz, un plancher bas suffit à
/// écarter le bruit occasionnel sans perdre les tags réels.
const LASTFM_POIDS_MINIMUM: u32 = 5;

/// Sous ce plancher, un genre décrit une poche et non une famille : son score
/// serait tiré par le hasard de quelques morceaux. Le seuil est relatif à la
/// taille de la famille, plus un minimum absolu pour les petites bibliothèques.
const PLANCHER_RELATIF: f64 = 0.01;

const PLANCHER_ABSOLU: i64 = 5;

/// Nomme chaque famille par ses genres, à partir des comptes bruts.
///
/// `effectifs` donne `(famille, nombre de morceaux)`, du plus grand au plus
/// petit ; `comptes` donne `(famille, genre, nombre de morceaux de ce genre)`.
///
/// **Ni le genre le plus fréquent, ni le plus caractéristique.** Le plus
/// fréquent ne distingue rien : « Rock » domine six des douze familles de la
/// bibliothèque de test. Le plus caractéristique — celui dont la part dans la
/// famille dépasse le plus sa part dans la bibliothèque — décrit une poche
/// marginale : il nommait « Ska Rock · Latin » une famille de 4 321 morceaux
/// menée par Bob Marley, Femi Kuti et James Brown, sur la foi de 52 morceaux.
///
/// Le score retenu, `part × log₂(sur-représentation)`, exige les deux : le
/// genre doit peser dans la famille **et** y être plus présent qu'ailleurs. La
/// même famille devient « Reggae · Pop ».
pub(super) fn nommer_les_familles(
    effectifs: &[(i64, i64)],
    comptes: &[(i64, String, i64)],
) -> Vec<(i64, String, i64)> {
    nommer_les_familles_option(effectifs, comptes)
        .into_iter()
        .enumerate()
        .map(|(rang, (famille, nom, effectif))| {
            (
                famille,
                nom.unwrap_or_else(|| format!("Famille {}", rang + 1)),
                effectif,
            )
        })
        .collect()
}

/// Le cœur de [`nommer_les_familles`], sans son repli « Famille N ».
///
/// Le repli est le bon choix pour une légende, qui doit toujours montrer un
/// nom — mais c'est un aveu d'absence, pas une proposition, et le vote à
/// trois sources ([`Library::nommer_par_groupe`]) ne doit jamais le traiter
/// comme telle : sans cette distinction, deux sources qui n'ont *toutes les
/// deux* rien à dire tombent sur le même « Famille N » (même rang, même
/// bibliothèque de familles) et semblent s'accorder — un faux consensus qui
/// a fait disparaître les vrais noms MusicBrainz derrière un désaccord
/// fantôme, mesuré en développant ce chantier.
fn nommer_les_familles_option(
    effectifs: &[(i64, i64)],
    comptes: &[(i64, String, i64)],
) -> Vec<(i64, Option<String>, i64)> {
    // La population de référence, c'est l'ensemble des morceaux classés : la
    // sur-représentation se mesure contre ce que la carte montre, pas contre
    // une bibliothèque dont une partie n'est pas encore analysée.
    let mut global: HashMap<&str, i64> = HashMap::new();
    let mut total = 0i64;
    for (_, genre, n) in comptes {
        *global.entry(genre.as_str()).or_default() += n;
        total += n;
    }

    let mut par_famille: HashMap<i64, Vec<(&str, i64)>> = HashMap::new();
    for (famille, genre, n) in comptes {
        par_famille
            .entry(*famille)
            .or_default()
            .push((genre.as_str(), *n));
    }

    let mut vus: HashSet<String> = HashSet::new();
    let mut sortie = Vec::with_capacity(effectifs.len());
    for (famille, effectif) in effectifs {
        let mut classe: Vec<(&str, f64)> = Vec::new();
        if let Some(genres) = par_famille.get(famille) {
            let dans_la_famille: i64 = genres.iter().map(|(_, n)| n).sum();
            let plancher =
                PLANCHER_ABSOLU.max((dans_la_famille as f64 * PLANCHER_RELATIF).round() as i64);
            for (genre, n) in genres {
                if *n < plancher || total == 0 || dans_la_famille == 0 {
                    continue;
                }
                let part = *n as f64 / dans_la_famille as f64;
                let ailleurs = global[genre] as f64 / total as f64;
                let score = part * (part / ailleurs).log2();
                // Un score négatif signale un genre sous-représenté : il dit
                // ce que la famille n'est pas, ce qui ne la nomme pas.
                if score > 0.0 {
                    classe.push((genre, score));
                }
            }
        }
        classe.sort_by(|a, b| b.1.total_cmp(&a.1));
        let genres: Vec<&str> = classe.into_iter().map(|(g, _)| g).collect();

        let nom = libeller(&genres, &vus);
        if let Some(n) = &nom {
            vus.insert(empreinte_libelle(n));
        }
        sortie.push((*famille, nom, *effectif));
    }
    sortie
}

/// Le libellé d'une famille : son meilleur genre, précisé par le meilleur
/// suivant qui ne le redise pas et ne donne pas un nom déjà pris.
///
/// Les deux règles sont chacune tirées d'un libellé qui n'apprenait rien :
/// « Electronic · Electro », où le second mot redit le premier ; et deux
/// familles ressorties « Metal · Rock », le rock dominant la moitié de la
/// bibliothèque. La seconde descend alors son classement — « Metal · Grunge ».
/// Met une majuscule aux mots entièrement minuscules d'un genre.
///
/// Les deux sources n'écrivent pas pareil : MusicBrainz impose le tout en
/// minuscules (`trip hop`, `boom bap`), les tags des fichiers arrivent en
/// capitales (`Reggae`, `Hip-Hop`). Mélangés dans une même légende, ça se voit.
/// On ne touche qu'aux mots tout en minuscules, pour ne pas défigurer `R&B`
/// ni `IDM`.
pub(super) fn capitaliser(genre: &str) -> String {
    genre
        .split_inclusive([' ', '-', '/'])
        .map(|mot| {
            if mot.chars().any(char::is_uppercase) {
                return mot.to_string();
            }
            let mut c = mot.chars();
            match c.next() {
                Some(p) => p.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Forme canonique d'un libellé : ses mots, triés.
///
/// Sert à repérer les permutations. « Electronic · Hip Hop » et « Hip Hop ·
/// Electronic » sont deux libellés distincts au sens des chaînes, et la même
/// chose pour qui lit la légende.
fn empreinte_libelle(nom: &str) -> String {
    let mut mots: Vec<&str> = nom.split(" · ").collect();
    mots.sort_unstable();
    mots.join("\u{0}")
}

fn libeller(genres: &[&str], vus: &HashSet<String>) -> Option<String> {
    let tete = capitaliser(genres.first()?);
    let tete = tete.as_str();
    let mut repli = None;
    for g in &genres[1..] {
        if se_redisent(tete, g) {
            continue;
        }
        let nom = format!("{tete} · {}", capitaliser(g));
        if !vus.contains(&empreinte_libelle(&nom)) {
            return Some(nom);
        }
        // Le meilleur doublon, gardé au cas où aucune paire ne soit libre :
        // mieux vaut un nom en double qu'un nom amputé.
        repli.get_or_insert(nom);
    }
    if !vus.contains(&empreinte_libelle(tete)) {
        return Some(tete.to_string());
    }
    repli.or_else(|| Some(tete.to_string()))
}

/// Deux genres se redisent-ils ?
///
/// Purement lexical : un mot commun, ou l'un préfixe de l'autre sur au moins
/// cinq lettres. Ça attrape « Electro » dans « Electronic » et « Hip-Hop »
/// dans « Rap/Hip Hop » sans confondre « Rock » et « Rockabilly ». Ça ne
/// rapproche pas deux synonymes qui ne se ressemblent pas — « Rap » et
/// « Hip-Hop » — ce qui demanderait un vocabulaire des genres, propre à
/// chaque bibliothèque.
pub(super) fn se_redisent(a: &str, b: &str) -> bool {
    let mots = |g: &str| -> Vec<String> {
        g.split(|c: char| !c.is_alphanumeric())
            .filter(|m| m.chars().count() >= 3)
            .map(str::to_lowercase)
            .collect()
    };
    let (ma, mb) = (mots(a), mots(b));
    ma.iter().any(|x| {
        mb.iter().any(|y| {
            x == y || (x.len().min(y.len()) >= 5 && (x.starts_with(y) || y.starts_with(x)))
        })
    })
}

/// Le libellé de tête d'un nom de famille (« X · Y » → « X »), normalisé
/// pour comparer trois sources qui n'écrivent pas pareil (casse, ponctuation)
/// — sert au vote de [`arbitrer_vote`], pas à l'affichage.
fn tete_normalisee(nom: &str) -> String {
    let tete = nom.split(" · ").next().unwrap_or(nom);
    tete.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Combine les trois votes d'une famille en un nom affiché et un drapeau de
/// confiance — voir `docs/nommage-familles.md`.
///
/// Volontairement simple, comme demandé : **au moins deux des trois
/// libellés de tête s'accordent** → fiable, ce libellé est affiché. Si les
/// trois divergent, une critique CritiqueBrainz disponible peut confirmer
/// l'un des trois (`critique_confirme`) ; sinon, repli sur le nom
/// MusicBrainz seul — le filet de sécurité déjà en place avant ce chantier,
/// pour que la carte garde toujours un nom.
fn arbitrer_vote(
    nom_mb: &str,
    nom_clap: &str,
    nom_lastfm: &str,
    critique_confirme: impl Fn(&str) -> bool,
) -> (bool, String) {
    let sources = [nom_mb, nom_clap, nom_lastfm];
    let normalises: Vec<Option<String>> = sources
        .iter()
        .map(|s| {
            let t = tete_normalisee(s);
            (!t.is_empty()).then_some(t)
        })
        .collect();

    for (i, a) in normalises.iter().enumerate() {
        let Some(a) = a else { continue };
        for (j, b) in normalises.iter().enumerate().skip(i + 1) {
            let Some(b) = b else { continue };
            if a == b {
                // Préfère la casse MusicBrainz quand elle est du bon côté de
                // l'accord — c'est la convention de capitalisation établie.
                let choisi = if i == 0 || j == 0 { nom_mb } else { sources[i] };
                return (true, choisi.to_string());
            }
        }
    }

    for nom in sources {
        if !nom.is_empty() && critique_confirme(nom) {
            return (true, nom.to_string());
        }
    }

    (false, nom_mb.to_string())
}

#[cfg(test)]
mod tests_vote {
    use super::arbitrer_vote;

    #[test]
    fn deux_sources_daccord_lemportent() {
        let (fiable, nom) = arbitrer_vote(
            "Alternative Metal",
            "A Heavy Metal Band With Screamed Vocals",
            "alternative metal",
            |_| false,
        );
        assert!(fiable);
        assert_eq!(nom, "Alternative Metal");
    }

    #[test]
    fn trois_sources_en_desaccord_replient_sur_musicbrainz() {
        let (fiable, nom) = arbitrer_vote(
            "Reggae · Rock",
            "An African Percussion Ensemble",
            "worldbeat",
            |_| false,
        );
        assert!(!fiable);
        assert_eq!(nom, "Reggae · Rock");
    }

    #[test]
    fn une_critique_confirme_une_proposition_divergente() {
        let (fiable, nom) = arbitrer_vote(
            "Reggae · Rock",
            "An African Percussion Ensemble",
            "worldbeat",
            |candidat| candidat == "An African Percussion Ensemble",
        );
        assert!(fiable);
        assert_eq!(nom, "An African Percussion Ensemble");
    }

    #[test]
    fn des_sources_vides_ne_saccordent_jamais_entre_elles() {
        let (fiable, nom) = arbitrer_vote("Reggae · Rock", "", "", |_| false);
        assert!(!fiable);
        assert_eq!(nom, "Reggae · Rock");
    }

    /// `arbitrer_vote` compare des chaînes, il ne sait pas qu'un repli
    /// « Famille N » n'est pas une vraie proposition — la garantie que deux
    /// replis identiques ne s'accordent jamais vient d'ailleurs : voir
    /// `nommer_les_familles_option` et le test de régression
    /// `db::tests::familles_votees_ignore_le_faux_accord_de_deux_replis`.
    /// Documenté ici pour que ce piège ne se réintroduise pas au niveau du
    /// vote lui-même : lui refiler « Famille 1 »/« Famille 1 » les ferait
    /// s'accorder à tort, exactement le bug mesuré en développant ce
    /// chantier.
    #[test]
    fn arbitrer_vote_ne_distingue_pas_un_repli_dune_vraie_proposition() {
        let (fiable, _) = arbitrer_vote("Rock · Grunge", "Famille 1", "Famille 1", |_| false);
        assert!(
            fiable,
            "arbitrer_vote seul ne peut pas le détecter — c'est voulu, voir le commentaire"
        );
    }
}

#[cfg(test)]
mod tests_ordre {
    use super::*;

    #[test]
    fn les_dates_iso_se_convertissent_en_cle() {
        assert_eq!(date_iso_vers_cle("1973-03-01"), Some(19_730_301));
        assert_eq!(date_iso_vers_cle("1973-03"), Some(19_730_300));
        assert_eq!(date_iso_vers_cle("1973"), Some(19_730_000));
        // Une date partielle doit rester **avant** toute date complète de la
        // même année : c'est ce que le remplissage par des zéros garantit.
        assert!(date_iso_vers_cle("1973").unwrap() < date_iso_vers_cle("1973-01-01").unwrap());
        assert_eq!(date_iso_vers_cle("pas une date"), None);
        assert_eq!(date_iso_vers_cle("1650-01-01"), None);
    }

    #[test]
    fn lepoch_se_convertit_en_cle() {
        // Valeurs de référence, vérifiées contre une bibliothèque de dates.
        assert_eq!(epoch_vers_cle(1_787_000_000), 20_260_817);
        assert_eq!(epoch_vers_cle(1_000_000_000), 20_010_909);
        assert_eq!(epoch_vers_cle(1_700_000_000), 20_231_114);
        assert_eq!(epoch_vers_cle(0), 19_700_101);
        // Monotone : c'est tout ce que l'ordre du peuplement lui demande.
        assert!(epoch_vers_cle(1_000_000_000) < epoch_vers_cle(1_700_000_000));
    }

    #[test]
    fn le_hachage_dalbum_est_stable_et_discriminant() {
        assert_eq!(hacher("a", "b"), hacher("a", "b"));
        assert_ne!(hacher("a", "b"), hacher("b", "a"));
        // Le séparateur évite qu'« ab » + « c » collisionne avec « a » + « bc ».
        assert_ne!(hacher("ab", "c"), hacher("a", "bc"));
    }

    /// L'échelle de datation, de bout en bout : chaque échelon doit prendre le
    /// relais du précédent, et un morceau sans rien doit tout de même arriver.
    #[test]
    fn lechelle_de_datation_rattrape_les_trous() {
        let lib = Library::open_in_memory().unwrap();
        let poser = |id: i64, annee: Option<i64>, album: &str, artiste: &str| {
            lib.conn
                .execute(
                    "INSERT INTO tracks (id, path, year, album, album_artist, track_no, added_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, 1, 1000000000)",
                    params![id, format!("/x/{id}.flac"), annee, album, artiste],
                )
                .unwrap();
        };
        poser(1, Some(1990), "Disque", "Groupe");
        poser(2, None, "Disque", "Groupe"); // rattrapé par l'album
        poser(3, None, "Autre", "Groupe"); // rattrapé par l'artiste
        poser(4, None, "Seul", "Inconnu"); // rien : date d'ingestion

        let ordre = lib.ordre_darrivee().unwrap();
        let par_id: HashMap<i64, &ArriveeBrute> = ordre.iter().map(|a| (a.track_id, a)).collect();
        assert_eq!(par_id[&1].source, "tag");
        assert_eq!(par_id[&2].source, "album");
        assert_eq!(par_id[&2].date, 19_900_000);
        assert_eq!(par_id[&3].source, "artiste");
        assert_eq!(par_id[&4].source, "ingestion");
        // Aucun morceau n'est perdu en route.
        assert_eq!(ordre.len(), 4);
    }

    /// `mb_poser_albums` doit garder la date de parution et les types
    /// secondaires du release-group, pas seulement son titre — sinon
    /// `ordre_darrivee` et `albums` n'ont jamais rien à corriger et
    /// retombent silencieusement sur le seul tag, même après une passe
    /// `enrich` réussie.
    #[test]
    fn mb_poser_albums_conserve_la_date_et_corrige_le_tag() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.upsert(&TrackMeta {
            path: "/m/1.flac".into(),
            album: Some("Disque".into()),
            album_artist: Some("Groupe".into()),
            mb_album_artist_id: Some("artiste-1".into()),
            year: Some(1999), // année de réédition, portée par le tag
            ..Default::default()
        })
        .unwrap();

        lib.mb_poser_albums(
            "artiste-1",
            &[(
                "rg-1".into(),
                "Disque".into(),
                "disque".into(),
                Vec::new(),
                Some("1973-03-01".into()), // date de l'œuvre, antérieure au tag
                None,
            )],
        )
        .unwrap();

        let ordre = lib.ordre_darrivee().unwrap();
        assert_eq!(ordre.len(), 1);
        assert_eq!(ordre[0].source, "musicbrainz");
        assert_eq!(ordre[0].date, 19_730_301);

        let albums = lib.albums(None).unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(
            albums[0].year,
            Some(1973),
            "l'année de l'album doit suivre MusicBrainz, pas le seul tag"
        );
    }

    /// Un titre à plusieurs mots doit rejoindre son release-group tout
    /// autant qu'un titre d'un seul mot. `title_norm` retire les espaces
    /// (`normaliser_titre`) : la jointure doit appliquer la même
    /// normalisation, pas un simple `lower(trim(...))` qui les garde — sinon
    /// la jointure ne matche jamais et l'année retombe silencieusement sur
    /// celle du tag (ex. réédition 2016 au lieu de la sortie 1991).
    #[test]
    fn mb_poser_albums_rejoint_un_titre_a_plusieurs_mots() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.upsert(&TrackMeta {
            path: "/m/1.flac".into(),
            album: Some("Temple of the Dog".into()),
            album_artist: Some("Temple of the Dog".into()),
            mb_album_artist_id: Some("artiste-1".into()),
            year: Some(2016), // année de la réédition 25e anniversaire
            ..Default::default()
        })
        .unwrap();

        lib.mb_poser_albums(
            "artiste-1",
            &[(
                "rg-1".into(),
                "Temple of the Dog".into(),
                crate::texte::normaliser_titre("Temple of the Dog"),
                Vec::new(),
                Some("1991-04-16".into()),
                None,
            )],
        )
        .unwrap();

        let albums = lib.albums(None).unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(
            albums[0].year,
            Some(1991),
            "l'année doit suivre MusicBrainz (sortie d'origine), pas le tag (réédition)"
        );
    }

    /// Une compilation date d'elle-même, pas des œuvres qu'elle rassemble :
    /// sa `first_release_date` de release-group ne doit pas écraser le tag.
    #[test]
    fn mb_poser_albums_ignore_la_date_dune_compilation() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.upsert(&TrackMeta {
            path: "/m/1.flac".into(),
            album: Some("Spawn".into()),
            album_artist: Some("Various".into()),
            mb_album_artist_id: Some("artiste-1".into()),
            year: Some(1997),
            ..Default::default()
        })
        .unwrap();

        lib.mb_poser_albums(
            "artiste-1",
            &[(
                "rg-1".into(),
                "Spawn".into(),
                "spawn".into(),
                Vec::new(),
                Some("2010-01-01".into()), // date d'une réédition de la compilation
                Some("Compilation".into()),
            )],
        )
        .unwrap();

        let albums = lib.albums(None).unwrap();
        assert_eq!(
            albums[0].year,
            Some(1997),
            "le tag doit l'emporter sur la compilation"
        );
    }

    /// Les pistes d'un album arrivent **ensemble**. Sans cela, les 1 341
    /// arrivées d'une même année les éparpilleraient et chacune irait fonder
    /// son propre établissement.
    #[test]
    fn un_album_arrive_en_bloc() {
        let lib = Library::open_in_memory().unwrap();
        for (id, album, piste) in [
            (1i64, "A", 1i64),
            (2, "B", 1),
            (3, "A", 2),
            (4, "B", 2),
            (5, "A", 3),
        ] {
            lib.conn
                .execute(
                    "INSERT INTO tracks (id, path, year, album, album_artist, track_no, added_at)
                     VALUES (?1, ?2, 2000, ?3, 'G', ?4, 0)",
                    params![id, format!("/x/{id}.flac"), album, piste],
                )
                .unwrap();
        }
        let ordre = lib.ordre_darrivee().unwrap();
        let albums: Vec<u64> = ordre.iter().map(|a| a.album).collect();
        // Trois pistes du même album, puis deux de l'autre : jamais alternés.
        let mut changements = 0;
        for p in albums.windows(2) {
            if p[0] != p[1] {
                changements += 1;
            }
        }
        assert_eq!(changements, 1, "les albums s'entremêlent : {albums:?}");
        // Et dans l'ordre des pistes à l'intérieur d'un album.
        let premier = albums[0];
        let pistes: Vec<u16> = ordre
            .iter()
            .filter(|a| a.album == premier)
            .map(|a| a.piste)
            .collect();
        assert!(pistes.windows(2).all(|p| p[0] <= p[1]), "{pistes:?}");
    }

    /// « Image du dossier » : le choix vit dans la base et l'emporte sur les
    /// tags, même quand le fichier audio est illisible — une recopie des MP3
    /// ne l'efface donc pas.
    #[test]
    fn le_choix_de_pochette_du_dossier_l_emporte_sur_les_tags() {
        let racine =
            std::env::temp_dir().join(format!("rusty-music-pochette-choix-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&racine);
        let dossier = racine.join("Artiste_1/Album 100%");
        std::fs::create_dir_all(dossier.join("CD2")).unwrap();
        let piste = dossier.join("01.mp3");
        std::fs::write(&piste, b"pas un mp3").unwrap();
        std::fs::write(dossier.join("cover.jpg"), b"jpeg du dossier").unwrap();

        let lib = Library::open_in_memory().unwrap();
        for p in [
            &piste,
            &dossier.join("CD2/01.mp3"),
            &racine.join("Artiste_1/autre.mp3"),
        ] {
            lib.conn
                .execute(
                    "INSERT INTO tracks (path) VALUES (?1)",
                    params![p.to_string_lossy()],
                )
                .unwrap();
        }
        let d = dossier.to_string_lossy();
        assert_eq!(
            lib.pistes_du_dossier(&d).unwrap(),
            vec![piste.to_string_lossy()]
        );

        // Sans choix : les tags d'abord, et ce fichier n'en a pas de lisibles.
        assert!(lib.lire_pochette(&piste).is_err());
        lib.choisir_pochette_dossier(&d, true).unwrap();
        let c = lib.lire_pochette(&piste).unwrap().unwrap();
        assert_eq!(c.data, b"jpeg du dossier");
        assert_eq!(c.source, crate::tags::CoverSource::Folder);
        lib.choisir_pochette_dossier(&d, false).unwrap();
        assert!(!lib.pochette_dossier_choisie(&d).unwrap());
        let _ = std::fs::remove_dir_all(&racine);
    }
}

/// Ce que la bibliothèque sait d'un morceau, pour le filtrer (filtres de
/// playlist de Lama, `rusty_music_lama::filtres_playlist`).
#[derive(Debug, Clone, Default)]
pub struct CaracteristiquesPiste {
    pub id: i64,
    pub artiste: Option<String>,
    pub annee: Option<i32>,
    pub duree_ms: Option<i64>,
    pub bpm: Option<f32>,
    pub energie: Option<f32>,
    /// Rang percentile 0..1 dans la bibliothèque (1 = le plus écouté).
    pub popularite: Option<f64>,
    /// Jours écoulés depuis la dernière écoute **de cet utilisateur** ; `None`
    /// = jamais écouté (aucune ligne dans `ecoute`).
    pub jours_depuis_ecoute: Option<u32>,
    /// Genres résolus, en minuscules.
    pub genres: Vec<String>,
}
