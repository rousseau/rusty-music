# SPDX-License-Identifier: GPL-3.0-or-later
"""Références de parité pour le portage de MuScriptor en Burn
(`crates/transcription/src/muscriptor/`).

Le code Python d'origine (github.com/muscriptor/muscriptor, MIT), en float32
sur CPU, sur un extrait mono à 16 kHz écrit tel quel pour que le Rust lise
exactement les mêmes échantillons. Écrit dans <sortie>/ :

- `audio16k.f32`      échantillons bruts (float32 petit-boutiste) ;
- `mel.f32`           log-mel du premier segment, [501, 512] ;
- `cond.f32`          préfixe de conditionnement projeté, [prefixe, dim] ;
- `logits.f32`        logits du premier pas (avant masquage), [card] ;
- `jetons.json`       jetons de chaque segment (prélude forcé compris, sans EOS) ;
- `notes.json`        notes décodées [{debut_s, fin_s, hauteur, instrument}].

Poids : ceux de Hugging Face (conditions acceptées par l'utilisateur).

    <venv>/bin/python reference.py <audio> <sortie> [--debut 30] [--duree 15] [--instruments electric_bass,acoustic_bass]
"""
import argparse, json, os
import numpy as np
import torch

def main():
    a = argparse.ArgumentParser()
    a.add_argument("audio"); a.add_argument("sortie")
    a.add_argument("--debut", type=float, default=30.0)
    a.add_argument("--duree", type=float, default=15.0)
    a.add_argument("--taille", default="medium")
    a.add_argument("--instruments", default="electric_bass,acoustic_bass")
    args = a.parse_args()
    os.makedirs(args.sortie, exist_ok=True)

    from muscriptor.transcription_model import TranscriptionModel, _SAMPLE_RATE
    from muscriptor.utils.audio import load_audio
    from muscriptor.tokenizer.mt3 import instrument_group_from_names
    import muscriptor.transcription_model as tm

    modele = TranscriptionModel.load_model(args.taille, device="cpu", dtype="float32")
    wav = load_audio(args.audio, target_sr=_SAMPLE_RATE)
    d0 = int(args.debut * _SAMPLE_RATE)
    wav = wav[:, d0:d0 + int(args.duree * _SAMPLE_RATE)].contiguous()
    wav.numpy().astype("<f4").tofile(os.path.join(args.sortie, "audio16k.f32"))
    instruments = args.instruments.split(",") if args.instruments else None

    # Conditionnement et premier pas, à la main (mêmes appels que generate).
    lm = modele._model
    seg = int(5.0 * _SAMPLE_RATE)
    chunk = wav[:, :seg]
    groupe = instrument_group_from_names(instruments) if instruments else None
    conds = modele._build_conditions(chunk, groupe)
    prep = lm.condition_provider.tokenize(conds)
    mel = lm.condition_provider.conditioners["self_wav"]._mel_embedding(prep["self_wav"])
    mel[0].numpy().astype("<f4").tofile(os.path.join(args.sortie, "mel.f32"))
    ct = lm.condition_provider(prep)
    cond = torch.cat([c for c, _ in reversed(list(ct.values()))], dim=1)  # ordre du préfixe
    cond[0].detach().numpy().astype("<f4").tofile(os.path.join(args.sortie, "cond.f32"))
    with torch.inference_mode():
        seq = torch.full((1, 1), lm.initial_token_id, dtype=torch.long)
        logits = lm(seq, ct, first_step=True)[0, -1]
    logits.numpy().astype("<f4").tofile(os.path.join(args.sortie, "logits.f32"))

    # Jetons par segment : on intercepte le flux de _generate_token_stream.
    from muscriptor.events import ChunkBoundary, ProgressEvent
    segments = []
    orig = modele._generate_token_stream
    def espion(*x, **k):
        for item in orig(*x, **k):
            if isinstance(item, ChunkBoundary): segments.append([])
            elif not isinstance(item, ProgressEvent): segments[-1].append(int(item))
            yield item
    modele._generate_token_stream = espion
    debuts, notes = {}, []
    for e in modele.transcribe((wav, _SAMPLE_RATE), instruments=instruments):
        n = type(e).__name__
        if n == "NoteEndEvent":
            s = e.start_event
            notes.append({"debut_s": s.start_time, "fin_s": e.end_time, "hauteur": s.pitch, "instrument": s.instrument})
    json.dump(segments, open(os.path.join(args.sortie, "jetons.json"), "w"))
    json.dump(sorted(notes, key=lambda n: (n["debut_s"], n["hauteur"])), open(os.path.join(args.sortie, "notes.json"), "w"))
    print(f"prefixe {cond.shape[1]}, {len(segments)} segments, {sum(map(len, segments))} jetons, {len(notes)} notes")

if __name__ == "__main__":
    main()
