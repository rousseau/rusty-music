// SPDX-License-Identifier: GPL-3.0-or-later
//! Un modèle de langue exécuté dans le processus : le second moteur de Lama,
//! à côté d'Ollama (`docs/recherche-lama-sans-ollama.md`).
//!
//! llama.cpp, par `llama-cpp-2` (MIT ou Apache-2.0), sur un GGUF. Même
//! consigne ([`ollama::systeme`]), même schéma ([`ollama::schema`], traduit en
//! grammaire par llama.cpp), même lecture de la réponse
//! ([`ollama::depuis_reponse`]) : seule la génération change. Feature
//! `lama-local`.

use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::session::{LlamaStateSeqFlags, SeqState};
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use llguidance::api::TopLevelGrammar;
use llguidance::{Matcher, ParserFactory};

use crate::error::Error;
use crate::ollama::{self, InterpretationLlm};

/// Mêmes bornes que la requête à Ollama (`ollama::interpreter`) : la consigne,
/// le schéma et la réponse tiennent dans ce contexte, et une spec complète
/// dans ce nombre de jetons.
const CONTEXTE: u32 = 6144;
const JETONS_MAX: usize = 700;

fn erreur(contexte: &str) -> impl Fn(String) -> Error + '_ {
    move |e| Error::ModeleLocal(format!("{contexte} : {e}"))
}

/// Fait lire `jetons` au contexte, à partir de la position `debut` ; `logits`
/// demande les probabilités du dernier, d'où part la génération.
fn lire(
    ctx: &mut LlamaContext<'_>,
    lot: &mut LlamaBatch,
    jetons: &[LlamaToken],
    debut: usize,
    logits: bool,
) -> crate::Result<()> {
    lot.clear();
    let dernier = jetons.len() - 1;
    for (i, &j) in jetons.iter().enumerate() {
        lot.add(j, (debut + i) as i32, &[0], logits && i == dernier)
            .map_err(|e| erreur("lot")(e.to_string()))?;
    }
    ctx.decode(lot)
        .map_err(|e| erreur("lecture de la demande")(e.to_string()))
}

/// Un modèle chargé, prêt à interpréter. Le chargement coûte (plusieurs Go
/// lus, puis copiés vers le processeur graphique) : on le garde d'une demande
/// à l'autre.
pub struct ModeleLocal {
    modele: LlamaModel,
    backend: LlamaBackend,
    /// De quoi contraindre le décodage par le schéma de [`ollama::schema`]
    /// (llguidance) : le modèle ne peut écrire que du JSON conforme. Préféré
    /// à la grammaire GBNF de llama.cpp : mêmes réponses, 5,3 s de médiane au
    /// lieu de 8,9 s (`docs/recherche-lama-sans-ollama.md`).
    contraintes: ParserFactory,
    /// La consigne déjà lue : ses jetons, et l'état du contexte après eux.
    /// Elle fait 2 600 jetons (60 genres, règles, schéma) et ne change
    /// qu'avec le vocabulaire de la bibliothèque ; la relire à chaque demande
    /// coûtait 4 à 5 s sur M4 Pro, la restaurer ne coûte presque rien.
    prefixe: Mutex<Option<(Vec<LlamaToken>, SeqState)>>,
}

impl ModeleLocal {
    /// Charge un GGUF, toutes ses couches sur le processeur graphique quand il
    /// y en a un (Metal sous macOS).
    pub fn charger(chemin: &Path) -> crate::Result<Self> {
        let mut backend =
            LlamaBackend::init().map_err(|e| erreur("initialisation")(e.to_string()))?;
        backend.void_logs();
        let params = LlamaModelParams::default().with_n_gpu_layers(u32::MAX);
        let modele = LlamaModel::load_from_file(&backend, chemin, &params)
            .map_err(|e| erreur("chargement")(format!("{} ({e})", chemin.display())))?;
        let contraintes = ParserFactory::new_simple(&LlamaSampler::llguidance_tok_env(&modele))
            .map_err(|e| erreur("vocabulaire pour llguidance")(e.to_string()))?;
        Ok(Self {
            modele,
            backend,
            contraintes,
            prefixe: Mutex::new(None),
        })
    }

    /// Le JSON que le modèle écrit pour `prompt`, contraint par le schéma, à
    /// température 0 (choix glouton) : le même texte rend la même réponse.
    pub fn generer(&self, systeme: &str, prompt: &str) -> crate::Result<String> {
        let texte = self.conversation(systeme, prompt)?;
        let vocab = self.modele.vocab();
        // Le gabarit écrit déjà les jetons spéciaux (`bos_token` compris) :
        // on les lit, on n'en ajoute pas.
        let jetons = vocab.tokenize(texte.as_bytes(), false, true);
        // Ce qui précède la demande : tout ce que la consigne fixe. Gardé à
        // part seulement si ses jetons sont bien le début de ceux du texte
        // entier (la frontière ne coupe pas un jeton).
        let prefixe = texte
            .rfind(prompt)
            .map(|i| vocab.tokenize(&texte.as_bytes()[..i], false, true))
            .filter(|p| !p.is_empty() && p.len() < jetons.len() && jetons.starts_with(p));
        if jetons.len() + JETONS_MAX > CONTEXTE as usize {
            return Err(Error::ModeleLocal(format!(
                "demande trop longue : {} jetons pour un contexte de {CONTEXTE}",
                jetons.len()
            )));
        }

        let params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(CONTEXTE))
            .with_n_batch(CONTEXTE);
        let mut ctx = self
            .modele
            .new_context(&self.backend, params)
            .map_err(|e| erreur("contexte")(e.to_string()))?;

