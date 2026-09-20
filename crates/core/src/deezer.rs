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

    /// L'URL de la page Deezer de l'album `album` de `artiste`, si la
    /// recherche rend un album qui concorde (voir [`url_lien_album`]).
    ///
    /// `Ok(None)` : Deezer ne connaît pas cet album. `Err` : panne réseau.
    pub fn lien_album(&self, artiste: &str, album: &str) -> Result<Option<String>> {
        for q in requetes_album(artiste, album) {
            let Some(v) = self.chercher("album", &q)? else {
                continue;
            };
            if let Some(url) = url_lien_album(&v, artiste, album) {
                return Ok(Some(url));
            }
        }
        Ok(None)
    }

    /// L'URL de la page Deezer de l'artiste `nom` (voir [`url_lien_artiste`]).
    ///
    /// `Ok(None)` : Deezer ne connaît pas cet artiste. `Err` : panne réseau.
    pub fn lien_artiste(&self, nom: &str) -> Result<Option<String>> {
        let Some(v) = self.chercher("artist", &echapper(nom))? else {
            return Ok(None);
        };
        Ok(url_lien_artiste(&v, nom))
    }

    /// La photo (500 px, JPEG) de l'artiste `nom`, si la recherche rend un
    /// artiste de **même nom** avec une image (voir [`url_photo_artiste`]).
    ///
    /// `Ok(None)` : Deezer ne connaît pas cet artiste, ou seulement des
    /// homonymes sans image (réponse définitive). `Err` : panne réseau —
    /// l'appelant ne doit pas la mettre en cache.
    pub fn photo_artiste(&self, nom: &str) -> Result<Option<Vec<u8>>> {
        let Some(v) = self.chercher("artist", &echapper(nom))? else {
            return Ok(None);
        };
        match url_photo_artiste(&v, nom) {
            Some(url) => {
                self.cadencer();
                crate::pochette::telecharger(&url)
            }
            None => Ok(None),
        }
    }
}

/// L'URL de la photo (`picture_big`, 500 px) de l'artiste `nom` dans une
/// réponse `search/artist`.
///
/// Un nom d'artiste n'est pas un identifiant : Deezer rend des homonymes, et
/// les comptes vides ont une URL d'image au segment vide (`…/artist//250x250…`).
/// On ne garde donc que les candidats **de même nom normalisé** (égalité, pas
/// inclusion — « Air » n'est pas « Air Supply ») dont l'image n'est pas vide,
/// et parmi eux le plus suivi (`nb_fan`).
fn url_photo_artiste(v: &Value, nom: &str) -> Option<String> {
    let attendu = cle_artiste(nom);
    if attendu.is_empty() {
        return None;
    }
    v["data"]
        .as_array()?
        .iter()
        .filter(|d| cle_artiste(d["name"].as_str().unwrap_or("")) == attendu)
        .filter_map(|d| {
            let url = d["picture_big"].as_str().filter(|u| !u.is_empty() && !u.contains("/artist//"))?;
            Some((d["nb_fan"].as_i64().unwrap_or(0), url))
        })
        .max_by_key(|(fans, _)| *fans)
        .map(|(_, url)| url.to_owned())
}

