# /// script
# dependencies = ["mir_eval", "numpy"]
# ///
# SPDX-License-Identifier: GPL-3.0-or-later
"""Note la pulsation (sortie JSON de `verif_pulsation`) contre les annotations
GTZAN : F1 des temps et des premiers temps (tolérance 70 ms, comme l'article
Beat This!), CMLt, AMLt.

    uv run experiments/pulsation/noter.py data/sortie.jsonl
"""
import json, os, sys
import numpy as np
import mir_eval

ICI = os.path.dirname(os.path.abspath(__file__))
ANNOT = os.path.join(ICI, "data", "beats")

def annotations(chemin):
    # genres/blues/blues.00000.au -> gtzan_blues_00000.beats
    base = os.path.basename(chemin).rsplit(".", 1)[0]  # blues.00000
    genre, num = base.split(".")
    f = os.path.join(ANNOT, f"gtzan_{genre}_{num}.beats")
    if not os.path.exists(f):
        return None
    t, r = [], []
    for l in open(f):
        p = l.split()
        if len(p) >= 2:
            t.append(float(p[0])); r.append(int(float(p[1])))
    t, r = np.array(t), np.array(r)
    return t, t[r == 1], genre

lignes = [json.loads(l) for l in open(sys.argv[1]) if l.startswith("{")]
res, par_genre = [], {}
for d in lignes:
    a = annotations(d["chemin"])
    if a is None:
        continue
    ref_t, ref_p, genre = a
    est_t, est_p = np.array(d["temps"]), np.array(d["premiers"])
    # mir_eval ignore les 5 premières secondes par défaut ; l'article Beat This!
    # ne les ignore pas — on garde tout.
    f_t = mir_eval.beat.f_measure(ref_t, est_t, f_measure_threshold=0.07)
    f_p = mir_eval.beat.f_measure(ref_p, est_p, f_measure_threshold=0.07)
    cml = mir_eval.beat.continuity(ref_t, est_t)
    r = (f_t, f_p, cml[1], cml[3], d["ms"])
    res.append(r)
    par_genre.setdefault(genre, []).append(r)

m = np.mean(res, axis=0)
print(f"{len(res)} clips — F1 temps {100*m[0]:.1f} · F1 premiers temps {100*m[1]:.1f} · CMLt {100*m[2]:.1f} · AMLt {100*m[3]:.1f} · {m[4]:.0f} ms/clip")
for g, v in sorted(par_genre.items()):
    v = np.mean(v, axis=0)
    print(f"  {g:10s} F1 temps {100*v[0]:5.1f} · premiers {100*v[1]:5.1f}")
