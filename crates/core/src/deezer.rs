// SPDX-License-Identifier: GPL-3.0-or-later
//! Deezer : le client, et rien d'autre.
//!
//! Comme [`crate::musicbrainz`] et [`crate::listenbrainz`], ce module parle au
//! réseau et rend des données ; il n'écrit pas en base. La passe qui l'emploie
//! est dans [`crate::popularite`].
//!
//! **API publique, sans compte ni clé.** Deezer sert le second signal de
//! popularité du chantier `docs/popularite.md` : le `rank` d'une piste, entre
//! ~10 000 et ~1 000 000.
//!
//! **Deezer n'indexe pas par MBID.** On retrouve une piste par recherche
//! `artist:"…" track:"…"`, et on ne retient un résultat que si **l'artiste et
//! le titre** concordent tous les deux. Sans cette double vérification, la
//! sonde de phase 0 a mesuré ~1 rapprochement sur 40 tombant sur un autre
//! morceau du même artiste.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{Error, Result};
use crate::musicbrainz::{cle_artiste, normaliser_titre};

/// Délai minimal entre deux requêtes. Deezer limite à ~50 requêtes par tranche
/// de 5 secondes ; 150 ms tient largement sous la barre.
const CADENCE: Duration = Duration::from_millis(150);

/// Combien de fois réessayer avant d'abandonner une recherche.
const ESSAIS: u32 = 4;

/// Client Deezer, cadencé. Un seul pour tout le processus.
pub struct Client {
    agent: ureq::Agent,
    dernier: Mutex<Option<Instant>>,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    pub fn new() -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .user_agent(format!("rusty-music/{}", env!("CARGO_PKG_VERSION")))
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        Self {
            agent,
            dernier: Mutex::new(None),
        }
    }

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

    /// Une recherche GET rendant du JSON, réessayée sur échec temporaire.
    fn chercher(&self, kind: &str, q: &str) -> Result<Option<Value>> {
        let url = format!("https://api.deezer.com/search/{kind}");
        let mut derniere = String::new();
        for essai in 0..ESSAIS {
            self.cadencer();
            match self
                .agent
                .get(&url)
                .query("q", q)
                .query("limit", "5")
                .call()
            {
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
                    std::thread::sleep(Duration::from_secs(1 << essai));
                }
            }
        }
        Err(Error::Reseau(format!(
            "{ESSAIS} tentatives sans succès sur {url} — {derniere}"
        )))
    }

    /// Le `rank` Deezer de la piste `titre` de `artiste`, si la recherche rend
    /// un résultat dont **l'artiste et le titre** concordent avec la demande.
    /// `None` si rien ne concorde — l'appelant marque quand même l'entité
    /// comme « demandée », pour ne pas y revenir.
    pub fn rang_piste(&self, artiste: &str, titre: &str) -> Result<Option<i64>> {
        let q = format!(
            "artist:\"{}\" track:\"{}\"",
            echapper(artiste),
            echapper(titre)
        );
        let Some(v) = self.chercher("track", &q)? else {
            return Ok(None);
        };
        let art_attendu = cle_artiste(artiste);
        let tit_attendu = normaliser_titre(titre);
        for d in v["data"].as_array().into_iter().flatten() {
            let art = cle_artiste(d["artist"]["name"].as_str().unwrap_or(""));
            let tit = normaliser_titre(d["title"].as_str().unwrap_or(""));
            if concorde(&art, &art_attendu) && concorde(&tit, &tit_attendu) {
                return Ok(Some(d["rank"].as_i64().unwrap_or(0)));
            }
        }
        Ok(None)
    }

    /// La pochette (500 px, JPEG) de l'album `album` de `artiste`, si la
    /// recherche rend un album qui concorde (voir [`url_pochette_album`]).
    ///
    /// `Ok(None)` : Deezer ne connaît pas cet album (réponse définitive).
    /// `Err` : panne réseau — l'appelant ne doit pas la mettre en cache.
    pub fn pochette_album(&self, artiste: &str, album: &str) -> Result<Option<Vec<u8>>> {
        for q in requetes_album(artiste, album) {
            let Some(v) = self.chercher("album", &q)? else {
                continue;
            };
            if let Some(url) = url_pochette_album(&v, artiste, album) {
                self.cadencer();
                return crate::pochette::telecharger(&url);
            }
        }
        Ok(None)
    }
}

