// SPDX-License-Identifier: GPL-3.0-or-later
//! Pochettes : extraction locale, repli en ligne (Deezer, TheAudioDB) et
//! cache sur disque.

use super::*;

/// Pochette d'un morceau, en `data:` URI directement consommable par `<img>`.
///
/// **Mise en cache sur disque**, sous `<données app>/pochettes/` — voir
/// [`cle_pochette`]. Le cache mémoire côté interface (`ui/js/`) évite de
/// repayer les 50 à 210 ms d'extraction pendant une session, mais repart à
/// froid à chaque lancement ; celui-ci persiste, et une pochette déjà servie
/// une fois ne relit plus jamais le fichier source tant qu'il n'a pas changé.
///
/// **Corps bloquant renvoyé sur le pool `spawn_blocking`.** La lecture disque
/// et le parsing des tags (`lofty`) coûtent 50 à 210 ms à froid, et la grille
/// d'albums en demande des dizaines d'un coup. Exécutées telles quelles sur le
/// pool du runtime, elles en occupaient tous les fils et une commande de
/// transport (`play`, `toggle_pause`) attendait derrière — la lecture partait
/// avec plusieurs secondes de retard pendant qu'on faisait défiler les
/// pochettes. Le pool dédié aux tâches bloquantes n'entre pas en concurrence
/// avec les commandes courtes.
#[tauri::command]
pub(crate) async fn cover(etat: State<'_, Etat>, path: String) -> Result<Option<String>, String> {
    let dossier_cache = etat.db.with_file_name("pochettes");
    let db_path = etat.db.clone();
    tauri::async_runtime::spawn_blocking(move || cover_extraire(&dossier_cache, &db_path, &path))
        .await
        .map_err(echec)?
}

pub(crate) fn cover_extraire(
    dossier_cache: &Path,
    db_path: &Path,
    path: &str,
) -> Result<Option<String>, String> {
    let chemin = Path::new(path);

    // Sans métadonnées lisibles (fichier disparu, permission refusée), pas de
    // clé de cache fiable : on extrait directement, comme avant ce cache.
    let mtime = std::fs::metadata(chemin)
        .ok()
        .and_then(|m| m.modified().ok());
    let cle = mtime.map(|m| cle_pochette(chemin, m));

    if let Some(cle) = &cle {
        if let Some(valeur) = lire_cache_pochette(dossier_cache, cle) {
            return Ok(valeur);
        }
    }

    let cover = cover_locale(db_path, chemin);
    let (valeur, definitif) = match cover {
        Some(c) => {
            let mime = c.mime.as_deref().unwrap_or("image/jpeg");
            (
                Some(format!("data:{mime};base64,{}", base64(&c.data))),
                true,
            )
        }
        None => match cover_depuis_reseau(dossier_cache, db_path, chemin) {
            Repli::Trouvee(v) => (Some(v), true),
            Repli::Absente => (None, true),
            // Une panne réseau n'est pas une réponse : ne rien écrire, le
            // prochain affichage réessaiera.
            Repli::Indeterminee => (None, false),
        },
    };
    if definitif {
        if let Some(cle) = &cle {
            ecrire_cache_pochette(dossier_cache, cle, valeur.as_deref());
        }
    }
    Ok(valeur)
}

/// Combien de pistes sœurs tenter avant de passer au réseau. Sur la
/// bibliothèque de test, quand la première piste n'a pas d'image, la
/// deuxième la porte (4 albums sur 4) : huit est large sans relire un album
/// de 55 pistes en entier.
pub(crate) const PISTES_SOEURS_MAX: usize = 8;

/// La pochette locale d'un morceau : ses tags, son dossier, puis celles des
/// autres pistes de l'album — la grille n'en présente qu'une (la première par
/// chemin), et c'est parfois précisément celle qui n'a pas d'image.
///
/// Un fichier dont les tags ne se lisent pas compte comme « sans pochette »
/// plutôt que d'échouer : il passe alors au repli réseau au lieu de
/// disparaître en silence (l'interface avale l'erreur de `cover`).
pub(crate) fn cover_locale(db_path: &Path, chemin: &Path) -> Option<rusty_music_core::Cover> {
    // La base d'abord : elle porte le choix « Image du dossier » de
    // l'utilisateur. Illisible, on lit quand même les tags.
    let Ok(lib) = rusty_music_core::db::Library::open(db_path) else {
        return rusty_music_core::tags::read_cover(chemin).ok().flatten();
    };
    if let Ok(Some(c)) = lib.lire_pochette(chemin) {
        return Some(c);
    }
    lib.pistes_soeurs(chemin, PISTES_SOEURS_MAX)
        .ok()?
        .iter()
        .find_map(|p| lib.lire_pochette(p).ok().flatten())
}

