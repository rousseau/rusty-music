#!/usr/bin/env python3
"""Référence ONNX Runtime pour l'essai d'import Burn de HTDemucs.

Fait passer par ONNX Runtime exactement le signal que `src/main.rs` fabrique,
et rend les mêmes chiffres : somme de l'entrée, forme de la sortie, RMS par
stem. Un import qui réussit mais rend autre chose n'a rien importé — c'est la
leçon d'`onnx-simplifier` sur CLAP.

    python3 reference_demucs.py [sortie.txt] [modele.onnx]
"""

import sys
import numpy as np
import onnxruntime as ort

CANAUX, ECHANTILLONS, SR = 2, 343_980, 44_100.0
NOMS = ["batterie", "basse", "autre", "voix"]


def melange():
    """Doit rester rigoureusement identique à `melange()` de src/main.rs."""
    i = np.arange(CANAUX * ECHANTILLONS, dtype=np.int64)
    c = (i // ECHANTILLONS).astype(np.float32)
    t = ((i % ECHANTILLONS).astype(np.float32) / np.float32(SR)).astype(np.float32)
    tau = np.float32(2.0 * np.pi)
    basse = np.sin(t * np.float32(55.0) * tau) * np.float32(0.35)
    voix = np.sin(t * np.float32(220.0) * tau) * np.float32(0.25)
    autre = np.sin(t * np.float32(880.0) * tau) * np.float32(0.15)
    phase = np.modf(t * np.float32(2.0))[0]
    batterie = np.where(
        phase < np.float32(0.01),
        np.float32(0.4) * (np.float32(1.0) - phase * np.float32(100.0)),
        np.float32(0.0),
    )
    return ((basse + voix + autre + batterie) * (np.float32(1.0) + np.float32(0.1) * c)).astype(np.float32)


def main():
    brut = melange()
    print(f"entrée : {brut.size} échantillons, somme {float(brut.astype(np.float64).sum()):.3f}")

    modele = sys.argv[2] if len(sys.argv) > 2 else "../../models/htdemucs.onnx"
    print(f"modèle : {modele}")
    s = ort.InferenceSession(modele, providers=["CPUExecutionProvider"])
    x = brut.reshape(1, CANAUX, ECHANTILLONS)

    import time
    t0 = time.time()
    y = s.run(None, {"mix": x})[0]
    print(f"  inférence : {int((time.time() - t0) * 1000)} ms — forme {y.shape}")

    plat = y.ravel()
    par = plat.size // len(NOMS)
    print(f"\n{plat.size} valeurs, {par} par stem :")
    for i, nom in enumerate(NOMS):
        seg = plat[i * par:(i + 1) * par].astype(np.float64)
        print(f"  {nom:<10} RMS {np.sqrt((seg ** 2).mean()):.6f}")

    if len(sys.argv) > 1:
        with open(sys.argv[1], "w") as f:
            f.write("\n".join(f"{v:e}" for v in plat))
        print(f"\nstems écrits dans {sys.argv[1]}")


if __name__ == "__main__":
    main()
