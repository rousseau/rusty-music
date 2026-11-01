// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests de la bibliothèque, sur une base en mémoire.

use super::bibliotheque::{histogrammer, titre_album_normalise, TEMPO_MIN, TEMPO_PAS};
use super::carte::{capitaliser, nommer_les_familles, se_redisent};
use super::enrichissement::rangs_percentiles;
use super::schema::{migrate, migrations};
use super::*;

/// L'ordre des trois sources, éprouvé cas par cas. C'est la décision du
/// 17 août : MusicBrainz d'abord, tag du fichier en repli.
#[test]
fn lalbum_lemporte_sur_lartiste_et_lartiste_sur_le_fichier() {
    let artiste = "mbid-radiohead";
    let albums = HashMap::from([
        (
            (artiste.to_string(), "kida".to_string()),
            "rg-kida".to_string(),
        ),
        (
            (artiste.to_string(), "okcomputer".to_string()),
            "rg-okc".to_string(),
        ),
    ]);
    let par_album = HashMap::from([
        (
            "rg-kida".to_string(),
            vec!["electronic".to_string(), "art rock".to_string()],
        ),
        // Un release-group connu mais sans genre : il ne doit pas capturer
        // le morceau, sinon celui-ci perdrait les genres de son artiste.
        ("rg-okc".to_string(), Vec::new()),
    ]);
    let par_artiste = HashMap::from([(artiste.to_string(), vec!["alternative rock".to_string()])]);

    let g = |album, tag| {
        genres_du_morceau(Some(artiste), album, tag, &albums, &par_album, &par_artiste)
    };

    // 1. l'album quand il sait — et son meilleur genre seulement : « art
    //    rock », deuxième de la liste, ne suit pas. Voir GENRES_PAR_ENTITE.
    assert_eq!(g(Some("Kid A"), Some("Rock")), ["electronic"]);
    // 2. l'artiste quand l'album ne sait pas
    assert_eq!(g(Some("OK Computer"), Some("Rock")), ["alternative rock"]);
    // 3. l'artiste aussi quand l'album nous est inconnu
    assert_eq!(g(Some("Amnesiac"), Some("Rock")), ["alternative rock"]);

    // Le titre se rapproche malgré les mentions d'édition et la casse.
    assert_eq!(g(Some("KID A (remaster)"), None), ["electronic"]);
}

/// Le repli n'est pas décoratif : les chanteurs bretons de la
/// bibliothèque de test n'ont aucun genre chez MusicBrainz, et le tag du
/// fichier est alors la seule chose qui sache de quoi il s'agit.
#[test]
fn le_tag_du_fichier_sauve_ce_que_musicbrainz_ignore() {
    let sans_album: HashMap<(String, String), String> = HashMap::new();
    let sans_genre: HashMap<String, Vec<String>> = HashMap::new();
    let g = genres_du_morceau(
        Some("mbid-inconnu"),
        Some("Kan ha diskan"),
        Some("Traditional"),
        &sans_album,
        &sans_genre,
        &sans_genre,
    );
    assert_eq!(g, ["Traditional"]);

    // Et un morceau que personne ne sait nommer ne rend rien plutôt
    // qu'une étiquette inventée.
    let rien = |tag| genres_du_morceau(None, None, tag, &sans_album, &sans_genre, &sans_genre);
    assert!(rien(None).is_empty());
    assert!(rien(Some("")).is_empty());
}

/// On ne verse pas les deux vocabulaires dans le même sac : « Rock » du
/// fichier et « rock » de MusicBrainz se cumuleraient, et le genre le plus
/// grossier redeviendrait le plus lourd.
#[test]
fn les_sources_ne_se_melangent_pas_pour_un_meme_morceau() {
    let par_artiste = HashMap::from([("a".to_string(), vec!["boom bap".to_string()])]);
    let sans_album: HashMap<(String, String), String> = HashMap::new();
    let sans_genre: HashMap<String, Vec<String>> = HashMap::new();
    let g = genres_du_morceau(
        Some("a"),
        None,
        Some("Rock"),
        &sans_album,
        &sans_genre,
        &par_artiste,
    );
    assert_eq!(g, ["boom bap"], "le tag du fichier ne doit pas s'ajouter");
}

/// Les deux sources n'écrivent pas pareil : MusicBrainz tout en
/// minuscules, les tags des fichiers en capitales. La légende doit s'en
/// remettre sans défigurer les sigles.
#[test]
fn la_legende_ne_melange_pas_les_casses() {
    assert_eq!(capitaliser("trip hop"), "Trip Hop");
    assert_eq!(capitaliser("boom bap"), "Boom Bap");
    assert_eq!(capitaliser("nu metal"), "Nu Metal");
    assert_eq!(capitaliser("anti-folk"), "Anti-Folk");
    // Ceux qui portent déjà une capitale sont laissés tels quels.
    assert_eq!(capitaliser("R&B"), "R&B");
    assert_eq!(capitaliser("Rap/Hip Hop"), "Rap/Hip Hop");
    assert_eq!(capitaliser("Children's"), "Children's");
    assert_eq!(capitaliser("IDM"), "IDM");
}

/// Le défaut relevé sur la vraie bibliothèque : une famille de 4 321
/// morceaux menée par Bob Marley, Femi Kuti et James Brown se nommait
/// « Ska Rock · Latin » — deux genres sur-représentés mais marginaux —
/// tandis que le reggae, cinq fois plus présent, était ignoré.
#[test]
fn un_genre_marginal_ne_nomme_pas_une_famille() {
    let comptes = vec![
        (1, "Rock".into(), 556),
        (1, "Pop".into(), 379),
        (1, "Reggae".into(), 320),
        (1, "Ska Rock".into(), 52),
        (2, "Rock".into(), 3000),
        (2, "Pop".into(), 400),
        (2, "Reggae".into(), 5),
        (2, "Ska Rock".into(), 6),
    ];
    let noms = nommer_les_familles(&[(1, 1307), (2, 3411)], &comptes);
    let nom = &noms.iter().find(|(c, _, _)| *c == 1).unwrap().1;

    // « Ska Rock » y est 3,2 fois sur-représenté — plus que la pop — et ne
    // la nomme pourtant pas : cinquante morceaux sur treize cents.
    // « Rock », lui, est majoritaire dans la famille sans lui être propre.
    assert_eq!(nom, "Reggae · Pop");
}

/// Le rock domine six familles sur douze : deux d'entre elles sortaient
/// « Metal · Rock ». Une légende de douze pastilles dont deux portent le
/// même nom ne sert à rien.
#[test]
fn deux_familles_ne_portent_pas_le_meme_nom() {
    let comptes = vec![
        (1, "Metal".into(), 440),
        (1, "Rock".into(), 687),
        (1, "Alternative".into(), 286),
        (2, "Metal".into(), 312),
        (2, "Rock".into(), 598),
        (2, "Grunge".into(), 208),
        (3, "Jazz".into(), 300),
    ];
    let noms = nommer_les_familles(&[(1, 1413), (2, 1118), (3, 300)], &comptes);
    let libelles: Vec<&str> = noms.iter().map(|(_, n, _)| n.as_str()).collect();
    let uniques: HashSet<&str> = libelles.iter().copied().collect();
    assert_eq!(
        uniques.len(),
        libelles.len(),
        "libellés en double : {libelles:?}"
    );
}

/// « Electronic · Electro » et « Hip-Hop · Rap/Hip Hop » : le second mot
/// redisait le premier au lieu de le préciser.
#[test]
fn un_libelle_ne_se_repete_pas() {
    assert!(se_redisent("Electronic", "Electro"));
    assert!(se_redisent("Hip-Hop", "Rap/Hip Hop"));
    assert!(se_redisent("Rock", "Hard Rock"));
    // Ces deux-là sont bien deux genres distincts.
    assert!(!se_redisent("Rock", "Rockabilly"));
    assert!(!se_redisent("Metal", "Grunge"));

    let comptes = vec![
        (1, "Electronic".into(), 372),
        (1, "Electro".into(), 219),
        (1, "Electronica".into(), 151),
        (1, "Jazz".into(), 184),
        (2, "Rock".into(), 2000),
    ];
    let noms = nommer_les_familles(&[(1, 926), (2, 2000)], &comptes);
    assert_eq!(
        noms.iter().find(|(c, _, _)| *c == 1).unwrap().1,
        "Electronic · Jazz"
    );
}

/// Une famille sans genre exploitable garde un nom : elle existe sur la
/// carte, l'utilisateur doit pouvoir la désigner.
#[test]
fn une_famille_sans_genre_garde_un_nom() {
    let noms = nommer_les_familles(&[(7, 42)], &[]);
    assert_eq!(noms, vec![(7, "Famille 1".to_string(), 42)]);
}

/// Une empreinte existe dès qu'elle est calculée ; sa place sur la carte
/// n'arrive qu'à la projection suivante. Les deux comptes diffèrent donc
/// pendant toute une analyse, et c'est le premier qui dit si un cache
/// d'empreintes est périmé — s'y tromper faisait recharger 55 Mo et
/// reconstruire le graphe des voisins à chaque requête.
#[test]
fn compter_les_empreintes_nest_pas_compter_les_points_de_la_carte() {
    let lib = Library::open_in_memory().unwrap();
    for i in 0..3 {
        lib.upsert(&TrackMeta {
            path: std::path::PathBuf::from(format!("/m/{i}.flac")),
            ..Default::default()
        })
        .unwrap();
        lib.save_embedding(i + 1, "essai", &[0.1, 0.2]).unwrap();
    }

    assert_eq!(lib.count_embeddings("essai").unwrap(), 3);
    assert_eq!(lib.embeddings("essai").unwrap().len(), 3);
    // Aucune projection encore : la carte est vide, les empreintes non.
    assert_eq!(lib.map_points("essai").unwrap().len(), 0);

    lib.update_map("essai", &[(1, 0.0, 0.0, 0)]).unwrap();
    assert_eq!(lib.map_points("essai").unwrap().len(), 1);
    assert_eq!(lib.count_embeddings("essai").unwrap(), 3);
    assert_eq!(lib.count_embeddings("autre-modele").unwrap(), 0);
}

