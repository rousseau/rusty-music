// SPDX-License-Identifier: GPL-3.0-or-later
//! Le mode Bibliothèque : statistiques et vérifications (éditions, pochettes).

use super::*;

impl Library {
    /* ------------------------------- statistiques (mode Bibliothèque) */

    /// Répartition de toute la bibliothèque par genre — même arbitrage que
    /// [`Self::familles`] (album MusicBrainz, puis artiste, puis tag du
    /// fichier ; voir [`genres_du_morceau`]), mais sur tous les morceaux, pas
    /// seulement ceux déjà placés sur la carte. Triée par effectif
    /// décroissant ; un morceau sans genre résolu compte dans `"—"`.
    pub fn stats_genres(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT mb_artist_id, album, genre FROM tracks")?;
        let pistes: Vec<(Option<String>, Option<String>, Option<String>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<std::result::Result<_, _>>()?;

        let par_artiste = self.mb_genres("artist", VOTES_MINIMUM)?;
        let par_album = self.mb_genres("release-group", VOTES_MINIMUM)?;
        let albums = self.mb_albums()?;

        let mut cumul: HashMap<String, i64> = HashMap::new();
        let mut sans_genre = 0i64;
        for (artiste, album, tag) in &pistes {
            let genres = genres_du_morceau(
                artiste.as_deref(),
                album.as_deref(),
                tag.as_deref(),
                &albums,
                &par_album,
                &par_artiste,
            );
            if genres.is_empty() {
                sans_genre += 1;
            }
            for genre in genres {
                *cumul.entry(genre).or_insert(0) += 1;
            }
        }
        if sans_genre > 0 {
            cumul.insert("—".to_string(), sans_genre);
        }
        let mut resultat: Vec<(String, i64)> = cumul.into_iter().collect();
        resultat.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(resultat)
    }

    /// Histogramme du tempo mesuré, par tranches de [`TEMPO_PAS`] BPM à
    /// partir de [`TEMPO_MIN`]. `LEFT JOIN` plutôt qu'un `SELECT bpm FROM
    /// descriptors` : un morceau jamais passé dans la passe « descripteurs »
    /// n'y a aucune ligne, et compterait sinon comme mesuré à zéro plutôt que
    /// comme non mesuré.
    pub fn stats_tempo(&self) -> Result<Histogramme> {
        let valeurs: Vec<Option<f64>> = self
            .conn
            .prepare("SELECT d.bpm FROM tracks t LEFT JOIN descriptors d ON d.track_id = t.id")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(histogrammer(&valeurs, TEMPO_MIN, TEMPO_PAS, TEMPO_TRANCHES))
    }

    /// Histogramme de la durée des morceaux, par tranches de [`DUREE_PAS`]
    /// (une minute) à partir de zéro.
    pub fn stats_durees(&self) -> Result<Histogramme> {
        let valeurs: Vec<Option<f64>> = self
            .conn
            .prepare("SELECT duration_ms FROM tracks")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(histogrammer(&valeurs, DUREE_MIN, DUREE_PAS, DUREE_TRANCHES))
    }

    /// Répartition par format de fichier (« MP3 », « FLAC »…). `NULL` — un
    /// morceau scanné avant que `tags::read` ne lise le format — compte à
    /// part : ce n'est pas « aucun format », c'est « pas encore mesuré ».
    pub fn stats_codecs(&self) -> Result<Vec<(String, i64)>> {
        let valeurs: Vec<Option<String>> = self
            .conn
            .prepare("SELECT codec FROM tracks")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        let mut cumul: HashMap<String, i64> = HashMap::new();
        for v in valeurs {
            *cumul
                .entry(v.unwrap_or_else(|| "non mesuré".to_string()))
                .or_insert(0) += 1;
        }
        let mut resultat: Vec<(String, i64)> = cumul.into_iter().collect();
        resultat.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(resultat)
    }

    /// Histogramme du débit audio, par tranches de [`BITRATE_PAS`] kb/s.
    /// Un format sans débit à annoncer (FLAC…) tombe au-delà de la dernière
    /// tranche plutôt que d'écraser l'échelle pensée pour le domaine
    /// habituel du MP3/AAC — voir [`BITRATE_MAX`].
    pub fn stats_bitrate(&self) -> Result<Histogramme> {
        let valeurs: Vec<Option<f64>> = self
            .conn
            .prepare("SELECT bitrate FROM tracks")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(histogrammer(
            &valeurs,
            BITRATE_MIN,
            BITRATE_PAS,
            BITRATE_TRANCHES,
        ))
    }

    /// Morceaux sans identifiant MusicBrainz d'artiste — ni le tag du
    /// fichier ni un rapprochement ultérieur ne les relie à MusicBrainz, donc
    /// ni genre affiné ni connexions d'artiste possibles pour eux.
    pub fn stats_sans_mbid(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM tracks WHERE mb_artist_id IS NULL",
            [],
            |r| r.get(0),
        )?)
    }

    /// Morceaux dont l'artiste n'a pas de biographie TheAudioDB — même
    /// résolution par MBID exact que [`Self::theaudiodb_candidats`] (pas de
    /// séparation d'un `mb_artist_id` à plusieurs valeurs).
    pub fn stats_sans_bio(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM tracks t
              WHERE t.mb_artist_id IS NULL OR t.mb_artist_id = ''
                 OR NOT EXISTS (
                      SELECT 1 FROM theaudiodb_artistes a
                       WHERE a.mb_artist_id = t.mb_artist_id
                         AND (a.biographie_en IS NOT NULL OR a.biographie_fr IS NOT NULL)
                    )",
            [],
            |r| r.get(0),
        )?)
    }

    /// Morceaux dont l'édition n'a ni crédit ni label Discogs — résolution
    /// directe par `mb_release_id`, comme [`Self::credits_pour_piste`].
    pub fn stats_sans_discogs(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM tracks t
              WHERE t.mb_release_id IS NULL OR t.mb_release_id = ''
                 OR NOT EXISTS (
                      SELECT 1 FROM editions_discogs e
                       WHERE e.mb_release_id = t.mb_release_id
                         AND e.discogs_release_id IS NOT NULL
                         AND (EXISTS (SELECT 1 FROM credits_discogs c
                                       WHERE c.discogs_release_id = e.discogs_release_id)
                           OR EXISTS (SELECT 1 FROM labels_discogs l
                                       WHERE l.discogs_release_id = e.discogs_release_id))
                    )",
            [],
            |r| r.get(0),
        )?)
    }

    /// Morceaux dont l'album n'a pas de critique CritiqueBrainz — résolution
    /// par release-group comme [`Self::critiques_pour_piste`], mais en un
    /// seul passage plutôt qu'une requête par piste : `mb_albums` et
    /// l'ensemble des release-groups déjà critiqués sont chacun chargés une
    /// fois, puis confrontés aux couples (artiste, album) groupés par la
    /// base elle-même.
    pub fn stats_sans_critique(&self) -> Result<i64> {
        let total = self.count()?;
        let albums = self.mb_albums()?;

        let mut stmt_critiquees = self
            .conn
            .prepare("SELECT DISTINCT mbid_release_group FROM critiques")?;
        let critiquees: HashSet<String> = stmt_critiquees
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<_, _>>()?;

        let mut stmt = self.conn.prepare(
            "SELECT mb_artist_id, album, COUNT(*) FROM tracks
              WHERE mb_artist_id IS NOT NULL AND mb_artist_id <> ''
                AND album IS NOT NULL AND album <> ''
              GROUP BY mb_artist_id, album",
        )?;
        let lignes = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        let mut avec_critique = 0i64;
        for ligne in lignes {
            let (artiste, album, n) = ligne?;
            let norme = crate::texte::normaliser_titre(&album);
            if albums
                .get(&(artiste, norme))
                .is_some_and(|rg| critiquees.contains(rg))
            {
                avec_critique += n;
            }
        }
        Ok(total - avec_critique)
    }

    /// Albums dont le même artiste (`album_artist` de préférence, sinon
    /// `artist` — même repli que [`Self::albums`]) porte plusieurs titres
    /// distincts qui ne diffèrent que par leur mention d'édition, ex.
    /// « Kid A » et « Kid A (Remaster) ».
    ///
    /// Le regroupement se fait sur un titre normalisé — tout ce qui précède
    /// la première parenthèse ou le premier crochet
    /// ([`titre_album_normalise`]), pas de registre d'éditions à maintenir,
    /// au prix d'un titre légitimement parenthétique tronqué à tort de temps
    /// en temps — mais le résultat rend le titre brut le plus représenté,
    /// pas la forme normalisée : celle-ci sert à regrouper, pas à afficher.
    pub fn editions_multiples(&self) -> Result<Vec<EditionsAlbum>> {
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(album_artist, artist), album, COUNT(*)
               FROM tracks
              WHERE album IS NOT NULL AND COALESCE(album_artist, artist) IS NOT NULL
              GROUP BY COALESCE(album_artist, artist), album",
        )?;
        let lignes: Vec<(String, String, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<std::result::Result<_, _>>()?;

        let mut par_cle: HashMap<(String, String), Vec<(String, i64)>> = HashMap::new();
        for (artiste, album, n) in lignes {
            let norme = titre_album_normalise(&album);
            par_cle
                .entry((artiste, norme))
                .or_default()
                .push((album, n));
        }
        let mut resultat: Vec<EditionsAlbum> = par_cle
            .into_iter()
            .filter(|(_, editions)| editions.len() > 1)
            .map(|((artiste, _norme), mut editions)| {
                editions.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
                (artiste, editions[0].0.clone(), editions)
            })
            .collect();
        resultat.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        Ok(resultat)
    }

    /// Répartition par humeur — tempo × énergie, tous deux déjà mesurés,
    /// **coupés à leur médiane du moment** plutôt qu'à un seuil absolu.
    ///
    /// AudioMuse-AI dérive ces catégories par similarité cosinus entre
    /// l'empreinte CLAP et un jeu de vecteurs-étiquettes (« dansant »,
    /// « agressif »…) — hors de portée ici, le modèle importé
    /// (`clap-audio-encoder-b5`) n'embarque que la tour audio, pas la tour
    /// texte qu'il faudrait pour situer un mot dans le même espace. Le
    /// substitut retenu réutilise ce qui est déjà mesuré. Le seuil absolu a
    /// été écarté après mesure : sur les 741 morceaux déjà remesurés au
    /// moment d'écrire ceci, l'énergie s'étale de 0,03 à 0,41 — un seuil fixe
    /// à 0,5 aurait vidé deux des quatre catégories. La médiane du moment
    /// s'ajuste à l'échelle réelle de la bibliothèque, quelle qu'elle soit,
    /// et reste correcte si cette échelle se déplace en cours de mesure.
    pub fn stats_humeur(&self) -> Result<Vec<(String, i64)>> {
        let valeurs: Vec<(Option<f64>, Option<f64>)> = self
            .conn
            .prepare(
                "SELECT d.bpm, d.energy FROM tracks t LEFT JOIN descriptors d ON d.track_id = t.id",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;

        let mediane = |mut v: Vec<f64>| -> Option<f64> {
            if v.is_empty() {
                return None;
            }
            v.sort_by(f64::total_cmp);
            Some(v[v.len() / 2])
        };
        let Some(med_bpm) = mediane(valeurs.iter().filter_map(|(b, _)| *b).collect()) else {
            return Ok(Vec::new());
        };
        let Some(med_energie) = mediane(valeurs.iter().filter_map(|(_, e)| *e).collect()) else {
            return Ok(Vec::new());
        };

        let mut cumul: HashMap<&str, i64> = HashMap::new();
        for (bpm, energie) in &valeurs {
            let etiquette = match (bpm, energie) {
                (Some(b), Some(e)) => match (*b >= med_bpm, *e >= med_energie) {
                    (true, true) => "Énergique",
                    (true, false) => "Enlevé",
                    (false, true) => "Intense",
                    (false, false) => "Calme",
                },
                _ => "non mesuré",
            };
            *cumul.entry(etiquette).or_insert(0) += 1;
        }
        let mut resultat: Vec<(String, i64)> =
            cumul.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        resultat.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        Ok(resultat)
    }

    /* --------------------------------------------- vérification des pochettes */

    /// Les pistes à regrouper par dossier pour la vérification des pochettes
    /// (`crate::pochettes_verif`) : chemin, taille, mtime, artiste d'album,
    /// album. Les fichiers déjà en échec de scan sont exclus — on ne rouvre
    /// pas un fichier qui a déjà fait trébucher son support.
    pub fn pistes_pour_verif_pochettes(&self) -> Result<Vec<PisteVerifPochette>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, COALESCE(size_bytes, 0), COALESCE(mtime, 0),
                    COALESCE(NULLIF(album_artist, ''), artist, ''), COALESCE(album, '')
               FROM tracks
              WHERE NOT EXISTS (SELECT 1 FROM scan_failures sf WHERE sf.path = tracks.path)
              ORDER BY path",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(PisteVerifPochette {
                    path: r.get(0)?,
                    taille: r.get(1)?,
                    mtime: r.get(2)?,
                    artiste: r.get(3)?,
                    album: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// `dossier → (signature, version)` des dossiers déjà vérifiés.
    pub fn signatures_verif_pochettes(
        &self,
    ) -> Result<std::collections::HashMap<String, (String, i32)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT dossier, signature, version FROM pochettes_verif")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?))))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    /// Écrit le résultat de la vérification d'un dossier. Réécrire remet
    /// `ignoree` à 0 : les fichiers ont changé, le signalement écarté
    /// auparavant ne vaut plus.
    pub fn enregistrer_verif_pochette(&self, v: &VerifPochette) -> Result<()> {
        self.conn.execute(
            "INSERT INTO pochettes_verif(dossier, artiste, album, signature, version,
                                         emb_hash, anomalie, detail, mesure, ignoree, verifie_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, strftime('%s','now'))
             ON CONFLICT(dossier) DO UPDATE SET
                artiste = excluded.artiste, album = excluded.album,
                signature = excluded.signature, version = excluded.version,
                emb_hash = excluded.emb_hash, anomalie = excluded.anomalie,
                detail = excluded.detail, mesure = excluded.mesure,
                ignoree = 0, verifie_at = excluded.verifie_at",
            params![
                v.dossier,
                v.artiste,
                v.album,
                v.signature,
                v.version,
                v.emb_hash,
                v.anomalie,
                v.detail,
                v.mesure
            ],
        )?;
        Ok(())
    }

    /// Retire les dossiers vérifiés qui ne sont plus dans la bibliothèque.
    pub fn elaguer_verif_pochettes(
        &self,
        presents: &std::collections::HashSet<String>,
    ) -> Result<usize> {
        let mut stmt = self.conn.prepare("SELECT dossier FROM pochettes_verif")?;
        let connus: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        drop(stmt);
        let mut n = 0;
        for d in connus.iter().filter(|d| !presents.contains(*d)) {
            n += self
                .conn
                .execute("DELETE FROM pochettes_verif WHERE dossier = ?1", params![d])?;
        }
        Ok(n)
    }

    /// Toutes les lignes de vérification, `ignoree` comprise.
    pub fn verifs_pochettes(&self) -> Result<Vec<(VerifPochette, bool)>> {
        let mut stmt = self.conn.prepare(
            "SELECT dossier, artiste, album, signature, version, emb_hash, anomalie,
                    detail, mesure, ignoree
               FROM pochettes_verif ORDER BY artiste, album, dossier",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    VerifPochette {
                        dossier: r.get(0)?,
                        artiste: r.get(1)?,
                        album: r.get(2)?,
                        signature: r.get(3)?,
                        version: r.get(4)?,
                        emb_hash: r.get(5)?,
                        anomalie: r.get(6)?,
                        detail: r.get(7)?,
                        mesure: r.get(8)?,
                    },
                    r.get::<_, i64>(9)? != 0,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Écarte le signalement d'un dossier (bouton « Ignorer »).
    pub fn ignorer_pochette_suspecte(&self, dossier: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE pochettes_verif SET ignoree = 1 WHERE dossier = ?1",
            params![dossier],
        )?;
        Ok(())
    }

    /// Fait afficher l'image du dossier (`cover.jpg`…) plutôt que celle des
    /// tags pour l'album de `dossier`, ou revient aux tags (`false`).
    pub fn choisir_pochette_dossier(&self, dossier: &str, oui: bool) -> Result<()> {
        if oui {
            self.conn.execute(
                "INSERT OR REPLACE INTO pochette_choix(dossier, source) VALUES (?1, 'dossier')",
                params![dossier],
            )?;
        } else {
            self.conn.execute(
                "DELETE FROM pochette_choix WHERE dossier = ?1",
                params![dossier],
            )?;
        }
        Ok(())
    }

    /// Les pistes posées directement dans `dossier` (pas dans ses
    /// sous-dossiers). Comparaison de préfixe plutôt que `LIKE` : `_` et `%`
    /// sont fréquents dans les noms de dossiers.
    pub fn pistes_du_dossier(&self, dossier: &str) -> Result<Vec<String>> {
        let prefixe = format!("{}/", dossier.trim_end_matches('/'));
        let mut stmt = self.conn.prepare(
            "SELECT path FROM tracks
              WHERE substr(path, 1, length(?1)) = ?1
                AND instr(substr(path, length(?1) + 1), '/') = 0
              ORDER BY path",
        )?;
        let pistes = stmt
            .query_map(params![prefixe], |r| r.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(pistes)
    }

    /// Le dossier d'album `dossier` doit-il montrer l'image du dossier ?
    pub fn pochette_dossier_choisie(&self, dossier: &str) -> Result<bool> {
        Ok(self
            .conn
            .prepare_cached(
                "SELECT 1 FROM pochette_choix WHERE dossier = ?1 AND source = 'dossier'",
            )?
            .exists(params![dossier])?)
    }

    /// La pochette de `path` en tenant compte du choix de l'utilisateur :
    /// l'image du dossier si elle a été choisie (et existe), sinon
    /// [`crate::tags::read_cover`] (tags d'abord, dossier en repli).
    pub fn lire_pochette(&self, path: &Path) -> Result<Option<crate::tags::Cover>> {
        if let Some(dossier) = path.parent() {
            if self.pochette_dossier_choisie(&dossier.to_string_lossy())? {
                if let Some(c) = crate::tags::read_folder_cover(path) {
                    return Ok(Some(c));
                }
            }
        }
        crate::tags::read_cover(path)
    }
}

