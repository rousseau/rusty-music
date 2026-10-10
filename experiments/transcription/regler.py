# SPDX-License-Identifier: GPL-3.0-or-later
"""Note les réglages produits par `examples/regler.rs` contre des tablatures
de référence (JSON Songsterr, usage local d'évaluation), morceau par morceau,
et classe les réglages sur la moyenne des morceaux.

    python3 regler.py <metrique> <dossier_morceau1>:<reference1.json> [...]

<metrique> : « attaques » (F1 des attaques × part de hauteurs justes) ou
« doigtes » (part de doigtés identiques parmi les hauteurs justes).
"""
import glob, json, os, sys
from fractions import Fraction

def ref_mesures(ref):
    accord, out = ref["tuning"], []
    for m in ref["measures"]:
        pos, notes = Fraction(0), []
        for b in m["voices"][0]["beats"]:
            num, den = b["duration"]
            d = Fraction(num, den) * 4
            if b.get("dots"): d *= Fraction(3, 2)
            if not b.get("rest") and not b.get("graceNote"):
                for n in b["notes"]:
                    if n.get("rest") or n.get("tie") or n.get("dead"): continue
                    notes.append((int(round(pos * 4)), accord[n["string"]] + n["fret"], 3 - n["string"], n["fret"]))
            pos += d
        out.append(notes)
    return out

def nos_mesures(m):
    out = []
    for x in m:
        pos, notes = 0, []
        for e in x["evenements"]:
            j = e["jeu"]
            if j and not e["lie"]: notes.append((pos, j["hauteur"], j["corde"], j["frette"]))
            pos += e["seiziemes"]
        out.append(notes)
    return out

def stats(R, N, dec):
    st = dict(ref=0, nous=0, att=0, justes=0, doigtes=0)
    for i, rn in enumerate(R):
        j = i - dec
        nn = list(N[j]) if 0 <= j < len(N) else []
        st["ref"] += len(rn); st["nous"] += len(nn)
        for (s, h, c, f) in rn:
            k = next((k for k, x in enumerate(nn) if x[0] == s), None)
            if k is None: continue
            x = nn.pop(k); st["att"] += 1
            if x[1] == h:
                st["justes"] += 1
                st["doigtes"] += (x[2], x[3]) == (c, f)
    return st

def note(st, metrique):
    if metrique == "doigtes": return st["doigtes"] / max(1, st["justes"])
    p, r = st["att"] / max(1, st["nous"]), st["att"] / max(1, st["ref"])
    f1 = 2 * p * r / max(1e-9, p + r)
    return f1 * st["justes"] / max(1, st["att"])

metrique, paires = sys.argv[1], [a.split(":") for a in sys.argv[2:]]
scores = {}
for dossier, refp in paires:
    R = ref_mesures(json.load(open(refp)))
    for f in sorted(glob.glob(f"{dossier}/*.json")):
        N = nos_mesures(json.load(open(f)))
        dec = max(range(-4, 5), key=lambda d: stats(R, N, d)["att"])
        scores.setdefault(os.path.basename(f)[:-5], []).append(note(stats(R, N, dec), metrique))
classe = sorted(scores.items(), key=lambda kv: -sum(kv[1]) / len(kv[1]))
for nom, v in classe[:8]: print(f"{nom:28s} moyenne {100*sum(v)/len(v):5.1f} %  " + "  ".join(f"{100*x:5.1f}" for x in v))
for nom, v in scores.items():
    if nom in ("bp_0.5_0.3_11", "dg_0.1_0.03_5_0.1"): print(f"(actuel) {nom:19s} moyenne {100*sum(v)/len(v):5.1f} %  " + "  ".join(f"{100*x:5.1f}" for x in v))
