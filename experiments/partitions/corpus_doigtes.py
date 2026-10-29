# SPDX-License-Identifier: GPL-3.0-or-later
"""Corpus du banc des doigtés (exemple `doigtes` du crate de transcription) :
les notes de tablatures de référence, avec corde et frette.

- livres : `reference.json` du banc (mesures où la tablature a été lue) ;
- Songsterr : parties de basse au format JSON de Songsterr
  (`$RUSTY_DEV/jeux/songsterr-basse/*.json`, récupérées par
  `songsterr_basse.py`).

Usage local et d'évaluation seulement : rien de ce qui est lu n'est versionné.
Les accords (plusieurs notes sur une même attaque) sont retirés : la
transcription de l'éditeur est monophonique. `premiers` : le début de chaque
mesure, pour `tablature::uniformiser`.

    python3 corpus_doigtes.py <sortie.json>
"""
import collections, glob, json, os, statistics, sys
import banc

SONGSTERR = os.path.join(banc.DEV, "jeux", "songsterr-basse")

def note(debut, fin, h):
    return {"debut_s": debut, "fin_s": fin, "hauteur": h, "amplitude": 1.0, "corde": None, "frette": None}

def livres():
    for d in sorted(glob.glob(os.path.join(banc.CACHE, "*/"))):
        ref = os.path.join(d, "reference.json")
        if not os.path.exists(ref): continue
        try:
            temps = json.load(open(os.path.join(d, "pulsation.json")))["temps"]
            noire = statistics.median(b - a for a, b in zip(temps, temps[1:]))
        except (OSError, KeyError, statistics.StatisticsError):
            noire = 0.5
        seizieme = noire / 4
        brutes, t0, premiers = [], 0.0, []
        for m in json.load(open(ref)):
            premiers.append(t0)
            if m.get("tablature") and not m.get("absente"):
                par_attaque = collections.Counter(x[0] for x in m["notes"])
                for x in m["notes"]:
                    if len(x) > 4 and x[3] is not None and par_attaque[x[0]] == 1:
                        brutes.append((t0 + x[0] * seizieme, t0 + (x[0] + max(x[2], 0.5)) * seizieme, x[1], x[3], x[4]))
            t0 += m["duree"] * seizieme
        if len(brutes) < 20: continue
        # cordes à vide : hauteur − frette, la plus fréquente par corde
        vides = {c: collections.Counter(h - f for _, _, h, cc, f in brutes if cc == c).most_common(1)[0][0] for c in {b[3] for b in brutes}}
        n = max(vides) + 1
        cordes = [vides.get(c, (vides[0] + 5 * c) if 0 in vides else 28 + 5 * c) for c in range(max(n, 4))]
        yield {"nom": "livre:" + os.path.basename(d.rstrip("/")), "cordes": cordes,
               "notes": [note(a, b, cordes[c] + f) for a, b, _, c, f in brutes], "verite": [[c, f] for *_, c, f in brutes],
               "premiers": premiers}

def songsterr():
    for f in sorted(glob.glob(os.path.join(SONGSTERR, "*.json"))):
        if f.endswith("index.json"): continue
        tab = json.load(open(f))
        tun = tab.get("tuning")
        if not tun or len(tun) not in (4, 5, 6): continue
        bpm = (tab.get("automations", {}).get("tempo") or [{"bpm": 120}])[0]["bpm"]
        sec, pos, notes, verite, premiers = 60 / bpm, 0.0, [], [], []
        for i, m in enumerate(tab["measures"]):
            premiers.append(pos * sec)
            for b in (m.get("voices") or [{"beats": []}])[0]["beats"]:
                n_, d_ = b["duration"]; dur = 4 * n_ / d_
                if b.get("graceNote"): dur = 0
                jouees = [n for n in b["notes"] if not n.get("rest") and "fret" in n]
                if len(jouees) == 1:
                    n = jouees[0]
                    if n.get("tie") and notes:
                        notes[-1]["fin_s"] = (pos + dur) * sec
                    elif not n.get("dead"):
                        notes.append(note(pos * sec, (pos + max(dur, 0.25)) * sec, tun[n["string"]] + n["fret"]))
                        verite.append([len(tun) - 1 - n["string"], n["fret"]])
                pos += dur
        if len(notes) >= 20:
            yield {"nom": "songsterr:" + os.path.basename(f)[:-5], "cordes": tun[::-1], "notes": notes, "verite": verite,
                   "premiers": premiers}

if __name__ == "__main__":
    corpus = list(livres()) + list(songsterr())
    json.dump(corpus, open(sys.argv[1], "w"))
    print(len(corpus), "morceaux,", sum(len(m["verite"]) for m in corpus), "notes →", sys.argv[1])
