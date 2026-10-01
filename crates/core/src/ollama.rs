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
        // Un départ ou une arrivée nommés font de la playlist un trajet : un
        // genre cité « en passant par » en est une étape (déjà dans `etapes`),
        // pas un filtre qui cloisonnerait toute la marche. Observé : même avec
        // la règle dans la consigne, un petit modèle met « hip hop » dans
        // `genres` pour « de X à RATM en passant par du hip hop ».
        let est_trajet = [&self.seed_artiste, &self.seed_morceau, &self.arrivee_artiste, &self.arrivee_morceau]
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
        for liste in [&mut self.genres, &mut self.exclure_genres] {
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

Réponds uniquement par un objet JSON, sans texte autour, avec exactement ces clés :
{
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
  "genres": genres voulus, UNIQUEMENT parmi la liste ci-dessous, et seulement si
            TOUTE la playlist doit rester dans ces genres (« une playlist de
            jazz »). Jamais quand le texte nomme un départ ou une arrivée : un
            genre cité « en passant par » est une étape, pas un filtre. Sinon [],
  "exclure_genres": genres refusés (« sans rock », « pas de rap »), UNIQUEMENT
                    parmi la liste ci-dessous, sinon [],
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
  "plafond_par_artiste": « pas plus de 2 morceaux par artiste » → 2, sinon null
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
  "seed_artiste": "Shootyz Groove", "seed_morceau": null,
  "arrivee_artiste": "Rage Against The Machine", "arrivee_morceau": null,
  "etapes": ["hip hop with a strong groove"], "n": null, "duree_minutes": null,
  "genres": [], "exclure_genres": [], "exclure_artistes": [],
  "annee_min": null, "annee_max": null, "bpm_min": null, "bpm_max": null,
  "energie": null, "popularite": null, "plafond_par_artiste": null
}

Exemple : pour « Une playlist calme de 60 minutes, sans rock, années 70 »
(le genre "rock" est dans la liste) :
{
  "seed_artiste": null, "seed_morceau": null,
  "arrivee_artiste": null, "arrivee_morceau": null,
  "etapes": ["calm, relaxing music"], "n": null, "duree_minutes": 60,
  "genres": [], "exclure_genres": ["rock"], "exclure_artistes": [],
  "annee_min": 1970, "annee_max": 1979, "bpm_min": null, "bpm_max": null,
  "energie": "calme", "popularite": null, "plafond_par_artiste": null
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
    json!({
        "type": "object",
        "properties": {
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
            "energie": { "type": ["string", "null"], "enum": ["calme", "moyenne", "intense", null] },
            "popularite": { "type": ["string", "null"], "enum": ["peu_connu", "connu", null] },
            "plafond_par_artiste": entier_ou_null,
        },
        "required": [
            "seed_artiste", "seed_morceau", "arrivee_artiste", "arrivee_morceau",
            "etapes", "n", "duree_minutes", "genres", "exclure_genres",
            "exclure_artistes", "annee_min", "annee_max", "bpm_min", "bpm_max",
            "energie", "popularite", "plafond_par_artiste"
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
        // Plafond de jetons : une spec complète en tient moins de 300. Sans lui,
        // un décodage contraint qui part en boucle ne s'arrête qu'au délai de
        // 300 s. Observé (gemma4:e4b-mlx, 6 prompts sur 53 dont 4 sur 5 de
        // ceux qui fixent une année) : le modèle écrit une spec correcte, puis
        // des espaces à l'infini au lieu de la clé requise suivante — voir
        // [`reparer_json_tronque`], qui sauve la spec ainsi coupée. Rendre les
        // clés facultatives n'y change rien de bon : le modèle n'en remplit
        // plus qu'une partie (20 prompts conformes sur 53, contre 46).
        //
        // `num_ctx` : consigne + schéma + prompt + réponse tiennent dans 4 096
        // jetons. Le contexte par défaut (131 072 sur les modèles MLX) réserve
        // un cache bien plus gros que nécessaire : observé, le modèle de 27 Go
        // (`qwen3.8:27b-mlx`) plantait en mémoire (« Insufficient Memory »,
        // Metal) sur une machine de 24 Go dès la première interprétation.
        "options": { "temperature": 0, "num_predict": 400, "num_ctx": 4096 },
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
        let ecartes = p.restreindre_au_vocabulaire(vocabulaire);
        if !ecartes.is_empty() {
            tracing::info!(?ecartes, "champ d'intention : genres inconnus de la bibliothèque, écartés");
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

/// Les modèles déjà installés localement (`ollama list`) — sert le
/// sélecteur du champ d'intention (icône 🦙) et [`modele_par_defaut`].
pub fn modeles(hote: &str) -> crate::Result<Vec<String>> {
    let url = format!("{hote}/api/tags");
    let (statut, brut) = requete(&url, TIMEOUT_LISTE, |agent| agent.get(&url).call())?;
    if !(200..300).contains(&statut) {
        return Err(Error::Reseau(format!("Ollama a refusé la requête ({statut}) : {brut}")));
    }
    let v: Value = serde_json::from_str(&brut)
        .map_err(|e| Error::Parsing(format!("réponse d'Ollama illisible : {e}")))?;
    Ok(v["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["name"].as_str().map(str::to_string))
        .collect())
}

/// Le premier modèle installé, faute de choix explicite de l'utilisateur —
/// voir la documentation de [`MODELE_DEFAUT`] sur pourquoi ce n'est pas un nom
/// fixe.
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
    /// trajet, pas un filtre — mais une exclusion tient toujours.
    #[test]
    fn un_trajet_ne_garde_pas_de_genre_filtrant() {
        let brut = r#"{"seed_artiste": "X", "arrivee_artiste": "Rage Against The Machine",
                        "etapes": ["hip hop"], "genres": ["hip hop"], "exclure_genres": ["country"]}"#;
        let p = interpretation_de(brut).expect("plan valide");
        assert!(p.genres.is_empty());
        assert_eq!(p.exclure_genres, vec!["country".to_string()]);
        // Sans départ ni arrivée, le genre voulu est gardé.
        let q = interpretation_de(r#"{"etapes": ["jazz"], "genres": ["jazz"]}"#).unwrap();
        assert_eq!(q.genres, vec!["jazz".to_string()]);
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
