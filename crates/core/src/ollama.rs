// SPDX-License-Identifier: GPL-3.0-or-later
//! Ollama : le client, et rien d'autre.
//!
//! Sert l'interprétation du champ d'intention d'Explorer (texte libre →
//! playlist) — un appel unique, déclenché par un clic, pas un traitement de
//! masse. Contrairement à [`crate::lastfm`] ou [`crate::listenbrainz`], pas
//! de cadence ni de réessais à ménager : Ollama tourne en local, et une
//! panne n'a qu'une cause plausible (le serveur n'est pas lancé), pas
//! plusieurs à distinguer artiste par artiste.
//!
//! Même serveur qu'`experiments/clap-texte/preparer_vocabulaire.py::
//! reecrire_ollama` (`qwen2.5:3b` par défaut, HTTP sur `localhost:11434`) —
//! seul usage préexistant d'Ollama dans ce dépôt, jusqu'ici toujours
//! hors-ligne et en Python. Rien n'écrit en base ici non plus.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::Error;
use crate::filtres_playlist::{FiltresPlaylist, NiveauEnergie, NiveauPopularite};

/// Modèle par défaut de `preparer_vocabulaire.py`, choisi là pour tourner
/// correctement sur un poste sans accélérateur — **jamais utilisé comme
/// repli silencieux ici** : deviner un nom fixe échoue dès que la machine ne
/// l'a pas (observé : absent, Ollama rend un 404 sur `/api/generate` que rien
/// ne distingue à l'œil d'un serveur injoignable). [`modele_par_defaut`]
/// interroge plutôt ce qui est réellement installé.
pub const MODELE_DEFAUT: &str = "qwen2.5:3b";
pub const HOTE_DEFAUT: &str = "http://localhost:11434";

/// L'interprétation d'un prompt de playlist en texte libre (Explorer →
/// champ d'intention), telle que le LLM la rend.
///
/// `etapes` n'est jamais vide dans une valeur retournée par [`interpreter`] —
/// un prompt ne décrivant qu'une seule ambiance donne une liste d'un seul
/// élément, jamais une liste vide.
///
/// `arrivee_artiste`/`arrivee_morceau` : absents de la première version, qui
/// ne savait que dériver depuis un départ, jamais viser une arrivée précise.
/// Un prompt « de X à Y en passant par Z » (observé en usage réel) confondait
/// alors X et Y sans schéma pour les distinguer — un modèle mettait l'arrivée
/// dans `seed_artiste`, un autre l'ignorait. Distincts de `seed_*` pour la
/// même raison qu'eux : laisser le modèle nommer précisément ce qu'il a
/// reconnu, plutôt que de le déduire d'une liste d'étapes en anglais.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterpretationLlm {
    /// Une phrase française où le LLM reformule la demande. **Premier champ du
    /// schéma** : il écrit sa compréhension avant de remplir le reste, ce qui
    /// aide l'extraction et donne à l'utilisateur de quoi vérifier d'un coup
    /// d'œil qu'il a été compris. Un récit du modèle, pas son raisonnement.
    #[serde(default)]
    pub reformulation: Option<String>,
    /// Une suite de **parties** (« rock pendant 12 minutes, puis hip hop pendant
    /// 20 »), chacune avec son genre, son énergie et sa durée. Vide : une seule
    /// partie implicite (les filtres globaux et `etapes`). Jamais une seule :
    /// [`InterpretationLlm::normaliser`] ramène une partie unique aux champs
    /// globaux.
    #[serde(default)]
    pub parties: Vec<PartieLlm>,
    /// La réponse du modèle telle qu'il l'a écrite (lignes vides retirées) —
    /// pour le dépliant « réponse brute » de l'interface. Jamais lue du JSON du
    /// modèle : posée par [`interpreter`].
    #[serde(default, skip_deserializing)]
    pub brut: String,
    // `#[serde(default)]` sur les quatre : un modèle qui oublie une clé plutôt
    // que d'y mettre `null` (plus probable à quatre clés optionnelles qu'à
    // deux) ne doit pas faire échouer toute l'interprétation pour autant.
    #[serde(default)]
    pub seed_artiste: Option<String>,
    #[serde(default)]
    pub seed_morceau: Option<String>,
    #[serde(default)]
    pub arrivee_artiste: Option<String>,
    #[serde(default)]
    pub arrivee_morceau: Option<String>,
    pub etapes: Vec<String>,
    #[serde(default)]
    pub n: Option<u32>,
    /// Durée totale voulue (« 60 mn », « une heure »). Prime sur `n` : le
    /// moteur arrête la playlist à cette durée, pas à un nombre de morceaux.
    #[serde(default)]
    pub duree_minutes: Option<u32>,
    #[serde(default)]
    pub plafond_par_artiste: Option<u32>,
    // Les filtres ci-dessous sont les arguments typés que le code applique à
    // la bibliothèque (`crate::filtres_playlist`) — voir
    // `docs/recherche-llm-playlist.md`. À plat, pas dans un sous-objet : un
    // petit modèle suit mieux un schéma sans imbrication.
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub exclure_genres: Vec<String>,
    #[serde(default)]
    pub exclure_artistes: Vec<String>,
    #[serde(default)]
    pub annee_min: Option<i32>,
    #[serde(default)]
    pub annee_max: Option<i32>,
    #[serde(default)]
    pub bpm_min: Option<f32>,
    #[serde(default)]
    pub bpm_max: Option<f32>,
    /// « calme », « moyenne » ou « intense » — lu avec tolérance
    /// ([`NiveauEnergie::depuis_texte`]).
    #[serde(default)]
    pub energie: Option<String>,
    /// « peu_connu » ou « connu ».
    #[serde(default)]
    pub popularite: Option<String>,
}

/// Les mots d'un texte, en minuscules et sans accents.
fn mots_sans_accent(texte: &str) -> Vec<String> {
    let sans: String = texte
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect();
    crate::filtres_playlist::mots(&sans)
}

/// Équivalences français → anglais des genres qu'un utilisateur écrit en
/// français et que la bibliothèque range en anglais.
const GENRES_FR: &[(&str, &str)] = &[
    ("monde", "world"),
    ("classique", "classical"),
    ("electronique", "electronic"),
    ("electro", "electronic"),
    ("film", "soundtrack"),
    ("films", "soundtrack"),
    ("bo", "soundtrack"),
    ("francaise", "francaise"),
];

/// Rang, dans les mots du texte, du premier mot qui nomme `genre` (hors le mot
/// générique « music »).
fn position_du_genre(genre: &str, texte: &[String]) -> Option<usize> {
    crate::filtres_playlist::variantes_de_genre(genre)
        .iter()
        .flat_map(|v| mots_sans_accent(v))
        .filter(|m| m != "music")
        .filter_map(|m| texte.iter().position(|t| genre_nomme(&m, std::slice::from_ref(t))))
        .min()
}

/// Distance d'édition d'au plus 1 (une lettre en plus, en moins ou changée).
fn a_une_faute_pres(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let (court, long) = if a.len() <= b.len() { (&a, &b) } else { (&b, &a) };
    let debut = court.iter().zip(long.iter()).take_while(|(x, y)| x == y).count();
    if court.len() == long.len() {
        court[debut + 1..] == long[debut + 1..]
    } else {
        court[debut..] == long[debut + 1..]
    }
}

/// `genre` est-il nommé par ces mots du texte ? Chaque mot du genre (et de ses
/// variantes, `rap` ↔ `hip hop`) doit retrouver un mot du texte : égal, ou — à
/// partir de 5 lettres — partageant un préfixe d'au moins 5 lettres.
fn genre_nomme(genre: &str, texte: &[String]) -> bool {
    let retrouve = |mot: &str| -> bool {
        texte.iter().any(|t| {
            if t == mot {
                return true;
            }
            if GENRES_FR.iter().any(|(fr, en)| *fr == t.as_str() && *en == mot) {
                return true;
            }
            let prefixe_commun = t.chars().zip(mot.chars()).take_while(|(a, b)| a == b).count();
            // Un préfixe de 5 lettres (« electronique » ~ `electronic`), ou une
            // faute de frappe (« roc » ~ `rock`, « jaz » ~ `jazz`) — mesuré :
            // « plylist calm de 1h san roc » perdait son refus du rock.
            (mot.len() >= 5 && t.len() >= 5 && prefixe_commun >= 5)
                || (mot.len() >= 4 && t.len() >= 3 && a_une_faute_pres(t, mot))
        })
    };
    crate::filtres_playlist::variantes_de_genre(genre).iter().any(|variante| {
        let mut mots_genre = mots_sans_accent(variante);
        // « music » est le mot le plus courant d'une demande de musique : il ne
        // doit pas, à lui seul, ancrer « world music » ou « children's music ».
        if mots_genre.len() > 1 {
            mots_genre.retain(|m| m != "music");
        }
        !mots_genre.is_empty() && mots_genre.iter().all(|m| retrouve(m))
    })
}