/// Une piste vue par la vérification des pochettes.
#[derive(Debug, Clone)]
pub struct PisteVerifPochette {
    pub path: String,
    pub taille: i64,
    pub mtime: i64,
    pub artiste: String,
    pub album: String,
}

/// Le résultat de la vérification d'un dossier (ligne de `pochettes_verif`).
#[derive(Debug, Clone, PartialEq)]
pub struct VerifPochette {
    pub dossier: String,
    pub artiste: String,
    pub album: String,
    pub signature: String,
    pub version: i32,
    /// Hachage de l'image intégrée — sert à repérer une même image sur des
    /// albums différents.
    pub emb_hash: Option<String>,
    /// `None`, `"illisible"` ou `"divergente"`.
    pub anomalie: Option<String>,
    pub detail: String,
    pub mesure: Option<f64>,
}

/// Répartition d'une valeur continue en tranches régulières (mode
/// Bibliothèque). `comptes[i]` couvre `[min + i*pas, min + (i+1)*pas)` ; ce
/// qui déborde par le haut tombe dans `hors_gamme`, ce qui manque dans
/// `sans_valeur` — deux absences de nature différente qu'il ne faut pas
/// confondre dans une même barre.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Histogramme {
    pub min: f64,
    pub pas: f64,
    pub comptes: Vec<i64>,
    pub hors_gamme: i64,
    pub sans_valeur: i64,
}

