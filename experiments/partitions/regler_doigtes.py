# SPDX-License-Identifier: GPL-3.0-or-later
"""Règle les coûts du doigté (`crates/transcription/src/tablature.rs`) contre
les tablatures publiées : pour chaque morceau déjà passé au banc, repose les
notes avec une grille de coûts (exemple `banc -- doigtes`) et compte les notes
dont corde et frette sont celles de la tablature.

    python3 regler_doigtes.py
"""
import glob, json, os, subprocess
from collections import defaultdict
import banc, comparer

def main():
    total = defaultdict(lambda: [0, 0]); par_morceau = defaultdict(dict)
    for d in sorted(glob.glob(os.path.join(banc.CACHE, "*/"))):
        tr, ref = os.path.join(d, "transcription.json"), os.path.join(d, "reference.json")
        if not (os.path.exists(tr) and os.path.exists(ref)): continue
        r = json.load(open(ref))
        if not any(m.get("tablature") for m in r): continue
        grille = os.path.join(d, "doigtes")
        subprocess.run([banc.BANC, "doigtes", tr, os.path.join(d, "pulsation.json"), grille],
                       check=True, stdout=subprocess.DEVNULL, cwd=banc.RACINE)
        # l'alignement ne dépend pas du doigté : une fois pour toutes
        phase, N, R, chemin, _ = comparer.comparer(json.load(open(tr)), r)
        for f in sorted(glob.glob(os.path.join(grille, "*.json"))):
            Nv = comparer.nos_mesures(json.load(open(f)), phase)
            st = comparer.noter(Nv, R, chemin)
            nom = os.path.basename(f)[3:-5]
            total[nom][0] += st["doigtes"]; total[nom][1] += st["doigtes_ref"]
            par_morceau[nom][os.path.basename(d.rstrip("/"))] = st["doigtes"] / max(1, st["doigtes_ref"])
    classe = sorted(total, key=lambda k: -total[k][0] / max(1, total[k][1]))
    print("corde_vide_aigue_pentehaut   doigtés identiques   (moyenne par morceau)")
    for k in classe[:12] + ["0.6_0_0_0.1"]:
        moy = sum(par_morceau[k].values()) / max(1, len(par_morceau[k]))
        print(f"{k:28} {total[k][0] / max(1, total[k][1]):6.3f}   ({moy:.3f}, {len(par_morceau[k])} morceaux)")

main()