/// Un moment d'une playlist en plusieurs parties. Les exclusions, années,
/// tempo, popularité et plafond restent **globaux** ; `genres` et `energie` de
/// la partie priment sur ceux de la playlist.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PartieLlm {
    /// Phrase anglaise décrivant ce moment — la cible CLAP-texte de la partie.
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub energie: Option<String>,
    #[serde(default)]
    pub duree_minutes: Option<u32>,
    #[serde(default)]
    pub n: Option<u32>,
}

/// Au plus ce nombre de parties : au-delà, ce n'est plus une suite de moments
/// mais un modèle qui a recopié une liste.
const PARTIES_MAX: usize = 6;

impl InterpretationLlm {
    /// Les contraintes typées que le code appliquera, sans les champs de
    /// structure (départ, arrivée, étapes, durée).
    pub fn filtres(&self) -> FiltresPlaylist {
        FiltresPlaylist {
            genres: self.genres.clone(),
            exclure_genres: self.exclure_genres.clone(),
            exclure_artistes: self.exclure_artistes.clone(),
            annee_min: self.annee_min,
            annee_max: self.annee_max,
            bpm_min: self.bpm_min,
            bpm_max: self.bpm_max,
            energie: self.energie.as_deref().and_then(NiveauEnergie::depuis_texte),
            popularite: self.popularite.as_deref().and_then(NiveauPopularite::depuis_texte),
        }
    }

    /// Ramène à des valeurs plausibles ce qu'un petit modèle rend de travers :
    /// chaînes vides, bornes inversées, année « 70 », durée de zéro minute…
    /// Mieux vaut ignorer un critère absurde que filtrer la bibliothèque
    /// dessus.
    fn normaliser(&mut self) {
        let propre = |v: &mut Vec<String>| {
            v.iter_mut().for_each(|s| *s = s.trim().to_string());
            v.retain(|s| !s.is_empty());
            v.dedup();
        };
        propre(&mut self.genres);
        propre(&mut self.exclure_genres);
        propre(&mut self.exclure_artistes);
        // Une **arrivée** nommée fait de la playlist un trajet : un genre cité
        // « en passant par » en est une étape (déjà dans `etapes`), pas un
        // filtre qui cloisonnerait toute la marche. Observé : même avec la
        // règle dans la consigne, un petit modèle met « hip hop » dans
        // `genres` pour « de X à RATM en passant par du hip hop ».
        // Un départ seul (« du r&b comme Ella Mai ») n'est pas un trajet : le
        // genre y est une contrainte, et le vider perdait 93 requêtes sur 151
        // de MusicRecoIntent (`experiments/musicrecointent`).
        let est_trajet = [&self.arrivee_artiste, &self.arrivee_morceau]
            .iter()
            .any(|c| c.as_deref().is_some_and(|s| !s.trim().is_empty()));
        if est_trajet {
            self.genres.clear();
        }
        // Un genre à la fois désiré et exclu : le rejet l'emporte.
        let exclus: Vec<String> = self.exclure_genres.iter().map(|g| g.to_lowercase()).collect();
        self.genres.retain(|g| !exclus.contains(&g.to_lowercase()));

        let annee = |a: Option<i32>| a.filter(|a| (1900..=2100).contains(a));
        self.annee_min = annee(self.annee_min);
        self.annee_max = annee(self.annee_max);
        if let (Some(min), Some(max)) = (self.annee_min, self.annee_max) {
            if min > max {
                self.annee_min = Some(max);
                self.annee_max = Some(min);
            }
        }
        let bpm = |b: Option<f32>| b.filter(|b| (30.0..=300.0).contains(b));
        self.bpm_min = bpm(self.bpm_min);
        self.bpm_max = bpm(self.bpm_max);
        if let (Some(min), Some(max)) = (self.bpm_min, self.bpm_max) {
            if min > max {
                self.bpm_min = Some(max);
                self.bpm_max = Some(min);
            }
        }
        self.duree_minutes = self.duree_minutes.filter(|d| (1..=1440).contains(d));
        self.plafond_par_artiste = self.plafond_par_artiste.filter(|p| *p >= 1);
        self.n = self.n.filter(|n| *n >= 1);
        self.reformulation = self
            .reformulation
            .take()
            .map(|r| r.trim().to_string())
            .filter(|r| !r.is_empty());
        self.normaliser_parties();
    }

    /// Nettoie les parties, et fixe ce qui revient à la playlist entière ou à
    /// chaque partie.
    fn normaliser_parties(&mut self) {
        let energie_valide =
            |e: Option<String>| e.filter(|e| NiveauEnergie::depuis_texte(e).is_some());
        let mut parties = std::mem::take(&mut self.parties);
        for p in &mut parties {
            p.description = p.description.trim().to_string();
            p.genres.iter_mut().for_each(|g| *g = g.trim().to_string());
            p.genres.retain(|g| !g.is_empty());
            p.genres.dedup();
            p.energie = energie_valide(p.energie.take());
            p.duree_minutes = p.duree_minutes.filter(|d| (1..=1440).contains(d));
            p.n = p.n.filter(|n| *n >= 1);
            // Une partie sans description mais avec un genre reste exploitable.
            if p.description.is_empty() {
                if let Some(g) = p.genres.first() {
                    p.description = format!("{g} music");
                }
            }
        }
        // Une partie sans rien : un modèle qui a laissé un gabarit vide.
        parties.retain(|p| !p.description.is_empty());
        parties.truncate(PARTIES_MAX);

        match parties.len() {
            0 => {}
            // Une partie unique n'est pas une suite : on la ramène aux champs
            // globaux, que le reste du moteur connaît déjà.
            1 => {
                let p = parties.remove(0);
                if self.genres.is_empty() {
                    self.genres = p.genres;
                }
                if self.energie.is_none() {
                    self.energie = p.energie;
                }
                if self.duree_minutes.is_none() && self.n.is_none() {
                    self.duree_minutes = p.duree_minutes;
                    self.n = p.n;
                }
                if self.etapes.iter().all(|e| e.trim().is_empty()) {
                    self.etapes = vec![p.description];
                }
            }
            _ => {
                // Un genre « puis » n'est pas un filtre commun : « rock puis hip
                // hop » ne doit pas devenir « rock ou hip hop » partout — c'est
                // précisément ce qu'on a observé avant les parties.
                self.genres.clear();
                // Sans taille par partie, la taille globale se partage également.
                let sans_taille = parties.iter().all(|p| p.duree_minutes.is_none() && p.n.is_none());
                if sans_taille {
                    let k = parties.len() as u32;
                    if let Some(d) = self.duree_minutes.take() {
                        for p in &mut parties {
                            p.duree_minutes = Some((d / k).max(1));
                        }
                    } else if let Some(n) = self.n.take() {
                        for p in &mut parties {
                            p.n = Some((n / k).max(1));
                        }
                    }
                } else {
                    // La durée totale est la somme des parties.
                    self.duree_minutes = None;
                    self.n = None;
                }
                // Les étapes de la marche sont les descriptions des parties.
                self.etapes = parties.iter().map(|p| p.description.clone()).collect();
            }
        }
        self.parties = parties;
    }

    /// Écarte les genres — voulus ou refusés — que **le texte de l'utilisateur ne
    /// nomme pas**. Un petit modèle en déduit de l'ambiance (mesuré : « de la
    /// musique énergique pour faire du sport » → `electronic`, `hard rock`,
    /// `heavy metal` ; « pour m'endormir » → un refus de `heavy metal`) : filtrer
    /// la bibliothèque sur un genre que l'utilisateur n'a jamais écrit est pire
    /// que de ne pas filtrer. Rend les genres écartés, pour le journal.
    ///
    /// Comparaison par mots, sans accents ni casse : « électronique » nomme
    /// `electronic`, « rap » nomme `hip hop` (mêmes variantes que le filtre),
    /// « monde » nomme `world music`. Un mot court (moins de 5 lettres) doit
    /// être là en entier : « rap » ne s'ancre pas dans « rapide ».
    pub fn ancrer_genres(&mut self, prompt: &str) -> Vec<String> {
        let texte = mots_sans_accent(prompt);
        let nomme = |genre: &String| genre_nomme(genre, &texte);
        let mut ecartes = Vec::new();
        let mut listes: Vec<&mut Vec<String>> = vec![&mut self.genres, &mut self.exclure_genres];
        listes.extend(self.parties.iter_mut().map(|p| &mut p.genres));
        for liste in listes {
            let (gardes, perdus): (Vec<String>, Vec<String>) =
                std::mem::take(liste).into_iter().partition(nomme);
            *liste = gardes;
            ecartes.extend(perdus);
        }
        ecartes
    }