/// Élague les fichiers disparus de la racine, et rien d'autre : un morceau
/// encore sur le disque et un morceau d'une autre racine doivent rester.
#[test]
fn prune_missing_ne_retire_que_les_disparus_de_la_racine() {
    let racine = std::env::temp_dir().join(format!("rusty-music-test-{}", std::process::id()));
    std::fs::create_dir_all(&racine).unwrap();
    let present = racine.join("present.flac");
    std::fs::write(&present, b"").unwrap();

    let lib = Library::open_in_memory().unwrap();
    for p in [
        present.clone(),
        racine.join("disparu.flac"),
        std::path::PathBuf::from("/une/autre/racine/disparu.flac"),
    ] {
        lib.upsert(&TrackMeta {
            path: p,
            ..Default::default()
        })
        .unwrap();
    }
    assert_eq!(lib.count().unwrap(), 3);

    assert_eq!(lib.prune_missing(&racine).unwrap(), 1);
    assert_eq!(lib.count().unwrap(), 2);

    // Le fichier encore présent est toujours là.
    let reste: i64 = lib
        .conn
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE path = ?1",
            params![present.to_string_lossy()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(reste, 1);

    std::fs::remove_dir_all(&racine).unwrap();
}

/// Bibliothèque miniature : deux albums d'un artiste, une compilation dont
/// les pistes portent des artistes différents, et un morceau sans artiste.
fn bibliotheque_test() -> Library {
    let lib = Library::open_in_memory().unwrap();
    let ajoute = |path: &str,
                  titre: &str,
                  artiste: Option<&str>,
                  album: &str,
                  album_artiste: Option<&str>,
                  no: i64,
                  annee: i64| {
        lib.upsert(&TrackMeta {
            path: path.into(),
            title: Some(titre.into()),
            artist: artiste.map(str::to_string),
            album: Some(album.into()),
            album_artist: album_artiste.map(str::to_string),
            track_no: Some(no),
            year: Some(annee),
            ..Default::default()
        })
        .unwrap();
    };

    ajoute(
        "/m/Air/Moon/02 Sexy Boy.mp3",
        "Sexy Boy",
        Some("Air"),
        "Moon Safari",
        None,
        2,
        1998,
    );
    ajoute(
        "/m/Air/Moon/01 La femme.mp3",
        "La femme d'argent",
        Some("Air"),
        "Moon Safari",
        None,
        1,
        1998,
    );
    ajoute(
        "/m/Air/Talkie/01 Venus.mp3",
        "Venus",
        Some("Air"),
        "Talkie Walkie",
        None,
        1,
        2004,
    );
    // Compilation : l'artiste d'album diffère des artistes de piste.
    ajoute(
        "/m/Comp/Spawn/03 Satan.mp4",
        "Satan",
        Some("Orbital"),
        "Spawn",
        Some("Various"),
        3,
        1997,
    );
    ajoute(
        "/m/Comp/Spawn/01 Trip.mp4",
        "Trip Like I Do",
        Some("Filter"),
        "Spawn",
        Some("Various"),
        1,
        1997,
    );
    // Sans artiste, comme les 55 morceaux non étiquetés de la vraie base.
    ajoute(
        "/m/_/Kanan/03 Bizied.mp3",
        "Bizied",
        None,
        "Kanañ a ri!",
        None,
        3,
        2017,
    );
    lib
}

/// Le cas qui motive tout le regroupement : trois pistes du même album,
/// dont deux en featuring. Les artistes de piste diffèrent, l'identifiant
/// d'artiste d'album est le même — une seule entrée doit sortir.
#[test]
fn artists_regroupe_les_featurings_par_identifiant_musicbrainz() {
    let lib = Library::open_in_memory().unwrap();
    let nerd = "3fb49f5a-fdc0-4789-9c84-22b38b3f3cb5";
    for (path, artiste) in [
        ("/m/nerd/01.mp3", "N.E.R.D"),
        ("/m/nerd/02.mp3", "N.E.R.D feat. Lee Harvey"),
        ("/m/nerd/03.mp3", "N.E.R.D feat. Vita"),
    ] {
        lib.upsert(&TrackMeta {
            path: path.into(),
            artist: Some(artiste.into()),
            album: Some("In Search Of...".into()),
            album_artist: Some("N.E.R.D".into()),
            mb_album_artist_id: Some(nerd.into()),
            ..Default::default()
        })
        .unwrap();
    }

    let artistes = lib.artists().unwrap();
    assert_eq!(artistes.len(), 1, "les featurings doivent fusionner");
    assert_eq!(artistes[0].name, "N.E.R.D");
    assert_eq!(artistes[0].mbid.as_deref(), Some(nerd));
    assert_eq!(artistes[0].tracks, 3);

    // Et l'on retrouve ses albums par identifiant.
    let albums = lib.albums_of_artist(Some(nerd), "N.E.R.D").unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].tracks, 3);
}

/// Couverture MusicBrainz partielle : certaines pistes d'un artiste
/// portent l'identifiant, d'autres non. Il ne doit pas pour autant
/// apparaître deux fois, avec des comptes d'albums qui se contredisent.
#[test]
fn artists_ne_dedouble_pas_un_artiste_partiellement_etiquete() {
    let lib = Library::open_in_memory().unwrap();
    let id = "3fb49f5a-fdc0-4789-9c84-22b38b3f3cb5";
    let poser = |path: &str, album: &str, mbid: Option<&str>| {
        lib.upsert(&TrackMeta {
            path: path.into(),
            artist: Some("N.E.R.D".into()),
            album: Some(album.into()),
            album_artist: Some("N.E.R.D".into()),
            mb_album_artist_id: mbid.map(str::to_string),
            ..Default::default()
        })
        .unwrap();
    };
    poser("/m/1.mp3", "In Search Of...", Some(id));
    poser("/m/2.mp3", "Fly or Die", None); // album non étiqueté

    let artistes = lib.artists().unwrap();
    assert_eq!(artistes.len(), 1, "artiste dédoublé : {artistes:?}");
    assert_eq!(
        artistes[0].albums, 2,
        "les deux albums doivent être comptés ensemble"
    );
    assert_eq!(artistes[0].mbid.as_deref(), Some(id));

    // Et l'ouvrir doit montrer les deux albums annoncés, pas seulement
    // celui qui porte l'identifiant.
    let albums = lib
        .albums_of_artist(artistes[0].mbid.as_deref(), &artistes[0].name)
        .unwrap();
    assert_eq!(
        albums.len(),
        2,
        "la ligne annonce plus d'albums qu'elle n'en ouvre"
    );
}

/// Ouvrir un artiste depuis une case de la grille d'albums : `AlbumRow` ne
/// porte pas d'identifiant, l'appel se fait donc sur le nom seul. Ses
/// albums étiquetés MusicBrainz doivent quand même remonter.
#[test]
fn albums_of_artist_sans_identifiant_rattrape_les_albums_etiquetes() {
    let lib = Library::open_in_memory().unwrap();
    let id = "3fb49f5a-fdc0-4789-9c84-22b38b3f3cb5";
    for (path, album) in [("/m/1.mp3", "In Search Of..."), ("/m/2.mp3", "Fly or Die")] {
        lib.upsert(&TrackMeta {
            path: path.into(),
            artist: Some("N.E.R.D".into()),
            album: Some(album.into()),
            album_artist: Some("N.E.R.D".into()),
            mb_album_artist_id: Some(id.into()),
            ..Default::default()
        })
        .unwrap();
    }

    let albums = lib.albums_of_artist(None, "N.E.R.D").unwrap();
    assert_eq!(
        albums.len(),
        2,
        "les albums étiquetés ne remontent pas : {albums:?}"
    );
}

/// La date d'ajout d'un album est la date de fichier **la plus récente** de
/// ses pistes — un morceau posé plus tard le remonte —, pas la date de
/// passage du scan. Une date absente ou nulle ne compte pas.
#[test]
fn albums_rendent_la_date_de_fichier_la_plus_recente() {
    let lib = Library::open_in_memory().unwrap();
    let pistes = [
        ("/m/a/1.mp3", "Ancien", 1_000_000_000),
        ("/m/a/2.mp3", "Ancien", 1_100_000_000),
        ("/m/b/1.mp3", "Récent", 1_700_000_000),
        ("/m/b/2.mp3", "Récent", 1_600_000_000),
        ("/m/c/1.mp3", "Sans date", 0),
    ];
    for (path, album, mtime) in pistes {
        lib.upsert(&TrackMeta {
            path: path.into(),
            artist: Some("X".into()),
            album: Some(album.into()),
            mtime,
            ..Default::default()
        })
        .unwrap();
    }
    let par_nom: std::collections::HashMap<_, _> = lib
        .albums(None)
        .unwrap()
        .into_iter()
        .map(|a| (a.name, a.ajoute_le))
        .collect();
    assert_eq!(par_nom["Ancien"], Some(1_100_000_000));
    assert_eq!(par_nom["Récent"], Some(1_700_000_000));
    assert_eq!(
        par_nom["Sans date"], None,
        "une date nulle n'est pas une date"
    );

    // Même colonne par la voie « albums d'un artiste ».
    let de_x = lib.albums_of_artist(None, "X").unwrap();
    assert_eq!(
        de_x.iter()
            .find(|a| a.name == "Récent")
            .and_then(|a| a.ajoute_le),
        Some(1_700_000_000)
    );
}

/// Sans identifiant, le repli se fait sur le nom — et deux artistes
/// distincts ne doivent pas fusionner sous prétexte qu'ils n'en ont pas.
#[test]
fn artists_repli_sur_le_nom_sans_identifiant() {
    let lib = Library::open_in_memory().unwrap();
    for (path, artiste) in [("/m/a/1.mp3", "Alpha"), ("/m/b/1.mp3", "Beta")] {
        lib.upsert(&TrackMeta {
            path: path.into(),
            artist: Some(artiste.into()),
            album: Some(format!("Album {artiste}")),
            ..Default::default()
        })
        .unwrap();
    }
    let artistes = lib.artists().unwrap();
    assert_eq!(artistes.len(), 2);
    assert!(artistes.iter().all(|a| a.mbid.is_none()));
}

#[test]
fn artists_compte_et_exclut_les_sans_artiste() {
    let lib = bibliotheque_test();
    let artistes = lib.artists().unwrap();
    let noms: Vec<&str> = artistes.iter().map(|a| a.name.as_str()).collect();
    // Ordre alphabétique ; le morceau sans artiste n'apparaît pas ; et les
    // pistes de la compilation se rangent sous leur artiste d'album plutôt
    // que d'ouvrir une entrée par invité (Orbital, Filter).
    assert_eq!(noms, ["Air", "Various"]);

    let air = &artistes[0];
    assert_eq!(air.tracks, 3);
    assert_eq!(air.albums, 2);

    let various = &artistes[1];
    assert_eq!(various.tracks, 2);
    assert_eq!(various.albums, 1);
}

#[test]
fn albums_regroupe_la_compilation_sous_son_artiste_dalbum() {
    let lib = bibliotheque_test();

    let tous = lib.albums(None).unwrap();
    let noms: Vec<&str> = tous.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        noms,
        ["Kanañ a ri!", "Moon Safari", "Spawn", "Talkie Walkie"]
    );

    // La compilation forme un seul album malgré deux artistes de piste.
    let spawn = tous.iter().find(|a| a.name == "Spawn").unwrap();
    assert_eq!(spawn.tracks, 2);
    assert_eq!(spawn.artist.as_deref(), Some("Various"));

    // Filtrer par artiste de piste ramène quand même la compilation.
    let chez_orbital = lib.albums(Some("Orbital")).unwrap();
    assert_eq!(chez_orbital.len(), 1);
    assert_eq!(chez_orbital[0].name, "Spawn");

    let chez_air = lib.albums(Some("Air")).unwrap();
    assert_eq!(chez_air.len(), 2);
}

#[test]
fn familles_des_albums_prend_le_cluster_majoritaire() {
    let lib = Library::open_in_memory().unwrap();
    let ajoute = |path: &str, album: &str, artiste: &str, cluster: Option<i64>| {
        let id = lib
            .upsert(&TrackMeta {
                path: path.into(),
                album: Some(album.into()),
                album_artist: Some(artiste.into()),
                ..Default::default()
            })
            .unwrap();
        if let Some(c) = cluster {
            lib.save_features(id, "clap", &[0.0], 0.0, 0.0, c).unwrap();
        }
    };

    ajoute("/m/a1.mp3", "A", "X", Some(1));
    ajoute("/m/a2.mp3", "A", "X", Some(1));
    ajoute("/m/a3.mp3", "A", "X", Some(2));
    ajoute("/m/b1.mp3", "B", "Y", Some(3));
    ajoute("/m/b2.mp3", "B", "Y", Some(3));
    // Égalité 5 / 7 : le plus petit numéro tranche.
    ajoute("/m/c1.mp3", "C", "Z", Some(7));
    ajoute("/m/c2.mp3", "C", "Z", Some(5));
    // Aucun morceau projeté : l'album n'apparaît pas, le filtre le laisse
    // visible par défaut.
    ajoute("/m/d1.mp3", "D", "W", None);

    let mut f = lib.familles_des_albums("clap").unwrap();
    f.sort();
    assert_eq!(
        f,
        vec![
            ("A".to_string(), Some("X".to_string()), 1),
            ("B".to_string(), Some("Y".to_string()), 3),
            ("C".to_string(), Some("Z".to_string()), 5),
        ]
    );
}