/// Artiste d'album « fourre-tout » des compilations : il ne dit rien de
/// l'album, et Deezer range souvent la compilation sous un éditeur.
fn est_artiste_divers(artiste: &str) -> bool {
    matches!(
        cle_artiste(artiste).as_str(),
        "variousartists" | "variousartist" | "artistesdivers" | "artistesvaries" | "variosartistas"
    )
}

/// Le titre débarrassé de sa mention d'édition : « Keep It Unreal - 10th
/// Anniversary Edition (Bonus Disc) » → « Keep It Unreal ». Rend le titre tel
/// quel s'il n'y a rien à retirer.
fn titre_court(album: &str) -> &str {
    let coupe = [" - ", " – ", " (", " ["]
        .iter()
        .filter_map(|m| album.find(m))
        .filter(|&i| i > 0)
        .min();
    coupe.map_or(album, |i| album[..i].trim())
}

/// Les recherches à tenter, de la plus précise à la plus large : le titre
/// tel quel, sans ses points (« Vol.1 » ne trouve rien chez Deezer, « Vol 1 »
/// trouve « Vol. 1 »), puis sans sa mention d'édition. Pour une compilation
/// (« Various Artists »), on cherche par titre seul.
fn requetes_album(artiste: &str, album: &str) -> Vec<String> {
    let mut titres = vec![album.to_string()];
    for t in [album.replace('.', " "), titre_court(album).to_string()] {
        let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
        if !titres.contains(&t) {
            titres.push(t);
        }
    }
    titres
        .iter()
        .map(|t| {
            if est_artiste_divers(artiste) {
                format!("album:\"{}\"", echapper(t))
            } else {
                format!("artist:\"{}\" album:\"{}\"", echapper(artiste), echapper(t))
            }
        })
        .collect()
}

/// Deux titres d'album normalisés concordent s'ils sont égaux, ou si l'un
/// commence par l'autre (« keepitunreal » ↔ « keepitunreal10thanniversary… »)
/// et que le plus court garde de quoi distinguer — sans quoi « G » recevrait
/// la pochette de n'importe quel album du même artiste qui commence par g.
fn titres_concordent(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || (a.len().min(b.len()) >= 5 && (a.starts_with(b) || b.starts_with(a)))
}

/// L'URL de la pochette du premier album de la réponse Deezer `v` qui
/// concorde avec la demande : titre concordant **et** artiste concordant —
/// sauf compilation, où l'artiste ne prouve rien et où le titre doit alors
/// être **égal**.
fn url_pochette_album(v: &Value, artiste: &str, album: &str) -> Option<String> {
    let divers = est_artiste_divers(artiste);
    let art_attendu = cle_artiste(artiste);
    let tit_attendu = normaliser_titre(album);
    v["data"].as_array()?.iter().find_map(|d| {
        let art = cle_artiste(d["artist"]["name"].as_str().unwrap_or(""));
        let tit = normaliser_titre(d["title"].as_str().unwrap_or(""));
        let concorde_album = if divers {
            !tit.is_empty() && tit == tit_attendu
        } else {
            concorde(&art, &art_attendu) && titres_concordent(&tit, &tit_attendu)
        };
        if !concorde_album {
            return None;
        }
        d["cover_big"]
            .as_str()
            .filter(|u| !u.is_empty())
            .map(str::to_owned)
    })
}

/// Deux chaînes normalisées concordent si elles sont égales ou si l'une
/// contient l'autre — « télépopmusik » ↔ « télépopmusik feat maud », « never
/// forget » ↔ « never forget instrumental version ».
fn concorde(a: &str, b: &str) -> bool {
    !a.is_empty() && !b.is_empty() && (a == b || a.contains(b) || b.contains(a))
}