/// Issue du repli réseau quand ni les tags ni le dossier n'ont de pochette.
pub(crate) enum Repli {
    Trouvee(String),
    /// Toutes les sources interrogées ont répondu « pas de pochette ».
    Absente,
    /// Au moins une source n'a pas pu répondre (panne réseau, base illisible).
    Indeterminee,
}

/// Réponse d'une source, avant agrégation.
pub(crate) enum Reponse {
    Image(String),
    Non,
    Panne,
}

/// Une source du repli, avec son cache **d'album** (`cle`, partagée par tous
/// les morceaux de l'album) : un hit ou un 404 déjà connu ne repart pas sur le
/// réseau ; une panne n'est jamais écrite.
pub(crate) fn source_de_pochette(
    dossier_cache: &Path,
    cle: &str,
    appel: impl FnOnce() -> rusty_music_core::error::Result<Option<Vec<u8>>>,
) -> Reponse {
    match lire_cache_pochette(dossier_cache, cle) {
        Some(Some(v)) => return Reponse::Image(v),
        Some(None) => return Reponse::Non,
        None => {}
    }
    match appel() {
        Ok(octets) => {
            let valeur = octets.map(|o| format!("data:image/jpeg;base64,{}", base64(&o)));
            ecrire_cache_pochette(dossier_cache, cle, valeur.as_deref());
            valeur.map_or(Reponse::Non, Reponse::Image)
        }
        Err(_) => Reponse::Panne,
    }
}

/// Clé de cache disque d'une pochette Deezer : par album (artiste et titre
/// normalisés), partagée par la grille d'Écouter et le fil Découvrir.
pub(crate) fn cle_pochette_deezer(artiste: &str, album: &str) -> String {
    format!(
        "dz-{:016x}",
        empreinte(
            format!(
                "{}\0{}",
                rusty_music_core::musicbrainz::cle_artiste(artiste),
                rusty_music_core::musicbrainz::normaliser_titre(album)
            )
            .as_bytes(),
        )
    )
}

/// Le client Deezer du processus : sa cadence (150 ms entre requêtes) n'a de
/// sens que partagée, la grille d'albums demande des dizaines de pochettes d'un
/// coup.
pub(crate) fn deezer() -> &'static rusty_music_core::deezer::Client {
    static CLIENT: std::sync::OnceLock<rusty_music_core::deezer::Client> =
        std::sync::OnceLock::new();
    CLIENT.get_or_init(rusty_music_core::deezer::Client::new)
}

/// Le client TheAudioDB (clé partagée) des photos d'artiste : sa cadence
/// (2,1 s entre requêtes, 30/min pour la clé partagée) n'a de sens que partagée
/// par tous les appels. Distinct du client de la passe « biographies », qui
/// porte la clé personnelle de l'utilisateur.
pub(crate) fn theaudiodb() -> &'static rusty_music_core::theaudiodb::Client {
    static CLIENT: std::sync::OnceLock<rusty_music_core::theaudiodb::Client> =
        std::sync::OnceLock::new();
    CLIENT.get_or_init(|| rusty_music_core::theaudiodb::Client::new(None))
}

