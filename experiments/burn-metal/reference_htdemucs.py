# SPDX-License-Identifier: GPL-3.0-or-later
"""Référence de parité de HTDemucs pour comparer les backends Burn
(`crates/editor`, demucs-core) au code d'origine (facebookresearch/demucs,
MIT), en float32 sur processeur.

Même découpe que demucs-core (`separate_with_listener`), pas celle
d'`apply_model` : segments de 343 980 échantillons complétés de zéros,
pas de 3/4 (25 % de recouvrement), fenêtre triangulaire, somme normalisée
par les poids. Seul le réseau diffère donc d'un côté à l'autre. Pour
`htdemucs_ft`, chaque stem vient de son propre modèle, comme dans
demucs-core.

    <env>/bin/python -I reference_htdemucs.py <mix.wav> <variante> <sortie> [--secondes 120] [--appareil mps]

Écrit `<sortie>/<stem>.wav` (float32), lisibles par `comparer.py`.
"""
import argparse, os
import numpy as np, soundfile as sf, torch

SEGMENT = 343_980


def triangle(n):
    if n <= 1:
        return np.ones(n, dtype=np.float32)
    i = np.arange(n, dtype=np.float32)
    return 1.0 - np.abs(2.0 * i / (n - 1) - 1.0)


def main():
    a = argparse.ArgumentParser()
    a.add_argument("mix"); a.add_argument("variante"); a.add_argument("sortie")
    a.add_argument("--secondes", type=float, default=120.0)
    a.add_argument("--appareil", default="cpu", help="cpu, ou mps pour mesurer la sensibilité du modèle au backend")
    args = a.parse_args()
    from demucs.pretrained import get_model
    from demucs.apply import BagOfModels

    sac = get_model(args.variante)
    modeles = list(sac.models) if isinstance(sac, BagOfModels) else [sac]
    poids = sac.weights if isinstance(sac, BagOfModels) else [[1.0] * len(sac.sources)]
    sources = sac.sources
    for m in modeles:
        m.eval().to(args.appareil)

    x, sr = sf.read(args.mix, dtype="float32", always_2d=True)
    assert sr == 44100
    x = x[: int(args.secondes * sr), :2].T.copy()  # [2, n]
    n = x.shape[1]
    sortie = np.zeros((len(sources), 2, n), dtype=np.float32)
    somme = np.zeros(n, dtype=np.float32)
    pas = SEGMENT * 3 // 4
    morceaux = 1 if n <= SEGMENT else -(-max(0, n - SEGMENT) // pas) + 1
    with torch.no_grad():
        for k in range(morceaux):
            d = k * pas; f = min(d + SEGMENT, n); l = f - d
            seg = np.zeros((2, SEGMENT), dtype=np.float32); seg[:, :l] = x[:, d:f]
            t = torch.from_numpy(seg)[None].to(args.appareil)
            y = np.zeros((len(sources), 2, SEGMENT), dtype=np.float32)
            for m, w in zip(modeles, poids):
                o = m(t)[0].cpu().numpy()
                for i, p in enumerate(w):
                    y[i] += p * o[i]
            tot = np.array([sum(w[i] for w in poids) for i in range(len(sources))], dtype=np.float32)
            y /= tot[:, None, None]
            fen = triangle(l) if n > SEGMENT else np.ones(l, dtype=np.float32)
            sortie[:, :, d:f] += y[:, :, :l] * fen
            somme[d:f] += fen
    sortie /= np.where(somme > 0, somme, 1.0)
    os.makedirs(args.sortie, exist_ok=True)
    for i, s in enumerate(sources):
        sf.write(os.path.join(args.sortie, f"{s}.wav"), sortie[i].T, sr, subtype="FLOAT")
    print(args.variante, sources, f"{n / sr:.1f} s, {morceaux} segments")


if __name__ == "__main__":
    main()
