#!/usr/bin/env python3
"""Note un estimateur de tempo contre les annotations GTZAN.

    experiments/tempo/preparer.sh                       # une fois
    cargo run --release -p rusty-music-analysis --example verif_tempo -- \\
        experiments/tempo/data/genres > experiments/tempo/data/ours.csv
    python3 experiments/tempo/noter_tempo.py experiments/tempo/data/ours.csv [autre.csv ...]

Métriques usuelles de l'estimation de tempo : **Acc1** (à ± 4 % du tempo de
référence) et **Acc2** (idem, en tolérant les erreurs d'octave et de mesure :
facteurs 2, 3, 1/2, 1/3). Chaque CSV : `chemin,bpm` (chemin relatif à `genres/`).
"""
import csv, collections, os, sys

ICI = os.path.dirname(os.path.abspath(__file__))
FACTEURS = (1, 2, 3, 0.5, 1 / 3)
NATURES = ((2, "×2 (trop rapide d'une octave)"), (0.5, "÷2 (trop lent d'une octave)"), (3, "×3"), (1 / 3, "÷3"),
           (1.5, "×3/2"), (2 / 3, "×2/3"), (4 / 3, "×4/3"), (3 / 4, "×3/4"))


def charger_gt():
    dossier, gt = os.path.join(ICI, "data", "bpm"), {}
    for f in os.listdir(dossier):
        if f.endswith(".bpm"):
            genre, num = f[len("gtzan_"):-4].rsplit("_", 1)
            gt[f"{genre}/{genre}.{num}.wav"] = float(open(os.path.join(dossier, f)).read().split()[0])
    return gt


def acc1(est, ref, tol=0.04):
    return abs(est - ref) <= tol * ref


def acc2(est, ref, tol=0.04):
    return any(abs(est * f - ref) <= tol * ref for f in FACTEURS)


def nature(est, ref, tol=0.04):
    if acc1(est, ref, tol):
        return "juste"
    return next((nom for f, nom in NATURES if abs(est / f - ref) <= tol * ref), "autre")


def evaluer(pred, gt, titre):
    ids = [k for k in gt if pred.get(k)]
    n = len(ids)
    a1, a2 = sum(acc1(pred[k], gt[k]) for k in ids), sum(acc2(pred[k], gt[k]) for k in ids)
    print(f"\n== {titre} : {n} clips sur {len(gt)} ==  Acc1 {100 * a1 / n:.1f} %   Acc2 {100 * a2 / n:.1f} %")
    c = collections.Counter(nature(pred[k], gt[k]) for k in ids)
    print("   nature des écarts :", {k: f"{100 * v / n:.1f} %" for k, v in c.most_common()})
    pg = collections.defaultdict(lambda: [0, 0, 0])
    for k in ids:
        g = pg[k.split("/")[0]]
        g[0] += 1; g[1] += acc1(pred[k], gt[k]); g[2] += acc2(pred[k], gt[k])
    print("   par genre (Acc1/Acc2) :", ", ".join(f"{g} {100 * v[1] / v[0]:.0f}/{100 * v[2] / v[0]:.0f}" for g, v in sorted(pg.items())))
    tr = lambda b: "<80" if b < 80 else "80-100" if b < 100 else "100-120" if b < 120 else "120-140" if b < 140 else "140-170" if b < 170 else "≥170"
    T = collections.defaultdict(collections.Counter)
    for k in ids:
        T[tr(gt[k])][nature(pred[k], gt[k])] += 1
    print("   par tempo de référence (juste / ÷2 / ×2) :", " | ".join(
        f"{t}: {100 * T[t]['juste'] / sum(T[t].values()):.0f}/{100 * T[t][NATURES[1][1]] / sum(T[t].values()):.0f}/{100 * T[t][NATURES[0][1]] / sum(T[t].values()):.0f}"
        for t in ["<80", "80-100", "100-120", "120-140", "140-170", "≥170"]))


def main():
    gt = charger_gt()
    valeurs = sorted(gt.values())
    print(f"{len(gt)} annotations ; tempo de référence : min {valeurs[0]:.0f}, médiane {valeurs[len(valeurs) // 2]:.0f}, max {valeurs[-1]:.0f} ; "
          f"{sum(v >= 170 for v in valeurs)} clips à 170 BPM ou plus")
    for f in sys.argv[1:]:
        pred = {r["chemin"]: (float(r["bpm"]) if r["bpm"] else None) for r in csv.DictReader(open(f))}
        evaluer(pred, gt, os.path.basename(f))


if __name__ == "__main__":
    main()