/// Un album et ses éditions multiples : l'artiste, le titre brut le plus
/// représenté, puis chaque édition (titre publié, nombre de pistes).
pub type EditionsAlbum = (String, String, Vec<(String, i64)>);

/// Les genres d'un morceau, de la source la plus précise à la plus grossière.
///
/// **L'album l'emporte sur l'artiste, l'artiste sur le fichier.** Chaque
/// échelon a sa raison :
///
/// - l'**album** distingue deux disques d'un même artiste : un enregistrement
///   acoustique d'un groupe électrique doit être étiqueté pour ce qu'il est.
///   Nos fichiers ne portent pas d'identifiant d'album, le rapprochement passe
///   donc par le titre normalisé ;
/// - l'**artiste** couvre le plus de morceaux, et son vocabulaire est curé —
///   `boom bap`, `nu metal`, `anti-folk` là où les fichiers disent « Rock » ;
/// - le **tag du fichier** reste en dernier recours, et il n'est pas
///   décoratif : les chanteurs bretons de la bibliothèque de test n'ont aucun
///   genre chez MusicBrainz, et c'est le fichier qui sait alors de quoi il
///   s'agit.
///
/// On ne mélange pas les sources pour un même morceau. Les verser ensemble
/// ferait cohabiter « Rock » et « rock », et le genre le plus grossier
/// redeviendrait le plus lourd — exactement ce qu'on cherche à quitter.
/// Le titre d'album jusqu'à la première parenthèse ou le premier crochet,
/// en minuscules — sert à rapprocher « Kid A » et « Kid A (Remaster) » sans
/// registre d'éditions à tenir à jour. Voir [`Library::editions_multiples`].
pub(super) fn titre_album_normalise(titre: &str) -> String {
    let t = titre.to_lowercase();
    let fin = t.find(['(', '[']).unwrap_or(t.len());
    t[..fin].trim().to_string()
}