    /// « Du folk puis de l'électronique » : un « puis » explicite entre deux
    /// genres nommés **est** une suite de parties, même quand le modèle les rend
    /// en filtre commun (mesuré : une partie des petits modèles le fait quand
    /// aucune durée n'est dite). Sans mot de séquence, « du rock et du jazz »
    /// reste un mélange. À appeler **après** [`Self::ancrer_genres`] : les genres
    /// sont alors tous nommés par le texte. Rend vrai si des parties ont été
    /// déduites.
    pub fn inferer_parties(&mut self, prompt: &str) -> bool {
        const SEQUENCE: &[&str] =
            &["puis", "ensuite", "apres", "then", "d'abord", "dabord", "suivi", "finir", "finis"];
        if !self.parties.is_empty() || self.genres.len() < 2 {
            return false;
        }
        let texte = mots_sans_accent(prompt);
        if !texte.iter().any(|m| SEQUENCE.contains(&m.as_str())) {
            return false;
        }
        // Dans l'ordre où le texte les nomme.
        let mut ordonnes: Vec<(usize, String)> = self
            .genres
            .iter()
            .filter_map(|g| position_du_genre(g, &texte).map(|i| (i, g.clone())))
            .collect();
        if ordonnes.len() < 2 {
            return false;
        }
        ordonnes.sort_by_key(|(i, _)| *i);
        self.parties = ordonnes
            .into_iter()
            .map(|(_, g)| PartieLlm { description: format!("{g} music"), genres: vec![g], ..Default::default() })
            .collect();
        self.normaliser_parties();
        true
    }

    /// Écarte les genres que la bibliothèque ne connaît pas : le LLM a reçu la
    /// liste réelle, mais un petit modèle en invente quand même, et un genre
    /// inventé viderait la sélection. Rend les genres écartés, pour le journal.
    /// Sans vocabulaire (`vide`), rien n'est écarté.
    fn restreindre_au_vocabulaire(&mut self, vocabulaire: &[String]) -> Vec<String> {
        if vocabulaire.is_empty() {
            return Vec::new();
        }
        let connu = |g: &String| vocabulaire.iter().any(|v| v.eq_ignore_ascii_case(g));
        let mut ecartes = Vec::new();
        let mut listes: Vec<&mut Vec<String>> = vec![&mut self.genres, &mut self.exclure_genres];
        listes.extend(self.parties.iter_mut().map(|p| &mut p.genres));
        for liste in listes {
            let (gardes, perdus): (Vec<String>, Vec<String>) =
                std::mem::take(liste).into_iter().partition(connu);
            *liste = gardes;
            ecartes.extend(perdus);
        }
        ecartes
    }
}

/// Consigne envoyée comme message système : impose le schéma JSON et la
/// langue des descripteurs — CLAP a été entraîné sur des légendes anglaises,
/// un mot nu ou une phrase française n'y donnerait rien de bon (voir
/// `docs/suite.md` §7 et `experiments/clap-texte/README.md`).
///
/// L'exemple mêlant départ **et** arrivée est déterminant, pas décoratif :
/// sans lui, observé en pratique, un modèle glisse le nom de l'arrivée dans
/// `seed_artiste` (le seul champ « artiste » qu'il connaît vraiment) plutôt
/// que dans `arrivee_artiste`.
///
/// **Régression du 14 septembre : l'abréviation « RATM » était rendue telle
/// quelle** — l'exemple ci-dessous l'écrivait même ainsi, apprenant au modèle
/// à faire pareil. `resoudre_piste_nommee` (`apps/desktop/src/main.rs`)
/// cherche un nom littéral dans la bibliothèque (préfixe FTS5) : « RATM » n'y
/// trouve jamais « Rage Against The Machine », faute de mot commun. Un modèle
/// connaît pourtant très bien ce sigle — la consigne le lui demande
/// maintenant explicitement, et l'exemple montre le nom développé, pas
/// l'abréviation d'origine.
const SYSTEME: &str = r#"Tu transformes une description de playlist musicale en JSON structuré.