#[test]
fn album_embeddings_moyenne_les_empreintes_dun_meme_album() {
    let lib = Library::open_in_memory().unwrap();
    let ajoute = |path: &str, artiste: &str, album: &str, vecteur: &[f32]| {
        let id = lib
            .upsert(&TrackMeta {
                path: path.into(),
                artist: Some(artiste.into()),
                album_artist: Some(artiste.into()),
                album: Some(album.into()),
                ..Default::default()
            })
            .unwrap();
        lib.save_features(id, "clap", vecteur, 0.0, 0.0, 0).unwrap();
    };
    ajoute("/m/x1.mp3", "X", "A", &[1.0, 0.0]);
    ajoute("/m/x2.mp3", "X", "A", &[3.0, 0.0]);
    ajoute("/m/y1.mp3", "Y", "B", &[0.0, 5.0]);

    let par_id: HashMap<i64, Vec<f32>> =
        lib.album_embeddings("clap").unwrap().into_iter().collect();

    assert_eq!(par_id[&(hacher("X", "A") as i64)], vec![2.0, 0.0]);
    assert_eq!(par_id[&(hacher("Y", "B") as i64)], vec![0.0, 5.0]);
}

#[test]
fn albums_annotes_croise_famille_et_date() {
    let lib = Library::open_in_memory().unwrap();
    let ajoute =
        |path: &str, artiste: &str, album: &str, annee: i64, cluster: i64, duree_ms: i64| {
            let id = lib
                .upsert(&TrackMeta {
                    path: path.into(),
                    artist: Some(artiste.into()),
                    album_artist: Some(artiste.into()),
                    album: Some(album.into()),
                    year: Some(annee),
                    duration_ms: Some(duree_ms),
                    ..Default::default()
                })
                .unwrap();
            lib.save_features(id, "clap", &[0.0], 0.0, 0.0, cluster)
                .unwrap();
        };
    ajoute("/m/x1.mp3", "X", "A", 1990, 1, 200_000);
    ajoute("/m/x2.mp3", "X", "A", 1990, 1, 180_000);
    ajoute("/m/x3.mp3", "X", "B", 1994, 2, 240_000);

    let par_id: HashMap<i64, AlbumNoeud> = lib
        .albums_annotes("clap")
        .unwrap()
        .into_iter()
        .map(|n| (n.id, n))
        .collect();

    let a = &par_id[&(hacher("X", "A") as i64)];
    assert_eq!(a.famille, Some(1));
    assert_eq!(a.date, 19_900_000);
    assert_eq!(a.artist, "X");
    assert_eq!(a.duree_ms, 380_000, "somme des durées de l'album");

    let b = &par_id[&(hacher("X", "B") as i64)];
    assert_eq!(b.famille, Some(2));
    assert_eq!(b.date, 19_940_000);
}

#[test]
fn tracks_of_album_respecte_lordre_du_disque() {
    let lib = bibliotheque_test();
    let pistes = lib.tracks_of_album("Moon Safari", None).unwrap();
    let titres: Vec<&str> = pistes.iter().filter_map(|t| t.title.as_deref()).collect();
    assert_eq!(titres, ["La femme d'argent", "Sexy Boy"]);
    assert_eq!(pistes[0].track_no, Some(1));
}

#[test]
fn search_couvre_titre_artiste_et_album() {
    let lib = bibliotheque_test();
    assert_eq!(lib.search("sexy", 50).unwrap().len(), 1); // titre
    assert_eq!(lib.search("AIR", 50).unwrap().len(), 3); // artiste, casse ignorée
    assert_eq!(lib.search("Moon", 50).unwrap().len(), 2); // album
    assert_eq!(lib.search("introuvable", 50).unwrap().len(), 0);
    assert_eq!(lib.search("a", 2).unwrap().len(), 2); // la limite s'applique
}

#[test]
fn search_albums_regroupe_par_album() {
    let lib = bibliotheque_test();
    // « Moon Safari » compte deux pistes retenues par la recherche : sans
    // le regroupement, il apparaîtrait deux fois.
    let r = lib.search_albums("Moon", 50).unwrap();
    assert_eq!(r.len(), 1, "l'album ne doit sortir qu'une fois : {r:?}");
    assert_eq!(r[0].1, "Moon Safari");
    assert_eq!(lib.search_albums("introuvable", 50).unwrap().len(), 0);
}

/// Mise à niveau d'une base peuplée avant que l'index de recherche
/// n'existe. Les autres tests ne voient pas ce cas : leurs lignes sont
/// insérées après, donc indexées au vol par les déclencheurs.
#[test]
fn migration_indexe_une_base_deja_peuplee() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../../sql/schema.sql"))
        .unwrap();
    conn.execute(
        "INSERT INTO tracks(path, title, album) VALUES ('/m/a.mp3', 'Bizied', 'Kanañ a ri!')",
        [],
    )
    .unwrap();

    migrate(&conn).unwrap();
    let lib = Library { conn };

    assert_eq!(
        lib.search("kanan", 10).unwrap().len(),
        1,
        "l'index n'a pas été reconstruit pour les lignes préexistantes"
    );
}

/// Une base enrichie par le code d'avant le correctif porte des artistes
/// marqués « déjà faits » sans aucune date sur leurs release-groups —
/// `mb_poser_albums` ne les écrivait pas encore. La migration doit les
/// remettre « à faire » pour qu'un `enrich` ultérieur les rattrape.
#[test]
fn purge_dates_remet_en_attente_les_artistes_deja_fetches() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../../sql/schema.sql"))
        .unwrap();
    conn.execute(
        "INSERT INTO tracks(path, artist, mb_artist_id) VALUES ('/m/a.mp3', 'X', 'mbid-x')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO mb_release_groups (mbid, artist_mbid, title, title_norm)
         VALUES ('rg-1', 'mbid-x', 'Album', 'album')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO mb_fetched (mbid, kind) VALUES ('mbid-x', 'artist')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO mb_fetched (mbid, kind) VALUES ('mbid-x', 'albums')",
        [],
    )
    .unwrap();

    migrate(&conn).unwrap();
    let lib = Library { conn };

    assert_eq!(
        lib.mb_artistes_en_attente("artist", 10).unwrap(),
        vec!["mbid-x".to_string()],
        "l'artiste doit redevenir « à faire » après la purge"
    );
}

/// Sans ligne sentinelle, la purge rejouerait à chaque démarrage — et un
/// artiste dont MusicBrainz ne connaît vraiment aucune date (une repasse
/// honnête peut légitimement en rendre aucune) serait remis « à faire »
/// indéfiniment, contrairement à l'esprit « reprend où elle s'est
/// arrêtée » du module.
#[test]
fn purge_dates_ne_se_repete_pas() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../../sql/schema.sql"))
        .unwrap();
    migrate(&conn).unwrap(); // premier « démarrage » : pose la sentinelle

    // Une repasse d'enrich réussie depuis : l'artiste est de nouveau
    // marqué fait, toujours sans date connue.
    conn.execute(
        "INSERT INTO tracks(path, artist, mb_artist_id) VALUES ('/m/a.mp3', 'X', 'mbid-x')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO mb_fetched (mbid, kind) VALUES ('mbid-x', 'artist')",
        [],
    )
    .unwrap();

    migrate(&conn).unwrap(); // deuxième « démarrage »
    let lib = Library { conn };

    assert!(
        lib.mb_artistes_en_attente("artist", 10).unwrap().is_empty(),
        "la purge ne doit jouer qu'une fois"
    );
}

#[test]
fn migrations_bien_formees() {
    migrations().validate().unwrap();
}

/// Une base neuve sort numérotée ; rouverte, elle ne rejoue rien. Témoin :
/// une année « 0 », que la migration n°1 efface, posée après la première
/// ouverture et toujours là après la seconde.
#[test]
fn migration_jouee_une_seule_fois() {
    let chemin = std::env::temp_dir().join(format!("rusty-music-migr-{}.db", std::process::id()));
    let nettoyer = || {
        for suffixe in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffixe}", chemin.display()));
        }
    };
    nettoyer();
    {
        let lib = Library::open(&chemin).unwrap();
        let v: i64 = lib
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert!(v >= 1, "user_version = {v}");
        lib.conn
            .execute("INSERT INTO tracks(path, year) VALUES ('/m/a.mp3', 0)", [])
            .unwrap();
    }
    let lib = Library::open(&chemin).unwrap();
    let annee: Option<i64> = lib
        .conn
        .query_row("SELECT year FROM tracks", [], |r| r.get(0))
        .unwrap();
    drop(lib);
    nettoyer();
    assert_eq!(annee, Some(0), "la migration n°1 a été rejouée");
}

/// Une base de la toute première génération (schéma du 19 août 2026,
/// sans numéro de version) : la migration n°1 la complète sans rien
/// perdre.
#[test]
fn migration_reconnait_la_premiere_generation() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE tracks (
             id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, size_bytes INTEGER,
             mtime INTEGER, title TEXT, artist TEXT, album TEXT, album_artist TEXT,
             genre TEXT, year INTEGER, track_no INTEGER, duration_ms INTEGER,
             sample_rate INTEGER, channels INTEGER, mb_recording_id TEXT,
             mb_artist_id TEXT, mb_album_artist_id TEXT,
             added_at INTEGER NOT NULL DEFAULT (strftime('%s','now')), analyzed_at INTEGER);
         CREATE TABLE descriptors (
             track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
             bpm REAL, musical_key TEXT, energy REAL, loudness REAL);
         CREATE TABLE artist_links (
             src_mbid TEXT NOT NULL, dst_mbid TEXT NOT NULL, relation TEXT NOT NULL,
             PRIMARY KEY (src_mbid, dst_mbid, relation));
         INSERT INTO tracks(path, title, artist, album, year)
             VALUES ('/m/a.flac', 'Bizied', 'Denez', 'Kanañ a ri!', 0);
         INSERT INTO descriptors(track_id, bpm) VALUES (1, 92.0);",
    )
    .unwrap();

    let conn = preparer(conn).unwrap();
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert!(v >= 1);
    for (table, colonne) in [
        ("tracks", "mb_release_id"),
        ("tracks", "bit_depth"),
        ("descriptors", "algo_version"),
        ("artist_links", "dst_name"),
    ] {
        let n: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = ?1"),
                [colonne],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "{table}.{colonne} manque");
    }
    let (bpm, annee): (f64, Option<i64>) = conn
        .query_row(
            "SELECT d.bpm, t.year FROM tracks t JOIN descriptors d ON d.track_id = t.id",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((bpm, annee), (92.0, None));
    let lib = Library { conn };
    assert_eq!(lib.search("kanan", 10).unwrap().len(), 1);
}

#[test]
fn search_ignore_les_accents() {
    let lib = bibliotheque_test();
    // Le point de tout l'exercice : la bibliothèque est pleine de titres
    // accentués, et on ne tape pas les accents en cherchant.
    assert_eq!(lib.search("kanan", 50).unwrap().len(), 1);
    assert_eq!(lib.search("Kanañ", 50).unwrap().len(), 1);
    assert_eq!(lib.search("KANAN A RI", 50).unwrap().len(), 1);
}

#[test]
fn search_ne_laisse_pas_la_saisie_devenir_une_requete() {
    let lib = bibliotheque_test();
    // Ces saisies contiennent des opérateurs FTS5 ou de la ponctuation
    // seule. Aucune ne doit remonter d'erreur de syntaxe ni tout ramener.
    for saisie in [
        "%",
        "*",
        "-",
        "\"",
        "AND",
        "a OR b",
        "NEAR(x y)",
        "^",
        "moon\"",
    ] {
        let r = lib.search(saisie, 50);
        assert!(
            r.is_ok(),
            "« {saisie} » a fait échouer la recherche : {r:?}"
        );
    }
    assert!(lib.search("%", 50).unwrap().is_empty());
    assert!(lib.search("AND", 50).unwrap().is_empty());
}

#[test]
fn search_porte_sur_des_mots_entiers() {
    let lib = bibliotheque_test();
    // « exy » ne trouve pas « Sexy » : on cherche des mots, pas des
    // sous-chaînes. Seul le dernier mot vaut préfixe, pour la frappe.
    assert!(lib.search("exy", 50).unwrap().is_empty());
    assert_eq!(lib.search("Sex", 50).unwrap().len(), 1);
    assert_eq!(lib.search("Sexy Boy", 50).unwrap().len(), 1);
}

#[test]
fn track_par_identifiant() {
    let lib = bibliotheque_test();
    let trouve = lib.search("Venus", 1).unwrap();
    let id = trouve[0].id;
    assert_eq!(
        lib.track(id).unwrap().unwrap().title.as_deref(),
        Some("Venus")
    );
    assert!(lib.track(999_999).unwrap().is_none());
}