// Tranches de 10 BPM : assez fin pour distinguer un tempo lent d'un modéré,
// assez large pour qu'une mesure à quelques BPM près reste dans sa tranche.
// 40 couvre les morceaux les plus lents de la bibliothèque de test, 220 les
// plus rapides sans avoir à étendre `hors_gamme` — voir `Library::stats_tempo`.
pub(super) const TEMPO_MIN: f64 = 40.0;

pub(super) const TEMPO_PAS: f64 = 10.0;

const TEMPO_TRANCHES: usize = 18;

// Tranches d'une minute, de 0 à 12 — au-delà, `hors_gamme` regroupe les
// morceaux longs (mix, live) plutôt que d'étirer l'histogramme pour eux.
const DUREE_MIN: f64 = 0.0;

const DUREE_PAS: f64 = 60_000.0;

const DUREE_TRANCHES: usize = 12;

// Tranches de 32 kb/s, de 32 à 320 — le domaine habituel du MP3/AAC (128 et
// 320 en sont les repères les plus lus). Un format sans débit constant
// (FLAC, WavPack…) atterrit largement au-delà : `hors_gamme` en dit alors le
// compte, pas une tranche étirée qui écraserait le MP3 dans un coin.
const BITRATE_MIN: f64 = 32.0;

const BITRATE_PAS: f64 = 32.0;

// 10, pas 9 : la tranche [320, 352) doit exister pour accueillir 320 kb/s
// pile — le repère « qualité maximale » du MP3 — dans une tranche plutôt
// que dans `hors_gamme`, où il se confondrait avec les formats sans perte.
const BITRATE_TRANCHES: usize = 10;

/// Répartit `valeurs` en tranches régulières de largeur `pas` à partir de
/// `min` — voir [`Histogramme`]. Sert au tempo (BPM) comme à la durée (ms) :
/// même forme, seules les bornes changent.
pub(super) fn histogrammer(
    valeurs: &[Option<f64>],
    min: f64,
    pas: f64,
    tranches: usize,
) -> Histogramme {
    let mut comptes = vec![0i64; tranches];
    let mut hors_gamme = 0i64;
    let mut sans_valeur = 0i64;
    for v in valeurs {
        match v {
            None => sans_valeur += 1,
            Some(v) if *v < min => hors_gamme += 1,
            Some(v) => match ((v - min) / pas) as usize {
                i if i < tranches => comptes[i] += 1,
                _ => hors_gamme += 1,
            },
        }
    }
    Histogramme {
        min,
        pas,
        comptes,
        hors_gamme,
        sans_valeur,
    }
}
