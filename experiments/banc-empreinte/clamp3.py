# SPDX-License-Identifier: GPL-3.0-or-later
"""Empreintes CLaMP 3 de l'échantillon G1, audio et texte.

On ne réécrit rien : l'outil officiel (`clamp3_embd.py --get_global`, dépôt
github.com/sanderwood/clamp3, code et poids `sander-wood/clamp3` **MIT**) fait
tout. Il passe l'audio par MERT-v1-95M (**CC BY-NC 4.0**) avant la tour audio
de CLaMP 3 : adopter CLaMP 3 voudrait donc dire adopter aussi les termes NC de
MERT. Poids téléchargés à la source, jamais republiés.

Ce script prépare ses entrées puis relit ses sorties :

    python clamp3.py preparer data $RUSTY_DEV/banc-empreinte/fenetres $RUSTY_DEV/banc-empreinte/clamp3
    (cd $RUSTY_DEV/banc-empreinte/clamp3-code && time python clamp3_embd.py ../clamp3/wav ../clamp3/audio --get_global)
    (cd $RUSTY_DEV/banc-empreinte/clamp3-code && python clamp3_embd.py ../clamp3/txt ../clamp3/texte --get_global)
    python clamp3.py relire data $RUSTY_DEV/banc-empreinte/clamp3 <secondes de la 1re commande>

`preparer` écrit, pour chaque morceau, les cinq fenêtres que CLAP a analysées
mises bout à bout (50 s, WAV mono 16 bits, 48 kHz : MERT rééchantillonne lui-même
à 24 kHz), et une phrase par requête (`etape`) en `.txt`.
"""

import sys
import wave
from pathlib import Path

import numpy as np


def preparer(dossier: Path, fenetres: Path, travail: Path):
    (travail / "wav").mkdir(parents=True, exist_ok=True)
    (travail / "txt").mkdir(parents=True, exist_ok=True)
    for l in (dossier / "echantillon.tsv").read_text().splitlines()[1:]:
        i = l.split("\t")[0]
        brut = (fenetres / f"{i}.i16").read_bytes()
        with wave.open(str(travail / "wav" / f"{i}.wav"), "wb") as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(48_000)
            w.writeframes(brut)
    for l in (dossier / "requetes.tsv").read_text().splitlines()[1:]:
        if l.strip():
            q, _, etape = l.split("\t")
            (travail / "txt" / f"{q}.txt").write_text(etape)


def relire(dossier: Path, travail: Path, secondes: float):
    ids = [l.split("\t")[0] for l in (dossier / "echantillon.tsv").read_text().splitlines()[1:]]
    qs = [l.split("\t")[0] for l in (dossier / "requetes.tsv").read_text().splitlines()[1:] if l.strip()]
    audio = np.stack([np.load(travail / "audio" / f"{i}.npy").reshape(-1) for i in ids])
    texte = np.stack([np.load(travail / "texte" / f"{q}.npy").reshape(-1) for q in qs])
    audio.astype("<f4").tofile(dossier / "clamp3.f32")
    texte.astype("<f4").tofile(dossier / "clamp3-texte.f32")
    (dossier / "cout-clamp3.txt").write_text(
        f"{secondes / len(ids):.3f} s par morceau (50 s d'audio, MERT compris, chargement compris)\n"
    )
    print(f"{len(ids)} morceaux, {len(qs)} requêtes, dimension {audio.shape[1]}")


if __name__ == "__main__":
    if sys.argv[1] == "preparer":
        preparer(Path(sys.argv[2]), Path(sys.argv[3]), Path(sys.argv[4]))
    else:
        relire(Path(sys.argv[2]), Path(sys.argv[3]), float(sys.argv[4]))
