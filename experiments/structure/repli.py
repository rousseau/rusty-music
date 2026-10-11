# SPDX-License-Identifier: GPL-3.0-or-later
"""Repli sans apprentissage pour les sections (plan 2.4) : point de
comparaison d'all-in-one, sur les mêmes morceaux et les mêmes mesures
(`allinone_ref.noter`).

Méthode classique (Foote 2000 ; McFee et Ellis 2014 pour la récurrence) :
- chroma CQT et MFCC moyennés par temps (temps de Beat This!, portage Rust,
  dans le cache de `experiments/annotations/pulsation.py`) ;
- matrice de récurrence (k plus proches voisins mutuels), lissée en
  diagonale ;
- nouveauté par noyau en damier gaussien de 32 temps, pics espacés d'au
  moins 16 temps (≈ 8 s) ;
- étiquettes A/B/C : regroupement agglomératif des sections sur leurs
  moyennes, autant de groupes que la moitié des sections (au plus 6).

    <env>/bin/python repli.py
"""
import json, os, subprocess
import numpy as np
import librosa, scipy.ndimage, scipy.signal
from sklearn.cluster import AgglomerativeClustering
import allinone_ref as ar

NOYAU, ECART = 32, 16

def damier(n):
    g = scipy.signal.windows.gaussian(n, n / 4)
    k = np.outer(g, g)
    s = np.where(np.arange(n) < n // 2, -1.0, 1.0)
    return k * np.outer(s, s)

def segments(e, temps):
    y, sr = librosa.load(e["chemin"], sr=22050, mono=True)
    chroma = librosa.feature.chroma_cqt(y=y, sr=sr, hop_length=512)
    mfcc = librosa.feature.mfcc(y=y, sr=sr, n_mfcc=13, hop_length=512)
    trames = librosa.time_to_frames(temps, sr=sr, hop_length=512)
    trames = librosa.util.fix_frames(trames, x_max=chroma.shape[1])
    x = np.vstack([librosa.util.sync(chroma, trames), librosa.util.normalize(librosa.util.sync(mfcc, trames), axis=1)])
    bornes_t = librosa.frames_to_time(trames, sr=sr, hop_length=512)
    r = librosa.segment.recurrence_matrix(x, mode="affinity", sym=True, width=3)
    r = librosa.segment.path_enhance(r, 9)
    n = r.shape[0]
    k = damier(NOYAU)
    rp = np.pad(r, NOYAU // 2)
    nouveaute = np.array([(rp[i:i + NOYAU, i:i + NOYAU] * k).sum() for i in range(n)])
    pics, _ = scipy.signal.find_peaks(nouveaute, distance=ECART, prominence=np.std(nouveaute) * 0.5)
    bornes = np.unique(np.concatenate([[0], pics, [n]]))
    moyennes = np.array([x[:, a:b].mean(1) for a, b in zip(bornes[:-1], bornes[1:])])
    nb = len(moyennes)
    groupes = AgglomerativeClustering(n_clusters=max(1, min(6, nb // 2))).fit_predict(moyennes) if nb > 1 else [0]
    t = np.append(bornes_t, len(y) / sr)
    return [dict(start=float(t[a]), end=float(t[b]), label="ABCDEF"[g]) for (a, b), g in zip(zip(bornes[:-1], bornes[1:]), groupes)]

def main():
    morceaux = ar.morceaux_uniques()
    puls = os.path.join(ar.CACHE, "pulsation.jsonl")
    faits = {json.loads(l)["chemin"] for l in open(puls)} if os.path.exists(puls) else set()
    manque = [e["chemin"] for e in morceaux if e["chemin"] not in faits]
    if manque:
        liste = os.path.join(ar.CACHE, "liste.txt")
        open(liste, "w").write("\n".join(manque) + "\n")
        r = subprocess.run([os.path.join(ar.RACINE, "target/release/examples/verif_pulsation"), liste, "complet"],
                           capture_output=True, text=True, check=True, cwd=ar.RACINE)
        open(puls, "a").write("".join(l + "\n" for l in r.stdout.splitlines() if l.startswith("{")))
    temps = {d["chemin"]: d["temps"] for d in map(json.loads, open(puls))}
    sortie = os.path.join(ar.CACHE, "repli")
    os.makedirs(sortie, exist_ok=True)
    def segs(e):
        f = os.path.join(sortie, ar.nom(e) + ".json")
        if not os.path.exists(f):
            json.dump(segments(e, temps[e["chemin"]]), open(f, "w"))
        return json.load(open(f))
    ar.noter(morceaux, segs)

if __name__ == "__main__":
    main()
