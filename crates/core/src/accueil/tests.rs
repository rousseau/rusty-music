// SPDX-License-Identifier: GPL-3.0-or-later
//! La page d'accueil sur une base fabriquée : chaque rangée pleine et vide,
//! le dédoublonnage, la stabilité dans la journée.

use super::*;
use crate::TrackMeta;

const MODELE: &str = "clap";
const DATE: &str = "2026-11-22";

fn jour() -> i64 {
    jour_de_date(DATE).unwrap()
}

struct Base {
    lib: Library,
    n: i64,
}

impl Base {
    fn new() -> Self {
        Base {
            lib: Library::open_in_memory().unwrap(),
            n: 0,
        }
    }

    /// Un album de `pistes` pistes, arrivé il y a `age` jours, avec une
    /// empreinte `vecteur` et une famille `cluster` pour chaque piste.
    fn album(
        &mut self,
        artiste: &str,
        nom: &str,
        pistes: i64,
        annee: i64,
        age: i64,
        cluster: Option<(i64, [f32; 2])>,
    ) {
        for k in 1..=pistes {
            self.n += 1;
            let id = self
                .lib
                .upsert(&TrackMeta {
                    path: format!("/m/{artiste}/{nom}/{k}.flac").into(),
                    title: Some(format!("{nom} {k}")),
                    album: Some(nom.into()),
                    album_artist: Some(artiste.into()),
                    artist: Some(artiste.into()),
                    mb_album_artist_id: Some(format!("mb-{artiste}")),
                    year: Some(annee),
                    track_no: Some(k),
                    mtime: (jour() - age) * JOUR,
                    ..Default::default()
                })
                .unwrap();
            if let Some((c, v)) = cluster {
                self.lib.save_features(id, MODELE, &v, 0.0, 0.0, c).unwrap();
            }
        }
    }

    /// La date d'œuvre MusicBrainz de l'album, au jour près.
    fn dater(&self, artiste: &str, nom: &str, date: &str) {
        self.lib
            .conn
            .execute(
                "INSERT INTO mb_release_groups (mbid, artist_mbid, title, title_norm, first_release_date)
                 VALUES (?1, ?2, ?3, titre_normalise(?3), ?4)",
                rusqlite::params![format!("rg-{artiste}-{nom}"), format!("mb-{artiste}"), nom, date],
            )
            .unwrap();
    }

    /// `pistes` pistes de l'album écoutées il y a `il_y_a` jours.
    fn ecouter(&self, artiste: &str, nom: &str, il_y_a: i64, pistes: i64) {
        for k in 1..=pistes {
            self.lib
                .conn
                .execute(
                    "INSERT INTO ecoute(chemin, le) VALUES (?1, ?2)",
                    rusqlite::params![
                        format!("/m/{artiste}/{nom}/{k}.flac"),
                        (jour() - il_y_a) * JOUR + 3600
                    ],
                )
                .unwrap();
        }
    }

    fn page(&self) -> PageAccueil {
        self.page_du(DATE, &HashMap::new())
    }

    fn page_du(&self, date: &str, decalages: &HashMap<String, u32>) -> PageAccueil {
        self.lib.page_accueil(MODELE, date, decalages).unwrap()
    }
}

fn rangee<'a>(p: &'a PageAccueil, id: &str) -> Option<&'a RangeeAccueil> {
    p.rangees.iter().find(|r| r.id == id)
}

fn noms(r: &RangeeAccueil) -> Vec<&str> {
    r.albums.iter().map(|a| a.name.as_str()).collect()
}

/// Chaque album n'apparaît qu'une fois sur la page, à la une comprise.
fn sans_doublon(p: &PageAccueil) {
    let mut vus = HashSet::new();
    let tous = p
        .une
        .iter()
        .map(|u| &u.album)
        .chain(p.rangees.iter().flat_map(|r| &r.albums));
    for a in tous {
        assert!(
            vus.insert((a.name.clone(), a.artist.clone())),
            "{} en double",
            a.name
        );
    }
}

#[test]
fn la_date_se_lit_et_se_relit() {
    assert_eq!(jour_de_date("1970-01-01"), Some(0));
    assert_eq!(jour_de_date("2000-03-01"), Some(11_017));
    let j = jour();
    assert_eq!(crate::db::epoch_vers_cle(j * JOUR), 20_261_122);
    assert_eq!(jour_de_date("2026-13-01"), None);
    assert_eq!(jour_de_date("hier"), None);
}

#[test]
fn le_seuil_d_ecoute_d_un_album() {
    assert_eq!(pistes_requises(1), 1);
    assert_eq!(pistes_requises(2), 1);
    assert_eq!(pistes_requises(4), 2);
    assert_eq!(pistes_requises(5), 3);
    assert_eq!(pistes_requises(12), 3);
}