#[test]
fn remove_root_emporte_les_morceaux_de_la_racine_seulement() {
    let lib = bibliotheque_test();
    lib.add_root(Path::new("/m/Air")).unwrap();
    lib.add_root(Path::new("/m/Comp")).unwrap();

    let racines = lib.roots().unwrap();
    assert_eq!(racines.len(), 2);
    assert_eq!(racines[0].path, "/m/Air");
    assert_eq!(racines[0].tracks, 3);

    // Changer de source : on retire l'ancienne racine et ses morceaux.
    assert_eq!(lib.remove_root(Path::new("/m/Air")).unwrap(), 3);
    assert_eq!(lib.count().unwrap(), 3);
    assert_eq!(lib.roots().unwrap().len(), 1);
    // Les autres racines sont intactes.
    assert_eq!(lib.albums(Some("Orbital")).unwrap().len(), 1);
}

#[test]
fn save_features_marque_le_morceau_analyse() {
    let lib = Library::open_in_memory().unwrap();
    let id = lib
        .upsert(&TrackMeta {
            path: "/m/a.flac".into(),
            title: Some("A".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(lib.pending_analysis("clap", 10).unwrap().len(), 1);

    lib.save_features(id, "clap", &[0.1, 0.2, 0.3], -0.5, 0.25, 2)
        .unwrap();

    // Le morceau sort de la file d'attente, et se retrouve sur la carte.
    assert!(lib.pending_analysis("clap", 10).unwrap().is_empty());
    // Mais il reste en attente pour tout autre modèle : c'est le point.
    // Avec l'ancien critère (`analyzed_at`), changer de représentation
    // laissait la moitié de la bibliothèque hors de la passe suivante.
    assert_eq!(lib.pending_analysis("clap-9f", 10).unwrap().len(), 1);
    let pts = lib.map_points("clap").unwrap();
    assert_eq!(pts.len(), 1);
    assert_eq!(pts[0].0, id);
    assert!((pts[0].1 + 0.5).abs() < 1e-6);
    assert_eq!(pts[0].3, 2);

    // Un autre modèle cohabite sans écraser le premier.
    lib.save_features(id, "musicnn", &[1.0], 0.0, 0.0, 0)
        .unwrap();
    assert_eq!(lib.map_points("clap").unwrap().len(), 1);
    assert_eq!(lib.map_points("musicnn").unwrap().len(), 1);

    // Réécrire le même couple met à jour au lieu de dupliquer.
    lib.save_features(id, "clap", &[0.9], 0.1, 0.1, 5).unwrap();
    let pts = lib.map_points("clap").unwrap();
    assert_eq!(pts.len(), 1);
    assert_eq!(pts[0].3, 5);
}

#[test]
fn upsert_is_idempotent() {
    let lib = Library::open_in_memory().unwrap();
    let m = TrackMeta {
        path: "/musique/a.flac".into(),
        title: Some("A".into()),
        ..Default::default()
    };
    let first = lib.upsert(&m).unwrap();
    let second = lib.upsert(&m).unwrap();
    assert_eq!(first, second);
    assert_eq!(lib.count().unwrap(), 1);
}

#[test]
fn pending_analysis_exclut_un_fichier_deja_en_echec() {
    let lib = Library::open_in_memory().unwrap();
    let m = TrackMeta {
        path: "/musique/casse.mp3".into(),
        title: Some("Casse".into()),
        ..Default::default()
    };
    lib.upsert(&m).unwrap();
    assert_eq!(lib.pending_analysis("clap", i64::MAX).unwrap().len(), 1);

    // Un fichier qui a déjà fait planter une passe ne doit pas être
    // retenté à chaque relance — voir `passe::empreintes`.
    lib.enregistrer_echec_scan(Path::new("/musique/casse.mp3"), "décodage impossible")
        .unwrap();
    assert!(lib.pending_analysis("clap", i64::MAX).unwrap().is_empty());

    // L'utilisateur peut lui redonner sa chance.
    lib.effacer_echec_scan(Path::new("/musique/casse.mp3"))
        .unwrap();
    assert_eq!(lib.pending_analysis("clap", i64::MAX).unwrap().len(), 1);
}

#[test]
fn qualite_piste_rend_ce_que_le_scan_a_lu() {
    let lib = Library::open_in_memory().unwrap();
    let id = lib
        .upsert(&TrackMeta {
            path: "/m/a.flac".into(),
            codec: Some("FLAC".into()),
            sample_rate: Some(44_100),
            channels: Some(2),
            bit_depth: Some(16),
            bitrate: None,
            ..Default::default()
        })
        .unwrap();

    let q = lib.qualite_piste(id).unwrap().unwrap();
    assert_eq!(q.codec.as_deref(), Some("FLAC"));
    assert_eq!(q.sample_rate, Some(44_100));
    assert_eq!(q.channels, Some(2));
    assert_eq!(q.bit_depth, Some(16));
    assert_eq!(q.bitrate, None);

    // Morceau absent : None, pas une erreur.
    assert!(lib.qualite_piste(999).unwrap().is_none());
}

#[test]
fn histogrammer_range_hors_gamme_et_sans_valeur() {
    let valeurs = vec![Some(5.0), Some(12.0), Some(19.0), Some(29.0), None];
    // Tranches [10,15) et [15,20) ; 5.0 est sous le plancher, 29.0 dépasse.
    let h = histogrammer(&valeurs, 10.0, 5.0, 2);
    assert_eq!(h.comptes, vec![1, 1]); // 12 -> [10,15), 19 -> [15,20)
    assert_eq!(h.hors_gamme, 2); // 5.0 (< min) et 29.0 (>= min + 2*pas)
    assert_eq!(h.sans_valeur, 1);
}

/// Sans genre MusicBrainz en base, le repli sur le tag du fichier
/// s'applique à toute la bibliothèque — et les morceaux sans tag
/// comptent dans « — » plutôt que de disparaître silencieusement.
#[test]
fn stats_genres_replie_sur_le_tag_et_compte_les_morceaux_sans_genre() {
    let lib = Library::open_in_memory().unwrap();
    for (path, genre) in [
        ("/m/a.mp3", Some("Rock")),
        ("/m/b.mp3", Some("Rock")),
        ("/m/c.mp3", Some("Jazz")),
        ("/m/d.mp3", None),
    ] {
        lib.upsert(&TrackMeta {
            path: path.into(),
            genre: genre.map(str::to_string),
            ..Default::default()
        })
        .unwrap();
    }
    let stats = lib.stats_genres().unwrap();
    assert_eq!(
        stats,
        vec![
            ("Rock".to_string(), 2),
            ("Jazz".to_string(), 1),
            ("—".to_string(), 1),
        ]
    );
}

/// Un morceau jamais rescanné depuis l'arrivée du champ `codec` n'a « pas
/// de format identifié » : un tag NULL, pas une valeur inventée.
#[test]
fn stats_codecs_distingue_le_format_du_non_mesure() {
    let lib = Library::open_in_memory().unwrap();
    for (path, codec) in [
        ("/m/a.mp3", Some("MP3")),
        ("/m/b.mp3", Some("MP3")),
        ("/m/c.flac", Some("FLAC")),
        ("/m/d.mp3", None),
    ] {
        lib.upsert(&TrackMeta {
            path: path.into(),
            codec: codec.map(str::to_string),
            ..Default::default()
        })
        .unwrap();
    }
    let stats = lib.stats_codecs().unwrap();
    assert_eq!(
        stats,
        vec![
            ("MP3".to_string(), 2),
            ("FLAC".to_string(), 1),
            ("non mesuré".to_string(), 1),
        ]
    );
}

#[test]
fn stats_bitrate_range_bien_le_mp3_et_ecarte_le_flac() {
    let lib = Library::open_in_memory().unwrap();
    for (path, bitrate) in [
        ("/m/a.mp3", Some(128)),
        ("/m/b.mp3", Some(320)),
        ("/m/c.flac", Some(1000)), // bien au-delà de BITRATE_TRANCHES*BITRATE_PAS
        ("/m/d.wav", None),
    ] {
        lib.upsert(&TrackMeta {
            path: path.into(),
            bitrate,
            ..Default::default()
        })
        .unwrap();
    }
    let h = lib.stats_bitrate().unwrap();
    assert_eq!(h.comptes.iter().sum::<i64>(), 2); // 128 et 320
    assert_eq!(h.hors_gamme, 1); // le FLAC à 1000 kb/s
    assert_eq!(h.sans_valeur, 1); // le WAV sans débit
}

/// Le `LEFT JOIN` doit distinguer « jamais mesuré » (aucune ligne dans
/// `descriptors`) et « mesuré, sans tempo trouvé » (`bpm` NULL) — les deux
/// sont « sans valeur » pour l'histogramme, mais un morceau avec un vrai
/// tempo doit atterrir dans sa tranche.
#[test]
fn stats_tempo_distingue_mesure_et_non_mesure() {
    let lib = Library::open_in_memory().unwrap();
    let jamais = lib
        .upsert(&TrackMeta {
            path: "/m/jamais.mp3".into(),
            ..Default::default()
        })
        .unwrap();
    let sans_tempo = lib
        .upsert(&TrackMeta {
            path: "/m/silence.mp3".into(),
            ..Default::default()
        })
        .unwrap();
    let mesure = lib
        .upsert(&TrackMeta {
            path: "/m/mesure.mp3".into(),
            ..Default::default()
        })
        .unwrap();
    let _ = jamais; // jamais analysé : aucune ligne `descriptors`, exprès.
    lib.save_descripteurs(
        sans_tempo, None, None, 0.0, -100.0, None, None, None, None, None, None, None, 1,
    )
    .unwrap();
    lib.save_descripteurs(
        mesure,
        Some(122.0),
        None,
        0.5,
        -12.0,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        1,
    )
    .unwrap();

    let h = lib.stats_tempo().unwrap();
    assert_eq!(h.sans_valeur, 2);
    assert_eq!(h.comptes.iter().sum::<i64>(), 1);
    let tranche = ((122.0 - TEMPO_MIN) / TEMPO_PAS) as usize;
    assert_eq!(h.comptes[tranche], 1);
}

/// Une mesure écrite par un algorithme plus ancien redevient « en
/// attente » — c'est ce qui permet à un correctif (l'inversion d'octave
/// du tempo, par exemple) de se rattraper tout seul, sans bouton
/// « refaire ce qui est déjà mesuré ».
#[test]
fn pending_descripteurs_reprend_une_mesure_perimee() {
    let lib = Library::open_in_memory().unwrap();
    let id = lib
        .upsert(&TrackMeta {
            path: "/m/perime.mp3".into(),
            ..Default::default()
        })
        .unwrap();
    lib.save_embedding(id, "modele", &[0.1, 0.2]).unwrap();
    lib.save_descripteurs(
        id,
        Some(90.0),
        None,
        0.5,
        -12.0,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        1,
    )
    .unwrap();

    // Version 1 : déjà mesuré, rien à refaire.
    assert!(lib
        .pending_descripteurs("modele", 1, 10)
        .unwrap()
        .is_empty());
    let (faits, total) = lib.compter_descripteurs("modele", 1).unwrap();
    assert_eq!((faits, total), (1, 1));

    // Version 2 (l'algorithme a changé) : la mesure d'avant ne compte plus.
    let pending = lib.pending_descripteurs("modele", 2, 10).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, id);
    let (faits, total) = lib.compter_descripteurs("modele", 2).unwrap();
    assert_eq!((faits, total), (0, 1));

    // Remesuré en version 2 : de nouveau à jour.
    lib.save_descripteurs(
        id,
        Some(180.0),
        None,
        0.5,
        -12.0,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        2,
    )
    .unwrap();
    assert!(lib
        .pending_descripteurs("modele", 2, 10)
        .unwrap()
        .is_empty());
}

#[test]
fn pending_loudness_reprend_une_mesure_perimee() {
    let lib = Library::open_in_memory().unwrap();
    let id = lib
        .upsert(&TrackMeta {
            path: "/m/perime.flac".into(),
            ..Default::default()
        })
        .unwrap();
    lib.save_loudness(id, -14.0, -1.0, 1).unwrap();

    // Version 1 : déjà mesuré, rien à refaire.
    assert!(lib.pending_loudness(1, 10).unwrap().is_empty());
    let (faits, total) = lib.compter_loudness(1).unwrap();
    assert_eq!((faits, total), (1, 1));

    // Version 2 (la méthode de mesure a changé) : la mesure d'avant ne
    // compte plus.
    let pending = lib.pending_loudness(2, 10).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, id);
    let (faits, total) = lib.compter_loudness(2).unwrap();
    assert_eq!((faits, total), (0, 1));

    // Remesuré en version 2 : de nouveau à jour.
    lib.save_loudness(id, -14.0, -1.0, 2).unwrap();
    assert!(lib.pending_loudness(2, 10).unwrap().is_empty());
}

/// Un morceau jamais passé dans la passe de loudness doit jouer sans
/// normalisation, jamais bloquer la lecture.
#[test]
fn gain_lecture_absent_ne_bloque_rien() {
    let lib = Library::open_in_memory().unwrap();
    lib.upsert(&TrackMeta {
        path: "/m/pasmesure.flac".into(),
        ..Default::default()
    })
    .unwrap();
    let gain = lib
        .gain_lecture(
            Path::new("/m/pasmesure.flac"),
            crate::loudness::ModeGain::Piste,
            crate::loudness::CIBLE_LUFS,
        )
        .unwrap();
    assert!(gain.is_none());
}

/// En mode album, le plafond anti-écrêtage reste calculé sur le pic
/// **de la piste réellement jouée** — jamais un pic d'album, qui
/// n'existe pas et n'aurait pas de sens (chaque piste peut écrêter
/// indépendamment des autres).
#[test]
fn gain_lecture_mode_album_utilise_le_pic_de_la_piste() {
    let lib = Library::open_in_memory().unwrap();
    let id = lib
        .upsert(&TrackMeta {
            path: "/m/piste-forte.flac".into(),
            album: Some("Album".into()),
            album_artist: Some("Artiste".into()),
            ..Default::default()
        })
        .unwrap();
    // Piste déjà proche de 0 dBTP : toute normalisation généreuse doit
    // être plafonnée.
    lib.save_loudness(id, -25.0, -0.2, crate::loudness::VERSION_LOUDNESS)
        .unwrap();
    lib.save_album_loudness(
        "Album",
        "Artiste",
        -25.0,
        1,
        crate::loudness::VERSION_LOUDNESS,
    )
    .unwrap();

    let gain = lib
        .gain_lecture(
            Path::new("/m/piste-forte.flac"),
            crate::loudness::ModeGain::Album,
            crate::loudness::CIBLE_LUFS,
        )
        .unwrap()
        .expect("mesurée");
    let attendu = crate::loudness::db_vers_lineaire(0.2); // plafond, pas le naïf (+7 dB)
    assert!(
        (gain - attendu).abs() < 1e-6,
        "gain = {gain}, attendu {attendu}"
    );
}

/// Mode album choisi, mais l'album n'a pas encore de ligne
/// `album_loudness` (pas assez de pistes mesurées pour le recalculer) :
/// on rejoue le gain piste plutôt que de bloquer ou d'inventer une
/// valeur d'album.
#[test]
fn gain_lecture_mode_album_replie_sur_la_piste_si_lalbum_manque() {
    let lib = Library::open_in_memory().unwrap();
    let id = lib
        .upsert(&TrackMeta {
            path: "/m/orphelin.flac".into(),
            album: Some("Album inachevé".into()),
            album_artist: Some("Artiste".into()),
            ..Default::default()
        })
        .unwrap();
    lib.save_loudness(id, -20.0, -1.0, crate::loudness::VERSION_LOUDNESS)
        .unwrap();

    let piste = lib
        .gain_lecture(
            Path::new("/m/orphelin.flac"),
            crate::loudness::ModeGain::Piste,
            crate::loudness::CIBLE_LUFS,
        )
        .unwrap();
    let album = lib
        .gain_lecture(
            Path::new("/m/orphelin.flac"),
            crate::loudness::ModeGain::Album,
            crate::loudness::CIBLE_LUFS,
        )
        .unwrap();
    assert_eq!(piste, album);
}

/// Une famille de huit morceaux, quatre genres — Jazz minoritaire, seul
/// hors du top 3 (`GENRES_DOMINANTS`). C'est lui, et seulement lui, qui
/// doit ressortir suspect.
#[test]
fn genres_suspects_ignore_le_dominant_et_signale_lisole() {
    let lib = Library::open_in_memory().unwrap();
    // Effectifs tous distincts : aucune égalité à départager dans le
    // classement des dominants.
    let genres = [("Rock", 4), ("Pop", 3), ("Electronic", 2), ("Jazz", 1)];
    let mut i = 0;
    for (genre, n) in genres {
        for _ in 0..n {
            let id = lib
                .upsert(&TrackMeta {
                    path: format!("/m/{i}.mp3").into(),
                    title: Some(format!("Piste {i}")),
                    artist: Some("Artiste".into()),
                    genre: Some(genre.to_string()),
                    ..Default::default()
                })
                .unwrap();
            lib.save_features(id, "clap", &[0.0], 0.0, 0.0, 0).unwrap();
            i += 1;
        }
    }
    let suspects = lib.genres_suspects("clap").unwrap();
    assert_eq!(suspects.len(), 1, "un seul suspect attendu : {suspects:?}");
    assert_eq!(suspects[0].2, "Jazz");
    assert_eq!(suspects[0].3, "Rock · Pop · Electronic");
}

/// Une famille trop petite (`PLANCHER_ABSOLU`) ne doit rien signaler : le
/// « dominant » d'un groupe de deux morceaux n'apprend rien.
#[test]
fn genres_suspects_ignore_les_petites_familles() {
    let lib = Library::open_in_memory().unwrap();
    for (i, genre) in ["Rock", "Jazz"].iter().enumerate() {
        let id = lib
            .upsert(&TrackMeta {
                path: format!("/m/{i}.mp3").into(),
                genre: Some(genre.to_string()),
                ..Default::default()
            })
            .unwrap();
        lib.save_features(id, "clap", &[0.0], 0.0, 0.0, 0).unwrap();
    }
    assert!(lib.genres_suspects("clap").unwrap().is_empty());
}

#[test]
fn editions_multiples_rapproche_les_mentions_dedition() {
    let lib = Library::open_in_memory().unwrap();
    for (i, (artiste, album)) in [
        ("Radiohead", "Kid A"),
        ("Radiohead", "Kid A (Remaster)"),
        ("Radiohead", "OK Computer"),
        // Autre artiste, même titre : ne doit pas se mélanger.
        ("Air", "Kid A"),
    ]
    .iter()
    .enumerate()
    {
        lib.upsert(&TrackMeta {
            path: format!("/m/{i}.mp3").into(),
            artist: Some(artiste.to_string()),
            album: Some(album.to_string()),
            ..Default::default()
        })
        .unwrap();
    }
    let editions = lib.editions_multiples().unwrap();
    assert_eq!(
        editions.len(),
        1,
        "une seule paire d'éditions : {editions:?}"
    );
    let (artiste, titre, versions) = &editions[0];
    assert_eq!(artiste, "Radiohead");
    assert_eq!(titre, "Kid A");
    assert_eq!(versions.len(), 2);
}

#[test]
fn titre_album_normalise_tronque_a_la_premiere_parenthese_ou_crochet() {
    assert_eq!(titre_album_normalise("Kid A"), "kid a");
    assert_eq!(titre_album_normalise("Kid A (Remaster)"), "kid a");
    assert_eq!(
        titre_album_normalise("OK Computer [Deluxe Edition]"),
        "ok computer"
    );
}

/// La médiane s'ajuste à l'échelle réelle des valeurs, même une échelle
/// resserrée loin de [0, 1] — le cas mesuré qui a écarté un seuil fixe.
#[test]
fn stats_humeur_se_calibre_sur_sa_propre_echelle() {
    let lib = Library::open_in_memory().unwrap();
    // Énergie resserrée entre 0,03 et 0,41, comme mesuré en vrai — un
    // seuil fixe à 0,5 rangerait tout le monde dans « lent/léger ».
    let pistes = [
        (60.0, 0.05),  // lent, léger  -> Calme
        (60.0, 0.40),  // lent, dense  -> Intense
        (150.0, 0.05), // rapide, léger -> Enlevé
        (150.0, 0.40), // rapide, dense -> Énergique
    ];
    for (i, (bpm, energie)) in pistes.iter().enumerate() {
        let id = lib
            .upsert(&TrackMeta {
                path: format!("/m/{i}.mp3").into(),
                ..Default::default()
            })
            .unwrap();
        lib.save_descripteurs(
            id,
            Some(*bpm as f32),
            None,
            *energie as f32,
            -10.0,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            1,
        )
        .unwrap();
    }
    let humeur = lib.stats_humeur().unwrap();
    let compte = |nom: &str| {
        humeur
            .iter()
            .find(|(n, _)| n == nom)
            .map(|(_, c)| *c)
            .unwrap_or(0)
    };
    assert_eq!(compte("Calme"), 1);
    assert_eq!(compte("Intense"), 1);
    assert_eq!(compte("Enlevé"), 1);
    assert_eq!(compte("Énergique"), 1);
}

#[test]
fn parametres_carte_retombe_sur_les_defauts_puis_retient_ce_quon_change() {
    let lib = Library::open_in_memory().unwrap();
    let p = lib.parametres_carte().unwrap();
    assert_eq!(p.familles, ParametresCarte::default().familles);
    assert_eq!(p.perplexite, ParametresCarte::default().perplexite);

    lib.set_parametre_carte("familles", 20.0).unwrap();
    let p = lib.parametres_carte().unwrap();
    assert_eq!(p.familles, 20);
    // Le reste n'a pas bougé : changer une clé ne doit pas réinitialiser
    // les autres.
    assert_eq!(p.epoques, ParametresCarte::default().epoques);

    // Rejouer la même clé met à jour, ne duplique pas.
    lib.set_parametre_carte("familles", 8.0).unwrap();
    assert_eq!(lib.parametres_carte().unwrap().familles, 8);
}

#[test]
fn liens_artiste_se_met_en_cache_meme_vide() {
    let mut lib = Library::open_in_memory().unwrap();
    assert!(!lib.liens_artiste_en_cache("a").unwrap());

    lib.enregistrer_liens_artiste(
        "a",
        &[crate::musicbrainz::Relation {
            dst_mbid: "b".to_string(),
            dst_name: "Artiste B".to_string(),
            relation: "member of band".to_string(),
        }],
    )
    .unwrap();
    assert!(lib.liens_artiste_en_cache("a").unwrap());
    let liens = lib.liens_artiste("a").unwrap();
    assert_eq!(
        liens,
        vec![(
            "b".to_string(),
            "Artiste B".to_string(),
            "member of band".to_string()
        )]
    );

    // Un artiste sans aucune relation connue doit rester marqué comme
    // déjà interrogé, sans quoi il serait réinterrogé à chaque visite.
    lib.enregistrer_liens_artiste("c", &[]).unwrap();
    assert!(lib.liens_artiste_en_cache("c").unwrap());
    assert!(lib.liens_artiste("c").unwrap().is_empty());
}

fn sortie(rg: &str, date_norm: &str, collab: Option<&str>) -> SortieARanger {
    SortieARanger {
        rg_mbid: rg.into(),
        titre: format!("Disque {rg}"),
        date_sortie: Some(date_norm.into()),
        date_sortie_norm: Some(date_norm.into()),
        type_primaire: Some("Album".into()),
        types_secondaires: None,
        collaborateurs: collab.map(str::to_string),
    }
}

#[test]
fn decouvrir_ajouter_sortie_preserve_vu_a_la_reinsertion() {
    let lib = Library::open_in_memory().unwrap();
    let hier = lib.date_il_y_a(1).unwrap();

    assert!(lib
        .decouvrir_ajouter_sortie("a", "Artiste A", &sortie("rg1", &hier, None))
        .unwrap());
    lib.decouvrir_marquer_passe("sorties").unwrap();
    lib.decouvrir_tout_vu().unwrap();

    // Une seconde passe revoit la même sortie : rien de neuf, et le « vu »
    // tient — sans quoi la pastille « nouveau » se rallumerait à chaque
    // actualisation.
    assert!(!lib
        .decouvrir_ajouter_sortie("a", "Artiste A", &sortie("rg1", &hier, None))
        .unwrap());
    assert!(lib
        .decouvrir_ajouter_sortie("a", "Artiste A", &sortie("rg2", &hier, None))
        .unwrap());

    let fil = lib.decouvrir_fil(30).unwrap();
    assert_eq!(fil.sorties.len(), 2);
    assert!(fil.sorties.iter().find(|s| s.rg_mbid == "rg1").unwrap().vu);
    assert!(!fil.sorties.iter().find(|s| s.rg_mbid == "rg2").unwrap().vu);
}

#[test]
fn decouvrir_fil_filtre_la_fenetre_et_separe_les_collaborations() {
    let lib = Library::open_in_memory().unwrap();
    let hier = lib.date_il_y_a(1).unwrap();
    let vieux = lib.date_il_y_a(120).unwrap();

    for s in [
        sortie("recent", &hier, None),
        sortie("collab", &hier, Some("Invité")),
        sortie("vieux", &vieux, None),
    ] {
        lib.decouvrir_ajouter_sortie("a", "Artiste A", &s).unwrap();
    }
    lib.decouvrir_marquer_passe("sorties").unwrap();

    let fil = lib.decouvrir_fil(30).unwrap();
    assert_eq!(
        fil.sorties.iter().map(|s| &s.rg_mbid).collect::<Vec<_>>(),
        ["recent"]
    );
    assert_eq!(
        fil.collaborations
            .iter()
            .map(|s| &s.rg_mbid)
            .collect::<Vec<_>>(),
        ["collab"]
    );
    assert!(fil.derniere_passe.is_some());
}

#[test]
fn decouvrir_en_attente_respecte_la_peremption_et_lordre() {
    let mut lib = Library::open_in_memory().unwrap();
    // Deux morceaux pour « gros », un seul pour « petit » : le plus fourni
    // d'abord.
    for (i, mbid, nom) in [
        (1, "gros", "Gros"),
        (2, "gros", "Gros"),
        (3, "petit", "Petit"),
    ] {
        lib.upsert(&TrackMeta {
            path: format!("/m/{i}.mp3").into(),
            title: Some("t".into()),
            artist: Some(nom.into()),
            album: Some("al".into()),
            album_artist: Some(nom.into()),
            mb_album_artist_id: Some(mbid.into()),
            ..Default::default()
        })
        .unwrap();
    }

    let attente = lib.decouvrir_en_attente("voisins", 30, 0).unwrap();
    assert_eq!(
        attente.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(),
        ["gros", "petit"]
    );

    // « gros » vient d'être interrogé : il sort de la liste jusqu'à
    // péremption.
    lib.decouvrir_poser_voisins("gros", &[]).unwrap();
    let attente = lib.decouvrir_en_attente("voisins", 30, 0).unwrap();
    assert_eq!(
        attente.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(),
        ["petit"]
    );
}

#[test]
fn decouvrir_voisins_ecarte_les_artistes_de_la_bibliotheque() {
    let mut lib = Library::open_in_memory().unwrap();
    lib.upsert(&TrackMeta {
        path: "/m/1.mp3".into(),
        title: Some("t".into()),
        artist: Some("Connu".into()),
        album: Some("al".into()),
        album_artist: Some("Connu".into()),
        mb_album_artist_id: Some("src".into()),
        ..Default::default()
    })
    .unwrap();
    lib.upsert(&TrackMeta {
        path: "/m/2.mp3".into(),
        title: Some("t".into()),
        artist: Some("Déjà là".into()),
        album: Some("al".into()),
        album_artist: Some("Déjà là".into()),
        mb_album_artist_id: Some("dedans".into()),
        ..Default::default()
    })
    .unwrap();

    lib.decouvrir_poser_voisins(
        "src",
        &[
            ("dehors".into(), "Dehors".into(), 0.9, "listenbrainz".into()),
            (
                "dedans".into(),
                "Déjà là".into(),
                0.8,
                "listenbrainz".into(),
            ),
        ],
    )
    .unwrap();

    let fil = lib.decouvrir_fil(30).unwrap();
    assert_eq!(fil.voisins.len(), 1);
    assert_eq!(fil.voisins[0].dst_mbid, "dehors");
    assert_eq!(fil.voisins[0].portes, vec!["Connu".to_string()]);
}

#[test]
fn rangs_percentiles_place_par_valeur_croissante() {
    let r = rangs_percentiles([(10, 5.0), (20, 5.0), (30, 100.0), (40, 1.0)].into_iter());
    assert_eq!(r[&40], 0.0, "la plus petite valeur → 0");
    assert_eq!(r[&30], 1.0, "la plus grande → 1");
    // Deux valeurs égales : même rang (part des valeurs strictement plus petites).
    assert_eq!(r[&10], r[&20]);
    assert!((r[&10] - (1.0 / 3.0)).abs() < 1e-9);
}

/// La popularité par morceau : posée par source, résolue au meilleur
/// échelon, mélangée en rang. Un morceau sans aucune source n'a pas de ligne.
#[test]
fn recalcul_track_popularite_resout_echelon_et_melange_les_rangs() {
    let mut lib = Library::open_in_memory().unwrap();
    let ajoute = |lib: &Library, id: i64, rec: &str, art: &str, album: &str| {
        lib.conn
            .execute(
                "INSERT INTO tracks (id, path, mb_recording_id, mb_artist_id, mb_album_artist_id, album, added_at)
                 VALUES (?1, ?2, ?3, ?4, ?4, ?5, 0)",
                params![id, format!("/m/{id}.flac"), rec, art, album],
            )
            .unwrap();
    };
    ajoute(&lib, 1, "rec-a", "art-x", "Disque X");
    ajoute(&lib, 2, "rec-b", "art-x", "Disque X"); // même album, pas de reco propre côté LB
    ajoute(&lib, 3, "rec-c", "art-y", "Disque Y");
    ajoute(&lib, 4, "rec-d", "art-z", "Disque Z"); // aucune source → pas de ligne

    lib.conn
        .execute(
            "INSERT INTO mb_release_groups (mbid, artist_mbid, title, title_norm)
             VALUES ('rg-x', 'art-x', 'Disque X', 'disquex'), ('rg-y', 'art-y', 'Disque Y', 'disquey')",
            [],
        )
        .unwrap();

    lib.pop_poser(
        "listenbrainz",
        "recording",
        &["rec-a".into(), "rec-c".into()],
        &[
            PopulariteBrute {
                mbid: "rec-a",
                ecoutes: 100,
                auditeurs: Some(10),
            },
            PopulariteBrute {
                mbid: "rec-c",
                ecoutes: 5,
                auditeurs: Some(1),
            },
        ],
    )
    .unwrap();
    lib.pop_poser(
        "listenbrainz",
        "release-group",
        &["rg-x".into(), "rg-y".into()],
        &[
            PopulariteBrute {
                mbid: "rg-x",
                ecoutes: 50,
                auditeurs: Some(5),
            },
            PopulariteBrute {
                mbid: "rg-y",
                ecoutes: 9,
                auditeurs: Some(2),
            },
        ],
    )
    .unwrap();
    lib.pop_poser(
        "deezer",
        "recording",
        &["rec-a".into()],
        &[PopulariteBrute {
            mbid: "rec-a",
            ecoutes: 900_000,
            auditeurs: None,
        }],
    )
    .unwrap();

    let couverts = lib.recalculer_track_popularite().unwrap();
    assert_eq!(couverts, 3, "le morceau 4 (aucune source) est écarté");

    let lignes: Vec<(i64, f64, String)> = lib
        .conn
        .prepare("SELECT track_id, relative, echelon FROM track_popularite ORDER BY track_id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap();

    assert_eq!(lignes.len(), 3);
    assert_eq!(lignes[0].0, 1);
    assert_eq!(lignes[0].2, "recording"); // reco LB + Deezer
    assert_eq!(lignes[1].0, 2);
    assert_eq!(lignes[1].2, "release-group"); // repli sur l'album X
    assert_eq!(lignes[2].2, "recording"); // reco LB connue
                                          // Le morceau 1 (le plus écouté partout) domine.
    assert!(lignes[0].1 > lignes[1].1 && lignes[0].1 > lignes[2].1);

    // `popularites` ne rend que les morceaux couverts parmi ceux demandés.
    let mut vus = lib.popularites(&[1, 2, 4, 999]).unwrap();
    vus.sort_by_key(|(id, ..)| *id);
    assert_eq!(
        vus.iter().map(|(id, ..)| *id).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(lib.popularites(&[]).unwrap(), Vec::new());
}

/// Une entité déjà interrogée ne revient pas dans les candidats — sauf si
/// `depuis` la déclare périmée.
#[test]
fn pop_candidats_excluent_ce_qui_est_deja_fait() {
    let mut lib = Library::open_in_memory().unwrap();
    lib.conn
        .execute(
            "INSERT INTO tracks (id, path, mb_recording_id, artist, title, added_at)
             VALUES (1, '/m/1.flac', 'rec-1', 'Art', 'Titre', 0)",
            [],
        )
        .unwrap();

    assert_eq!(lib.pop_recordings_candidats(0, 100).unwrap().len(), 1);

    // Fait sur une seule source : encore candidat (l'autre reste à faire).
    lib.pop_poser("listenbrainz", "recording", &["rec-1".into()], &[])
        .unwrap();
    assert_eq!(lib.pop_recordings_candidats(0, 100).unwrap().len(), 1);

    // Fait sur les deux : plus candidat.
    lib.pop_poser("deezer", "recording", &["rec-1".into()], &[])
        .unwrap();
    assert!(lib.pop_recordings_candidats(0, 100).unwrap().is_empty());

    // `depuis` très grand : tout fetch compte comme périmé, l'entité revient.
    let futur = i64::MAX;
    assert_eq!(lib.pop_recordings_candidats(futur, 100).unwrap().len(), 1);
    assert!(lib
        .pop_deja_fait("listenbrainz", "recording", futur)
        .unwrap()
        .is_empty());
    assert_eq!(
        lib.pop_deja_fait("listenbrainz", "recording", 0)
            .unwrap()
            .len(),
        1
    );
}

/// La ligne d'alerte : rien à signaler tant que rien n'est vieux ; l'âge
/// d'un fetch le fait basculer dans « périmé ».
#[test]
fn popularite_fraicheur_compte_le_perime() {
    let mut lib = Library::open_in_memory().unwrap();
    lib.conn
        .execute(
            "INSERT INTO tracks (id, path, added_at) VALUES (1, '/m/1.flac', 0)",
            [],
        )
        .unwrap();
    lib.pop_poser("listenbrainz", "recording", &["rec-neuf".into()], &[])
        .unwrap();
    // Un fetch daté d'il y a 200 jours.
    lib.conn
        .execute(
            "INSERT INTO popularite_fetched (mbid, kind, source, at)
             VALUES ('rec-vieux', 'recording', 'listenbrainz',
                     strftime('%s','now') - 200*86400)",
            [],
        )
        .unwrap();

    let (couverts, _plus_ancienne, perimes) = lib.popularite_fraicheur(90).unwrap();
    assert_eq!(couverts, 0, "aucun track_popularite calculé ici");
    assert_eq!(perimes, 1, "seul le fetch de 200 j dépasse 90 j");
    // Seuil relevé au-delà de l'âge du vieux : plus rien de périmé.
    assert_eq!(lib.popularite_fraicheur(300).unwrap().2, 0);
}

/// Un artiste sans MBID n'entre jamais dans les candidats — aucun repli
/// par nom, voir `docs/enrichissement-lecteur.md`.
#[test]
fn theaudiodb_candidats_ignore_les_artistes_sans_mbid() {
    let mut lib = Library::open_in_memory().unwrap();
    lib.conn
        .execute_batch(
            "INSERT INTO tracks (id, path, mb_artist_id, added_at) VALUES (1, '/m/1.flac', 'art-1', 0);
             INSERT INTO tracks (id, path, mb_artist_id, added_at) VALUES (2, '/m/2.flac', NULL, 0);",
        )
        .unwrap();
    assert_eq!(
        lib.theaudiodb_candidats(0, 100).unwrap(),
        vec!["art-1".to_string()]
    );

    lib.theaudiodb_poser(
        &["art-1".to_string()],
        &[BioBrute {
            mb_artist_id: "art-1".into(),
            id_theaudiodb: Some("111".into()),
            biographie_en: Some("bio en".into()),
            biographie_fr: Some("bio fr".into()),
        }],
    )
    .unwrap();
    assert!(lib.theaudiodb_candidats(0, 100).unwrap().is_empty());

    let bios = lib.bio_pour_piste(1).unwrap();
    assert_eq!(bios.len(), 1);
    assert_eq!(bios[0].biographie_fr.as_deref(), Some("bio fr"));
    assert!(lib.bio_pour_piste(2).unwrap().is_empty());
}

/// `stats_sans_bio` compte un morceau sans MBID d'artiste comme sans
/// biographie, et un morceau dont l'artiste n'a été interrogé qu'à vide
/// (marqué « déjà demandé » sans qu'aucune biographie n'ait été posée)
/// tout autant — seule une biographie effectivement posée retire un
/// morceau du compte.
#[test]
fn stats_sans_bio_retombe_sur_absence_de_mbid_et_de_biographie() {
    let mut lib = Library::open_in_memory().unwrap();
    lib.conn
        .execute_batch(
            "INSERT INTO tracks (id, path, mb_artist_id, added_at) VALUES (1, '/m/1.flac', 'art-1', 0);
             INSERT INTO tracks (id, path, mb_artist_id, added_at) VALUES (2, '/m/2.flac', 'art-2', 0);
             INSERT INTO tracks (id, path, mb_artist_id, added_at) VALUES (3, '/m/3.flac', NULL, 0);",
        )
        .unwrap();
    assert_eq!(lib.stats_sans_bio().unwrap(), 3);

    // Interrogé mais sans biographie trouvée : ne compte toujours pas.
    lib.theaudiodb_poser(&["art-2".to_string()], &[]).unwrap();
    assert_eq!(lib.stats_sans_bio().unwrap(), 3);

    lib.theaudiodb_poser(
        &["art-1".to_string()],
        &[BioBrute {
            mb_artist_id: "art-1".into(),
            id_theaudiodb: Some("111".into()),
            biographie_en: Some("bio en".into()),
            biographie_fr: None,
        }],
    )
    .unwrap();
    assert_eq!(
        lib.stats_sans_bio().unwrap(),
        2,
        "seul le morceau 1 a désormais une biographie"
    );
}

/// `stats_sans_discogs` exige un crédit *ou* un label — l'un des deux
/// suffit à sortir un morceau du compte, mais une édition liée sans que
/// l'import n'ait rien trouvé y reste.
#[test]
fn stats_sans_discogs_exige_un_credit_ou_un_label() {
    let mut lib = Library::open_in_memory().unwrap();
    lib.conn
        .execute_batch(
            "INSERT INTO tracks (id, path, mb_release_id, added_at) VALUES (1, '/m/1.flac', 'rel-1', 0);
             INSERT INTO tracks (id, path, mb_release_id, added_at) VALUES (2, '/m/2.flac', 'rel-2', 0);
             INSERT INTO tracks (id, path, mb_release_id, added_at) VALUES (3, '/m/3.flac', NULL, 0);",
        )
        .unwrap();
    assert_eq!(lib.stats_sans_discogs().unwrap(), 3);

    // Édition reliée mais l'import n'y a rien trouvé : compte encore.
    lib.discogs_lier_edition("rel-2", Some(500)).unwrap();
    assert_eq!(lib.stats_sans_discogs().unwrap(), 3);

    lib.labels_poser(
        500,
        &[LabelDiscogs {
            nom: "Label".into(),
            catno: "".into(),
            discogs_label_id: None,
        }],
    )
    .unwrap();
    assert_eq!(
        lib.stats_sans_discogs().unwrap(),
        2,
        "le morceau 2 a désormais un label"
    );
}

/// `stats_sans_critique` résout le release-group par (artiste, titre
/// normalisé) — même appariement que `critiques_pour_piste` — plutôt que
/// de compter les critiques une piste à la fois.
#[test]
fn stats_sans_critique_resout_par_artiste_et_titre_normalise() {
    let mut lib = Library::open_in_memory().unwrap();
    lib.conn
        .execute_batch(
            "INSERT INTO tracks (id, path, mb_artist_id, album, added_at)
               VALUES (1, '/m/1.flac', 'art-1', 'Titre', 0);
             INSERT INTO tracks (id, path, mb_artist_id, album, added_at)
               VALUES (2, '/m/2.flac', 'art-2', 'Autre Album', 0);
             INSERT INTO mb_release_groups (mbid, artist_mbid, title, title_norm)
               VALUES ('rg-1', 'art-1', 'Titre', 'titre');",
        )
        .unwrap();
    // Le morceau 2 n'a pas de release-group connu du tout.
    assert_eq!(lib.stats_sans_critique().unwrap(), 2);

    lib.critiques_poser("rg-1", &[]).unwrap();
    assert_eq!(
        lib.stats_sans_critique().unwrap(),
        2,
        "release-group interrogé, mais rien trouvé"
    );

    lib.critiques_poser(
        "rg-1",
        &[CritiqueBrute {
            id: "crit-1".into(),
            auteur: None,
            licence_id: "CC BY-SA 3.0".into(),
            licence_nom: None,
            langue: None,
            texte: "Un album marquant.".into(),
            url_originale: None,
        }],
    )
    .unwrap();
    assert_eq!(
        lib.stats_sans_critique().unwrap(),
        1,
        "le morceau 1 a désormais une critique"
    );
}

/// Le lien Discogs se pose y compris quand il est absent (`None`) — pour
/// ne pas revérifier une édition sans relation cataloguée à chaque passe.
#[test]
fn discogs_liaison_candidats_et_lien_absent_memorise() {
    let lib = Library::open_in_memory().unwrap();
    lib.conn
        .execute(
            "INSERT INTO tracks (id, path, mb_release_id, added_at)
             VALUES (1, '/m/1.flac', 'rel-1', 0)",
            [],
        )
        .unwrap();
    assert_eq!(
        lib.discogs_liaison_candidats(100).unwrap(),
        vec!["rel-1".to_string()]
    );

    lib.discogs_lier_edition("rel-1", None).unwrap();
    assert!(lib.discogs_liaison_candidats(100).unwrap().is_empty());
    assert!(lib.discogs_wanted_ids().unwrap().is_empty());
}

/// Les crédits se retrouvent depuis une piste via son édition MusicBrainz,
/// et un import réécrit ceux d'une édition plutôt que de les cumuler.
#[test]
fn credits_poser_remplace_les_credits_dune_edition() {
    let mut lib = Library::open_in_memory().unwrap();
    lib.conn
        .execute(
            "INSERT INTO tracks (id, path, mb_release_id, added_at)
             VALUES (1, '/m/1.flac', 'rel-1', 0)",
            [],
        )
        .unwrap();
    lib.discogs_lier_edition("rel-1", Some(249_504)).unwrap();

    lib.credits_poser(
        249_504,
        &[CreditDiscogs {
            personne: "Ancien".into(),
            role: "Mixed By".into(),
            pistes: String::new(),
            discogs_artist_id: None,
        }],
    )
    .unwrap();
    assert_eq!(lib.credits_pour_piste(1).unwrap().len(), 1);

    lib.credits_poser(
        249_504,
        &[CreditDiscogs {
            personne: "Nouveau".into(),
            role: "Producer".into(),
            pistes: "A1, A2".into(),
            discogs_artist_id: Some(1),
        }],
    )
    .unwrap();
    let credits = lib.credits_pour_piste(1).unwrap();
    assert_eq!(credits.len(), 1, "l'ancien crédit doit avoir été remplacé");
    assert_eq!(credits[0].personne, "Nouveau");
    assert_eq!(credits[0].pistes, "A1, A2");
}

/// Même structure que `credits_poser_remplace_les_credits_dune_edition` :
/// un import réécrit les labels d'une édition plutôt que de les cumuler.
#[test]
fn labels_poser_remplace_les_labels_dune_edition() {
    let mut lib = Library::open_in_memory().unwrap();
    lib.conn
        .execute(
            "INSERT INTO tracks (id, path, mb_release_id, added_at)
             VALUES (1, '/m/1.flac', 'rel-1', 0)",
            [],
        )
        .unwrap();
    lib.discogs_lier_edition("rel-1", Some(249_504)).unwrap();

    lib.labels_poser(
        249_504,
        &[LabelDiscogs {
            nom: "Ancien Label".into(),
            catno: "OLD1".into(),
            discogs_label_id: None,
        }],
    )
    .unwrap();
    assert_eq!(lib.labels_pour_piste(1).unwrap().len(), 1);

    lib.labels_poser(
        249_504,
        &[LabelDiscogs {
            nom: "Nouveau Label".into(),
            catno: "NEW1".into(),
            discogs_label_id: Some(1),
        }],
    )
    .unwrap();
    let labels = lib.labels_pour_piste(1).unwrap();
    assert_eq!(labels.len(), 1, "l'ancien label doit avoir été remplacé");
    assert_eq!(labels[0].nom, "Nouveau Label");
    assert_eq!(labels[0].catno, "NEW1");
}

/// La justification même de la fonctionnalité : une édition sur un
/// sous-label (Big Dada) porte aussi son label parent (Ninja Tune), les
/// deux doivent revenir, triés par nom — pas un seul « label principal ».
#[test]
fn labels_pour_piste_rend_plusieurs_labels_dune_edition() {
    let mut lib = Library::open_in_memory().unwrap();
    lib.conn
        .execute(
            "INSERT INTO tracks (id, path, mb_release_id, added_at)
             VALUES (1, '/m/1.flac', 'rel-1', 0)",
            [],
        )
        .unwrap();
    lib.discogs_lier_edition("rel-1", Some(249_504)).unwrap();

    lib.labels_poser(
        249_504,
        &[
            LabelDiscogs {
                nom: "Ninja Tune".into(),
                catno: "ZEN123".into(),
                discogs_label_id: Some(222),
            },
            LabelDiscogs {
                nom: "Big Dada".into(),
                catno: "BD456".into(),
                discogs_label_id: Some(333),
            },
        ],
    )
    .unwrap();

    let labels = lib.labels_pour_piste(1).unwrap();
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[0].nom, "Big Dada");
    assert_eq!(labels[1].nom, "Ninja Tune");
}

/// Le cas qui a motivé `discogs_importes` : une édition reliée mais
/// jamais importée (liaison faite sous un binaire d'avant ce correctif,
/// ou import interrompu) doit rester détectée comme « utile à importer »
/// même si *cette* liaison-ci n'a rien relié de neuf — l'ancien critère
/// (« la liaison vient-elle de relier quelque chose ? ») la manquait.
#[test]
fn discogs_import_utile_detecte_une_edition_reliee_jamais_importee() {
    let lib = Library::open_in_memory().unwrap();
    assert!(
        !lib.discogs_import_utile().unwrap(),
        "rien de relié : rien à importer"
    );

    lib.discogs_lier_edition("rel-1", Some(249_504)).unwrap();
    assert!(
        lib.discogs_import_utile().unwrap(),
        "reliée, jamais importée"
    );

    lib.discogs_marquer_importe(249_504).unwrap();
    assert!(
        !lib.discogs_import_utile().unwrap(),
        "marquée importée, même sans crédit ni label trouvé"
    );

    // Une deuxième édition reliée, elle, redevient utile — la marque de
    // la première ne doit pas masquer la nouvelle.
    lib.discogs_lier_edition("rel-2", Some(300_000)).unwrap();
    assert!(lib.discogs_import_utile().unwrap());
}

/// Le repli pochette ne dépend que des tags de la piste : release MBID
/// telle quelle, et artiste d'album de préférence à l'artiste de la piste.
#[test]
fn repli_pochette_lit_release_et_artiste_album_depuis_les_tags() {
    let lib = Library::open_in_memory().unwrap();
    lib.conn
        .execute(
            "INSERT INTO tracks (path, artist, album, album_artist, mb_release_id)
             VALUES ('/m/a.mp3', 'Invité', 'Album', 'Groupe', 'rel-1'),
                    ('/m/b.mp3', 'Solo', 'Autre', '', NULL)",
            [],
        )
        .unwrap();
    let a = Path::new("/m/a.mp3");
    let b = Path::new("/m/b.mp3");
    assert_eq!(lib.release_pour_path(a).unwrap().as_deref(), Some("rel-1"));
    assert_eq!(lib.release_pour_path(b).unwrap(), None);
    assert_eq!(
        lib.release_pour_path(Path::new("/m/inconnu.mp3")).unwrap(),
        None
    );
    assert_eq!(
        lib.artiste_album_pour_path(a).unwrap(),
        Some(("Groupe".to_string(), "Album".to_string()))
    );
    lib.conn
        .execute(
            "INSERT INTO tracks (path, artist, album, album_artist)
             VALUES ('/m/a2.mp3', 'Autre invité', 'Album', 'Groupe'),
                    ('/m/a3.mp3', 'Groupe', 'Album', 'Autre groupe')",
            [],
        )
        .unwrap();
    assert_eq!(
        lib.pistes_soeurs(a, 8).unwrap(),
        [PathBuf::from("/m/a2.mp3")],
        "même album et même artiste d'album, sans la piste elle-même ni un homonyme"
    );
    assert_eq!(
        lib.artiste_album_pour_path(b).unwrap(),
        Some(("Solo".to_string(), "Autre".to_string())),
        "album_artist vide : repli sur l'artiste de la piste"
    );
}

/// « Aucune critique » se mémorise comme les autres réponses vides du
/// projet — l'album ne revient pas à chaque passe.
#[test]
fn critiques_poser_memorise_labsence_de_critique() {
    let mut lib = Library::open_in_memory().unwrap();
    lib.conn
        .execute(
            "INSERT INTO mb_release_groups (mbid, artist_mbid, title, title_norm)
             VALUES ('rg-1', 'art-1', 'Titre', 'titre')",
            [],
        )
        .unwrap();
    assert_eq!(
        lib.critiques_candidats(0, 100).unwrap(),
        vec!["rg-1".to_string()]
    );

    lib.critiques_poser("rg-1", &[]).unwrap();
    assert!(lib.critiques_candidats(0, 100).unwrap().is_empty());

    lib.critiques_poser(
        "rg-1",
        &[CritiqueBrute {
            id: "crit-1".into(),
            auteur: Some("Quelquun".into()),
            licence_id: "CC BY-SA 3.0".into(),
            licence_nom: Some("Creative Commons Attribution-ShareAlike 3.0".into()),
            langue: Some("fr".into()),
            texte: "Un album marquant.".into(),
            url_originale: Some("https://critiquebrainz.org/review/crit-1".into()),
        }],
    )
    .unwrap();
    let critiques = lib.critiques_pour_release_group("rg-1").unwrap();
    assert_eq!(critiques.len(), 1);
    assert_eq!(critiques[0].auteur.as_deref(), Some("Quelquun"));
}

/// `mb_release_groups` porte toute la discographie d'un artiste (utile à
/// `mb_albums` pour désambiguïser un titre), mais un seul des deux albums
/// de l'artiste ici est réellement dans la bibliothèque — seul son
/// release-group doit ressortir, pas celui du disque qu'on n'a pas. Voir
/// `critiques::actualiser`, qui s'en sert pour ne pas interroger
/// CritiqueBrainz sur des albums jamais affichés.
#[test]
fn release_groups_possedes_exclut_le_reste_de_la_discographie() {
    let lib = Library::open_in_memory().unwrap();
    lib.conn
        .execute_batch(
            "INSERT INTO mb_release_groups (mbid, artist_mbid, title, title_norm)
             VALUES ('rg-possede', 'art-1', 'Album Possédé', 'albumpossede'),
                    ('rg-absent', 'art-1', 'Autre Album', 'autrealbum')",
        )
        .unwrap();
    lib.upsert(&TrackMeta {
        path: "/m/1.flac".into(),
        mb_artist_id: Some("art-1".into()),
        album: Some("Album Possédé".into()),
        ..Default::default()
    })
    .unwrap();

    let possedes = lib.release_groups_possedes().unwrap();
    assert_eq!(possedes, HashSet::from(["rg-possede".to_string()]));
}

/// Bout en bout : `familles_votees` combine MusicBrainz, CLAP-texte et
/// Last.fm. Une famille où deux sources s'accordent sort fiable ; une où
/// les trois divergent replie sur MusicBrainz, comme avant ce chantier.
#[test]
fn familles_votees_distingue_laccord_du_desaccord() {
    let mut lib = Library::open_in_memory().unwrap();

    let ajoute = |lib: &Library, path: &str, artiste_mbid: &str, cluster: i64| -> i64 {
        let id = lib
            .upsert(&TrackMeta {
                path: path.into(),
                artist: Some(artiste_mbid.into()),
                mb_artist_id: Some(artiste_mbid.into()),
                ..Default::default()
            })
            .unwrap();
        lib.save_features(id, "clap", &[0.0], 0.0, 0.0, cluster)
            .unwrap();
        id
    };

    // Famille 1 : MusicBrainz et Last.fm s'accordent sur « metal ».
    for i in 0..5 {
        let id = ajoute(&lib, &format!("/m/x{i}.mp3"), "mbid-x", 1);
        lib.conn
            .execute(
                "UPDATE features SET texte_label = 'A Punk Rock Band Playing Fast'
                  WHERE track_id = ?1 AND model = 'clap'",
                rusqlite::params![id],
            )
            .unwrap();
    }
    lib.conn
        .execute(
            "INSERT INTO mb_genres (mbid, kind, genre, votes) VALUES ('mbid-x','artist','Alternative Metal',5)",
            [],
        )
        .unwrap();
    lib.lastfm_tags_poser(
        "mbid-x",
        &[crate::lastfm::Tag {
            nom: "alternative metal".into(),
            poids: 50,
        }],
    )
    .unwrap();

    // Famille 2 : les trois sources divergent, comme le ska français de
    // l'essai — repli attendu sur le nom MusicBrainz seul.
    for i in 0..5 {
        let id = ajoute(&lib, &format!("/m/y{i}.mp3"), "mbid-y", 2);
        lib.conn
            .execute(
                "UPDATE features SET texte_label = 'An African Percussion Ensemble'
                  WHERE track_id = ?1 AND model = 'clap'",
                rusqlite::params![id],
            )
            .unwrap();
    }
    lib.conn
        .execute(
            "INSERT INTO mb_genres (mbid, kind, genre, votes) VALUES ('mbid-y','artist','Reggae',5)",
            [],
        )
        .unwrap();
    lib.lastfm_tags_poser(
        "mbid-y",
        &[crate::lastfm::Tag {
            nom: "worldbeat".into(),
            poids: 50,
        }],
    )
    .unwrap();

    let votes = lib.familles_votees("clap").unwrap();
    let f1 = votes.iter().find(|v| v.cluster == 1).unwrap();
    assert!(f1.fiable, "metal : MusicBrainz et Last.fm s'accordent");
    assert_eq!(f1.nom_affiche, "Alternative Metal");

    let f2 = votes.iter().find(|v| v.cluster == 2).unwrap();
    assert!(
        !f2.fiable,
        "trois sources en désaccord, comme le ska de l'essai"
    );
    assert_eq!(f2.nom_affiche, "Reggae");
    assert_eq!(f2.musicbrainz, "Reggae");
    assert_eq!(f2.clap_texte, "An African Percussion Ensemble");
    assert_eq!(f2.lastfm, "Worldbeat");

    // `familles()` (les 5 points de lecture de l'interface) projette le
    // même nom affiché, signature inchangée.
    let simples = lib.familles("clap").unwrap();
    assert!(simples.contains(&(1, "Alternative Metal".to_string(), 5)));
    assert!(simples.contains(&(2, "Reggae".to_string(), 5)));

    // Affinage local : les mêmes morceaux, redécoupés en sous-groupes
    // ignorant `features.cluster` — l'identifiant de sous-groupe est
    // local à cet appel, jamais réécrit en base.
    let mut assignations: Vec<(i64, i64)> = Vec::new();
    for (i, (id, _)) in lib
        .embeddings_du_cluster("clap", 1)
        .unwrap()
        .into_iter()
        .enumerate()
    {
        assignations.push((id, (i % 2) as i64));
    }
    for (id, _) in lib.embeddings_du_cluster("clap", 2).unwrap() {
        assignations.push((id, 9)); // un sous-groupe à part, sans rapport
    }
    let sous = lib.sous_groupes_votes("clap", &assignations).unwrap();
    // Trois sous-groupes locaux (0, 1, 9), aucun n'est un cluster 1/2 de
    // `features.cluster` — la fonction ne lit que ce qu'on lui donne.
    let mut ids: Vec<i64> = sous.iter().map(|v| v.cluster).collect();
    ids.sort();
    assert_eq!(ids, vec![0, 1, 9]);
}

/// Régression, mesurée en développant ce chantier sur la bibliothèque
/// réelle : quand CLAP-texte et Last.fm n'ont **aucune** donnée (l'état
/// courant tant que le vocabulaire n'est pas généré et que Last.fm n'a
/// pas tourné), les deux tombaient sur le même repli « Famille N » —
/// même rang, même liste de familles — et `arbitrer_vote` prenait ce
/// faux consensus pour un accord, effaçant les vrais noms MusicBrainz de
/// toute la carte. `familles()` doit rendre les noms MusicBrainz
/// intacts, comme avant l'introduction du vote.
#[test]
fn familles_votees_ignore_le_faux_accord_de_deux_replis() {
    let lib = Library::open_in_memory().unwrap();
    let ajoute = |path: &str, artiste_mbid: &str, cluster: i64| {
        let id = lib
            .upsert(&TrackMeta {
                path: path.into(),
                mb_artist_id: Some(artiste_mbid.into()),
                ..Default::default()
            })
            .unwrap();
        lib.save_features(id, "clap", &[0.0], 0.0, 0.0, cluster)
            .unwrap();
    };
    for i in 0..5 {
        ajoute(&format!("/m/x{i}.mp3"), "mbid-x", 1);
    }
    for i in 0..5 {
        ajoute(&format!("/m/y{i}.mp3"), "mbid-y", 2);
    }
    lib.conn
        .execute(
            "INSERT INTO mb_genres (mbid, kind, genre, votes) VALUES ('mbid-x','artist','Rock',5)",
            [],
        )
        .unwrap();
    lib.conn
        .execute(
            "INSERT INTO mb_genres (mbid, kind, genre, votes) VALUES ('mbid-y','artist','Jazz',5)",
            [],
        )
        .unwrap();
    // Ni `texte_label` (aucun vocabulaire généré), ni `lastfm_tags`
    // (aucune passe Last.fm) — l'état par défaut d'une bibliothèque qui
    // n'a pas encore mis en place ces deux sources.

    let votes = lib.familles_votees("clap").unwrap();
    let f1 = votes.iter().find(|v| v.cluster == 1).unwrap();
    let f2 = votes.iter().find(|v| v.cluster == 2).unwrap();
    assert!(!f1.fiable);
    assert!(!f2.fiable);
    assert_eq!(f1.nom_affiche, "Rock");
    assert_eq!(f2.nom_affiche, "Jazz");
    assert_ne!(
        f1.nom_affiche, f2.nom_affiche,
        "pas de faux consensus entre les deux familles"
    );
}
