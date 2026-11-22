// SPDX-License-Identifier: GPL-3.0-or-later
//! La page d'accueil d'Écouter : un album à la une, puis des rangées
//! d'albums titrées par leur raison (`docs/plan-ecouter-accueil.md`, A1).
//!
//! Tout est local et rien n'est écrit : la page se recalcule à chaque appel à
//! partir de ce que la base sait déjà —
//! - albums, année fiable, date d'ajout : [`Library::albums`] ;
//! - date d'œuvre au jour près, piste → album : [`Library::ordre_darrivee`] ;
//! - écoutes : [`Library::journal_ecoutes`] ;
//! - familles : [`Library::familles_des_albums`], [`Library::familles`] ;
//! - proximité sonore : [`Library::album_embeddings`] (centroïdes CLAP).
//!
//! **Stable sur la journée** : chaque tirage part d'une graine faite de la
//! date locale, de l'id de la rangée et de son décalage ; le « ↻ » d'une
//! rangée n'augmente que le sien. Les seuils viennent de la mesure A0
//! (`docs/journal.md`, 22 nov. 2026).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::db::{hacher, AlbumRow, Library};
use crate::error::{Error, Result};

/// Albums au plus par rangée.
pub const TAILLE_RANGEE: usize = 20;
/// En dessous, la rangée ne s'affiche pas.
pub const MIN_RANGEE: usize = 4;
/// Rangées au plus sur la page.
pub const RANGEES_MAX: usize = 7;

const JOUR: i64 = 86_400;

