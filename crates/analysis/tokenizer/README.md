# Tokeniseur de la tour texte de CLAP

`tokenizer.json` — le tokeniseur RoBERTa de `laion/clap-htsat-unfused`
(Apache-2.0, même dépôt que l'encodeur audio traduit dans `models/`, voir le
module doc de `crate`), tel que `transformers` le publie.

Contrairement aux poids `.bpk` (des centaines de Mo, reconstruits par
`scripts/preparer-*.sh` et gitignorés), ce fichier pèse 2 Mo : un vocabulaire,
pas un poids de modèle — committé comme `crates/analysis/vocabulaire/`, et
embarqué dans le binaire par `include_bytes!`
(`crates/analysis/src/encodeur_texte.rs`), pas lu à l'exécution.

Récupéré une fois depuis le cache Hugging Face après un premier
`scripts/preparer-clap-texte.sh` (`~/.cache/huggingface/hub/models--laion--
clap-htsat-unfused/snapshots/*/tokenizer.json`) ; à ne recopier que si CLAP
change de modèle de référence.
