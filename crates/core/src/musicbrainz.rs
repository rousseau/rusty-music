// SPDX-License-Identifier: GPL-3.0-or-later
//! Genres MusicBrainz : le client, et rien d'autre.
//!
//! Ce module parle au réseau et rend des données ; il n'écrit pas en base et
//! ne décide de rien. La passe qui l'emploie est dans [`crate::enrichir`].
//!
//! **Pourquoi une source de genres de plus.** Les fichiers en portent déjà un,
//! sur 90 % des morceaux — mais saisi à la main, à l'échelle de l'album, et
//! grossier : « Rock » couvre 40 % de la bibliothèque de test, et
//! « Children's » y nommait la famille de Regina Spektor et Nina Simone.
//! Mesuré sur la même bibliothèque, MusicBrainz améliore neuf familles sur
//! douze (`experiments/musicbrainz-genres/`).
//!
//! **Deux conditions d'accès, pas deux recommandations.** MusicBrainz exige un
//! agent qui identifie l'application et donne un contact, et limite à une
//! requête par seconde. Sans agent on est refusé ; en allant plus vite on
//! récolte des 503. Les deux sont tenues ici, dans le client, pour qu'aucun
//! appelant n'ait à y penser.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{Error, Result};

/// Un genre et le nombre de votes qui l'appuient.
///
/// MusicBrainz est contributif : le vote sépare le consensus de l'accident.
/// Un unique `amapiano` posé sur Yann Tiersen suffisait à nommer une famille
/// entière de piano néoclassique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Genre {
    pub nom: String,
    pub votes: i64,
}

/// Un album au sens de MusicBrainz — un *release-group*, c'est-à-dire l'œuvre
/// indépendamment de ses rééditions, remasters et pressages.
#[derive(Debug, Clone)]
pub struct Album {
    pub mbid: String,
    pub titre: String,
    pub genres: Vec<Genre>,
    /// `first-release-date` : la première parution du release-group, tous
    /// pressages confondus. Souvent partielle (« 2026 », « 2026-08 »), parfois
    /// absente. Sert au mode Découvrir à ne garder que les sorties récentes.
    pub date_sortie: Option<String>,
    /// `primary-type` : Album, EP, Single, Broadcast, Other.
    pub type_primaire: Option<String>,
    /// `secondary-types` : Compilation, Live, Remix, Soundtrack…
    pub types_secondaires: Vec<String>,
}

/// Une relation entre deux artistes, telle que MusicBrainz la nomme —
/// « member of band », « instrumental supporting musician »…, parfois
/// précisée d'un attribut (« guitar », « vocal »).
///
/// Le sens de lecture n'est pas toujours le même : MusicBrainz porte un
/// `direction` (« backward »/« forward ») que ce module ne redresse pas —
/// le type est gardé tel quel, dans le sens où l'API le rend pour
/// l'artiste demandé. Assez pour afficher « X — member of band — Y », pas
/// encore assez pour un graphe orienté qu'on parcourrait dans les deux
/// sens sans ambiguïté.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relation {
    pub dst_mbid: String,
    pub dst_name: String,
    pub relation: String,
}

/// Délai minimal entre deux requêtes. Une seconde est la limite annoncée ;
/// la marge évite de la frôler quand l'horloge et le réseau se décalent.
const CADENCE: Duration = Duration::from_millis(1_100);

/// Nombre de release-groups par page. 100 est le maximum accepté ; la plupart
/// des artistes tiennent en une seule requête.
const PAR_PAGE: usize = 100;

/// Plafond de pages parcourues pour un artiste. Un vrai groupe tient largement
/// en dessous — même Frank Zappa n'a pas 600 albums. Au-delà, c'est un artiste
/// spécial de MusicBrainz (« Various Artists », crédité sur des dizaines de
/// milliers de compilations) : sans ce plafond, une passe s'y enlise pour des
/// heures, une requête par seconde.
const MAX_PAGES: usize = 6;

/// Combien de fois réessayer avant d'abandonner un identifiant.
const ESSAIS: u32 = 4;