/// Repli quand ni les tags ni le dossier n'ont de pochette (`docs/suite.md`,
/// dette « restent pochettes »), du plus fiable au plus flou :
///
/// 1. Cover Art Archive par **release** (`MUSICBRAINZ_ALBUMID` des tags) —
///    exacte, sans jointure de titre ;
/// 2. Cover Art Archive par **release-group** (artiste MBID + titre normalisé) ;
/// 3. Deezer par artiste + album, pour ce qui n'a aucun identifiant
///    MusicBrainz (~11 % de la bibliothèque) ou que CAA ne connaît pas.
///
/// Chaque étape ne passe à la suivante que sur « pas de pochette » ou panne.
/// Les clés de cache sont par album (`caar-`, `caa-` — partagée avec
/// `decouvrir_pochette` —, `dz-`) : un album nu n'est redemandé qu'une fois,
/// jamais par morceau. Rien n'est écrit dans la bibliothèque de l'utilisateur.
///
/// Ouvre sa propre connexion à la base plutôt que de faire remonter `Library`
/// jusqu'ici — même raison que le scan (voir le champ `db` de `Etat`).
pub(crate) fn cover_depuis_reseau(dossier_cache: &Path, db_path: &Path, chemin: &Path) -> Repli {
    let Ok(lib) = rusty_music_core::db::Library::open(db_path) else {
        return Repli::Indeterminee;
    };
    let release = lib.release_pour_path(chemin).ok().flatten();
    let rg_mbid = lib.release_group_pour_path(chemin).ok().flatten();
    let artiste_album = lib.artiste_album_pour_path(chemin).ok().flatten();
    drop(lib);

    let mut panne = false;
    let mut essayer = |reponse: Reponse| match reponse {
        Reponse::Image(v) => Some(v),
        Reponse::Non => None,
        Reponse::Panne => {
            panne = true;
            None
        }
    };

    if let Some(id) = &release {
        let r = source_de_pochette(dossier_cache, &format!("caar-{id}"), || {
            rusty_music_core::pochette::release(id)
        });
        if let Some(v) = essayer(r) {
            return Repli::Trouvee(v);
        }
    }
    if let Some(rg) = &rg_mbid {
        let r = source_de_pochette(dossier_cache, &format!("caa-{rg}"), || {
            rusty_music_core::pochette::release_group(rg)
        });
        if let Some(v) = essayer(r) {
            return Repli::Trouvee(v);
        }
    }
    if let Some((artiste, album)) = &artiste_album {
        let cle = cle_pochette_deezer(artiste, album);
        let r = source_de_pochette(dossier_cache, &cle, || {
            deezer().pochette_album(artiste, album)
        });
        if let Some(v) = essayer(r) {
            return Repli::Trouvee(v);
        }
    }
    if panne {
        Repli::Indeterminee
    } else {
        Repli::Absente
    }
}

/// Les chemins de piste d'au plus `max` albums d'un artiste qui portent une
/// pochette lisible — la matière du tuilage en mosaïque de la vue Artistes.
///
/// Rend des chemins, pas des images : l'interface les repasse à `cover`, dont
/// le cache mémoire (borné, partagé avec la grille d'albums et l'inspecteur)
/// et le cache disque font le reste. Le tri des tags, lui, se fait ici une
/// fois — chaque album candidat passe par [`cover_extraire`], qui remplit le
/// cache disque au passage et permet d'écarter les albums sans pochette (d'où
/// un résultat parfois plus court que `max`).
#[tauri::command]
pub(crate) async fn artist_covers(
    etat: State<'_, Etat>,
    name: String,
    mbid: Option<String>,
    max: usize,
) -> Result<Vec<String>, String> {
    let albums = {
        let lib = verrou(&etat.lib);
        lib.albums_of_artist(mbid.as_deref(), &name)
            .map_err(echec)?
    };
    // Borne dure : un artiste prolifique ne doit pas faire lire les tags de
    // cent albums pour une vignette qui n'en montre que quatre.
    let max = max.min(9);
    let dossier_cache = etat.db.with_file_name("pochettes");
    let db_path = etat.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut chemins: Vec<String> = Vec::new();
        for album in albums {
            if chemins.len() >= max {
                break;
            }
            if let Ok(Some(_)) = cover_extraire(&dossier_cache, &db_path, &album.path) {
                chemins.push(album.path);
            }
        }
        Ok(chemins)
    })
    .await
    .map_err(echec)?
}

/// Marqueur d'entrée « pas de pochette » dans le cache disque : un octet nul,
/// qu'aucune image ni aucune `data:` URI valide ne peut produire — pas
/// d'ambiguïté avec un contenu réel. Sans lui, un morceau sans pochette
/// resonderait ses tags à chaque affichage, cache ou pas.
pub(crate) const SANS_POCHETTE: &[u8] = b"\0";