/// Un album tel que la page le montre ; `name` et `artist` font la clé
/// d'album de l'interface (`cleAlbum`), comme pour [`AlbumRow`].
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AlbumAccueil {
    pub name: String,
    pub artist: Option<String>,
    pub year: Option<i64>,
    pub tracks: i64,
    /// Une piste de l'album, pour sa pochette.
    pub path: String,
    /// L'artiste d'album chez MusicBrainz (`mb_album_artist_id`), quand les
    /// tags le portent : l'image large de l'album à la une (A3) se résout par
    /// lui, jamais par le nom.
    pub artist_mbid: Option<String>,
    /// Ce qui distingue l'album dans sa rangée (« il y a 30 ans »), s'il y a lieu.
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RangeeAccueil {
    /// `oublies`, `anniversaire`, `epoque-1` à `epoque-3`, `jamais`,
    /// `parce-que`, `hasard`, `nouveaux` : la clé de son décalage.
    pub id: String,
    pub titre: String,
    pub raison: String,
    pub albums: Vec<AlbumAccueil>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AlaUne {
    /// La rangée d'où il vient.
    pub rangee: String,
    /// Le surtitre (« Sorti il y a 30 ans cette semaine »).
    pub raison: String,
    pub album: AlbumAccueil,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PageAccueil {
    /// La date locale qui a servi de graine, AAAA-MM-JJ.
    pub date: String,
    pub une: Option<AlaUne>,
    pub rangees: Vec<RangeeAccueil>,
}

/// Pistes distinctes qu'il faut écouter un même jour pour qu'un album de `n`
/// pistes compte comme écouté ce jour-là : trois, ou la moitié (A0).
pub fn pistes_requises(n: i64) -> i64 {
    let n = n.max(1);
    n.min(3).min((n + 1) / 2)
}

/// Le palier de R1 selon l'ancienneté du journal (A0 : un journal de 49 jours
/// vide la rangée même à trois mois) : `(journées minimum, jours sans
/// écoute, titre)`.
fn palier_oublies(anciennete_journal: i64) -> (usize, i64, &'static str) {
    if anciennete_journal < 183 {
        (1, 30, "Pas écoutés depuis un mois")
    } else if anciennete_journal < 730 {
        (3, 91, "Pas écoutés depuis trois mois")
    } else {
        (3, 365, "Pas écoutés depuis un an")
    }
}

/// Ce que la page sait d'un album.
struct Info {
    cle: u64,
    row: AlbumRow,
    /// Journées (jours depuis l'époque) où l'album compte comme écouté, triées.
    jours: Vec<i64>,
    /// Date d'œuvre MusicBrainz au jour près, AAAAMMJJ.
    date_jour: Option<u32>,
    famille: Option<i64>,
    popularite: Option<f64>,
    artist_mbid: Option<String>,
}

impl Info {
    fn derniere(&self) -> Option<i64> {
        self.jours.last().copied()
    }
    fn vers(&self, note: Option<String>) -> AlbumAccueil {
        AlbumAccueil {
            name: self.row.name.clone(),
            artist: self.row.artist.clone(),
            year: self.row.year,
            tracks: self.row.tracks,
            path: self.row.path.clone(),
            artist_mbid: self.artist_mbid.clone(),
            note,
        }
    }
}

/// Une rangée avant dédoublonnage : ses candidats, déjà dans l'ordre.
struct Vivier {
    id: String,
    titre: String,
    raison: String,
    /// `(clé d'album, note)`.
    albums: Vec<(u64, Option<String>)>,
    /// Le candidat à la une de cette rangée et son surtitre, tiré sans
    /// décalage : le « ↻ » d'une rangée ne change pas l'album à la une.
    une: Option<(u64, String)>,
}

impl Library {
    /// La page d'accueil du jour `date_locale` (AAAA-MM-JJ), pour le modèle
    /// d'empreintes `model`. `decalages` porte le nombre de « ↻ » de chaque
    /// rangée (par id) ; une rangée absente vaut 0.
    pub fn page_accueil(
        &self,
        model: &str,
        date_locale: &str,
        decalages: &HashMap<String, u32>,
    ) -> Result<PageAccueil> {
        let aujourdhui = jour_de_date(date_locale)
            .ok_or_else(|| Error::Parsing(format!("date locale : {date_locale}")))?;
        let fin_du_jour = aujourdhui * JOUR + JOUR - 1;
        let fixe = self.fixe_accueil(model)?;
        let infos = self.infos_accueil(&fixe)?;
        let decal = |id: &str| decalages.get(id).copied().unwrap_or(0);
        let graine = |id: &str| Alea::depuis(date_locale, id, decal(id));

        // Toujours parcourir les albums dans le même ordre : l'ordre d'une
        // HashMap change d'un appel à l'autre, le tirage ne doit pas.
        let mut ordre: Vec<&Info> = infos.values().collect();
        ordre.sort_by_key(|i| i.cle);

        let premiere_ecoute = self.conn.query_row("SELECT MIN(le) FROM ecoute", [], |r| {
            r.get::<_, Option<i64>>(0)
        })?;
        let anciennete = premiere_ecoute.map_or(0, |le| aujourdhui - le.div_euclid(JOUR));
        let ecoute_depuis = |i: &Info| i.derniere().map(|d| aujourdhui - d);
        let jamais_ecoute = |i: &Info| i.jours.is_empty();

        // R1 — oubliés.
        let (journees_min, delai, titre_r1) = palier_oublies(anciennete);
        let mut r1: Vec<&Info> = ordre
            .iter()
            .copied()
            .filter(|i| {
                i.jours.len() >= journees_min && ecoute_depuis(i).is_some_and(|e| e > delai)
            })
            .collect();
        r1.sort_by_key(|i| (std::cmp::Reverse(i.jours.len()), i.derniere(), i.cle));
        let une_r1 = r1
            .first()
            .map(|i| (i.cle, titre_r1.replace("Pas écoutés", "Pas écouté")));
        let mut r1: Vec<(u64, Option<String>)> = r1.iter().map(|i| (i.cle, None)).collect();
        tourner(&mut r1, decal("oublies"));

        // R3 — anniversaires, à ± 3 jours.
        let (annee_auj, doy_auj) = annee_et_rang(aujourdhui);
        let mut r3: Vec<(bool, i32, i64, u64)> = ordre
            .iter()
            .filter_map(|i| {
                let d = i.date_jour?;
                let doy = rang_dans_annee(d);
                let mut n = annee_auj - (d / 10_000) as i64;
                let ecart = doy - doy_auj;
                // La fenêtre enjambe le Nouvel An : l'anniversaire tombe dans
                // l'année voisine.
                let ecart = if ecart > 182 {
                    n -= 1;
                    ecart - 365
                } else if ecart < -182 {
                    n += 1;
                    ecart + 365
                } else {
                    ecart
                };
                (ecart.abs() <= 3 && n >= 1).then_some((annee_ronde(n), ecart.abs(), n, i.cle))
            })
            .collect();
        r3.sort_by_key(|&(rond, ecart, n, cle)| (!rond, ecart, std::cmp::Reverse(n), cle));
        let une_r3 = r3
            .first()
            .map(|&(_, _, n, cle)| (cle, format!("Sorti il y a {n} ans cette semaine")));
        let mut r3: Vec<(u64, Option<String>)> = r3
            .into_iter()
            .map(|(_, _, n, cle)| (cle, Some(format!("il y a {n} ans"))))
            .collect();
        tourner(&mut r3, decal("anniversaire"));

        // R2 — jamais écoutés, entrés il y a plus d'un mois ; pondéré par la
        // popularité.
        let ancien = |i: &Info| i.row.ajoute_le.is_none_or(|t| fin_du_jour - t >= 30 * JOUR);
        let r2 = melanger_pondere(
            ordre
                .iter()
                .filter(|i| jamais_ecoute(i) && ancien(i))
                .map(|i| (i.cle, 0.25 + i.popularite.unwrap_or(0.25)))
                .collect(),
            &mut graine("jamais"),
        );

        // R6 — arrivés ce mois-ci, pas encore écoutés.
        let mut r6: Vec<&Info> = ordre
            .iter()
            .copied()
            .filter(|i| jamais_ecoute(i) && !ancien(i))
            .collect();
        r6.sort_by_key(|i| (std::cmp::Reverse(i.row.ajoute_le), i.cle));
        let mut r6: Vec<(u64, Option<String>)> = r6.iter().map(|i| (i.cle, None)).collect();
        tourner(&mut r6, decal("nouveaux"));

        // R5 — au hasard, ce qui dort pèse plus. Les arrivées du mois ont
        // leur rangée (R6, plus bas dans la page) : R5 ne les prend pas.
        let poids_r5: Vec<(u64, f64)> = ordre
            .iter()
            .filter(|i| !jamais_ecoute(i) || ancien(i))
            .map(|i| {
                let w = match ecoute_depuis(i) {
                    None => 3.0,
                    Some(e) if e > 365 => 3.0,
                    Some(e) if e >= 30 => 1.0,
                    Some(_) => 0.1,
                };
                (i.cle, w)
            })
            .collect();
        let une_r5 = melanger_pondere(
            poids_r5.clone(),
            &mut Alea::depuis(date_locale, "hasard", 0),
        )
        .first()
        .map(|&c| (c, "Au hasard".to_string()));
        let r5 = melanger_pondere(poids_r5, &mut graine("hasard"));

        // R7 — voisins sonores de l'album le plus écouté ces 14 jours.
        let x = ordre
            .iter()
            .filter_map(|i| {
                let n = i.jours.iter().filter(|&&d| aujourdhui - d < 14).count();
                (n > 0).then_some((n, i.derniere(), i.cle))
            })
            .max()
            .map(|(_, _, cle)| &infos[&cle]);
        let r7 = match x {
            Some(x) => {
                let centroides = &fixe.centroides;
                let mut voisins: Vec<(f32, u64)> = match centroides.get(&x.cle) {
                    Some(cx) => ordre
                        .iter()
                        .filter(|i| {
                            i.row.artist != x.row.artist && ecoute_depuis(i).is_none_or(|e| e >= 30)
                        })
                        .filter_map(|i| Some((cosinus(cx, centroides.get(&i.cle)?), i.cle)))
                        .collect(),
                    None => Vec::new(),
                };
                voisins.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
                let mut v: Vec<(u64, Option<String>)> =
                    voisins.into_iter().map(|(_, c)| (c, None)).collect();
                tourner(&mut v, decal("parce-que"));
                Some(Vivier {
                    id: "parce-que".into(),
                    titre: format!("Parce que vous avez écouté {}", x.row.name),
                    raison: format!(
                        "Proches à l'oreille de {} ({}), écouté ces deux dernières semaines",
                        x.row.name,
                        x.row.artist.as_deref().unwrap_or("artiste inconnu")
                    ),
                    albums: v,
                    une: None,
                })
            }
            None => None,
        };

        let vivier = |id: &str, titre: &str, raison: &str, albums, une| Vivier {
            id: id.into(),
            titre: titre.into(),
            raison: raison.into(),
            albums,
            une,
        };
        let sans_note = |v: Vec<u64>| v.into_iter().map(|c| (c, None)).collect::<Vec<_>>();
        let v_r1 = vivier(
            "oublies",
            titre_r1,
            "Des albums que vous avez écoutés, puis laissés de côté",
            r1,
            une_r1,
        );
        let v_r3 = vivier(
            "anniversaire",
            "Sortis cette semaine",
            "Leur sortie a un anniversaire cette semaine, à trois jours près",
            r3,
            une_r3,
        );
        let v_r2 = vivier(
            "jamais",
            "Jamais écoutés",
            "Dans la bibliothèque depuis plus d'un mois, jamais écoutés",
            sans_note(r2),
            None,
        );
        let v_r5 = vivier(
            "hasard",
            "Au hasard",
            "Un tirage du jour, qui favorise ce qui dort",
            sans_note(r5),
            une_r5,
        );
        let v_r6 = vivier(
            "nouveaux",
            "Ajoutés récemment, pas encore écoutés",
            "Arrivés ces trente derniers jours",
            r6,
            None,
        );

        // R4 — familles × décennies : autant de rangées que la page en laisse
        // la place, entre une et trois.
        // R8 — l'actualité : ce que la passe Découvrir a déjà rangé, rien de
        // plus (aucune requête réseau ici).
        let r8 = self.vivier_actualite(&ordre, decal("actualite"))?;

        let autres = [&v_r1, &v_r3, &v_r2, &v_r5, &v_r6]
            .iter()
            .filter(|v| v.albums.len() >= MIN_RANGEE)
            .count()
            + [&r7, &r8]
                .iter()
                .filter(|v| v.as_ref().is_some_and(|v| v.albums.len() >= MIN_RANGEE))
                .count();
        let places = RANGEES_MAX.saturating_sub(autres).clamp(1, 3);
        let r4 = rangees_epoque(&fixe.noms, &ordre, places, date_locale, &decal);

        // L'ordre de la page : à la une, R1, R3, R4, R2, R7, R8, R5, R6.
        let mut viviers: Vec<Vivier> = vec![v_r1, v_r3];
        viviers.extend(r4);
        viviers.push(v_r2);
        viviers.extend(r7);
        viviers.extend(r8);
        viviers.push(v_r5);
        viviers.push(v_r6);

        // L'album à la une : R8, puis R3, puis R1, puis R5.
        let une = ["actualite", "anniversaire", "oublies", "hasard"]
            .iter()
            .find_map(|id| {
                let v = viviers.iter().find(|v| v.id == *id)?;
                let (cle, raison) = v.une.clone()?;
                Some(AlaUne {
                    rangee: v.id.clone(),
                    raison,
                    album: infos[&cle].vers(None),
                })
            });
        let mut pris: HashSet<u64> = HashSet::new();
        if let Some(u) = &une {
            pris.insert(hacher(
                u.album.artist.as_deref().unwrap_or(""),
                &u.album.name,
            ));
        }

        // Qui se sert le premier : les rangées ciblées (actualité,
        // anniversaires, oubliés, voisins, époques, arrivées), puis les deux
        // réservoirs tirés au hasard (jamais écoutés, au hasard). Dans l'ordre
        // de la page, « Jamais écoutés » passait avant l'actualité et lui
        // prenait les disques de l'artiste (vu en A4, comme R5 avec R6 en A1).
        // L'affichage, lui, garde l'ordre de la page.
        let priorite = |id: &str| match id {
            "actualite" => 0,
            "anniversaire" => 1,
            "oublies" => 2,
            "parce-que" => 3,
            id if id.starts_with("epoque") => 4,
            "nouveaux" => 5,
            "jamais" => 6,
            _ => 7, // hasard
        };
        let mut par_priorite: Vec<(usize, Vivier)> = viviers.into_iter().enumerate().collect();
        par_priorite.sort_by_key(|(rang, v)| (priorite(&v.id), *rang));
        let mut retenues: Vec<(usize, RangeeAccueil)> = Vec::new();
        for (rang, v) in par_priorite {
            let albums: Vec<(u64, Option<String>)> = v
                .albums
                .into_iter()
                .filter(|(c, _)| !pris.contains(c))
                .take(TAILLE_RANGEE)
                .collect();
            if albums.len() < MIN_RANGEE {
                continue;
            }
            pris.extend(albums.iter().map(|(c, _)| *c));
            retenues.push((
                rang,
                RangeeAccueil {
                    id: v.id,
                    titre: v.titre,
                    raison: v.raison,
                    albums: albums
                        .into_iter()
                        .map(|(c, note)| infos[&c].vers(note))
                        .collect(),
                },
            ));
        }
        retenues.sort_by_key(|(rang, _)| *rang);
        let rangees: Vec<RangeeAccueil> = retenues
            .into_iter()
            .map(|(_, r)| r)
            .take(RANGEES_MAX)
            .collect();

        Ok(PageAccueil {
            date: date_locale.to_string(),
            une,
            rangees,
        })
    }
}

/// Les rangées R4 : cases famille × décennie d'au moins huit albums,
/// coupées en demi-décennies au-delà de seize (A0) ; `places` rangées au
/// plus, chacune sur sa case tirée du jour.
fn rangees_epoque(
    noms: &HashMap<i64, String>,
    ordre: &[&Info],
    places: usize,
    date: &str,
    decal: &dyn Fn(&str) -> u32,
) -> Vec<Vivier> {
    let mut decennies: HashMap<(i64, i64), Vec<u64>> = HashMap::new();
    for i in ordre {
        if let (Some(f), Some(y)) = (i.famille, i.row.year) {
            decennies.entry((f, y / 10 * 10)).or_default().push(i.cle);
        }
    }
    // (famille, début, longueur) → albums.
    let mut cases: Vec<((i64, i64, i64), Vec<u64>)> = Vec::new();
    for ((f, d), albums) in decennies {
        if albums.len() >= 16 {
            for debut in [d, d + 5] {
                let moitie: Vec<u64> = albums
                    .iter()
                    .copied()
                    .filter(|c| {
                        let y = ordre
                            .binary_search_by_key(c, |i| i.cle)
                            .ok()
                            .and_then(|k| ordre[k].row.year);
                        y.is_some_and(|y| (debut..debut + 5).contains(&y))
                    })
                    .collect();
                if moitie.len() >= 8 {
                    cases.push(((f, debut, 5), moitie));
                }
            }
        } else if albums.len() >= 8 {
            cases.push(((f, d, 10), albums));
        }
    }
    cases.sort_by_key(|(k, _)| *k);
    if cases.is_empty() {
        return Vec::new();
    }
    let mut alea = Alea::depuis(date, "epoque", 0);
    let ordre_cases = melanger_pondere(
        (0..cases.len() as u64).map(|k| (k, 1.0)).collect(),
        &mut alea,
    );

    let mut prises: HashSet<u64> = HashSet::new();
    let mut viviers = Vec::new();
    for slot in 0..places.min(cases.len()) {
        let id = format!("epoque-{}", slot + 1);
        let depart = slot + decal(&id) as usize * places;
        let Some(k) = (0..ordre_cases.len())
            .map(|j| ordre_cases[(depart + j) % ordre_cases.len()])
            .find(|k| !prises.contains(k))
        else {
            break;
        };
        prises.insert(k);
        let ((f, debut, longueur), albums) = &cases[k as usize];
        let famille = noms
            .get(f)
            .cloned()
            .unwrap_or_else(|| format!("Famille {f}"));
        let periode = if *longueur == 10 {
            format!("années {debut}")
        } else {
            format!("{debut}–{}", debut + 4)
        };
        let tirage = melanger_pondere(
            albums.iter().map(|&c| (c, 1.0)).collect(),
            &mut Alea::depuis(date, &id, decal(&id)),
        );
        viviers.push(Vivier {
            titre: format!("{famille} · {periode}"),
            raison: format!("La famille {famille}, sortis en {periode}"),
            albums: tirage.into_iter().map(|c| (c, None)).collect(),
            une: None,
            id,
        });
    }
    viviers
}

impl Library {
    /// R8 : les artistes de la bibliothèque qui ont sorti un album ou un EP ces
    /// 30 derniers jours, d'après le fil Découvrir (`decouvrir_fil`), et
    /// leurs disques qu'on possède. Un artiste qui en a au moins
    /// [`MIN_RANGEE`] a sa rangée (« Nouvel album de X ») ; le « ↻ » passe au
    /// suivant. Sinon, une rangée commune, la sortie en note sous chaque
    /// album. `None` si la passe Découvrir n'a rien rangé.
    fn vivier_actualite(&self, ordre: &[&Info], decalage: u32) -> Result<Option<Vivier>> {
        let fil = self.decouvrir_fil(30)?;
        // Albums et EP seulement (« nouvel album »), le plus récent d'abord.
        let mut sorties: Vec<&crate::db::SortieFil> = fil
            .sorties
            .iter()
            .chain(&fil.collaborations)
            .filter(|s| matches!(s.type_primaire.as_deref(), Some("Album") | Some("EP")))
            .collect();
        sorties.sort_by(|a, b| {
            b.date_sortie
                .cmp(&a.date_sortie)
                .then(a.artiste_mbid.cmp(&b.artiste_mbid))
        });

        // Les disques possédés de chaque artiste, les plus écoutés d'abord.
        let mut artistes: Vec<(&crate::db::SortieFil, Vec<&Info>)> = Vec::new();
        for s in sorties {
            if artistes
                .iter()
                .any(|(a, _)| a.artiste_mbid == s.artiste_mbid)
            {
                continue;
            }
            let mut siens: Vec<&Info> = ordre
                .iter()
                .copied()
                .filter(|i| i.artist_mbid.as_deref() == Some(s.artiste_mbid.as_str()))
                .collect();
            if siens.is_empty() {
                continue;
            }
            siens.sort_by_key(|i| {
                (
                    std::cmp::Reverse(i.jours.len()),
                    std::cmp::Reverse(i.row.year),
                    i.cle,
                )
            });
            artistes.push((s, siens));
        }
        let Some((premier, siens)) = artistes.first() else {
            return Ok(None);
        };
        let nature = |s: &crate::db::SortieFil| {
            if s.type_primaire.as_deref() == Some("EP") {
                "Nouvel EP"
            } else {
                "Nouvel album"
            }
        };
        let une = Some((
            siens[0].cle,
            format!(
                "{} de {}\u{a0}: «\u{a0}{}\u{a0}»",
                nature(premier),
                premier.artiste_nom,
                premier.titre
            ),
        ));

        let pleins: Vec<&(&crate::db::SortieFil, Vec<&Info>)> = artistes
            .iter()
            .filter(|(_, l)| l.len() >= MIN_RANGEE)
            .collect();
        if !pleins.is_empty() {
            let (s, siens) = pleins[decalage as usize % pleins.len()];
            let date = date_en_francais(s.date_sortie.as_deref());
            return Ok(Some(Vivier {
                id: "actualite".into(),
                titre: format!("{} de {} · ses disques chez vous", nature(s), s.artiste_nom),
                raison: format!("«\u{a0}{}\u{a0}», sorti {date} (fil Découvrir)", s.titre),
                albums: siens.iter().map(|i| (i.cle, None)).collect(),
                une,
            }));
        }
        // Personne n'a assez de disques pour sa rangée : une rangée commune,
        // trois disques par artiste au plus.
        let mut albums: Vec<(u64, Option<String>)> = artistes
            .iter()
            .flat_map(|(s, l)| {
                l.iter().take(3).map(move |i| {
                    (
                        i.cle,
                        Some(format!("nouveau\u{a0}: «\u{a0}{}\u{a0}»", s.titre)),
                    )
                })
            })
            .collect();
        tourner(&mut albums, decalage);
        Ok(Some(Vivier {
            id: "actualite".into(),
            titre: "Ils viennent de sortir un disque".into(),
            raison: "Des artistes de vos albums, d'après le fil Découvrir (30 derniers jours)"
                .into(),
            albums,
            une,
        }))
    }

    /// Ce qui ne change pas d'une écoute à l'autre, mis en cache tant que la
    /// base n'a pas bougé (voir [`Self::empreinte_accueil`]) : c'est le plus
    /// cher de la page (familles, centroïdes CLAP). Le journal, lui, est relu
    /// à chaque appel.
    fn fixe_accueil(&self, model: &str) -> Result<Arc<Fixe>> {
        static CACHE: Mutex<Option<(String, Arc<Fixe>)>> = Mutex::new(None);
        // Une base en mémoire (les tests) n'a pas de chemin : jamais en cache.
        let cle = match self.conn.path().filter(|p| !p.is_empty()) {
            Some(chemin) => Some(format!(
                "{chemin}\u{1f}{model}\u{1f}{}",
                self.empreinte_accueil(model)?
            )),
            None => None,
        };
        if let Some(cle) = &cle {
            let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((k, fixe)) = cache.as_ref() {
                if k == cle {
                    return Ok(fixe.clone());
                }
            }
        }
        let fixe = Arc::new(self.calculer_fixe(model)?);
        if let Some(cle) = cle {
            *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some((cle, fixe.clone()));
        }
        Ok(fixe)
    }

    /// Ce qui, en changeant, périme le cache : pistes (ajout, retrait,
    /// retouche), projection et familles, popularité, release-groups.
    fn empreinte_accueil(&self, model: &str) -> Result<String> {
        Ok(self.conn.query_row(
            "SELECT (SELECT COUNT(*) || '/' || IFNULL(MAX(id), 0) || '/' || IFNULL(MAX(mtime), 0)
                       || '/' || IFNULL(SUM(id * 31 + mtime), 0) FROM tracks)
                    || '|' || (SELECT COUNT(*) || '/' || IFNULL(MAX(computed_at), 0)
                       FROM features WHERE model = ?1)
                    || '|' || (SELECT COUNT(*) || '/' || IFNULL(MAX(calcule_le), 0) FROM track_popularite)
                    || '|' || (SELECT COUNT(*) FROM mb_release_groups)",
            [model],
            |r| r.get(0),
        )?)
    }

    fn calculer_fixe(&self, model: &str) -> Result<Fixe> {
        let mut albums: HashMap<u64, InfoFixe> = self
            .albums(None)?
            .into_iter()
            .map(|row| {
                let cle = hacher(row.artist.as_deref().unwrap_or(""), &row.name);
                (
                    cle,
                    InfoFixe {
                        row,
                        date_jour: None,
                        famille: None,
                        popularite: None,
                        artist_mbid: None,
                    },
                )
            })
            .collect();

        let mut album_de_piste: HashMap<i64, u64> = HashMap::new();
        for a in self.ordre_darrivee()? {
            let Some(info) = albums.get_mut(&a.album) else {
                continue;
            };
            album_de_piste.insert(a.track_id, a.album);
            let au_jour = a.date % 100 != 0 && (a.date / 100) % 100 != 0;
            if a.source == "musicbrainz" && au_jour {
                info.date_jour = Some(info.date_jour.map_or(a.date, |d| d.min(a.date)));
            }
        }

        for (album, artiste, cluster) in self.familles_des_albums(model)? {
            if let Some(i) = albums.get_mut(&hacher(artiste.as_deref().unwrap_or(""), &album)) {
                i.famille = Some(cluster);
            }
        }

        let mut stmt = self.conn.prepare(
            "SELECT album, COALESCE(album_artist, artist), MIN(mb_album_artist_id)
               FROM tracks
              WHERE album IS NOT NULL AND mb_album_artist_id IS NOT NULL
              GROUP BY album, COALESCE(album_artist, artist)",
        )?;
        let mbids = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for m in mbids {
            let (album, artiste, mbid) = m?;
            if let Some(i) = albums.get_mut(&hacher(artiste.as_deref().unwrap_or(""), &album)) {
                i.artist_mbid = Some(mbid);
            }
        }

        let mut stmt = self.conn.prepare(
            "SELECT t.album, COALESCE(t.album_artist, t.artist), AVG(tp.relative)
               FROM tracks t JOIN track_popularite tp ON tp.track_id = t.id
              WHERE t.album IS NOT NULL
              GROUP BY t.album, COALESCE(t.album_artist, t.artist)",
        )?;
        let pops = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, f64>(2)?,
            ))
        })?;
        for p in pops {
            let (album, artiste, pop) = p?;
            if let Some(i) = albums.get_mut(&hacher(artiste.as_deref().unwrap_or(""), &album)) {
                i.popularite = Some(pop);
            }
        }

        Ok(Fixe {
            albums,
            album_de_piste,
            noms: self
                .familles(model)?
                .into_iter()
                .map(|(c, nom, _)| (c, nom))
                .collect(),
            centroides: self
                .album_embeddings(model)?
                .into_iter()
                .map(|(id, v)| (id as u64, v))
                .collect(),
        })
    }

    /// Les albums avec leurs journées d'écoute, d'après le journal du moment.
    fn infos_accueil(&self, fixe: &Fixe) -> Result<HashMap<u64, Info>> {
        let mut pistes_du_jour: HashMap<(u64, i64), HashSet<i64>> = HashMap::new();
        for (piste, le) in self.journal_ecoutes()? {
            if let Some(&album) = fixe.album_de_piste.get(&piste) {
                pistes_du_jour
                    .entry((album, le.div_euclid(JOUR)))
                    .or_default()
                    .insert(piste);
            }
        }
        let mut jours: HashMap<u64, Vec<i64>> = HashMap::new();
        for ((album, jour), pistes) in pistes_du_jour {
            if pistes.len() as i64 >= pistes_requises(fixe.albums[&album].row.tracks) {
                jours.entry(album).or_default().push(jour);
            }
        }
        Ok(fixe
            .albums
            .iter()
            .map(|(&cle, f)| {
                let mut j = jours.remove(&cle).unwrap_or_default();
                j.sort_unstable();
                (
                    cle,
                    Info {
                        cle,
                        row: f.row.clone(),
                        jours: j,
                        date_jour: f.date_jour,
                        famille: f.famille,
                        popularite: f.popularite,
                        artist_mbid: f.artist_mbid.clone(),
                    },
                )
            })
            .collect())
    }
}