/// Client MusicBrainz, cadencé.
///
/// Un seul suffit pour tout le processus : c'est lui qui porte l'horloge du
/// débit. En créer deux reviendrait à doubler la cadence sans le vouloir.
pub struct Client {
    agent: ureq::Agent,
    /// Instant du dernier départ de requête, partagé entre fils.
    dernier: Mutex<Option<Instant>>,
}

impl Client {
    /// `contact` est l'adresse que MusicBrainz pourra joindre en cas d'abus.
    /// Elle part dans l'en-tête `User-Agent`, comme leur documentation l'exige.
    pub fn new(contact: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .user_agent(format!(
                "rusty-music/{} ( {contact} )",
                env!("CARGO_PKG_VERSION")
            ))
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        Self {
            agent,
            dernier: Mutex::new(None),
        }
    }

    /// Patiente le temps qu'il faut pour ne pas dépasser une requête/seconde.
    fn cadencer(&self) {
        let mut dernier = self.dernier.lock().expect("horloge du débit");
        if let Some(precedent) = *dernier {
            let ecoule = precedent.elapsed();
            if ecoule < CADENCE {
                std::thread::sleep(CADENCE - ecoule);
            }
        }
        *dernier = Some(Instant::now());
    }

    /// Une requête, réessayée sur échec temporaire.
    ///
    /// `404` est une réponse, pas une panne : l'identifiant est inconnu, on
    /// rend `None` et on n'y revient pas. `503` est le signal de débit de
    /// MusicBrainz — on patiente en doublant l'attente. Tout le reste est
    /// traité comme temporaire : le réseau d'un poste local coupe et revient.
    fn json(&self, url: &str) -> Result<Option<Value>> {
        let mut derniere = String::new();
        for essai in 0..ESSAIS {
            self.cadencer();
            match self.agent.get(url).call() {
                Ok(mut r) => {
                    let corps = r
                        .body_mut()
                        .read_to_string()
                        .map_err(|e| Error::Reseau(format!("lecture du corps : {e}")))?;
                    return serde_json::from_str(&corps)
                        .map(Some)
                        .map_err(|e| Error::Reseau(format!("JSON illisible : {e}")));
                }
                Err(ureq::Error::StatusCode(404)) => return Ok(None),
                Err(e) => {
                    derniere = e.to_string();
                    // 1 s, 2 s, 4 s : le temps que la fenêtre de débit passe.
                    std::thread::sleep(Duration::from_secs(1 << essai));
                }
            }
        }
        Err(Error::Reseau(format!(
            "{ESSAIS} tentatives sans succès sur {url} — {derniere}"
        )))
    }

    /// Les genres d'un artiste, du plus voté au moins voté.
    ///
    /// Une liste vide est une réponse valable : beaucoup d'artistes n'ont
    /// aucun genre chez MusicBrainz — les chanteurs bretons de la
    /// bibliothèque de test, par exemple. C'est à l'appelant de retomber sur
    /// le tag du fichier.
    pub fn genres_artiste(&self, mbid: &str) -> Result<Vec<Genre>> {
        let url = format!("https://musicbrainz.org/ws/2/artist/{mbid}?inc=genres&fmt=json");
        Ok(self.json(&url)?.map(|v| genres_de(&v)).unwrap_or_default())
    }