/// Clé de cache : hachage FNV-1a (même construction que `db::hacher`) du
/// chemin **et** de la date de modification du fichier.
///
/// La mtime dans la clé, pas seulement le chemin, est ce qui rend
/// l'invalidation automatique : remplacer le fichier (nouveau tag,
/// ré-encodage) change sa mtime, donc sa clé — l'ancienne entrée reste sur
/// disque, inerte, plutôt que d'exiger une purge active.
pub(crate) fn cle_pochette(path: &Path, mtime: std::time::SystemTime) -> String {
    let epoch = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut octets = path.to_string_lossy().into_owned().into_bytes();
    octets.extend_from_slice(&epoch.to_le_bytes());
    format!("{:016x}", empreinte(&octets))
}

/// Hachage FNV-1a 64 bits.
pub(crate) fn empreinte(octets: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for octet in octets {
        h ^= *octet as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

/// Durée de validité d'une entrée « pas de pochette » : passé ce délai elle
/// est redemandée — CAA et Deezer s'enrichissent, et l'enrichissement
/// MusicBrainz local peut arriver après le premier affichage. Les pochettes
/// trouvées, elles, ne périment pas.
pub(crate) const NEGATIF_VALIDITE: Duration = Duration::from_secs(30 * 24 * 3600);

/// Lit une entrée du cache. `None` extérieur = pas en cache (à calculer),
/// `Some(None)` intérieur = en cache et confirmé sans pochette (depuis moins
/// de [`NEGATIF_VALIDITE`]).
pub(crate) fn lire_cache_pochette(dossier: &Path, cle: &str) -> Option<Option<String>> {
    lire_cache_pochette_avec(dossier, cle, NEGATIF_VALIDITE)
}

pub(crate) fn lire_cache_pochette_avec(
    dossier: &Path,
    cle: &str,
    validite: Duration,
) -> Option<Option<String>> {
    let chemin = dossier.join(cle);
    let octets = std::fs::read(&chemin).ok()?;
    if octets == SANS_POCHETTE {
        let age = std::fs::metadata(&chemin)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok());
        // Âge illisible : mieux vaut redemander qu'enterrer l'album.
        return match age {
            Some(a) if a <= validite => Some(None),
            _ => None,
        };
    }
    Some(String::from_utf8(octets).ok())
}

/// Efface les entrées « pas de pochette » d'avant la distinction entre
/// « 404 » et « panne réseau » : elles peuvent être des pannes gravées.
/// Une seule fois (marqueur à côté du cache).
pub(crate) fn purger_negatifs_pochettes(dossier: &Path) {
    let marqueur = dossier.join(".negatifs-purges-v1");
    if marqueur.exists() {
        return;
    }
    if let Ok(entrees) = std::fs::read_dir(dossier) {
        for e in entrees.flatten() {
            if std::fs::read(e.path()).is_ok_and(|o| o == SANS_POCHETTE) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let _ = std::fs::write(&marqueur, b"1");
}

/// Écrit une entrée. Une erreur d'écriture (disque plein, dossier en lecture
/// seule) ne doit pas faire échouer la requête : la pochette a déjà été
/// extraite, autant la rendre, le prochain appel réessaiera le cache.
///
/// **Sans limite de taille pour l'instant** : contrairement à `target/` (un
/// artefact de compilation qui peut regrossir indéfiniment à chaque
/// recompilation), le nombre de pochettes distinctes est borné par celui des
/// albums de la bibliothèque, et chaque entrée pèse au plus quelques centaines
/// de Ko. Si ça devient un problème mesuré, appliquer la même politique que
/// `POCHETTES_MAX` côté JS (`js/socle.js`) plutôt qu'inventer une seconde règle.
pub(crate) fn ecrire_cache_pochette(dossier: &Path, cle: &str, valeur: Option<&str>) {
    if std::fs::create_dir_all(dossier).is_err() {
        return;
    }
    let _ = std::fs::write(
        dossier.join(cle),
        valeur.map_or(SANS_POCHETTE, str::as_bytes),
    );
}

/// Encodage base64 sans dépendance : la seule chose qu'on ait à encoder ici,
/// ce sont des pochettes, et l'ajouter au projet pour ça ne se justifie pas.
pub(crate) fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for bloc in data.chunks(3) {
        let b = [
            bloc[0],
            *bloc.get(1).unwrap_or(&0),
            *bloc.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if bloc.len() > 1 {
            T[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if bloc.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::Path;

    use std::time::Duration;

    fn dossier_de_test(nom: &str) -> std::path::PathBuf {
        // `std::env::temp_dir()`, jamais un chemin fixe du dépôt — voir la
        // consigne de l'incident des 199 Go.
        let d = std::env::temp_dir().join(format!(
            "rusty-music-test-pochettes-{nom}-{}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&d).ok();
        d
    }

    #[test]
    fn une_cle_change_avec_le_chemin_et_la_mtime() {
        let t0 = std::time::UNIX_EPOCH;
        let t1 = t0 + std::time::Duration::from_secs(1);
        let a = Path::new("/a.mp3");
        let b = Path::new("/b.mp3");
        assert_ne!(
            cle_pochette(a, t0),
            cle_pochette(b, t0),
            "chemins différents"
        );
        assert_ne!(
            cle_pochette(a, t0),
            cle_pochette(a, t1),
            "mtimes différentes"
        );
        assert_eq!(cle_pochette(a, t0), cle_pochette(a, t0), "déterministe");
    }

    #[test]
    fn le_cache_sert_ce_quil_a_recu() {
        let dossier = dossier_de_test("sert");
        let cle = "abc";
        ecrire_cache_pochette(&dossier, cle, Some("data:image/jpeg;base64,zzz"));
        assert_eq!(
            lire_cache_pochette(&dossier, cle),
            Some(Some("data:image/jpeg;base64,zzz".to_string()))
        );
        std::fs::remove_dir_all(&dossier).ok();
    }

    #[test]
    fn le_cache_distingue_absent_de_confirme_sans_pochette() {
        let dossier = dossier_de_test("sans");
        assert_eq!(
            lire_cache_pochette(&dossier, "jamais-vu"),
            None,
            "pas encore en cache"
        );
        ecrire_cache_pochette(&dossier, "sans-art", None);
        assert_eq!(
            lire_cache_pochette(&dossier, "sans-art"),
            Some(None),
            "en cache, confirmé sans pochette"
        );
        std::fs::remove_dir_all(&dossier).ok();
    }

    #[test]
    fn un_negatif_perime_est_redemande_pas_une_pochette_trouvee() {
        let dossier = dossier_de_test("negatif-perime");
        ecrire_cache_pochette(&dossier, "sans", None);
        ecrire_cache_pochette(&dossier, "avec", Some("data:image/jpeg;base64,zzz"));
        for cle in ["sans", "avec"] {
            let f = std::fs::File::options()
                .write(true)
                .open(dossier.join(cle))
                .unwrap();
            f.set_modified(std::time::SystemTime::now() - Duration::from_secs(40 * 24 * 3600))
                .unwrap();
        }
        assert_eq!(
            lire_cache_pochette(&dossier, "sans"),
            None,
            "négatif de 40 j : à redemander"
        );
        assert_eq!(
            lire_cache_pochette(&dossier, "avec"),
            Some(Some("data:image/jpeg;base64,zzz".to_string())),
            "une pochette trouvée ne périme pas"
        );
        std::fs::remove_dir_all(&dossier).ok();
    }

    #[test]
    fn la_purge_efface_les_negatifs_une_seule_fois() {
        let dossier = dossier_de_test("purge");
        ecrire_cache_pochette(&dossier, "sans", None);
        ecrire_cache_pochette(&dossier, "avec", Some("data:image/jpeg;base64,zzz"));
        purger_negatifs_pochettes(&dossier);
        assert!(!dossier.join("sans").exists());
        assert!(dossier.join("avec").exists());
        // Un négatif écrit après la purge est un vrai 404 : on le garde.
        ecrire_cache_pochette(&dossier, "sans2", None);
        purger_negatifs_pochettes(&dossier);
        assert!(dossier.join("sans2").exists());
        std::fs::remove_dir_all(&dossier).ok();
    }

    #[test]
    fn une_mtime_differente_retrouve_une_cle_differente_donc_pas_lentree_perimee() {
        let dossier = dossier_de_test("perime");
        let chemin = Path::new("/musique/piste.mp3");
        let ancienne = cle_pochette(chemin, std::time::UNIX_EPOCH);
        ecrire_cache_pochette(&dossier, &ancienne, Some("vieille-pochette"));

        let nouvelle = cle_pochette(
            chemin,
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(60),
        );
        assert_eq!(
            lire_cache_pochette(&dossier, &nouvelle),
            None,
            "le fichier a changé : la clé change, l'ancienne entrée ne sert plus"
        );
        std::fs::remove_dir_all(&dossier).ok();
    }
}
