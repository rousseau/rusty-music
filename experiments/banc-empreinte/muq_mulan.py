# SPDX-License-Identifier: GPL-3.0-or-later
"""Empreintes MuQ-MuLan de l'échantillon G1, audio et texte.

Lit les fenêtres que CLAP a analysées (`banc_empreinte fenetres`, 5 × 10 s,
mono 48 kHz, i16), les ramène à 24 kHz, encode chaque fenêtre, puis fait comme
l'application pour CLAP : moyenne des fenêtres, ramenée sur la sphère unité.
La colonne `etape` de `requetes.tsv` (l'anglais que le Lama donne à la tour
texte) passe par la tour texte de MuLan.

Poids : `OpenMuQ/MuQ-MuLan-large`, **CC BY-NC 4.0**, téléchargés à la source
dans le cache Hugging Face (mettre `HF_HOME` sous `RUSTY_DEV`), jamais
republiés. Code `muq` : MIT.

    HF_HOME=$RUSTY_DEV/hf python muq_mulan.py data $RUSTY_DEV/banc-empreinte/fenetres

Écrit `data/muq-mulan.f32`, `data/muq-mulan-texte.f32` et
`data/cout-muq-mulan.txt` (secondes par morceau, hors décodage).
"""

import sys
import time
from pathlib import Path

import numpy as np
import torch
import torchaudio.functional as AF
from muq import MuQMuLan

FENETRES, SR_SOURCE, SR = 5, 48_000, 24_000


def appareil():
    if torch.backends.mps.is_available():
        return "mps"
    return "cuda" if torch.cuda.is_available() else "cpu"


def main(dossier: Path, fenetres: Path):
    lignes = (dossier / "echantillon.tsv").read_text().splitlines()[1:]
    ids = [l.split("\t")[0] for l in lignes]
    requetes = [l.split("\t")[2] for l in (dossier / "requetes.tsv").read_text().splitlines()[1:] if l.strip()]
    dev = appareil()
    mulan = MuQMuLan.from_pretrained("OpenMuQ/MuQ-MuLan-large").to(dev).eval()

    sortie, temps = [], []
    with torch.no_grad():
        for k, i in enumerate(ids):
            brut = np.fromfile(fenetres / f"{i}.i16", dtype="<i2").astype(np.float32) / 32767.0
            blocs = torch.from_numpy(brut.reshape(FENETRES, -1))
            blocs = AF.resample(blocs, SR_SOURCE, SR).to(dev)
            debut = time.perf_counter()
            e = mulan(wavs=blocs)  # (5, 512)
            if dev == "mps":
                torch.mps.synchronize()
            temps.append(time.perf_counter() - debut)
            v = e.float().mean(0)
            sortie.append((v / v.norm()).cpu().numpy())
            if k % 100 == 0:
                print(f"{k}/{len(ids)} · {np.median(temps):.3f} s par morceau", flush=True)
        t = mulan(texts=requetes).float().cpu().numpy()

    np.asarray(sortie, dtype="<f4").tofile(dossier / "muq-mulan.f32")
    t.astype("<f4").tofile(dossier / "muq-mulan-texte.f32")
    (dossier / "cout-muq-mulan.txt").write_text(
        f"{dev} médiane {np.median(temps):.3f} s, moyenne {np.mean(temps):.3f} s par morceau (5 × 10 s)\n"
    )
    print(f"terminé : {len(sortie)} morceaux, {np.median(temps):.3f} s par morceau sur {dev}")


if __name__ == "__main__":
    main(Path(sys.argv[1]), Path(sys.argv[2]))
