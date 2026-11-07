// SPDX-License-Identifier: GPL-3.0-or-later
//! Les balades gardées (`docs/plan-balade.md`, P5) : chaque balade et ses pas,
//! notés au fil de la marche par `explorer::balade`.
//!
//! De là viennent les **sentiers battus** (combien de fois chaque tronçon du
//! graphe de voirie a été parcouru, [`Library::passages_sentiers`]) et la
//! liste des **balades passées**, redessinables et rejouables
//! ([`Library::balades_passees`], [`Library::pas_de_balade`]).

use super::*;

/// Un pas de balade : le morceau, et dans la ville le chemin qui y mène.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct PasBalade {
    /// 0 pour le départ.
    pub rang: i64,
    pub morceau: i64,
    pub sommet_depart: Option<u32>,
    pub sommet_arrivee: Option<u32>,
    /// Les sommets parcourus, départ et arrivée compris ; vide au départ et
    /// dans le graphe des voisins.
    pub chemin: Vec<u32>,
    /// Choisi d'un geste au carrefour, plutôt que la pente.
    pub choisi: bool,
    /// Voies offertes : 1 sans carrefour.
    pub voies: u32,
}

/// Une balade passée, telle que le panneau de la carte la liste.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct BaladePassee {
    pub id: i64,
    pub debut: i64,
    /// `"ville"` ou `"voisins"`.
    pub terrain: String,
    pub pas: i64,
    /// Durée des morceaux joués, bout à bout.
    pub duree_ms: i64,
    /// Titre et artiste du départ, pour la reconnaître.
    pub titre: Option<String>,
    pub artiste: Option<String>,
}

fn chemin_texte(chemin: &[u32]) -> Option<String> {
    (!chemin.is_empty()).then(|| {
        chemin
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    })
}

fn chemin_lu(texte: Option<String>) -> Vec<u32> {
    texte
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| s.parse().ok())
        .collect()
}