        let t = Instant::now();
        let mut lot = LlamaBatch::new(CONTEXTE as usize, 1);
        let mut deja = 0;
        if let Some(prefixe) = &prefixe {
            let mut cache = self.prefixe.lock().unwrap_or_else(|e| e.into_inner());
            let reprise = match cache.as_ref() {
                Some((j, etat)) if j == prefixe => ctx.state_seq_set(etat, 0).is_ok(),
                _ => false,
            };
            if !reprise {
                lire(&mut ctx, &mut lot, prefixe, 0, false)?;
                let etat = ctx
                    .state_seq_get(0, LlamaStateSeqFlags::empty())
                    .map_err(|e| erreur("état de la consigne")(e.to_string()))?;
                *cache = Some((prefixe.clone(), etat));
            }
            tracing::debug!(reprise, jetons = prefixe.len(), "modèle local : consigne");
            deja = prefixe.len();
        }
        lire(&mut ctx, &mut lot, &jetons[deja..], deja, true)?;
        tracing::debug!(
            jetons = jetons.len() - deja,
            duree = ?t.elapsed(),
            "modèle local : demande lue"
        );
        let t = Instant::now();

        let grammaire = self
            .contraintes
            .create_parser(TopLevelGrammar::from_json_schema(ollama::schema()))
            .map_err(|e| erreur("schéma")(e.to_string()))?;
        let grammaire = LlamaSampler::from(Matcher::new(Ok(grammaire)));
        let mut echantillon = LlamaSampler::chain_simple([grammaire, LlamaSampler::greedy()]);

        let mut sortie = Vec::new();
        let mut position = jetons.len() as i32;
        for _ in 0..JETONS_MAX {
            // `sample` accepte aussi le jeton : la grammaire avance d'elle-même.
            let jeton = echantillon.sample(&ctx, lot.n_tokens() - 1);
            if vocab.is_eog(jeton) {
                break;
            }
            sortie.extend(vocab.token_to_piece(jeton, false, None));
            lot.clear();
            lot.add(jeton, position, &[0], true)
                .map_err(|e| erreur("lot")(e.to_string()))?;
            position += 1;
            ctx.decode(&mut lot)
                .map_err(|e| erreur("génération")(e.to_string()))?;
        }
        tracing::debug!(
            jetons = position as usize - jetons.len(),
            duree = ?t.elapsed(),
            "modèle local : réponse écrite"
        );
        Ok(String::from_utf8_lossy(&sortie).into_owned())
    }

    /// La conversation (consigne, demande, début de réponse) mise en forme par
    /// le gabarit du modèle lui-même (`tokenizer.chat_template` du GGUF).
    ///
    /// Rendu par minijinja : `llama_chat_apply_template` ne reconnaît que des
    /// gabarits connus de llama.cpp, et rend une erreur sur celui de Gemma 4
    /// (un programme Jinja de 18 Ko). Écrire le format à la main, ce serait le
    /// réécrire, et le tenir à jour à chaque modèle.
    fn conversation(&self, systeme: &str, prompt: &str) -> crate::Result<String> {
        let gabarit = self
            .modele
            .chat_template(None)
            .map_err(|e| erreur("gabarit de conversation")(e.to_string()))?;
        let gabarit = gabarit
            .to_string()
            .map_err(|e| erreur("gabarit de conversation")(e.to_string()))?;
        let vocab = self.modele.vocab();
        let bos =
            String::from_utf8_lossy(&vocab.token_to_piece(vocab.bos(), true, None)).into_owned();

        let mut env = minijinja::Environment::new();
        env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);
        env.add_function(
            "raise_exception",
            |m: String| -> Result<String, minijinja::Error> {
                Err(minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    m,
                ))
            },
        );
        env.add_template("conversation", &gabarit)
            .map_err(|e| erreur("gabarit de conversation")(e.to_string()))?;
        env.get_template("conversation")
            .and_then(|t| {
                t.render(minijinja::context! {
                    messages => [
                        minijinja::context! { role => "system", content => systeme },
                        minijinja::context! { role => "user", content => prompt },
                    ],
                    bos_token => bos,
                    add_generation_prompt => true,
                    // Pas de raisonnement avant la réponse : la grammaire veut du
                    // JSON dès le premier jeton (même choix que `"think": false`
                    // côté Ollama).
                    enable_thinking => false,
                })
            })
            .map_err(|e| erreur("gabarit de conversation")(e.to_string()))
    }

    /// Interprète `prompt` comme [`ollama::interpreter`], avec ce modèle.
    pub fn interpreter(
        &self,
        prompt: &str,
        vocabulaire: &[String],
    ) -> crate::Result<InterpretationLlm> {
        tracing::info!(%prompt, "champ d'intention : interrogation du modèle local");
        let brut = self.generer(&ollama::systeme(vocabulaire), prompt)?;
        tracing::info!(brut_json = %brut, "champ d'intention : réponse brute du modèle local");
        ollama::depuis_reponse(&brut, prompt, vocabulaire)
    }
}
