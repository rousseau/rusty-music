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

use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::Error;

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
#[derive(Debug, Clone, Deserialize, PartialEq)]
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
                  texte (« partir de », « commencer par »…), ou null si aucun,
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
  "n": le nombre de morceaux voulu si le texte le précise, sinon null
}

Important pour "seed_artiste" et "arrivee_artiste" : rends toujours le nom
COMPLET, usuel et non ambigu de l'artiste — jamais un sigle, une abréviation
ou un surnom, même si c'est ce que le texte emploie. Ce nom sert ensuite à
chercher l'artiste dans une bibliothèque musicale, où il est rangé sous son
nom complet. Exemples : « RATM » → "Rage Against The Machine" ; « GNR » →
"Guns N' Roses" ; « les Beatles » → "The Beatles".

Exemple : pour « Partir de Shootyz Groove. Arriver à RATM. En passant par du
hip hop », la bonne réponse est :
{
  "seed_artiste": "Shootyz Groove", "seed_morceau": null,
  "arrivee_artiste": "Rage Against The Machine", "arrivee_morceau": null,
  "etapes": ["hip hop with a strong groove"], "n": null
}

"etapes" ne doit jamais être une liste vide : si le texte ne décrit qu'une
seule ambiance, rends une liste à un seul élément."#;

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
pub fn interpreter(hote: &str, modele: &str, prompt: &str) -> crate::Result<InterpretationLlm> {
    let corps = serde_json::to_string(&json!({
        "model": modele,
        "system": SYSTEME,
        "prompt": prompt,
        "format": "json",
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

    let resultat = interpretation_de(brut_json);
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

/// Décode et valide le JSON rendu par le modèle — extrait pour se tester sans
/// réseau, même patron que `lastfm::tags_de`.
fn interpretation_de(brut: &str) -> crate::Result<InterpretationLlm> {
    let mut interpretation: InterpretationLlm = serde_json::from_str(brut).map_err(|e| {
        Error::Parsing(format!("JSON d'Ollama hors-schéma : {e} — « {brut} »"))
    })?;

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
        )
        .expect("Ollama doit répondre");
        eprintln!("{p:#?}");
        assert!(!p.etapes.is_empty());
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