Réponds uniquement par un objet JSON, sans texte autour, avec exactement ces clés,
dans cet ordre :
{
  "reformulation": UNE phrase en français qui reformule ce que l'utilisateur
                   demande (parties, durées et refus compris). Écris-la EN
                   PREMIER : elle te sert à comprendre la demande avant de
                   remplir le reste,
  "seed_artiste": nom COMPLET de l'artiste ou du groupe DE DÉPART cité dans le
                  texte (« partir de », « commencer par », « comme »,
                  « dans l'esprit de »…), ou null si aucun,
  "seed_morceau": titre exact du morceau de départ, ou null si aucun,
  "arrivee_artiste": nom COMPLET de l'artiste ou du groupe D'ARRIVÉE cité dans
                     le texte (« arriver à », « terminer par », « finir
                     sur »…), ou null si aucune arrivée n'est demandée — NE
                     PAS confondre avec "seed_artiste", qui est le départ,
  "arrivee_morceau": titre exact du morceau d'arrivée, ou null si aucun,
  "etapes": une liste ordonnée d'une ou plusieurs phrases descriptives EN ANGLAIS,
            chacune décrivant une ambiance, un style ou une évolution sonore
            voulue EN CHEMIN (entre le départ et l'arrivée, s'il y en a une) —
            jamais un simple mot-clé isolé. Exemple correct : "a song with a
            strong focus on drums". Exemple incorrect : "drums".
  "n": le nombre de MORCEAUX voulu si le texte le précise, sinon null,
  "duree_minutes": la durée TOTALE voulue en minutes (« 60 mn », « une heure »
                   → 60), sinon null — jamais dans "n",
  "genres": genres voulus, UNIQUEMENT parmi la liste ci-dessous, et UNIQUEMENT
            si le texte NOMME ce genre (« du jazz », « du reggae ») et que TOUTE
            la playlist doit y rester. Une ambiance, un usage ou un moment
            (« pour courir », « pour m'endormir », « un road trip ») ne nomme
            AUCUN genre : []. Jamais quand le texte nomme un départ ou une
            arrivée : un genre cité « en passant par » est une étape, pas un
            filtre. Sinon [],
  "exclure_genres": genres que le texte REFUSE EXPLICITEMENT (« sans rock »,
                    « pas de rap »), UNIQUEMENT parmi la liste ci-dessous. Ne
                    déduis jamais un refus d'une ambiance : sinon [],
  "exclure_artistes": artistes refusés (« sans Prince »), noms complets, sinon [],
  "annee_min", "annee_max": bornes d'années (« années 70 » → 1970 et 1979), sinon null,
  "bpm_min", "bpm_max": bornes de tempo, UNIQUEMENT si le texte parle de tempo,
                        de rythme ou de BPM (« rythme lent » → bpm_max 90,
                        « rapide » → bpm_min 130, ou les chiffres cités).
                        « calme » seul ne parle PAS de tempo : null,
  "energie": "calme", "moyenne" ou "intense", UNIQUEMENT si le texte parle
             d'ambiance ou d'énergie (calme, posé, dynamique, « pour courir »),
             sinon null,
  "popularite": "peu_connu" (« méconnus », « oubliés », « confidentiels ») ou
                "connu" (« tubes », « classiques connus »), sinon null,
  "plafond_par_artiste": « pas plus de 2 morceaux par artiste » → 2, sinon null,
  "parties": VIDE ([]) dans la plupart des cas. Ne la remplis QUE si le texte
             enchaîne plusieurs moments distincts, chacun avec son style
             (« du rock puis du hip hop », « 30 minutes de calme, ensuite du
             dynamique »). Les durées ne sont PAS nécessaires : « du folk puis
             de l'électronique » = deux parties. Une partie par moment, dans
             l'ordre :
             {"description": phrase EN ANGLAIS qui décrit ce moment,
              "genres": genres voulus POUR CE MOMENT (liste ci-dessous), sinon [],
              "energie": "calme", "moyenne", "intense" ou null,
              "duree_minutes": durée de CE moment (« pendant 12 minutes » → 12)
                               ou null,
              "n": nombre de morceaux de CE moment, ou null}
             Avec des parties, "duree_minutes" et "n" de la playlist valent
             null (la durée totale est la somme des parties) et "etapes"
             reprend leurs descriptions. Une évolution continue (« commence
             doucement et monte vers du rock ») n'est PAS une suite de
             parties : elle reste dans "etapes", avec "parties": []
}

Ne remplis un filtre que si le texte le demande : en cas de doute, null ou [].
Ce que l'utilisateur REFUSE ne va jamais dans "etapes" ni dans "genres" mais
dans les clés "exclure_…". Les étapes décrivent toujours l'ambiance voulue, y
compris pour un filtre : « calme » → une étape comme "calm, relaxing music".

Genres connus de la bibliothèque : {GENRES}.

Important pour "seed_artiste", "arrivee_artiste" et "exclure_artistes" : rends
toujours le nom COMPLET, usuel et non ambigu de l'artiste — jamais un sigle,
une abréviation ou un surnom, même si c'est ce que le texte emploie. Ce nom
sert ensuite à chercher l'artiste dans une bibliothèque musicale, où il est
rangé sous son nom complet. Exemples : « RATM » → "Rage Against The Machine" ;
« GNR » → "Guns N' Roses" ; « les Beatles » → "The Beatles".

Exemple : pour « Partir de Shootyz Groove. Arriver à RATM. En passant par du
hip hop », la bonne réponse est :
{
  "reformulation": "Partir de Shootyz Groove, passer par du hip hop et finir sur Rage Against The Machine.",
  "seed_artiste": "Shootyz Groove", "seed_morceau": null,
  "arrivee_artiste": "Rage Against The Machine", "arrivee_morceau": null,
  "etapes": ["hip hop with a strong groove"], "n": null, "duree_minutes": null,
  "genres": [], "exclure_genres": [], "exclure_artistes": [],
  "annee_min": null, "annee_max": null, "bpm_min": null, "bpm_max": null,
  "energie": null, "popularite": null, "plafond_par_artiste": null, "parties": []
}

Exemple : pour « Une playlist calme de 60 minutes, sans rock, années 70 »
(le genre "rock" est dans la liste) :
{
  "reformulation": "Une heure de musique calme, sans rock, des années 70.",
  "seed_artiste": null, "seed_morceau": null,
  "arrivee_artiste": null, "arrivee_morceau": null,
  "etapes": ["calm, relaxing music"], "n": null, "duree_minutes": 60,
  "genres": [], "exclure_genres": ["rock"], "exclure_artistes": [],
  "annee_min": 1970, "annee_max": 1979, "bpm_min": null, "bpm_max": null,
  "energie": "calme", "popularite": null, "plafond_par_artiste": null, "parties": []
}

Exemple : pour « Du jazz pendant 20 minutes, puis du reggae pendant 10 minutes »
(les genres "jazz" et "reggae" sont dans la liste) :
{
  "reformulation": "Vingt minutes de jazz, puis dix minutes de reggae.",
  "seed_artiste": null, "seed_morceau": null,
  "arrivee_artiste": null, "arrivee_morceau": null,
  "etapes": ["jazz music", "reggae music"], "n": null, "duree_minutes": null,
  "genres": [], "exclure_genres": [], "exclure_artistes": [],
  "annee_min": null, "annee_max": null, "bpm_min": null, "bpm_max": null,
  "energie": null, "popularite": null, "plafond_par_artiste": null,
  "parties": [
    {"description": "jazz music", "genres": ["jazz"], "energie": null, "duree_minutes": 20, "n": null},
    {"description": "reggae music", "genres": ["reggae"], "energie": null, "duree_minutes": 10, "n": null}
  ]
}

"etapes" ne doit jamais être une liste vide : si le texte ne décrit qu'une
seule ambiance, rends une liste à un seul élément."#;

/// Nombre de genres montrés au LLM : les plus représentés. Au-delà, le prompt
/// grossit pour des genres que presque aucune playlist ne demande ; la liste
/// complète sert quand même à **valider** sa réponse
/// ([`InterpretationLlm::restreindre_au_vocabulaire`]).
const GENRES_DANS_LE_PROMPT: usize = 60;

/// La consigne complète : [`SYSTEME`] avec le vocabulaire réel des genres.
fn systeme(vocabulaire: &[String]) -> String {
    let genres = if vocabulaire.is_empty() {
        "(aucune liste : laisse \"genres\" et \"exclure_genres\" vides)".to_string()
    } else {
        vocabulaire
            .iter()
            .take(GENRES_DANS_LE_PROMPT)
            .map(|g| format!("\"{g}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    SYSTEME.replace("{GENRES}", &genres)
}

/// Le schéma JSON passé à Ollama dans `format` : le décodage est alors
/// **contraint** (clés et types garantis), la consigne n'a plus à y veiller —
/// voir `docs/recherche-llm-playlist.md`. Tout est requis (un champ sans objet
/// se rend `null` ou `[]`) : un petit modèle qui peut omettre une clé en omet
/// trop.
fn schema() -> Value {
    let texte_ou_null = json!({ "type": ["string", "null"] });
    let entier_ou_null = json!({ "type": ["integer", "null"] });
    let nombre_ou_null = json!({ "type": ["number", "null"] });
    let liste = json!({ "type": "array", "items": { "type": "string" } });
    let energie = json!({ "type": ["string", "null"], "enum": ["calme", "moyenne", "intense", null] });
    // L'ordre des clés est celui où le modèle les écrit (`preserve_order`) :
    // `reformulation` d'abord, `parties` en dernier — c'est la plus lourde, et
    // une réponse coupée par le plafond de jetons perd plutôt la fin que le
    // milieu (voir `reparer_json_tronque`).
    json!({
        "type": "object",
        "properties": {
            "reformulation": { "type": "string" },
            "seed_artiste": texte_ou_null,
            "seed_morceau": texte_ou_null,
            "arrivee_artiste": texte_ou_null,
            "arrivee_morceau": texte_ou_null,
            "etapes": { "type": "array", "items": { "type": "string" }, "minItems": 1 },
            "n": entier_ou_null,
            "duree_minutes": entier_ou_null,
            "genres": liste,
            "exclure_genres": liste,
            "exclure_artistes": liste,
            "annee_min": entier_ou_null,
            "annee_max": entier_ou_null,
            "bpm_min": nombre_ou_null,
            "bpm_max": nombre_ou_null,
            "energie": energie,
            "popularite": { "type": ["string", "null"], "enum": ["peu_connu", "connu", null] },
            "plafond_par_artiste": entier_ou_null,
            "parties": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "description": { "type": "string" },
                        "genres": liste,
                        "energie": energie,
                        "duree_minutes": entier_ou_null,
                        "n": entier_ou_null,
                    },
                    "required": ["description", "genres", "energie", "duree_minutes", "n"],
                },
            },
        },
        "required": [
            "reformulation", "seed_artiste", "seed_morceau", "arrivee_artiste", "arrivee_morceau",
            "etapes", "n", "duree_minutes", "genres", "exclure_genres",
            "exclure_artistes", "annee_min", "annee_max", "bpm_min", "bpm_max",
            "energie", "popularite", "plafond_par_artiste", "parties"
        ],
    })
}

/// Interroge Ollama pour interpréter `prompt`. Un seul appel, sans réessai :
/// une panne ici n'a qu'une cause utile à signaler — le serveur ne répond
/// pas — que l'appelant doit montrer telle quelle plutôt que masquer par un
/// repli silencieux (voir la discussion dans le plan Explorer « texte →
/// playlist » : un descripteur composé du prompt brut, non traduit, donnerait
/// un résultat CLAP de mauvaise qualité sans que l'utilisateur comprenne
/// pourquoi).
///
/// Pas de `Client` à construire à l'avance, contrairement à `lastfm`/
/// `listenbrainz` : un appel local, unique, déclenché par un clic n'a rien à
/// cadencer ni à garder d'un appel à l'autre.
///
/// `vocabulaire` : les genres réels de la bibliothèque, du plus au moins
/// représenté (`Library::vocabulaire_genres`) — montrés au LLM, puis utilisés
/// pour écarter les genres qu'il inventerait quand même. Vide : aucun filtre
/// de genre n'est demandé ni gardé.
pub fn interpreter(
    hote: &str,
    modele: &str,
    prompt: &str,
    vocabulaire: &[String],
) -> crate::Result<InterpretationLlm> {
    let corps = serde_json::to_string(&json!({
        "model": modele,
        "system": systeme(vocabulaire),
        "prompt": prompt,
        // Un vrai schéma, pas `"json"` : décodage contraint, clés et types
        // garantis. À température 0, le même texte rend la même
        // interprétation — de quoi repérer une régression de la consigne.
        "format": schema(),
        // Plafond de jetons : une spec complète (reformulation et parties comprises) en tient moins de 500. Sans lui,
        // un décodage contraint qui part en boucle ne s'arrête qu'au délai de
        // 300 s. Observé (gemma4:e4b-mlx, 6 prompts sur 53 dont 4 sur 5 de
        // ceux qui fixent une année) : le modèle écrit une spec correcte, puis
        // des espaces à l'infini au lieu de la clé requise suivante — voir
        // [`reparer_json_tronque`], qui sauve la spec ainsi coupée. Rendre les
        // clés facultatives n'y change rien de bon : le modèle n'en remplit
        // plus qu'une partie (20 prompts conformes sur 53, contre 46).
        //
        // `num_ctx` : consigne + schéma + prompt + réponse tiennent dans 6 144
        // jetons. Le contexte par défaut (131 072 sur les modèles MLX) réserve
        // un cache bien plus gros que nécessaire : observé, le modèle de 27 Go
        // (`qwen3.8:27b-mlx`) plantait en mémoire (« Insufficient Memory »,
        // Metal) sur une machine de 24 Go dès la première interprétation.
        "options": { "temperature": 0, "num_predict": 700, "num_ctx": 6144 },
        "stream": false,
        // Un modèle « qui réfléchit » (`ollama list` : capacité `thinking`,
        // observé sur qwen3.8:27b-mlx) met sinon sa réponse dans le champ
        // `thinking` et laisse `response` vide — `interpretation_de` sait de
        // toute façon lire l'un ou l'autre, mais couper la réflexion évite
        // une minute d'attente pour rien sur un prompt aussi court. Ignoré
        // sans effet par un modèle qui ne raisonne pas.
        "think": false,
    }))
    .map_err(|e| Error::Parsing(format!("encodage de la requête Ollama : {e}")))?;

    tracing::info!(%modele, %prompt, "champ d'intention : interrogation d'Ollama");

    let url = format!("{hote}/api/generate");
    let (statut, brut_reponse) = requete(&url, TIMEOUT_GENERATION, |agent| {
        agent.post(&url).content_type("application/json").send(corps.as_str())
    })?;

    let v: Value = serde_json::from_str(&brut_reponse)
        .map_err(|e| Error::Parsing(format!("réponse d'Ollama illisible : {e}")))?;

    if statut == 404 {
        // Le cas observé en pratique : `modele` n'est pas dans `ollama list`.
        // Sans ce cas à part, l'échec rendrait un « Ollama ne répond pas »
        // trompeur alors qu'Ollama répond très bien — juste pas ce modèle.
        return Err(Error::Reseau(format!(
            "modèle Ollama « {modele} » introuvable — `ollama pull {modele}`, \
             ou choisissez un autre modèle"
        )));
    }
    if !(200..300).contains(&statut) {
        let message = v["error"].as_str().unwrap_or(brut_reponse.as_str());
        return Err(Error::Reseau(format!("Ollama a refusé la requête ({statut}) : {message}")));
    }

    // `response` d'abord ; `thinking` en repli — un modèle qui raisonne peut y
    // mettre sa réponse même avec `"think": false` dans la requête (tous ne
    // l'honorent pas). Vide des deux côtés : `interpretation_de("")` échoue
    // proprement en JSON illisible, pas un message de repli à écrire ici.
    let brut_json = v["response"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| v["thinking"].as_str())
        .unwrap_or_default();
    tracing::info!(%brut_json, "champ d'intention : réponse brute d'Ollama");

    let mut resultat = interpretation_de(brut_json);
    if let Ok(p) = &mut resultat {
        p.brut = brut_json
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let ecartes = p.restreindre_au_vocabulaire(vocabulaire);
        if !ecartes.is_empty() {
            tracing::info!(?ecartes, "champ d'intention : genres inconnus de la bibliothèque, écartés");
        }
        let inventes = p.ancrer_genres(prompt);
        if !inventes.is_empty() {
            tracing::info!(?inventes, "champ d'intention : genres que le texte ne nomme pas, écartés");
        }
        if p.inferer_parties(prompt) {
            tracing::info!("champ d'intention : suite de genres lue comme des parties");
        }
    }
    match &resultat {
        // `?` (Debug) : voir tout de suite si `seed_artiste` a été confondu
        // avec `seed_morceau`, si les étapes sont bien en anglais, etc. — le
        // genre de détail qu'un simple « ça a marché » ne dit pas.
        Ok(p) => tracing::info!(?p, "champ d'intention : interprétation retenue"),
        Err(e) => tracing::warn!(erreur = %e, "champ d'intention : réponse d'Ollama inexploitable"),
    }
    resultat
}

/// Les modèles déjà installés localement (`ollama list`), du **plus petit au
/// plus gros** — sert le sélecteur du champ d'intention (icône 🦙) et
/// [`modele_par_defaut`].
pub fn modeles(hote: &str) -> crate::Result<Vec<String>> {
    let url = format!("{hote}/api/tags");
    let (statut, brut) = requete(&url, TIMEOUT_LISTE, |agent| agent.get(&url).call())?;
    if !(200..300).contains(&statut) {
        return Err(Error::Reseau(format!("Ollama a refusé la requête ({statut}) : {brut}")));
    }
    let v: Value = serde_json::from_str(&brut)
        .map_err(|e| Error::Parsing(format!("réponse d'Ollama illisible : {e}")))?;
    Ok(trier_par_taille(
        v["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| Some((m["name"].as_str()?.to_string(), m["size"].as_u64()))),
    ))
}

/// Du plus petit au plus gros ; un modèle dont la taille n'est pas annoncée
/// passe après les autres (on ne le choisit pas par défaut sans savoir ce
/// qu'il coûte), à taille égale l'ordre d'Ollama est conservé.
fn trier_par_taille(modeles: impl IntoIterator<Item = (String, Option<u64>)>) -> Vec<String> {
    let mut v: Vec<(String, Option<u64>)> = modeles.into_iter().collect();
    v.sort_by_key(|(_, taille)| taille.unwrap_or(u64::MAX));
    v.into_iter().map(|(nom, _)| nom).collect()
}

/// Le **plus petit** modèle installé, faute de choix explicite de
/// l'utilisateur : le plus rapide à répondre, et celui qui tient dans la
/// mémoire de n'importe quelle machine (un 27 B échoue par manque de mémoire
/// sur 25 Go). Voir la documentation de [`MODELE_DEFAUT`] sur pourquoi ce
/// n'est pas un nom fixe.
pub fn modele_par_defaut(hote: &str) -> crate::Result<String> {
    modeles(hote)?.into_iter().next().ok_or_else(|| {
        Error::Reseau("aucun modèle Ollama installé — `ollama pull <modèle>`".to_string())
    })
}

/// `/api/tags` — une simple lecture de métadonnées, jamais lente.
const TIMEOUT_LISTE: Duration = Duration::from_secs(10);
/// `/api/generate` — un modèle local de plusieurs dizaines de Go peut mettre
/// plus d'une minute à répondre à un prompt pourtant court (mesuré : 87 s sur
/// un modèle « qui réfléchit » de 18 Go, `think: false` compris). Généreux
/// plutôt que de croire l'utilisateur mal servi alors que le modèle choisi
/// est simplement gros pour la machine.
const TIMEOUT_GENERATION: Duration = Duration::from_secs(300);

/// Envoie une requête construite par `faire`, sans lever d'erreur sur un
/// statut 4xx/5xx (`http_status_as_error(false)`) : Ollama met son message
/// utile — « modèle introuvable », par exemple — dans le corps JSON d'une
/// réponse d'erreur, qu'un statut traité en `Err` par défaut chez `ureq`
/// jetterait avant que l'appelant ne puisse le lire. Rend `(statut, corps)`,
/// à l'appelant de décider ce qu'un statut donné signifie.
fn requete(
    url: &str,
    timeout: Duration,
    faire: impl FnOnce(&ureq::Agent) -> Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> crate::Result<(u16, String)> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into();

    let mut reponse =
        faire(&agent).map_err(|e| Error::Reseau(format!("Ollama ne répond pas sur {url} : {e}")))?;
    let statut = reponse.status().as_u16();
    let corps = reponse
        .body_mut()
        .read_to_string()
        .map_err(|e| Error::Reseau(format!("lecture de la réponse d'Ollama : {e}")))?;
    Ok((statut, corps))
}

/// Referme un JSON coupé net : chaînes, tableaux et objets encore ouverts, avec
/// une virgule pendante retirée. Une clé écrite à moitié (`"cle":` sans
/// valeur) n'est pas réparable : on la retire aussi. `None` si rien n'est à
/// réparer ou si le texte n'a pas d'accolade ouvrante.
///
/// Les clés manquantes retombent sur `#[serde(default)]` : une spec coupée
/// avant `popularite` est une spec valide sans popularité demandée.
fn reparer_json_tronque(brut: &str) -> Option<String> {
    let debut = brut.find('{')?;
    let mut sortie = brut[debut..].trim_end().to_string();
    let (mut pile, mut dans_chaine, mut echappe) = (Vec::new(), false, false);
    for c in sortie.chars() {
        if dans_chaine {
            match (echappe, c) {
                (true, _) => echappe = false,
                (false, '\\') => echappe = true,
                (false, '"') => dans_chaine = false,
                _ => {}
            }
        } else {
            match c {
                '"' => dans_chaine = true,
                '{' => pile.push('}'),
                '[' => pile.push(']'),
                '}' | ']' => {
                    pile.pop();
                }
                _ => {}
            }
        }
    }
    if pile.is_empty() && !dans_chaine {
        return None;
    }
    if dans_chaine {
        sortie.push('"');
    }
    // Une clé sans valeur (`"cle":` ou `"cle"`) ou une virgule pendante.
    loop {
        let t = sortie.trim_end();
        if let Some(sans) = t.strip_suffix(',') {
            sortie = sans.to_string();
        } else if let Some(sans) = t.strip_suffix(':') {
            let sans = sans.trim_end();
            match sans.rfind('"').and_then(|fin| sans[..fin].rfind('"')) {
                Some(ouvrante) => sortie = sans[..ouvrante].to_string(),
                None => return None,
            }
        } else {
            sortie = t.to_string();
            break;
        }
    }
    while let Some(f) = pile.pop() {
        sortie.push(f);
    }
    Some(sortie)
}

/// Décode et valide le JSON rendu par le modèle — extrait pour se tester sans
/// réseau, même patron que `lastfm::tags_de`.
fn interpretation_de(brut: &str) -> crate::Result<InterpretationLlm> {
    let mut interpretation: InterpretationLlm = match serde_json::from_str(brut) {
        Ok(p) => p,
        Err(e) => {
            // Coupé net par le plafond de jetons ? On tente de le refermer
            // avant de renoncer — l'erreur d'origine reste celle qu'on rend.
            let reparee = reparer_json_tronque(brut)
                .and_then(|r| serde_json::from_str::<InterpretationLlm>(&r).ok());
            match reparee {
                Some(p) => {
                    tracing::warn!("champ d'intention : réponse d'Ollama coupée, refermée");
                    p
                }
                None => {
                    // Sans les espaces d'une boucle de décodage : lisible dans un message.
                    let court: String = brut.split_whitespace().collect::<Vec<_>>().join(" ");
                    return Err(Error::Parsing(format!(
                        "JSON d'Ollama hors-schéma : {e} — « {court} »"
                    )));
                }
            }
        }
    };

    interpretation.normaliser();
    interpretation.etapes.retain(|e| !e.trim().is_empty());
    if interpretation.etapes.is_empty() {
        return Err(Error::Parsing(
            "aucune étape exploitable extraite du prompt".to_string(),
        ));
    }
    Ok(interpretation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpretation_de_lit_un_plan_complet() {
        let brut = r#"{
            "seed_artiste": "Soul Coughing",
            "seed_morceau": null,
            "etapes": ["a song with a strong focus on drums", "drum and bass with fast breakbeat drum patterns"],
            "n": 20
        }"#;
        let p = interpretation_de(brut).expect("plan valide");
        assert_eq!(p.seed_artiste.as_deref(), Some("Soul Coughing"));
        assert_eq!(p.seed_morceau, None);
        assert_eq!(p.etapes.len(), 2);
        assert_eq!(p.n, Some(20));
    }

    #[test]
    fn interpretation_de_accepte_labsence_de_seed() {
        let brut = r#"{"seed_artiste": null, "seed_morceau": null, "etapes": ["a piano ballad"], "n": null}"#;
        let p = interpretation_de(brut).expect("plan valide sans seed");
        assert_eq!(p.seed_artiste, None);
        assert_eq!(p.n, None);
    }

    /// Régression : « de X à Y en passant par Z » doit distinguer le départ
    /// de l'arrivée dans deux champs séparés — la confusion des deux (observée
    /// en usage réel sur un vrai modèle, voir `interpreter_distingue_...`
    /// ci-dessous) est justement ce que ce schéma existe pour éviter.
    #[test]
    fn interpretation_de_lit_un_depart_et_une_arrivee_distincts() {
        let brut = r#"{
            "seed_artiste": "Shootyz Groove", "seed_morceau": null,
            "arrivee_artiste": "RATM", "arrivee_morceau": null,
            "etapes": ["hip hop with a strong groove"], "n": null
        }"#;
        let p = interpretation_de(brut).expect("plan valide");
        assert_eq!(p.seed_artiste.as_deref(), Some("Shootyz Groove"));
        assert_eq!(p.arrivee_artiste.as_deref(), Some("RATM"));
    }

    /// Un modèle qui suit encore l'ancien schéma (sans les clés `arrivee_*`)
    /// ne doit pas faire échouer toute l'interprétation — `#[serde(default)]`
    /// les retombe sur `None`, comme une arrivée non demandée.
    #[test]
    fn interpretation_de_accepte_labsence_des_cles_darrivee() {
        let brut = r#"{"seed_artiste": null, "seed_morceau": null, "etapes": ["a piano ballad"], "n": null}"#;
        let p = interpretation_de(brut).expect("plan valide sans les clés d'arrivée");
        assert_eq!(p.arrivee_artiste, None);
        assert_eq!(p.arrivee_morceau, None);
    }

    /// Régression : une liste d'étapes vide (ou pleine de chaînes vides) ne
    /// doit jamais atteindre le graphe — `chemin::guidee` n'aurait rien à y
    /// faire, et composer quand même produirait une marche non guidée sans
    /// que l'échec soit visible.
    #[test]
    fn interpretation_de_refuse_une_liste_detapes_vide() {
        let brut = r#"{"seed_artiste": null, "seed_morceau": null, "etapes": [], "n": null}"#;
        assert!(matches!(interpretation_de(brut), Err(Error::Parsing(_))));
    }

    #[test]
    fn interpretation_de_refuse_des_etapes_toutes_vides() {
        let brut = r#"{"seed_artiste": null, "seed_morceau": null, "etapes": ["  ", ""], "n": null}"#;
        assert!(matches!(interpretation_de(brut), Err(Error::Parsing(_))));
    }

    /// « Une playlist calme de 60 minutes, sans rock, années 70 » : tout ce
    /// que l'ancien schéma perdait (durée, exclusion, époque, niveau
    /// d'énergie) doit arriver intact dans les filtres.
    #[test]
    fn interpretation_de_lit_les_filtres_et_la_duree() {
        let brut = r#"{
            "seed_artiste": null, "seed_morceau": null,
            "arrivee_artiste": null, "arrivee_morceau": null,
            "etapes": ["calm, relaxing music"], "n": null, "duree_minutes": 60,
            "genres": [], "exclure_genres": ["rock"], "exclure_artistes": [],
            "annee_min": 1970, "annee_max": 1979, "bpm_min": null, "bpm_max": null,
            "energie": "calme", "popularite": null, "plafond_par_artiste": null
        }"#;
        let p = interpretation_de(brut).expect("plan valide");
        assert_eq!(p.duree_minutes, Some(60));
        let f = p.filtres();
        assert_eq!(f.exclure_genres, vec!["rock".to_string()]);
        assert_eq!((f.annee_min, f.annee_max), (Some(1970), Some(1979)));
        assert_eq!(f.energie, Some(NiveauEnergie::Calme));
        assert_eq!(f.popularite, None);
    }

    /// Un plan à l'ancien schéma (sans aucun des nouveaux champs) reste lisible
    /// et ne restreint rien.
    #[test]
    fn interpretation_de_sans_filtres_rend_des_filtres_vides() {
        let brut = r#"{"seed_artiste": null, "seed_morceau": null, "etapes": ["a piano ballad"], "n": 10}"#;
        let p = interpretation_de(brut).expect("plan valide");
        assert!(p.filtres().est_vide());
        assert_eq!(p.duree_minutes, None);
    }

    #[test]
    fn la_normalisation_ecarte_les_valeurs_absurdes() {
        let brut = r#"{
            "etapes": ["x"], "n": 0, "duree_minutes": 0,
            "genres": ["Jazz", " ", "jazz"], "exclure_genres": ["JAZZ"],
            "annee_min": 1979, "annee_max": 1970, "bpm_min": 500, "bpm_max": 120,
            "energie": "bof", "plafond_par_artiste": 0
        }"#;
        let p = interpretation_de(brut).expect("plan valide");
        assert_eq!(p.n, None);
        assert_eq!(p.duree_minutes, None);
        assert_eq!(p.plafond_par_artiste, None);
        assert!(p.genres.is_empty(), "un genre exclu ne peut pas être aussi voulu : {:?}", p.genres);
        assert_eq!((p.annee_min, p.annee_max), (Some(1970), Some(1979)), "bornes remises dans l'ordre");
        assert_eq!((p.bpm_min, p.bpm_max), (None, Some(120.0)), "500 BPM est absurde");
        assert_eq!(p.filtres().energie, None, "niveau inconnu ignoré, pas une erreur");
    }

    /// « De X à RATM en passant par du hip hop » : le genre est une étape du
    /// trajet, pas un filtre — mais une exclusion tient toujours. Seule une
    /// arrivée fait un trajet.
    #[test]
    fn un_trajet_ne_garde_pas_de_genre_filtrant() {
        let brut = r#"{"seed_artiste": "X", "arrivee_artiste": "Rage Against The Machine",
                        "etapes": ["hip hop"], "genres": ["hip hop"], "exclure_genres": ["country"]}"#;
        let p = interpretation_de(brut).expect("plan valide");
        assert!(p.genres.is_empty());
        assert_eq!(p.exclure_genres, vec!["country".to_string()]);
        // Sans arrivée, le genre voulu est gardé — avec ou sans départ nommé.
        let q = interpretation_de(r#"{"etapes": ["jazz"], "genres": ["jazz"]}"#).unwrap();
        assert_eq!(q.genres, vec!["jazz".to_string()]);
        let d = interpretation_de(r#"{"seed_artiste": "Ella Mai", "etapes": ["r&b"], "genres": ["r&b"]}"#).unwrap();
        assert_eq!(d.genres, vec!["r&b".to_string()], "un départ seul n'est pas un trajet");
    }

    #[test]
    fn le_modele_par_defaut_est_le_plus_petit() {
        let installes = vec![
            ("gros:27b".to_string(), Some(18_000_000_000)),
            ("inconnu".to_string(), None),
            ("petit:4b".to_string(), Some(9_000_000_000)),
        ];
        assert_eq!(trier_par_taille(installes), vec!["petit:4b", "gros:27b", "inconnu"]);
    }

    /// Mesuré sur gemma4:e4b-mlx : une ambiance ou un usage devenait des
    /// filtres de genre que personne n'avait écrits.
    #[test]
    fn un_genre_que_le_texte_ne_nomme_pas_est_ecarte() {
        let brut = r#"{"etapes": ["x"], "genres": ["electronic", "hard rock"],
                        "exclure_genres": ["heavy metal"]}"#;
        let mut p = interpretation_de(brut).unwrap();
        let ecartes = p.ancrer_genres("De la musique énergique pour faire du sport");
        assert!(p.genres.is_empty() && p.exclure_genres.is_empty(), "{p:?}");
        assert_eq!(ecartes.len(), 3);
        // Nommés, ils restent — avec accents, casse, synonymes français et variantes.
        for (genre, texte) in [
            ("electronic", "Un peu d'ÉLECTRONIQUE"),
            ("classical", "de la musique classique"),
            ("hip hop", "du rap"),
            ("rap", "du hip hop"),
            ("hip hop", "du hip-hop"),
            ("world music", "des musiques du monde"),
            ("rock", "sans rock"),
            ("metal", "du métal"),
        ] {
            let mut p = interpretation_de(r#"{"etapes": ["x"]}"#).unwrap();
            p.genres = vec![genre.into()];
            assert!(p.ancrer_genres(texte).is_empty(), "{genre} / {texte}");
            assert_eq!(p.genres.len(), 1);
        }
        // Un mot court ne s'ancre pas dans un mot plus long.
        let mut p = interpretation_de(r#"{"etapes": ["x"]}"#).unwrap();
        p.genres = vec!["rap".into(), "pop".into()];
        let ecartes = p.ancrer_genres("un rythme rapide et populaire");
        assert_eq!(ecartes.len(), 2, "{ecartes:?}");
        // Les genres des parties sont aussi vérifiés.
        let mut p = interpretation_de(
            r#"{"etapes": ["x"], "parties": [
                {"description": "a", "genres": ["rock"]}, {"description": "b", "genres": ["jazz"]}]}"#,
        )
        .unwrap();
        let ecartes = p.ancrer_genres("du rock puis de la douceur");
        assert_eq!(ecartes, vec!["jazz".to_string()]);
    }

    #[test]
    fn une_faute_de_frappe_ne_fait_pas_perdre_le_genre() {
        let mut p = interpretation_de(r#"{"etapes": ["x"]}"#).unwrap();
        p.exclure_genres = vec!["rock".into()];
        assert!(p.ancrer_genres("plylist calm de 1h san roc").is_empty());
        assert!(a_une_faute_pres("roc", "rock") && a_une_faute_pres("jaz", "jazz"));
        assert!(a_une_faute_pres("rcck", "rock") && !a_une_faute_pres("rap", "rock"));
        assert!(!a_une_faute_pres("rouge", "rock"));
    }

    /// « Du folk puis de l'électronique » rendu en filtre commun par le
    /// modèle : le « puis » en fait une suite, dans l'ordre du texte.
    #[test]
    fn un_puis_entre_deux_genres_nommes_donne_des_parties() {
        let mut p = interpretation_de(
            r#"{"etapes": ["x"], "genres": ["electronic", "folk"], "duree_minutes": 40}"#,
        )
        .unwrap();
        let prompt = "Du folk puis de l'électronique, 40 minutes en tout";
        p.ancrer_genres(prompt);
        assert!(p.inferer_parties(prompt));
        let genres: Vec<&str> = p.parties.iter().map(|x| x.genres[0].as_str()).collect();
        assert_eq!(genres, ["folk", "electronic"], "dans l'ordre du texte, pas celui du modèle");
        assert!(p.genres.is_empty());
        assert!(p.parties.iter().all(|x| x.duree_minutes == Some(20)), "{:?}", p.parties);
        // Sans mot de séquence : un mélange, pas des parties.
        let mut q = interpretation_de(r#"{"etapes": ["x"], "genres": ["rock", "jazz"]}"#).unwrap();
        assert!(!q.inferer_parties("Du rock et du jazz"));
        assert!(q.parties.is_empty() && q.genres.len() == 2);
        // Déjà des parties : rien à faire.
        let mut r = interpretation_de(
            r#"{"etapes": ["x"], "genres": ["rock", "jazz"],
                "parties": [{"description": "a"}, {"description": "b"}]}"#,
        )
        .unwrap();
        assert!(!r.inferer_parties("du rock puis du jazz"));
    }

    #[test]
    fn les_genres_inconnus_de_la_bibliotheque_sont_ecartes() {
        let brut = r#"{"etapes": ["x"], "genres": ["Jazz", "zouglou"], "exclure_genres": ["rock", "trapcore"]}"#;
        let mut p = interpretation_de(brut).expect("plan valide");
        let vocab = vec!["jazz".to_string(), "rock".to_string()];
        let ecartes = p.restreindre_au_vocabulaire(&vocab);
        assert_eq!(p.genres, vec!["Jazz".to_string()]);
        assert_eq!(p.exclure_genres, vec!["rock".to_string()]);
        assert_eq!(ecartes, vec!["zouglou".to_string(), "trapcore".to_string()]);
        // Sans vocabulaire, rien n'est écarté.
        let mut q = interpretation_de(brut).unwrap();
        assert!(q.restreindre_au_vocabulaire(&[]).is_empty());
        assert_eq!(q.genres.len(), 2);
    }

    #[test]
    fn la_consigne_montre_le_vocabulaire_reel() {
        let vocab: Vec<String> = ["jazz", "dub"].iter().map(|s| s.to_string()).collect();
        let s = systeme(&vocab);
        assert!(s.contains("\"jazz\", \"dub\""));
        assert!(!s.contains("{GENRES}"));
        assert!(systeme(&[]).contains("aucune liste"));
    }

    /// Le schéma passé à Ollama et la structure lue côté Rust doivent rester
    /// d'accord : une clé ajoutée à l'un et oubliée dans l'autre se paierait
    /// en silence (clé jamais rendue, ou jamais lue).
    #[test]
    fn le_schema_couvre_exactement_les_champs_lus() {
        let s = schema();
        let mut cles: Vec<String> =
            s["properties"].as_object().unwrap().keys().cloned().collect();
        let mut requises: Vec<String> = s["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        cles.sort();
        requises.sort();
        assert_eq!(cles, requises, "toute clé du schéma est requise");

        // Une interprétation complète, une valeur par clé du schéma.
        let complet: Value = json!({
            "reformulation": "r", "parties": [],
            "seed_artiste": "a", "seed_morceau": "b", "arrivee_artiste": "c",
            "arrivee_morceau": "d", "etapes": ["e"], "n": 5, "duree_minutes": 5,
            "genres": ["g"], "exclure_genres": ["h"], "exclure_artistes": ["i"],
            "annee_min": 1990, "annee_max": 1999, "bpm_min": 80.0, "bpm_max": 120.0,
            "energie": "calme", "popularite": "connu", "plafond_par_artiste": 2
        });
        let mut lues: Vec<String> = complet.as_object().unwrap().keys().cloned().collect();
        lues.sort();
        assert_eq!(cles, lues, "le schéma décrit toutes les clés de la structure");
        let p: InterpretationLlm = serde_json::from_value(complet).expect("tout se lit");
        assert_eq!(p.plafond_par_artiste, Some(2));
    }

    /// Régression réelle (1ᵉʳ octobre) : sur « Des morceaux des années 80 », le
    /// modèle écrit la spec, puis des espaces à l'infini à la place de la clé
    /// requise suivante — coupé par le plafond de jetons.
    #[test]
    fn interpretation_de_referme_une_spec_coupee_apres_des_espaces() {
        let brut = format!(
            "{{\n  \"annee_max\": 1989,\n  \"annee_min\": 1980,\n  \"etapes\": [\"music from the 1980s\"],\n  \"n\": null,\n  \"plafond_par_artiste\": null\n{}",
            "  \n".repeat(300)
        );
        let p = interpretation_de(&brut).expect("réparable");
        assert_eq!((p.annee_min, p.annee_max), (Some(1980), Some(1989)));
        assert_eq!(p.etapes, vec!["music from the 1980s".to_string()]);
    }

    #[test]
    fn la_reparation_couvre_chaine_cle_et_virgule_pendantes() {
        // Coupé dans une chaîne.
        let p = interpretation_de(r#"{"etapes": ["calm, relax"#).expect("chaîne refermée");
        assert_eq!(p.etapes, vec!["calm, relax".to_string()]);
        // Coupé après une virgule.
        let p = interpretation_de(r#"{"etapes": ["a"], "n": 5,"#).expect("virgule retirée");
        assert_eq!(p.n, Some(5));
        // Coupé entre une clé et sa valeur.
        let p = interpretation_de(r#"{"etapes": ["a"], "n":"#).expect("clé orpheline retirée");
        assert_eq!(p.n, None);
        // Coupé dans un tableau imbriqué, avec un guillemet échappé.
        let p = interpretation_de(r#"{"etapes": ["il dit \"ok\"", "b"#).expect("tableau refermé");
        assert_eq!(p.etapes.len(), 2);
        // Rien à réparer : l'erreur d'origine reste une erreur.
        assert!(matches!(interpretation_de("pas du json"), Err(Error::Parsing(_))));
        // Réparable en JSON mais sans étapes : refusé comme avant.
        assert!(matches!(interpretation_de(r#"{"n": 5,"#), Err(Error::Parsing(_))));
    }

    /// « Rock pendant 12 minutes, puis hip hop pendant 20 minutes » : avant les
    /// parties, le modèle rendait une étape « a transition from rock to hip
    /// hop », perdait les durées et faisait de « rock, hip hop » un filtre
    /// commun.
    #[test]
    fn des_parties_gardent_leurs_genres_et_leurs_durees() {
        let brut = r#"{
            "reformulation": "Douze minutes de rock, puis vingt de hip hop.",
            "etapes": ["rock music", "hip hop music"], "genres": ["rock", "hip hop"],
            "duree_minutes": 32,
            "parties": [
              {"description": "rock music", "genres": ["rock"], "energie": null, "duree_minutes": 12, "n": null},
              {"description": "hip hop music", "genres": ["hip hop"], "energie": null, "duree_minutes": 20, "n": null}
            ]
        }"#;
        let p = interpretation_de(brut).expect("plan valide");
        assert_eq!(p.parties.len(), 2);
        assert_eq!(p.parties[0].duree_minutes, Some(12));
        assert_eq!(p.parties[1].genres, vec!["hip hop".to_string()]);
        assert!(p.genres.is_empty(), "un genre « puis » n'est pas un filtre commun");
        assert_eq!(p.duree_minutes, None, "la durée totale est la somme des parties");
        assert_eq!(p.etapes, vec!["rock music".to_string(), "hip hop music".to_string()]);
        assert!(p.reformulation.as_deref().is_some_and(|r| r.contains("rock")));
    }

    #[test]
    fn une_partie_unique_se_ramene_aux_champs_globaux() {
        let brut = r#"{"etapes": ["x"], "parties": [
            {"description": "calm music", "genres": ["jazz"], "energie": "calme", "duree_minutes": 30, "n": null}]}"#;
        let p = interpretation_de(brut).expect("plan valide");
        assert!(p.parties.is_empty());
        assert_eq!(p.genres, vec!["jazz".to_string()]);
        assert_eq!(p.duree_minutes, Some(30));
        assert_eq!(p.filtres().energie, Some(NiveauEnergie::Calme));
    }

    #[test]
    fn la_taille_globale_se_partage_entre_des_parties_sans_taille() {
        let brut = r#"{"etapes": ["x"], "duree_minutes": 60, "parties": [
            {"description": "a"}, {"description": "b"}, {"description": "c"}]}"#;
        let p = interpretation_de(brut).expect("plan valide");
        assert_eq!(p.parties.len(), 3);
        assert!(p.parties.iter().all(|x| x.duree_minutes == Some(20)));
        assert_eq!(p.duree_minutes, None);
        let brut = r#"{"etapes": ["x"], "n": 10, "parties": [{"description": "a"}, {"description": "b"}]}"#;
        let p = interpretation_de(brut).unwrap();
        assert!(p.parties.iter().all(|x| x.n == Some(5)));
    }

    #[test]
    fn les_parties_vides_ou_en_trop_sont_ecartees() {
        let mut parties = String::new();
        for i in 0..9 {
            parties.push_str(&format!(r#"{{"description": "p{i}", "duree_minutes": 5}},"#));
        }
        let brut = format!(
            r#"{{"etapes": ["x"], "parties": [{{"description": "  "}}, {{"genres": []}}, {parties} {{"description": "dernière"}}]}}"#
        );
        let p = interpretation_de(&brut).expect("plan valide");
        assert_eq!(p.parties.len(), PARTIES_MAX);
        assert!(p.parties.iter().all(|x| !x.description.is_empty()));
        // Description absente mais genre présent : exploitable.
        let q = interpretation_de(
            r#"{"etapes": ["x"], "parties": [{"genres": ["jazz"]}, {"description": "b"}]}"#,
        )
        .unwrap();
        assert_eq!(q.parties[0].description, "jazz music");
    }

    #[test]
    fn les_genres_des_parties_sont_valides_contre_le_vocabulaire() {
        let mut p = interpretation_de(
            r#"{"etapes": ["x"], "parties": [
                {"description": "a", "genres": ["rock", "zouglou"]}, {"description": "b", "genres": ["jazz"]}]}"#,
        )
        .unwrap();
        let ecartes = p.restreindre_au_vocabulaire(&["rock".into(), "jazz".into()]);
        assert_eq!(ecartes, vec!["zouglou".to_string()]);
        assert_eq!(p.parties[0].genres, vec!["rock".to_string()]);
    }

    /// Le schéma garde l'ordre voulu (`preserve_order`) : `reformulation` en
    /// tête — c'est ce qui lui permet de « comprendre » avant de remplir le reste.
    #[test]
    fn le_schema_ecrit_la_reformulation_en_premier_et_les_parties_en_dernier() {
        let s = schema();
        let cles: Vec<&String> = s["properties"].as_object().unwrap().keys().collect();
        assert_eq!(cles.first().map(|c| c.as_str()), Some("reformulation"));
        assert_eq!(cles.last().map(|c| c.as_str()), Some("parties"));
    }

    #[test]
    fn interpretation_de_refuse_un_json_hors_schema() {
        let brut = r#"{"pas": "le bon schéma"}"#;
        assert!(matches!(interpretation_de(brut), Err(Error::Parsing(_))));
    }

    /// Contre une vraie instance Ollama locale, pas simulée — ignoré par
    /// défaut (CI n'a pas Ollama, ce n'est pas une dépendance du projet) :
    /// `cargo test -p rusty-music-core ollama:: -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn interpreter_fonctionne_contre_un_vrai_ollama() {
        let modele = modele_par_defaut(HOTE_DEFAUT).expect("un modèle installé");
        eprintln!("modèle : {modele}");
        let p = interpreter(
            HOTE_DEFAUT,
            &modele,
            "Partir d'un morceau de Soul Coughing. Poursuivre avec un focus sur la \
             batterie, en faisant des liens entre morceaux avec des motifs de batterie \
             allant vers la drum and bass. 20 morceaux.",
            &[],
        )
        .expect("Ollama doit répondre");
        eprintln!("{p:#?}");
        assert!(!p.etapes.is_empty());
    }

    /// Les prompts qui échouaient avec l'ancien schéma — durée, exclusion,
    /// époque, niveau d'énergie — contre un vrai Ollama. `OLLAMA_MODELE`
    /// choisit le modèle (sinon le premier installé) ; `OLLAMA_VOCAB` donne
    /// des genres séparés par des virgules. Ignoré par défaut.
    /// `OLLAMA_MODELE=gemma4:e4b-mlx cargo test -p rusty-music-core
    /// interpreter_extrait_les_filtres -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn interpreter_extrait_les_filtres_sur_un_vrai_ollama() {
        let modele = std::env::var("OLLAMA_MODELE")
            .ok()
            .or_else(|| modele_par_defaut(HOTE_DEFAUT).ok())
            .expect("un modèle installé");
        let vocab: Vec<String> = std::env::var("OLLAMA_VOCAB")
            .unwrap_or_else(|_| "rock,jazz,hip hop,electronic,pop,classical,reggae,metal,folk,soul".into())
            .split(',')
            .map(|g| g.trim().to_string())
            .collect();
        eprintln!("modèle : {modele}");
        for prompt in [
            "Une playlist calme de 60 minutes, sans rock, années 70",
            "Des morceaux de jazz peu connus, pas plus de 2 par artiste, rythme lent",
            "Une heure de musique énergique pour courir, sans Prince",
            "Partir de Shootyz Groove. Arriver à RATM. En passant par du hip hop",
            "Une playlist pour travailler",
        ] {
            let debut = std::time::Instant::now();
            let p = interpreter(HOTE_DEFAUT, &modele, prompt, &vocab);
            eprintln!("\n« {prompt} » ({} s)\n{p:#?}\nfiltres : {:?}", debut.elapsed().as_secs(), p.as_ref().ok().map(|p| p.filtres()));
            assert!(p.is_ok(), "{p:?}");
        }
    }

    /// Régression réelle (14 septembre 2026) : « Partir de Shootyz Groove.
    /// Arriver à RATM. En passant par du hip hop » confondait départ et
    /// arrivée avant l'ajout de `arrivee_artiste`/`arrivee_morceau` — un
    /// modèle mettait l'arrivée dans `seed_artiste`, un autre abandonnait le
    /// départ. Ignoré par défaut, comme le test ci-dessus.
    #[test]
    #[ignore]
    fn interpreter_distingue_depart_et_arrivee_sur_un_vrai_ollama() {
        let modele = modele_par_defaut(HOTE_DEFAUT).expect("un modèle installé");
        eprintln!("modèle : {modele}");
        let p = interpreter(
            HOTE_DEFAUT,
            &modele,
            "Partir de Shootyz Groove. Arriver à RATM. En passant par du hip hop",
            &[],
        )
        .expect("Ollama doit répondre");
        eprintln!("{p:#?}");
        assert_eq!(
            p.seed_artiste.as_deref(),
            Some("Shootyz Groove"),
            "le départ doit être Shootyz Groove, pas confondu avec l'arrivée"
        );
        // Pas juste "reconnue" : « RATM » tel quel ne trouve jamais « Rage
        // Against The Machine » dans une bibliothèque (recherche par préfixe
        // littéral, `apps/desktop/src/main.rs::resoudre_piste_nommee`) — la
        // régression réelle du 14 septembre, silencieuse jusqu'à l'écoute.
        assert_eq!(
            p.arrivee_artiste.as_deref(),
            Some("Rage Against The Machine"),
            "l'arrivée doit être développée en nom complet, pas laissée en sigle"
        );
    }
}