/// Retire les guillemets d'un terme de recherche : ils fermeraient la chaîne
/// `artist:"…"` de la requête Deezer au mauvais endroit.
fn echapper(s: &str) -> String {
    s.replace('"', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concorde_egalite_et_inclusion() {
        assert!(concorde("nirvana", "nirvana"));
        assert!(concorde("telepopmusikfeatmaud", "telepopmusik"));
        assert!(concorde("neverforget", "neverforgetinstrumentalversion"));
        assert!(!concorde("nirvana", "nirwana"));
        assert!(!concorde("", "nirvana"));
    }

    fn reponse() -> Value {
        serde_json::json!({ "data": [
            { "title": "Nevermind (Deluxe)", "artist": { "name": "Nirvana" },
              "cover_big": "https://cdn/nevermind.jpg" },
            { "title": "Nevermind", "artist": { "name": "Nirvana Tribute Band" },
              "cover_big": "https://cdn/tribute.jpg" },
            { "title": "In Utero", "artist": { "name": "Nirvana" },
              "cover_big": "" },
        ]})
    }

    #[test]
    fn pochette_album_retient_l_album_de_l_artiste() {
        assert_eq!(
            url_pochette_album(&reponse(), "Nirvana", "Nevermind").as_deref(),
            Some("https://cdn/nevermind.jpg")
        );
    }

    #[test]
    fn pochette_album_rejette_un_autre_artiste_ou_un_autre_album() {
        assert_eq!(url_pochette_album(&reponse(), "Pearl Jam", "Nevermind"), None);
        assert_eq!(url_pochette_album(&reponse(), "Nirvana", "Bleach"), None);
    }

    #[test]
    fn pochette_album_accepte_un_titre_a_rallonge_qui_commence_comme_celui_de_deezer() {
        let v = serde_json::json!({ "data": [
            { "title": "Keep It Unreal (10th Anniversary Analogue Remaster Edition)",
              "artist": { "name": "Mr. Scruff" }, "cover_big": "https://cdn/kiu.jpg" },
        ]});
        assert_eq!(
            url_pochette_album(
                &v,
                "Mr. Scruff",
                "Keep It Unreal - 10th Anniversary Analogue Remaster Edition (Bonus Disc)"
            )
            .as_deref(),
            Some("https://cdn/kiu.jpg")
        );
    }

    #[test]
    fn pochette_album_un_titre_court_ne_prend_pas_n_importe_quel_album_de_l_artiste() {
        let v = serde_json::json!({ "data": [
            { "title": "Greatest Hits", "artist": { "name": "Fingathing" },
              "cover_big": "https://cdn/gh.jpg" },
        ]});
        assert_eq!(url_pochette_album(&v, "Fingathing", "G"), None);
    }

    #[test]
    fn compilation_cherche_par_titre_seul_et_exige_l_egalite() {
        assert_eq!(
            requetes_album("Various Artists", "Nova Rare Grooves Reggae Vol.1"),
            [
                "album:\"Nova Rare Grooves Reggae Vol.1\"",
                "album:\"Nova Rare Grooves Reggae Vol 1\"",
            ]
        );
        let v = serde_json::json!({ "data": [
            { "title": "Nova Rare Grooves Reggae, Vol. 1", "artist": { "name": "Nova Tunes" },
              "cover_big": "https://cdn/nova.jpg" },
            { "title": "Nova Rare Grooves Reggae, Vol. 2", "artist": { "name": "Nova Tunes" },
              "cover_big": "https://cdn/nova2.jpg" },
        ]});
        assert_eq!(
            url_pochette_album(&v, "Various Artists", "Nova Rare Grooves Reggae Vol.1").as_deref(),
            Some("https://cdn/nova.jpg")
        );
        // Un préfixe ne suffit pas quand l'artiste ne garantit rien.
        assert_eq!(url_pochette_album(&v, "Various Artists", "Nova Rare Grooves"), None);
    }

    #[test]
    fn requetes_album_ajoute_le_titre_court_apres_le_titre_complet() {
        assert_eq!(
            requetes_album("Mr. Scruff", "Keep It Unreal - 10th Anniversary (Bonus Disc)"),
            [
                "artist:\"Mr. Scruff\" album:\"Keep It Unreal - 10th Anniversary (Bonus Disc)\"",
                "artist:\"Mr. Scruff\" album:\"Keep It Unreal\"",
            ]
        );
        assert_eq!(requetes_album("Nirvana", "Nevermind").len(), 1);
    }

    #[test]
    fn pochette_album_ignore_une_url_vide() {
        assert_eq!(url_pochette_album(&reponse(), "Nirvana", "In Utero"), None);
    }
}