impl Library {
    /// Ouvre une balade sur `terrain` (`"ville"` ou `"voisins"`), maintenant.
    pub fn ouvrir_balade(&self, terrain: &str) -> Result<i64> {
        self.conn
            .execute("INSERT INTO balade(terrain) VALUES (?1)", [terrain])?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Note un pas de la balade `balade`. Un pas déjà noté au même rang est
    /// remplacé : une reprise de session peut le rejouer.
    pub fn noter_pas(&self, balade: i64, pas: &PasBalade) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO balade_pas
                (balade, rang, morceau, sommet_depart, sommet_arrivee, chemin, choisi, voies)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                balade,
                pas.rang,
                pas.morceau,
                pas.sommet_depart,
                pas.sommet_arrivee,
                chemin_texte(&pas.chemin),
                pas.choisi,
                pas.voies,
            ],
        )?;
        Ok(())
    }

    /// Les balades d'au moins deux morceaux, la plus récente d'abord. Une
    /// balade lancée puis arrêtée sur son départ n'est pas une balade.
    pub fn balades_passees(&self, limite: usize) -> Result<Vec<BaladePassee>> {
        let limite = i64::try_from(limite).unwrap_or(i64::MAX);
        let mut stmt = self.conn.prepare(
            "SELECT b.id, b.debut, b.terrain, COUNT(*),
                    COALESCE(SUM(t.duration_ms), 0),
                    (SELECT t0.title FROM balade_pas p0 JOIN tracks t0 ON t0.id = p0.morceau
                      WHERE p0.balade = b.id AND p0.rang = 0),
                    (SELECT t0.artist FROM balade_pas p0 JOIN tracks t0 ON t0.id = p0.morceau
                      WHERE p0.balade = b.id AND p0.rang = 0)
               FROM balade b
               JOIN balade_pas p ON p.balade = b.id
               LEFT JOIN tracks t ON t.id = p.morceau
              GROUP BY b.id
             HAVING COUNT(*) >= 2
              ORDER BY b.debut DESC, b.id DESC
              LIMIT ?1",
        )?;
        let balades = stmt
            .query_map([limite], |r| {
                Ok(BaladePassee {
                    id: r.get(0)?,
                    debut: r.get(1)?,
                    terrain: r.get(2)?,
                    pas: r.get(3)?,
                    duree_ms: r.get(4)?,
                    titre: r.get(5)?,
                    artiste: r.get(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(balades)
    }

    /// Les pas de la balade `id`, dans l'ordre.
    pub fn pas_de_balade(&self, id: i64) -> Result<Vec<PasBalade>> {
        let mut stmt = self.conn.prepare(
            "SELECT rang, morceau, sommet_depart, sommet_arrivee, chemin, choisi, voies
               FROM balade_pas WHERE balade = ?1 ORDER BY rang",
        )?;
        let pas = stmt
            .query_map([id], |r| {
                Ok(PasBalade {
                    rang: r.get(0)?,
                    morceau: r.get(1)?,
                    sommet_depart: r.get(2)?,
                    sommet_arrivee: r.get(3)?,
                    chemin: chemin_lu(r.get(4)?),
                    choisi: r.get(5)?,
                    voies: r.get(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(pas)
    }

    /// Les sentiers battus : pour chaque arête du graphe de voirie parcourue
    /// en balade, `(a, b)` avec `a < b`, le nombre de pas qui l'ont empruntée.
    /// Un pas qui repasse sur la même arête ne la compte qu'une fois.
    pub fn passages_sentiers(&self) -> Result<HashMap<(u32, u32), u32>> {
        let mut stmt = self
            .conn
            .prepare("SELECT chemin FROM balade_pas WHERE chemin IS NOT NULL")?;
        let mut passages: HashMap<(u32, u32), u32> = HashMap::new();
        for chemin in stmt.query_map([], |r| r.get::<_, Option<String>>(0))? {
            let sommets = chemin_lu(chemin?);
            let aretes: HashSet<(u32, u32)> = sommets
                .windows(2)
                .filter(|w| w[0] != w[1])
                .map(|w| (w[0].min(w[1]), w[0].max(w[1])))
                .collect();
            for a in aretes {
                *passages.entry(a).or_default() += 1;
            }
        }
        Ok(passages)
    }

    /// Les morceaux écoutés au moins une fois (table `ecoute`) : ce que le
    /// brouillard de la carte découvre.
    pub fn morceaux_ecoutes(&self) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT t.id FROM ecoute e JOIN tracks t ON t.path = e.chemin ORDER BY t.id",
        )?;
        let ids = stmt
            .query_map([], |r| r.get::<_, i64>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bibliotheque() -> Library {
        let lib = Library::open_in_memory().unwrap();
        for (id, titre) in [(1, "Un"), (2, "Deux"), (3, "Trois")] {
            lib.conn
                .execute(
                    "INSERT INTO tracks(id, path, title, artist, duration_ms)
                     VALUES (?1, ?2, ?3, 'Artiste', 60000)",
                    params![id, format!("/m/{id}.flac"), titre],
                )
                .unwrap();
        }
        lib
    }

    fn pas(rang: i64, morceau: i64, chemin: &[u32], choisi: bool) -> PasBalade {
        PasBalade {
            rang,
            morceau,
            sommet_depart: chemin.first().copied(),
            sommet_arrivee: chemin.last().copied(),
            chemin: chemin.to_vec(),
            choisi,
            voies: if choisi { 3 } else { 1 },
        }
    }

    #[test]
    fn une_balade_se_relit_pas_a_pas() {
        let lib = bibliotheque();
        let b = lib.ouvrir_balade("ville").unwrap();
        let attendus = [pas(0, 1, &[], false), pas(1, 2, &[10, 11, 12], true)];
        for p in &attendus {
            lib.noter_pas(b, p).unwrap();
        }
        assert_eq!(lib.pas_de_balade(b).unwrap(), attendus);
        let passees = lib.balades_passees(10).unwrap();
        assert_eq!(passees.len(), 1);
        assert_eq!(passees[0].pas, 2);
        assert_eq!(passees[0].duree_ms, 120_000);
        assert_eq!(passees[0].titre.as_deref(), Some("Un"));
    }

    #[test]
    fn une_balade_dun_seul_morceau_nest_pas_listee() {
        let lib = bibliotheque();
        let b = lib.ouvrir_balade("voisins").unwrap();
        lib.noter_pas(b, &pas(0, 1, &[], false)).unwrap();
        assert!(lib.balades_passees(10).unwrap().is_empty());
    }

    #[test]
    fn les_sentiers_comptent_chaque_arete_une_fois_par_pas_dans_les_deux_sens() {
        let lib = bibliotheque();
        let b = lib.ouvrir_balade("ville").unwrap();
        lib.noter_pas(b, &pas(1, 2, &[1, 2, 3, 2], false)).unwrap();
        lib.noter_pas(b, &pas(2, 3, &[3, 2], false)).unwrap();
        let s = lib.passages_sentiers().unwrap();
        assert_eq!(s.get(&(1, 2)), Some(&1));
        assert_eq!(s.get(&(2, 3)), Some(&2));
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn les_morceaux_ecoutes_viennent_du_journal() {
        let lib = bibliotheque();
        lib.noter_ecoute("/m/2.flac").unwrap();
        lib.noter_ecoute("/m/2.flac").unwrap();
        lib.noter_ecoute("/ailleurs.flac").unwrap();
        assert_eq!(lib.morceaux_ecoutes().unwrap(), vec![2]);
    }
}