#[test]
fn une_bibliotheque_vide_donne_une_page_vide() {
    let b = Base::new();
    let p = b.page();
    assert!(p.une.is_none());
    assert!(p.rangees.is_empty());
    assert!(b
        .lib
        .page_accueil(MODELE, "n'importe", &HashMap::new())
        .is_err());
}

#[test]
fn jamais_ecoutes_et_nouveaux_se_partagent_selon_l_arrivee() {
    let mut b = Base::new();
    for k in 0..8 {
        b.album("Anciens", &format!("Vieux {k}"), 4, 2001, 400, None);
    }
    for k in 0..5 {
        b.album("Neufs", &format!("Neuf {k}"), 4, 2020, 3 + k, None);
    }
    let p = b.page();
    sans_doublon(&p);
    let r6 = rangee(&p, "nouveaux").expect("R6");
    assert_eq!(
        noms(r6),
        vec!["Neuf 0", "Neuf 1", "Neuf 2", "Neuf 3", "Neuf 4"],
        "le plus récent d'abord"
    );
    let r2 = rangee(&p, "jamais").expect("R2");
    assert!(r2.albums.iter().all(|a| a.name.starts_with("Vieux")));
    // La une (au hasard, faute de mieux) vient de l'un d'eux ; R5 n'a plus
    // rien à montrer qui ne soit déjà ailleurs.
    let une = p.une.as_ref().expect("une");
    assert_eq!(
        (une.rangee.as_str(), une.raison.as_str()),
        ("hasard", "Au hasard")
    );
    assert!(rangee(&p, "hasard").is_none());
    assert_eq!(r2.albums.len() + 5 + 1, 13);
}

#[test]
fn une_rangee_de_moins_de_quatre_albums_ne_s_affiche_pas() {
    let mut b = Base::new();
    for k in 0..3 {
        b.album("Neufs", &format!("Neuf {k}"), 4, 2020, 2, None);
    }
    let p = b.page();
    assert!(rangee(&p, "nouveaux").is_none());
}

#[test]
fn les_oublies_suivent_le_palier_du_journal() {
    let mut b = Base::new();
    for k in 0..6 {
        b.album("Écoutés", &format!("Écouté {k}"), 6, 2000, 400, None);
    }
    // Journal de 100 jours : palier d'un mois, une journée suffit.
    b.ecouter("Écoutés", "Écouté 0", 100, 3);
    b.ecouter("Écoutés", "Écouté 0", 90, 3);
    for k in 1..4 {
        b.ecouter("Écoutés", &format!("Écouté {k}"), 40 + k, 3);
    }
    b.ecouter("Écoutés", "Écouté 4", 5, 3); // trop récent
    b.ecouter("Écoutés", "Écouté 5", 60, 2); // 2 pistes sur 6 : pas écouté
    let p = b.page();
    // Le plus écouté passe à la une ; le reste par nombre de journées puis
    // par ancienneté.
    let une = p.une.as_ref().unwrap();
    assert_eq!(une.rangee, "oublies");
    assert_eq!(une.raison, "Pas écouté depuis un mois");
    assert_eq!(une.album.name, "Écouté 0");
    // Trois oubliés restants : moins de quatre, la rangée tombe.
    assert!(rangee(&p, "oublies").is_none());

    b.album("Écoutés", "Écouté 6", 6, 2000, 400, None);
    b.ecouter("Écoutés", "Écouté 6", 70, 3);
    let p = b.page();
    let r1 = rangee(&p, "oublies").expect("R1");
    assert_eq!(r1.titre, "Pas écoutés depuis un mois");
    assert_eq!(
        noms(r1),
        vec!["Écouté 6", "Écouté 3", "Écouté 2", "Écouté 1"]
    );
    assert_eq!(palier_oublies(200).1, 91);
    assert_eq!(palier_oublies(800).1, 365);
}

