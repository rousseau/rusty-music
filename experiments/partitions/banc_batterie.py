# SPDX-License-Identifier: GPL-3.0-or-later
"""Banc de la transcription de batterie (ADTOF, `crates/transcription`)
contre des transcriptions publiées (thedrumninja.com, PDF Sibelius lus par
`lire_batterie.py`). Même alignement que la basse (`comparer.py`) : une
« hauteur » est ici une pièce (36 grosse caisse, 38 caisse claire, 45 toms,
42 charleston, 49 cymbales).

Une transcription Drum Ninja note le groove de chaque section et le répète
(« % », reprises, D.S.) : les variations et les fills joués ne sont pas tous
écrits. L'alignement par sauts le tolère ; le rappel reste le chiffre sûr.

    python3 banc_batterie.py [id ...]
"""
import json, os, subprocess, sys
import banc, comparer, lire_batterie

LIVRES = os.path.expanduser(os.environ.get("LIVRES", "~/Exp/tmp"))
PIECES = {36: "grosse caisse", 38: "caisse claire", 45: "toms", 42: "charleston", 49: "cymbales"}
CODES = {"grosse_caisse": 36, "caisse_claire": 38, "toms": 45, "charleston": 42, "cymbales": 49}

def en_evenements(mesures):
    """Mesures de batterie → format de `comparer.nos_mesures` (une pièce par
    événement, durée portée par la dernière)."""
    out = []
    for m in mesures:
        ev = []
        for f in m["frappes"]:
            ps = [CODES[p] for p in f["pieces"]]
            if not ps: ev.append({"seiziemes": f["seiziemes"], "jeu": None, "lie": False}); continue
            for k, c in enumerate(ps):
                ev.append({"seiziemes": f["seiziemes"] if k == len(ps) - 1 else 0, "jeu": {"hauteur": c}, "lie": False})
        out.append({"temps": m["temps"], "evenements": ev})
    return {"mesures": out}

def main():
    conf = json.load(open(os.path.join(banc.ICI, "morceaux.json")))
    voulus = set(sys.argv[1:])
    tot = {c: dict(ref=0, nous=0, ok=0, tol=0) for c in PIECES}
    print(f"{'morceau':24} {'mes.':>4} {'réf':>5} {'nous':>5}   F1    F1±1   " + "  ".join(f"{n[:6]:>6}" for n in PIECES.values()))
    for m in conf["batterie"]:
        if voulus and m["id"] not in voulus: continue
        d, stem, puls = banc.preparer_audio(m, "drums")
        tr = os.path.join(d, "batterie.json")
        if not os.path.exists(tr):
            subprocess.run([banc.BANC, "batterie", stem, puls, tr], check=True, stdout=subprocess.DEVNULL, cwd=banc.RACINE)
        ref = lire_batterie.lire(os.path.join(LIVRES, m["pdf"]))
        json.dump(ref, open(os.path.join(d, "reference-batterie.json"), "w"))
        t = en_evenements(json.load(open(tr))["mesures"])
        phase, N, R, chemin, st = comparer.comparer(t, ref)
        f1 = lambda a, s=st: 2 * a / max(1, s["ref"] + s["nous"])
        par = []
        for c in PIECES:
            Nc = [[n for n in o if n[1] == c] for o in N]; Rc = [[n for n in r if n[1] == c] for r in R]
            s = comparer.noter(Nc, Rc, chemin)
            for k, v in (("ref", s["ref"]), ("nous", s["nous"]), ("ok", s["exactes"]), ("tol", s["tol"])): tot[c][k] += v
            par.append(f"{2 * s['exactes'] / max(1, s['ref'] + s['nous']):6.2f}")
        print(f"{m['id']:24} {len(N):4d} {st['ref']:5d} {st['nous']:5d}  {f1(st['exactes']):.2f}  {f1(st['tol']):.2f}   " + "  ".join(par))
    print("TOTAL par pièce (F1 exact / ±1) :", ", ".join(
        f"{PIECES[c]} {2 * v['ok'] / max(1, v['ref'] + v['nous']):.2f}/{2 * v['tol'] / max(1, v['ref'] + v['nous']):.2f}" for c, v in tot.items()))

if __name__ == "__main__":
    main()
