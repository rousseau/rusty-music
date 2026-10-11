# SPDX-License-Identifier: GPL-3.0-or-later
"""Pulsation (Beat This!, portage Rust) contre les temps annotés des
morceaux appariés (`apparier.py`) : de vrais mélanges de la bibliothèque,
là où GTZAN n'a que des extraits de 30 s.

Mêmes mesures que `experiments/pulsation/noter.py` (F1 à 70 ms, CMLt, AMLt).
Avant de noter, on cale : nos fichiers sont souvent d'autres éditions
(remasters) qui ne commencent pas au même instant. On cherche le décalage
constant (±3 s, pas de 5 ms) qui maximise le F1 à tolérance large (150 ms)
des temps **et** des premiers temps (un décalage d'un temps entier laisse
le F1 des temps intact), au centre du plateau des meilleurs, puis on
l'affine par la médiane des résidus ; on note enfin à 70 ms. Ce calage favorise un peu le pisteur ; un morceau dont
le meilleur F1 large reste bas est une autre version (montage, vitesse) :
il est écarté.

    python pulsation.py
"""
import json, os, subprocess, sys
import numpy as np
import mir_eval
import apparier as ap

RACINE = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
PISTEUR = os.path.join(RACINE, "target/release/examples/verif_pulsation")
CACHE = os.path.join(ap.DEV, "banc-annotations")

def temps_annotes(e):
    t, r = [], []
    for l in open(e["temps"], errors="replace"):
        p = l.replace(",", " ").split()
        if len(p) < 2: continue
        try: t.append(float(p[0])); r.append(int(float(p[1])))
        except ValueError: continue
    t, r = np.array(t), np.array(r)
    return t, t[r == 1]

def caler(ref_t, ref_p, est_t, est_p):
    f = lambda r, e, dt: mir_eval.beat.f_measure(r + dt, e, f_measure_threshold=0.15)
    dts = np.arange(-3, 3.0001, 0.005)
    score = np.array([f(ref_t, est_t, dt) + f(ref_p, est_p, dt) for dt in dts])
    plateau = dts[score >= score.max() - 1e-9]
    dt = float(np.median(plateau))
    proches = np.array([est_t[np.argmin(np.abs(est_t - (x + dt)))] - (x + dt) for x in ref_t])
    proches = proches[np.abs(proches) < 0.15]
    if len(proches): dt += float(np.median(proches))
    return dt, f(ref_t, est_t, dt)

def main():
    os.makedirs(CACHE, exist_ok=True)
    morceaux = [e for e in json.load(open(os.path.join(ap.ANN, "appariement.json"))) if e.get("temps")]
    sortie = os.path.join(CACHE, "pulsation.jsonl")
    faits = {json.loads(l)["chemin"] for l in open(sortie)} if os.path.exists(sortie) else set()
    a_faire = [e["chemin"] for e in morceaux if e["chemin"] not in faits]
    if a_faire:
        liste = os.path.join(CACHE, "liste.txt")
        open(liste, "w").write("\n".join(a_faire) + "\n")
        r = subprocess.run([PISTEUR, liste, "complet"], capture_output=True, text=True, cwd=RACINE)
        if r.returncode: sys.exit(r.stderr[-2000:])
        open(sortie, "a").write("".join(l + "\n" for l in r.stdout.splitlines() if l.startswith("{")))
    est = {d["chemin"]: d for d in map(json.loads, open(sortie))}
    res, decales = [], []
    for e in morceaux:
        d = est.get(e["chemin"])
        if not d: continue
        ref_t, ref_p = temps_annotes(e)
        est_t, est_p = np.array(d["temps"]), np.array(d["premiers"])
        ecart, large = caler(ref_t, ref_p, est_t, est_p)
        ref_t, ref_p = ref_t + ecart, ref_p + ecart
        f_t = mir_eval.beat.f_measure(ref_t, est_t, f_measure_threshold=0.07)
        f_p = mir_eval.beat.f_measure(ref_p, est_p, f_measure_threshold=0.07)
        cml = mir_eval.beat.continuity(ref_t, est_t)
        ligne = (f_t, f_p, cml[1], cml[3])
        print(f"{e['jeu']:10} {e['artiste'][:18]:18} {e['titre'][:28]:28} F1 {f_t:.2f} premiers {f_p:.2f} "
              f"décalage {1000 * ecart:+.0f} ms (F1 large {large:.2f})")
        (decales if large < 0.5 else res).append(ligne)
    m = np.mean(res, axis=0)
    print(f"\n{len(res)} morceaux calés — F1 temps {100*m[0]:.1f} · premiers temps {100*m[1]:.1f} · "
          f"CMLt {100*m[2]:.1f} · AMLt {100*m[3]:.1f} ; {len(decales)} écartés (F1 large < 0,5 après calage)")

if __name__ == "__main__":
    main()