#[test]
fn les_anniversaires_a_trois_jours_pres_annees_rondes_en_tete() {
    let mut b = Base::new();
    let sorties = [
        ("A", "1996-11-24"), // 30 ans, rond
        ("B", "2003-11-22"), // 23 ans
        ("C", "2016-11-19"), // 10 ans, rond
        ("D", "1990-11-26"), // 36 ans
        ("E", "2001-11-30"), // hors fenêtre
        ("F", "2026-11-22"), // N = 0
    ];
    for (nom, date) in sorties {
        b.album("Dates", nom, 4, 2000, 400, None);
        b.dater("Dates", nom, date);
    }
    let p = b.page();
    let une = p.une.as_ref().unwrap();
    assert_eq!(une.rangee, "anniversaire");
    assert_eq!(une.raison, "Sorti il y a 30 ans cette semaine");
    assert_eq!(une.album.name, "A");
    assert_eq!(
        une.album.artist_mbid.as_deref(),
        Some("mb-Dates"),
        "pour l'image large (A3)"
    );
    // Restent C (10 ans) et B (23 ans, le jour même) ; D est à 4 jours, hors
    // fenêtre : deux albums, la rangée tombe.
    assert!(rangee(&p, "anniversaire").is_none());

    b.album("Dates", "G", 4, 2000, 400, None);
    b.dater("Dates", "G", "1976-11-21");
    b.album("Dates", "H", 4, 2000, 400, None);
    b.dater("Dates", "H", "2011-11-25");
    let p = b.page();
    let r3 = rangee(&p, "anniversaire").expect("R3");
    // G (50 ans, à un jour) passe devant A à la une ; puis les ronds, puis
    // les autres par proximité du jour.
    let une = p.une.as_ref().unwrap();
    assert_eq!(
        (une.album.name.as_str(), une.raison.as_str()),
        ("G", "Sorti il y a 50 ans cette semaine")
    );
    assert_eq!(noms(r3), vec!["A", "C", "B", "H"]);
    assert_eq!(r3.albums[0].note.as_deref(), Some("il y a 30 ans"));
}

#[test]
fn l_anniversaire_enjambe_le_nouvel_an() {
    let mut b = Base::new();
    b.album("Dates", "Réveillon", 4, 1996, 400, None);
    b.dater("Dates", "Réveillon", "1996-12-30");
    let p = b.page_du("2027-01-01", &HashMap::new());
    let une = p.une.unwrap();
    assert_eq!(une.raison, "Sorti il y a 30 ans cette semaine");
}

#[test]
fn les_epoques_croisent_famille_et_decennie() {
    let mut b = Base::new();
    for k in 0..9 {
        b.album(
            "Rock",
            &format!("R{k}"),
            3,
            1990 + k,
            400,
            Some((0, [1.0, 0.0])),
        );
    }
    for k in 0..5 {
        b.album(
            "Jazz",
            &format!("J{k}"),
            3,
            1960 + k,
            400,
            Some((1, [0.0, 1.0])),
        );
    }
    let p = b.page();
    sans_doublon(&p);
    let r4 = rangee(&p, "epoque-1").expect("R4");
    assert!(r4.titre.ends_with("· années 1990"), "{}", r4.titre);
    assert!(r4.albums.iter().all(|a| a.name.starts_with('R')));
    assert!(
        rangee(&p, "epoque-2").is_none(),
        "le jazz n'a que cinq albums"
    );
}

#[test]
fn une_grosse_case_se_coupe_en_demi_decennies() {
    let mut b = Base::new();
    for k in 0..18 {
        b.album(
            "Rock",
            &format!("R{k}"),
            3,
            1990 + k % 10,
            400,
            Some((0, [1.0, 0.0])),
        );
    }
    let p = b.page();
    let titres: Vec<&str> = p
        .rangees
        .iter()
        .filter(|r| r.id.starts_with("epoque"))
        .map(|r| r.titre.as_str())
        .collect();
    assert!(
        titres
            .iter()
            .all(|t| t.ends_with("1990–1994") || t.ends_with("1995–1999")),
        "{titres:?}"
    );
    assert!(!titres.is_empty());
}

#[test]
fn parce_que_cherche_les_voisins_d_un_autre_artiste() {
    let mut b = Base::new();
    b.album("Moi", "Écouté", 4, 2000, 400, Some((0, [1.0, 0.0])));
    b.album("Moi", "Même artiste", 4, 2000, 400, Some((0, [1.0, 0.0])));
    // Arrivés il y a dix jours : ni R2 ni R5 ne les prennent avant R7.
    for (k, v) in [0.9f32, 0.8, 0.7, 0.6, 0.1].iter().enumerate() {
        b.album(
            &format!("Autre {k}"),
            &format!("Voisin {k}"),
            4,
            1950 + 10 * k as i64, // une décennie chacun : pas de case R4
            10,
            Some((0, [*v, 1.0 - v])),
        );
    }
    b.album("Récent", "Écouté hier", 4, 2000, 400, Some((0, [1.0, 0.0])));
    b.ecouter("Moi", "Écouté", 2, 4);
    b.ecouter("Moi", "Écouté", 3, 4);
    b.ecouter("Récent", "Écouté hier", 1, 4);
    let p = b.page();
    let r7 = rangee(&p, "parce-que").unwrap_or_else(|| panic!("R7 : {p:#?}"));
    assert_eq!(r7.titre, "Parce que vous avez écouté Écouté");
    assert_eq!(
        noms(r7),
        vec!["Voisin 0", "Voisin 1", "Voisin 2", "Voisin 3", "Voisin 4"]
    );
}