/// La part de la page qui ne dépend pas du journal.
struct Fixe {
    albums: HashMap<u64, InfoFixe>,
    album_de_piste: HashMap<i64, u64>,
    /// Nom affiché de chaque famille.
    noms: HashMap<i64, String>,
    /// Centroïde CLAP de chaque album.
    centroides: HashMap<u64, Vec<f32>>,
}

struct InfoFixe {
    artist_mbid: Option<String>,
    row: AlbumRow,
    date_jour: Option<u32>,
    famille: Option<i64>,
    popularite: Option<f64>,
}

/// Une date MusicBrainz, brute (« 2026-11-06 », « 2026-11 », « 2026 »), en
/// français : « le 6 novembre 2026 », « en novembre 2026 », « en 2026 » ;
/// rien de connu : « ce mois-ci » (la fenêtre du fil).
fn date_en_francais(date: Option<&str>) -> String {
    const MOIS: [&str; 12] = [
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ];
    let mut p = date.unwrap_or("").split('-');
    let annee = p
        .next()
        .filter(|a| a.len() == 4 && a.bytes().all(|b| b.is_ascii_digit()));
    let mois = p
        .next()
        .and_then(|m| m.parse::<usize>().ok())
        .filter(|m| (1..=12).contains(m));
    let jour = p
        .next()
        .and_then(|j| j.parse::<u32>().ok())
        .filter(|j| (1..=31).contains(j));
    match (annee, mois, jour) {
        (Some(a), Some(m), Some(j)) => format!(
            "le {} {} {a}",
            if j == 1 {
                "1er".to_string()
            } else {
                j.to_string()
            },
            MOIS[m - 1]
        ),
        (Some(a), Some(m), None) => format!("en {} {a}", MOIS[m - 1]),
        (Some(a), None, _) => format!("en {a}"),
        _ => "ce mois-ci".into(),
    }
}

