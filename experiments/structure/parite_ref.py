# SPDX-License-Identifier: GPL-3.0-or-later
"""Référence de parité pour le portage Burn d'all-in-one : sur un dossier de
stems (`bass.wav`, `drums.wav`, `other.wav`, `vocals.wav`), éventuellement
coupés aux N premières secondes, écrit dans `<sortie>` :
- les stems coupés (entrée commune aux deux côtés) ;
- `spec.f32` : spectrogramme de madmom, `[4, T, 81]` ;
- `pli0-<sortie>.f32` et `ensemble-<sortie>.f32` : logits `beat`,
  `downbeat`, `section` `[T]` et `function` `[10, T]` ;
- `reference.json` : forme, segments de l'ensemble.

    PYTHONPATH=natten_pur <env>/bin/python parite_ref.py <stems> <sortie> [--secondes N]
"""
import argparse, json, os, sys
import numpy as np
import soundfile as sf
import torch

ICI = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(ICI, "natten_pur"))
DEV = os.path.expanduser(os.environ.get("RUSTY_DEV", "/Volumes/LaCieDock/rusty_music-dev"))
os.environ.setdefault("HF_HUB_CACHE", os.path.join(DEV, "hf", "hub"))
STEMS = ["bass", "drums", "other", "vocals"]

def ecrire(chemin, x):
    np.ascontiguousarray(x, dtype="<f4").tofile(chemin)

def main():
    a = argparse.ArgumentParser()
    a.add_argument("stems"); a.add_argument("sortie")
    a.add_argument("--secondes", type=float)
    args = a.parse_args()
    from allin1.spectrogram import extract_spectrograms
    from allin1.models import load_pretrained_model
    from allin1.postprocessing import postprocess_functional_structure
    os.makedirs(args.sortie, exist_ok=True)
    dossier = os.path.join(args.sortie, "stems")
    os.makedirs(dossier, exist_ok=True)
    for s in STEMS:
        x, sr = sf.read(os.path.join(args.stems, f"{s}.wav"), dtype="int16", always_2d=True)
        if args.secondes: x = x[: int(args.secondes * sr)]
        sf.write(os.path.join(dossier, f"{s}.wav"), x, sr, subtype="PCM_16")
    from pathlib import Path
    spec_path = extract_spectrograms([Path(dossier)], Path(args.sortie), multiprocess=False)[0]
    spec = np.load(spec_path)
    ecrire(os.path.join(args.sortie, "spec.f32"), spec)
    x = torch.from_numpy(spec).unsqueeze(0)
    with torch.no_grad():
        for nom, modele in (("pli0", load_pretrained_model("harmonix-fold0")), ("ensemble", load_pretrained_model("harmonix-all"))):
            sortie = modele(x)
            for champ in ("beat", "downbeat", "section", "function"):
                ecrire(os.path.join(args.sortie, f"{nom}-{champ}.f32"), getattr(sortie, f"logits_{champ}")[0].numpy())
            if nom == "ensemble":
                segments = [dict(debut=s.start, fin=s.end, etiquette=s.label) for s in postprocess_functional_structure(sortie, modele.cfg)]
    json.dump(dict(forme=list(spec.shape), segments=segments), open(os.path.join(args.sortie, "reference.json"), "w"), indent=1)
    print(spec.shape, len(segments), "segments")

if __name__ == "__main__":
    main()