    /// Tous les albums d'un artiste, **avec leurs genres, en une requête**.
    ///
    /// C'est le point qui rend l'échelon album abordable. Interroger chaque
    /// album séparément demanderait deux requêtes par disque — l'enregistrement
    /// pour retrouver son release-group, puis le release-group pour ses
    /// genres — soit plus de quatre mille appels sur la bibliothèque de test.
    /// Le parcours par artiste en demande un peu moins de deux mille, et rend
    /// au passage les titres, dont on a besoin puisque nos fichiers ne portent
    /// pas d'identifiant d'album.
    pub fn albums_artiste(&self, mbid: &str) -> Result<Vec<Album>> {
        let mut albums = Vec::new();
        let mut depuis = 0usize;
        loop {
            let url = format!(
                "https://musicbrainz.org/ws/2/release-group?artist={mbid}\
                 &inc=genres&limit={PAR_PAGE}&offset={depuis}&fmt=json"
            );
            let Some(v) = self.json(&url)? else {
                return Ok(albums);
            };
            let page = v["release-groups"].as_array().cloned().unwrap_or_default();
            let recus = page.len();
            for rg in page {
                let (Some(mbid), Some(titre)) = (rg["id"].as_str(), rg["title"].as_str()) else {
                    continue;
                };
                albums.push(Album {
                    mbid: mbid.to_string(),
                    titre: titre.to_string(),
                    genres: genres_de(&rg),
                    date_sortie: rg["first-release-date"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
                    type_primaire: rg["primary-type"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
                    types_secondaires: rg["secondary-types"]
                        .as_array()
                        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                        .unwrap_or_default(),
                });
            }
            let total = v["release-group-count"].as_u64().unwrap_or(0) as usize;
            depuis += recus;
            // `recus == 0` protège du cas où le compte annoncé dépasse ce que
            // l'API rend réellement : sans lui, la boucle tournerait sans fin.
            if depuis >= total || recus == 0 {
                return Ok(albums);
            }
            if depuis >= MAX_PAGES * PAR_PAGE {
                tracing::warn!(
                    %mbid, total,
                    "discographie tronquée à {} albums — artiste spécial ?",
                    MAX_PAGES * PAR_PAGE
                );
                return Ok(albums);
            }
        }
    }

    /// Les artistes reliés à celui-ci — membre de, collaborateur,
    /// fondateur… Mode Découvrir.
    ///
    /// Une liste vide est une réponse valable, comme pour les genres : la
    /// plupart des artistes n'ont aucune relation cataloguée.
    pub fn relations_artiste(&self, mbid: &str) -> Result<Vec<Relation>> {
        let url = format!("https://musicbrainz.org/ws/2/artist/{mbid}?inc=artist-rels&fmt=json");
        Ok(self
            .json(&url)?
            .map(|v| relations_de(&v))
            .unwrap_or_default())
    }
}

/// Extrait les relations vers d'autres artistes d'une réponse `artist-rels`.
///
/// `target-type` filtre les entités non-artiste : `inc=artist-rels` ne
/// devrait rendre que ça, mais un champ manquant ou inattendu ne doit pas
/// produire une ligne à moitié vide plutôt que d'être sauté.
fn relations_de(v: &Value) -> Vec<Relation> {
    v["relations"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|r| r["target-type"].as_str() == Some("artist"))
                .filter_map(|r| {
                    let dst_mbid = r["artist"]["id"].as_str()?.to_string();
                    let dst_name = r["artist"]["name"].as_str()?.to_string();
                    let base = r["type"].as_str()?;
                    // Un attribut (« guitar », « vocal »…) précise le type
                    // générique : « instrumental supporting musician » seul
                    // ne dit pas sur quoi.
                    let attributs: Vec<&str> = r["attributes"]
                        .as_array()
                        .map(|a| a.iter().filter_map(Value::as_str).collect())
                        .unwrap_or_default();
                    let relation = if attributs.is_empty() {
                        base.to_string()
                    } else {
                        format!("{base} ({})", attributs.join(", "))
                    };
                    Some(Relation {
                        dst_mbid,
                        dst_name,
                        relation,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Extrait et classe les genres d'une entité MusicBrainz quelconque.
fn genres_de(v: &Value) -> Vec<Genre> {
    let mut g: Vec<Genre> = v["genres"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some(Genre {
                        nom: x["name"].as_str()?.to_string(),
                        votes: x["count"].as_i64().unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    g.sort_by(|a, b| b.votes.cmp(&a.votes).then_with(|| a.nom.cmp(&b.nom)));
    g
}

/// Réduit un titre d'album à ce qui permet de le reconnaître.
///
/// Nos fichiers ne portent pas d'identifiant d'album : le rapprochement se
/// fait par le titre, et les titres divergent. « In Utero (super deluxe) » chez
/// nous, « In Utero » chez MusicBrainz ; « Songs of Freedom » contre « Songs Of
/// Freedom ». On retire donc les parenthèses et crochets de fin — qui portent
/// presque toujours une mention d'édition — les accents, la ponctuation et la
/// casse.
pub fn normaliser_titre(titre: &str) -> String {
    let mut s = titre.trim();
    // Répété : « Album (deluxe) [remaster] » porte deux mentions.
    loop {
        let coupe = match s.chars().last() {
            Some(')') => s.rfind('('),
            Some(']') => s.rfind('['),
            _ => None,
        };
        match coupe {
            Some(i) if i > 0 => s = s[..i].trim(),
            _ => break,
        }
    }
    s.chars()
        .filter_map(|c| {
            let c = sans_accent(c);
            c.is_alphanumeric()
                .then(|| c.to_lowercase().next().unwrap_or(c))
        })
        .collect()
}

/// Réduit un nom d'artiste à ce qui permet de le reconnaître d'une source à
/// l'autre : minuscules, sans accent ni ponctuation, et sans le crédit
/// secondaire d'un « X feat. Y ». Sert au rapprochement Deezer, qui se fait
/// par recherche « artiste + titre » faute de MBID.
pub fn cle_artiste(nom: &str) -> String {
    let bas = nom.to_lowercase();
    let mut s = bas.as_str();
    for coupe in [" feat.", " feat ", " ft.", " ft ", " featuring ", " & ", " and ", " x ", " vs ", " vs."] {
        if let Some(i) = s.find(coupe) {
            s = &s[..i];
        }
    }
    s.chars()
        .filter_map(|c| {
            let c = sans_accent(c);
            c.is_alphanumeric().then_some(c)
        })
        .collect()
}

/// Complète une date MusicBrainz partielle en `YYYY-MM-DD`.
///
/// `first-release-date` vaut « 2026 », « 2026-08 » ou « 2026-08-15 » — parfois
/// rien. Complétée au premier jour du mois ou de l'année, elle se compare et se
/// trie comme une chaîne, sans dépendre d'un calendrier : c'est tout ce dont le
/// filtre de fenêtre du mode Découvrir a besoin. Rend `None` si l'entrée n'a
/// pas au moins une année de quatre chiffres.
pub fn completer_date(brute: &str) -> Option<String> {
    let mut parts = brute.trim().splitn(3, '-');
    let annee = parts.next()?;
    if annee.len() != 4 || !annee.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mois = parts.next().filter(|m| m.len() == 2).unwrap_or("01");
    let jour = parts.next().filter(|j| j.len() == 2).unwrap_or("01");
    Some(format!("{annee}-{mois}-{jour}"))
}

/// Rabat les lettres accentuées les plus courantes sur leur base.
///
/// Écrit à la main plutôt qu'avec une bibliothèque de normalisation Unicode :
/// c'est vingt lignes contre une dépendance, et le besoin se limite aux
/// alphabets latins que portent des titres d'albums.
fn sans_accent(c: char) -> char {
    match c {
        'à'..='å' | 'À'..='Å' => 'a',
        'è'..='ë' | 'È'..='Ë' => 'e',
        'ì'..='ï' | 'Ì'..='Ï' => 'i',
        'ò'..='ö' | 'Ò'..='Ö' => 'o',
        'ù'..='ü' | 'Ù'..='Ü' => 'u',
        'ç' | 'Ç' => 'c',
        'ñ' | 'Ñ' => 'n',
        'ý' | 'ÿ' | 'Ý' => 'y',
        autre => autre,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les titres à rapprocher viennent de deux sources qui ne s'accordent ni
    /// sur la casse, ni sur les accents, ni sur les mentions d'édition.
    #[test]
    fn les_titres_se_rapprochent_malgre_leurs_variantes() {
        let meme = |a: &str, b: &str| {
            assert_eq!(
                normaliser_titre(a),
                normaliser_titre(b),
                "« {a} » et « {b} » devraient se rapprocher"
            );
        };
        meme("In Utero (super deluxe)", "In Utero");
        meme("Songs of Freedom", "Songs Of Freedom");
        meme("Ride the Lightning [remastered]", "Ride the Lightning");
        meme("Album (deluxe) [remaster]", "Album");
        meme("L'Odyssée du réel", "L'Odyssee du reel");
        meme("Été 67", "ete67");

        // Et deux disques différents doivent le rester.
        assert_ne!(normaliser_titre("Kid A"), normaliser_titre("Amnesiac"));
        // Une parenthèse qui porte tout le titre n'est pas une mention
        // d'édition : la retirer ne laisserait rien.
        assert_eq!(normaliser_titre("(What's the Story) Morning Glory?"), {
            let s = normaliser_titre("(What's the Story) Morning Glory?");
            assert!(!s.is_empty(), "titre vidé par la normalisation");
            s
        });
    }

    #[test]
    fn les_genres_sortent_du_plus_vote_au_moins_vote() {
        let v: Value = serde_json::from_str(
            r#"{"genres":[{"name":"rock","count":3},{"name":"ska","count":9},
                          {"name":"reggae","count":9},{"name":"dub","count":1}]}"#,
        )
        .expect("JSON de test");
        let g = genres_de(&v);
        assert_eq!(
            g.iter().map(|x| x.nom.as_str()).collect::<Vec<_>>(),
            // À votes égaux, l'ordre alphabétique : sans ce départage, deux
            // passes sur la même donnée pourraient nommer différemment.
            ["reggae", "ska", "rock", "dub"]
        );
        assert_eq!(g[0].votes, 9);
    }

    #[test]
    fn une_entite_sans_genre_rend_une_liste_vide() {
        let v: Value = serde_json::from_str(r#"{"id":"x","name":"y"}"#).expect("JSON de test");
        assert!(genres_de(&v).is_empty());
    }

    /// Extrait d'une vraie réponse `artist-rels` (Nirvana), gardé en dur —
    /// pas d'accès réseau dans les tests.
    #[test]
    fn relations_de_ne_garde_que_les_artistes_et_ajoute_lattribut() {
        let v: Value = serde_json::from_str(
            r#"{"relations":[
                {"type":"instrumental supporting musician","direction":"backward",
                 "target-type":"artist","attributes":["guitar"],
                 "artist":{"id":"258e917c-4cf0-4a1a-a07d-dacfe6b93398","name":"John Duncan"}},
                {"type":"member of band","direction":"backward","target-type":"artist",
                 "attributes":[],
                 "artist":{"id":"aaaaaaaa-0000-0000-0000-000000000000","name":"Kurt Cobain"}},
                {"type":"publié par","target-type":"label",
                 "label":{"id":"bbbb","name":"Geffen"}}
            ]}"#,
        )
        .expect("JSON de test");
        let r = relations_de(&v);
        assert_eq!(r.len(), 2, "la relation vers un label doit être écartée : {r:?}");
        assert_eq!(
            r[0],
            Relation {
                dst_mbid: "258e917c-4cf0-4a1a-a07d-dacfe6b93398".to_string(),
                dst_name: "John Duncan".to_string(),
                relation: "instrumental supporting musician (guitar)".to_string(),
            }
        );
        assert_eq!(r[1].relation, "member of band", "pas d'attribut, pas de parenthèses");
    }

    #[test]
    fn relations_de_dune_reponse_sans_relations_rend_une_liste_vide() {
        let v: Value = serde_json::from_str(r#"{"id":"x","name":"y"}"#).expect("JSON de test");
        assert!(relations_de(&v).is_empty());
    }

    #[test]
    fn completer_date_remplit_les_dates_partielles() {
        assert_eq!(completer_date("2026").as_deref(), Some("2026-01-01"));
        assert_eq!(completer_date("2026-08").as_deref(), Some("2026-08-01"));
        assert_eq!(completer_date("2026-08-15").as_deref(), Some("2026-08-15"));
        assert_eq!(completer_date(" 2026-08-15 ").as_deref(), Some("2026-08-15"));
        // Une comparaison de chaînes suffit alors à ordonner deux sorties.
        assert!(completer_date("2026-08-01") > completer_date("2026-07-31"));
    }

    #[test]
    fn completer_date_rejette_ce_qui_na_pas_dannee() {
        assert_eq!(completer_date(""), None);
        assert_eq!(completer_date("????"), None);
        assert_eq!(completer_date("26-08"), None);
    }
}
