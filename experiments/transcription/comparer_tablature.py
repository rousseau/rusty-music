# SPDX-License-Identifier: GPL-3.0-or-later
"""Compare une transcription de basse (mesures quantifiées de
`crates/transcription`) à une tablature de référence au format JSON de
Songsterr (une partie). Usage local et d'évaluation seulement : la référence
n'est ni copiée dans le dépôt ni redistribuée.

    python3 comparer_tablature.py <mesures.json> <reference_songsterr.json>

Mesure par mesure (après recherche du meilleur alignement) : attaques
retrouvées à la double croche, hauteurs (justes / octave / autres),
doigtés (même corde et même frette).
"""
import json, sys
from fractions import Fraction

nous = json.load(open(sys.argv[1]))
ref = json.load(open(sys.argv[2]))
accord = ref["tuning"]  # corde 0 = la plus aiguë

def ref_mesures():
    out = []
    for m in ref["measures"]:
        sig = m.get("signature")
        pos = Fraction(0)
        notes = []
        for b in m["voices"][0]["beats"]:
            num, den = b["duration"]
            d = Fraction(num, den) * 4  # en noires
            if b.get("dots"): d *= Fraction(3, 2) if b["dots"] == 1 else Fraction(7, 4)
            if b.get("tuplet"): d *= Fraction(b["tupletStop"] if False else 2, 3)
            if not b.get("rest") and not b.get("graceNote"):
                for n in b["notes"]:
                    if n.get("rest") or n.get("tie") or n.get("dead"): continue
                    corde = n["string"]
                    notes.append((int(round(pos * 4)), accord[corde] + n["fret"], 3 - corde, n["fret"]))
            pos += d
        out.append(notes)
    return out

def nos_mesures():
    out = []
    for m in nous:
        pos, notes = 0, []
        for e in m["evenements"]:
            j = e["jeu"]
            if j and not e["lie"]:
                notes.append((pos, j["hauteur"], j["corde"], j["frette"]))
            pos += e["seiziemes"]
        out.append(notes)
    return out

R, N = ref_mesures(), nos_mesures()

def comparer(decalage):
    st = dict(ref=0, nous=0, attaques=0, justes=0, octave=0, doigtes=0)
    for i, rn in enumerate(R):
        j = i - decalage
        nn = N[j] if 0 <= j < len(N) else []
        st["ref"] += len(rn); st["nous"] += len(nn)
        libres = list(nn)
        for (s, h, c, f) in rn:
            k = next((k for k, x in enumerate(libres) if x[0] == s), None)
            if k is None: continue
            x = libres.pop(k); st["attaques"] += 1
            if x[1] == h:
                st["justes"] += 1
                if (x[2], x[3]) == (c, f): st["doigtes"] += 1
            elif (x[1] - h) % 12 == 0: st["octave"] += 1
    return st

meilleur = max(range(-4, 5), key=lambda d: comparer(d)["attaques"])
st = comparer(meilleur)
pc = lambda a, b: f"{100 * a / max(1, b):.0f} %"
print(f"alignement : nos mesures décalées de {meilleur} par rapport à la référence")
print(f"notes : référence {st['ref']}, nous {st['nous']}")
print(f"attaques retrouvées (même double croche) : {st['attaques']} — rappel {pc(st['attaques'], st['ref'])}, précision {pc(st['attaques'], st['nous'])}")
print(f"  hauteur juste : {pc(st['justes'], st['attaques'])} · erreur d'octave : {pc(st['octave'], st['attaques'])} · autre : {pc(st['attaques'] - st['justes'] - st['octave'], st['attaques'])}")
print(f"  doigté identique (corde + frette), parmi les hauteurs justes : {pc(st['doigtes'], st['justes'])}")