#[test]
fn parce_que_est_absente_sans_ecoute_recente() {
    let mut b = Base::new();
    for k in 0..6 {
        b.album("A", &format!("A{k}"), 4, 2000, 400, Some((0, [1.0, 0.0])));
    }
    b.ecouter("A", "A0", 20, 4);
    assert!(rangee(&b.page(), "parce-que").is_none());
}

fn grande_base() -> Base {
    let mut b = Base::new();
    for k in 0..60 {
        b.album(
            &format!("Artiste {}", k % 7),
            &format!("Album {k}"),
            4,
            1990 + k % 30,
            400,
            None,
        );
    }
    b
}

#[test]
fn la_page_est_stable_dans_la_journee_et_change_le_lendemain() {
    let b = grande_base();
    let p1 = b.page();
    let p2 = b.page();
    assert_eq!(p1, p2);
    sans_doublon(&p1);
    let demain = b.page_du("2026-11-23", &HashMap::new());
    assert_ne!(
        rangee(&p1, "jamais").map(noms),
        rangee(&demain, "jamais").map(noms)
    );
}

#[test]
fn le_bouton_retirer_ne_change_que_sa_rangee() {
    let b = grande_base();
    let p = b.page();
    let decal = HashMap::from([("hasard".to_string(), 1)]);
    let q = b.page_du(DATE, &decal);
    assert_eq!(p.une, q.une, "la une ne bouge pas");
    assert_eq!(rangee(&p, "jamais"), rangee(&q, "jamais"));
    assert_ne!(rangee(&p, "hasard"), rangee(&q, "hasard"));
    sans_doublon(&q);
}

#[test]
fn sept_rangees_au_plus() {
    let mut b = Base::new();
    // De quoi remplir chaque rangée.
    for k in 0..40 {
        b.album(
            &format!("Vieux {}", k % 9),
            &format!("V{k}"),
            4,
            1990 + k % 10,
            400,
            Some((0, [1.0, k as f32 / 40.0])),
        );
    }
    for k in 0..30 {
        b.album(
            &format!("Jazz {}", k % 9),
            &format!("J{k}"),
            4,
            1960 + k % 10,
            400,
            Some((1, [0.0, 1.0])),
        );
    }
    for k in 0..6 {
        b.album("Neufs", &format!("N{k}"), 4, 2020, 2, None);
    }
    for k in 0..8 {
        b.ecouter(
            &format!("Vieux {}", k % 9),
            &format!("V{k}"),
            50 + k as i64,
            4,
        );
    }
    b.ecouter("Vieux 8", "V8", 2, 4);
    for k in 0..6 {
        let nom = format!("D{k}");
        b.album("Dates", &nom, 4, 2000, 400, None);
        b.dater("Dates", &nom, &format!("19{}6-11-2{k}", 5 + k % 4));
    }
    let p = b.page();
    sans_doublon(&p);
    assert!(p.rangees.len() <= RANGEES_MAX);
    let ids: Vec<&str> = p.rangees.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids[..2], ["oublies", "anniversaire"], "{ids:?}");
    assert!(ids.contains(&"epoque-1"), "{ids:?}");
}

#[test]
fn le_cache_suit_le_journal_et_les_arrivees() {
    let chemin =
        std::env::temp_dir().join(format!("rusty-music-accueil-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&chemin);
    let mut b = Base {
        lib: Library::open(&chemin).unwrap(),
        n: 0,
    };
    for k in 0..6 {
        b.album("Neufs", &format!("N{k}"), 4, 2020, 2, None);
    }
    let p = b.page();
    // Rien d'ancien : ni R5 ni album à la une, les six sont dans R6.
    assert!(p.une.is_none());
    assert_eq!(rangee(&p, "nouveaux").unwrap().albums.len(), 6);
    assert_eq!(p, b.page(), "même page depuis le cache");

    // Une écoute : relue à chaque appel, sans attendre le cache.
    b.ecouter("Neufs", "N0", 0, 4);
    b.ecouter("Neufs", "N1", 0, 4);
    let p = b.page();
    let r6 = rangee(&p, "nouveaux").unwrap();
    assert!(!noms(r6).contains(&"N0") && !noms(r6).contains(&"N1"));

    // Un album de plus périme le cache.
    b.album("Neufs", "N6", 4, 2020, 1, None);
    let p = b.page();
    let tous: Vec<&str> = p
        .une
        .iter()
        .map(|u| u.album.name.as_str())
        .chain(p.rangees.iter().flat_map(noms))
        .collect();
    assert!(tous.contains(&"N6"), "{tous:?}");
    drop(b);
    let _ = std::fs::remove_file(&chemin);
}