/// L'URL de la page Deezer de l'artiste `nom` dans une réponse `search/artist` :
/// même garde-fou d'homonymie que [`url_photo_artiste`] (nom normalisé
/// **identique**, le plus suivi), mais sans exiger d'image — on veut la page,
/// pas la photo.
fn url_lien_artiste(v: &Value, nom: &str) -> Option<String> {
    let attendu = cle_artiste(nom);
    if attendu.is_empty() {
        return None;
    }
    v["data"]
        .as_array()?
        .iter()
        .filter(|d| cle_artiste(d["name"].as_str().unwrap_or("")) == attendu)
        .filter_map(|d| {
            let url = d["link"].as_str().filter(|u| est_lien_deezer(u))?;
            Some((d["nb_fan"].as_i64().unwrap_or(0), url))
        })
        .max_by_key(|(fans, _)| *fans)
        .map(|(_, url)| url.to_owned())
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
/// concorde avec la demande (voir [`champ_album`]).
fn url_pochette_album(v: &Value, artiste: &str, album: &str) -> Option<String> {
    champ_album(v, artiste, album, "cover_big")
}

/// L'URL de la page Deezer du premier album de la réponse `v` qui concorde
/// avec la demande — mêmes règles que la pochette. Seul un lien Deezer est
/// rendu : l'interface l'ouvre dans le navigateur, on n'y envoie pas une URL
/// arbitraire venue d'une réponse d'API.
fn url_lien_album(v: &Value, artiste: &str, album: &str) -> Option<String> {
    champ_album(v, artiste, album, "link").filter(|u| est_lien_deezer(u))
}

/// Un lien de page Deezer (et pas un hôte étranger glissé dans une réponse).
fn est_lien_deezer(url: &str) -> bool {
    url.starts_with("https://www.deezer.com/")
}

/// Le champ `champ` du premier album de la réponse Deezer `v` qui concorde
/// avec la demande : titre concordant **et** artiste concordant — sauf
/// compilation, où l'artiste ne prouve rien et où le titre doit alors être
/// **égal**. Un champ vide ne concorde pas : on passe à l'album suivant.
fn champ_album(v: &Value, artiste: &str, album: &str, champ: &str) -> Option<String> {
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
        d[champ]
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

    /// Forme réelle d'une réponse `search/artist` (Khruangbin) : le vrai
    /// artiste, puis des homonymes sans fans dont l'image est vide.
    fn reponse_artistes() -> Value {
        serde_json::json!({ "data": [
            { "name": "Khruangbin", "nb_fan": 0,
              "picture_big": "https://cdn/images/artist//500x500-000000-80-0-0.jpg" },
            { "name": "Khruangbin", "nb_fan": 97949,
              "picture_big": "https://cdn/images/artist/1f18/500x500-000000-80-0-0.jpg" },
            { "name": "Childish Gambino & Khruangbin", "nb_fan": 2,
              "picture_big": "https://cdn/images/artist//500x500-000000-80-0-0.jpg" },
        ]})
    }

    #[test]
    fn photo_artiste_prend_le_plus_suivi_parmi_les_homonymes_avec_image() {
        assert_eq!(
            url_photo_artiste(&reponse_artistes(), "Khruangbin").as_deref(),
            Some("https://cdn/images/artist/1f18/500x500-000000-80-0-0.jpg")
        );
    }

    #[test]
    fn photo_artiste_ignore_un_homonyme_dont_l_image_est_vide() {
        let v = serde_json::json!({ "data": [
            { "name": "Khruangbin", "nb_fan": 12,
              "picture_big": "https://cdn/images/artist//500x500-000000-80-0-0.jpg" },
        ]});
        assert_eq!(url_photo_artiste(&v, "Khruangbin"), None);
    }

    #[test]
    fn photo_artiste_exige_le_meme_nom_pas_une_inclusion() {
        let v = serde_json::json!({ "data": [
            { "name": "Air Supply", "nb_fan": 500000, "picture_big": "https://cdn/air-supply.jpg" },
        ]});
        assert_eq!(url_photo_artiste(&v, "Air"), None);
        assert_eq!(url_photo_artiste(&v, "Air Supply").as_deref(), Some("https://cdn/air-supply.jpg"));
        assert_eq!(url_photo_artiste(&serde_json::json!({ "data": [] }), "Air"), None);
        assert_eq!(url_photo_artiste(&v, ""), None);
    }

    #[test]
    fn lien_album_rend_la_page_de_l_album_de_l_artiste() {
        let v = serde_json::json!({ "data": [
            { "title": "Mordechai", "artist": { "name": "Khruangbin" },
              "link": "https://www.deezer.com/album/135203602" },
            { "title": "Mordechai", "artist": { "name": "Un autre" },
              "link": "https://www.deezer.com/album/1" },
        ]});
        assert_eq!(
            url_lien_album(&v, "Khruangbin", "Mordechai").as_deref(),
            Some("https://www.deezer.com/album/135203602")
        );
        assert_eq!(url_lien_album(&v, "Pearl Jam", "Mordechai"), None);
        assert_eq!(url_lien_album(&v, "Khruangbin", "Con Todo El Mundo"), None);
    }

    #[test]
    fn lien_album_et_artiste_refusent_un_hote_etranger() {
        let album = serde_json::json!({ "data": [
            { "title": "Mordechai", "artist": { "name": "Khruangbin" },
              "link": "https://example.org/phishing" },
        ]});
        assert_eq!(url_lien_album(&album, "Khruangbin", "Mordechai"), None);
        let artiste = serde_json::json!({ "data": [
            { "name": "Khruangbin", "nb_fan": 9, "link": "http://www.deezer.com/artist/1" },
        ]});
        assert_eq!(url_lien_artiste(&artiste, "Khruangbin"), None);
    }

    #[test]
    fn lien_artiste_prefere_le_plus_suivi_et_exige_le_meme_nom() {
        let v = serde_json::json!({ "data": [
            { "name": "Khruangbin", "nb_fan": 0, "link": "https://www.deezer.com/artist/2" },
            { "name": "Khruangbin", "nb_fan": 97949, "link": "https://www.deezer.com/artist/5328540" },
            { "name": "Air Supply", "nb_fan": 500000, "link": "https://www.deezer.com/artist/3" },
        ]});
        assert_eq!(
            url_lien_artiste(&v, "Khruangbin").as_deref(),
            Some("https://www.deezer.com/artist/5328540")
        );
        assert_eq!(url_lien_artiste(&v, "Air"), None);
        assert_eq!(url_lien_artiste(&v, ""), None);
    }
}