/// 10, 20, 25, 30, 40… : les anniversaires qu'on fête.
fn annee_ronde(n: i64) -> bool {
    n >= 10 && (n % 10 == 0 || n == 25)
}

/// Fait passer la rangée triée de `decalage` pages : le « ↻ » d'une rangée
/// sans hasard montre la suite plutôt que la même chose.
fn tourner<T>(v: &mut [T], decalage: u32) {
    if !v.is_empty() {
        let k = (decalage as usize * TAILLE_RANGEE) % v.len();
        v.rotate_left(k);
    }
}

fn cosinus(a: &[f32], b: &[f32]) -> f32 {
    let (mut ab, mut aa, mut bb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    if aa == 0.0 || bb == 0.0 {
        0.0
    } else {
        ab / (aa.sqrt() * bb.sqrt())
    }
}

/// Générateur splitmix64 : assez bon pour tirer des pochettes, et
/// reproductible d'une plateforme à l'autre.
struct Alea(u64);

impl Alea {
    fn depuis(date: &str, rangee: &str, decalage: u32) -> Self {
        Alea(hacher(date, &format!("{rangee}\u{1f}{decalage}")))
    }
    fn suivant(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    /// Uniforme dans ]0, 1].
    fn unite(&mut self) -> f64 {
        ((self.suivant() >> 11) + 1) as f64 / (1u64 << 53) as f64
    }
}

/// Mélange pondéré sans remise (Efraimidis–Spirakis) : `u^(1/w)` par
/// élément, puis tri décroissant. Les entrées doivent arriver dans un ordre
/// fixe pour que le tirage le soit.
fn melanger_pondere(entrees: Vec<(u64, f64)>, alea: &mut Alea) -> Vec<u64> {
    let mut cles: Vec<(f64, u64)> = entrees
        .into_iter()
        .map(|(c, w)| (alea.unite().ln() / w.max(1e-6), c))
        .collect();
    cles.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    cles.into_iter().map(|(_, c)| c).collect()
}

/// `AAAA-MM-JJ` → jours depuis le 1970-01-01 (algorithme de Howard Hinnant,
/// l'inverse de [`crate::db::epoch_vers_cle`]).
fn jour_de_date(date: &str) -> Option<i64> {
    let mut p = date.trim().split('-');
    let a: i64 = p.next()?.parse().ok()?;
    let m: i64 = p.next()?.parse().ok()?;
    let j: i64 = p.next()?.parse().ok()?;
    if p.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&j) {
        return None;
    }
    let a = if m <= 2 { a - 1 } else { a };
    let ere = a.div_euclid(400);
    let yoe = a - ere * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + j - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(ere * 146_097 + doe - 719_468)
}

/// L'année et le rang du jour `jour` (jours depuis l'époque).
fn annee_et_rang(jour: i64) -> (i64, i32) {
    let cle = crate::db::epoch_vers_cle(jour * JOUR);
    ((cle / 10_000) as i64, rang_dans_annee(cle))
}

/// Rang d'une clé AAAAMMJJ dans une année de 365 jours (le 29 février se
/// confond avec le 1er mars).
fn rang_dans_annee(cle: u32) -> i32 {
    const CUMUL: [i32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let m = ((cle / 100) % 100).clamp(1, 12) as usize;
    let j = (cle % 100).clamp(1, 31) as i32;
    (CUMUL[m - 1] + j - 1).min(364)
}

#[cfg(test)]
mod tests;
