# SPDX-License-Identifier: GPL-3.0-or-later
"""Compare une transcription de basse (sortie de l'exemple `banc`) à la ligne
de basse d'une partition publiée (`reference.py`).

Une partition écrite n'est pas une chronologie : reprises, voltas, D.S. al
Coda, « w/ Bass Fig. 1 », silences de plusieurs mesures comptés pour une. On
aligne donc nos mesures sur les mesures écrites par un Viterbi qui autorise :
la mesure suivante (gratuit), un saut n'importe où (reprise, coda : pénalité),
rester sur une mesure vide (silence de plusieurs mesures), ou ne rien
expliquer (page absente du livre, OMR raté : état « hors partition »).

Notes : même double croche (ou ±1) et même hauteur. On compte aussi les notes
que nous écrivons là où la partition ne fait pas jouer la basse.

    python3 comparer.py transcription.json reference.json [--detail]
"""
import json, sys

SAUT, RESTER_VIDE, HORS = 0.6, 0.05, 0.12

def nos_mesures(t, phase=0):
    """Notes à plat sur une grille de doubles croches, redécoupées en mesures
    de 16 — `phase` en temps décale le découpage (premier temps mal placé)."""
    notes, g = [], 0
    for m in t["mesures"]:
        pos = g
        for e in m["evenements"]:
            j = e["jeu"]
            if j and not e["lie"]: notes.append((pos, j["hauteur"], j.get("corde"), j.get("frette")))
            pos += e["seiziemes"]
        g += 4 * m["temps"]
    g -= 0
    debut = -4 * phase
    n = (g - debut + 15) // 16
    out = [[] for _ in range(n)]
    for p, h, c, f in notes:
        k = (p - debut) // 16
        if 0 <= k < n: out[k].append(((p - debut) % 16, h, c, f))
    return out

def ref_mesures(r):
    """Les mesures sans portée de basse sont retirées : l'alignement y
    substitue un saut (figure rejouée) ou l'état hors partition (tacet)."""
    out = []
    for m in r:
        d = m["duree"] or 16
        # durée lue fausse (rythme mal reconnu) : on ramène à 4/4
        s = 16 / d if d != 16 and 10 <= d <= 24 else 1
        out.append([(round(n[0] * s), n[1], *(n[3:5] if len(n) >= 5 else (None, None)))
                    for n in m["notes"] if round(n[0] * s) < 16])
    return out

def garder(r):
    return [m for m in r if not m.get("absente")]

def apparier(o, r, tol):
    """Paires (note à nous, note de référence) : attaque à `tol` doubles
    croches près, la même hauteur d'abord, puis l'octave, puis le reste."""
    libres = list(range(len(r))); paires = []
    for rang in (0, 1, 2):
        for i, (p, h, *_) in enumerate(o):
            if any(i == a for a, _ in paires): continue
            for k in libres:
                rp, rh = r[k][0], r[k][1]
                if abs(rp - p) > tol: continue
                if rang == 0 and rh != h: continue
                if rang == 1 and (rh - h) % 12: continue
                paires.append((i, k)); libres.remove(k); break
    return paires

def sim(o, r):
    if not o and not r: return 1.0
    if not o or not r: return 0.0
    s = 0
    for i, k in apparier(o, r, 1):
        s += 1.0 if o[i][1] == r[k][1] else 0.7 if (o[i][1] - r[k][1]) % 12 == 0 else 0.3
    return 2 * s / (len(o) + len(r))

def aligner(N, R):
    M = len(R); X = M  # état hors partition
    S = [[sim(o, r) for r in R] for o in N]
    vide = [not r for r in R]
    prev = [S[0][j] - SAUT * (j > 0) for j in range(M)] + [HORS]
    retours = []
    for i in range(1, len(N)):
        meilleur = max(range(M + 1), key=lambda j: prev[j]); mv = prev[meilleur]
        cur, ret = [], []
        for j in range(M):
            cands = [(mv - SAUT, meilleur)]
            if j > 0: cands.append((prev[j - 1], j - 1))
            if vide[j]: cands.append((prev[j] - RESTER_VIDE, j))
            cands.append((prev[X] - SAUT / 2, X))
            v, d = max(cands)
            cur.append(v + S[i][j]); ret.append(d)
        v, d = max((prev[X], X), (mv, meilleur))
        cur.append(v + HORS); ret.append(d)
        retours.append(ret); prev = cur
    j = max(range(M + 1), key=lambda j: prev[j]); total = prev[j]
    chemin = [j]
    for ret in reversed(retours):
        j = ret[j]; chemin.append(j)
    return chemin[::-1], total

def noter(N, R, chemin, avec_tab=None):
    st = dict(ref=0, nous=0, exactes=0, tol=0, octave=0, autres=0, hors=0, vides=0, vides_notes=0, notes_silence=0,
              doigtes_ref=0, doigtes=0, ref_tab=0, nous_tab=0, exactes_tab=0)
    for o, j in zip(N, chemin):
        if j == len(R): st["hors"] += 1; continue
        r = R[j]
        if not r:
            st["vides"] += 1
            if o: st["vides_notes"] += 1; st["notes_silence"] += len(o)
        st["ref"] += len(r); st["nous"] += len(o)
        p0 = apparier(o, r, 0)
        ex = sum(1 for i, k in p0 if o[i][1] == r[k][1])
        st["exactes"] += ex
        if avec_tab and avec_tab[j]:
            st["ref_tab"] += len(r); st["nous_tab"] += len(o); st["exactes_tab"] += ex
        for i, k in apparier(o, r, 1):
            if o[i][1] == r[k][1]:
                st["tol"] += 1
                if r[k][2] is not None:
                    st["doigtes_ref"] += 1
                    st["doigtes"] += (o[i][2], o[i][3]) == (r[k][2], r[k][3])
            elif (o[i][1] - r[k][1]) % 12 == 0: st["octave"] += 1
            else: st["autres"] += 1
    return st

def comparer(t, r):
    r = garder(r)
    R = ref_mesures(r)
    best = None
    for phase in range(4):
        N = nos_mesures(t, phase)
        chemin, total = aligner(N, R)
        if best is None or total > best[0]: best = (total, phase, N, chemin)
    _, phase, N, chemin = best
    return phase, N, R, chemin, noter(N, R, chemin, [bool(m.get("tablature")) for m in r])

if __name__ == "__main__":
    t = json.load(open(sys.argv[1])); r = json.load(open(sys.argv[2]))
    phase, N, R, chemin, st = comparer(t, r)
    r = garder(r)
    f1 = lambda a: 2 * a / max(1, st["ref"] + st["nous"])
    print(json.dumps({"phase": phase, "mesures": len(N), **st,
                      "f1_exact": round(f1(st["exactes"]), 3), "f1_tol": round(f1(st["tol"]), 3),
                      "f1_tab": round(2 * st["exactes_tab"] / max(1, st["ref_tab"] + st["nous_tab"]), 3)}))
    if "--detail" in sys.argv:
        for i, (o, j) in enumerate(zip(N, chemin)):
            rr = R[j] if j < len(R) else None
            print(f"{i:4d} → {'hors' if rr is None else r[j]['numero']:>4}  nous {' '.join(f'{p}:{h}' for p, h, *_ in o)}")
            if rr is not None: print(f"{'':12}réf  {' '.join(f'{p}:{h}' for p, h, *_ in rr)}")
