# SPDX-License-Identifier: GPL-3.0-or-later
"""MuScriptor (Kyutai × Mirelo, code MIT, poids CC BY-NC 4.0 + conditions)
au banc de la batterie, contre ADTOF — évaluation seulement, comme
`muscriptor_banc.py` pour la basse.

Le stem de batterie est transcrit restreint au groupe « drums » ; les notes
General MIDI sont ramenées aux cinq pièces d'ADTOF, puis mises en mesure par
la chaîne de l'éditeur (exemple `banc -- coups`) et comparées aux
transcriptions Drum Ninja comme dans `banc_batterie.py`.

    <venv>/bin/python muscriptor_batterie.py [--taille small|medium|large] [id ...]
"""
import argparse, json, os, subprocess, sys, time
import banc, banc_batterie, comparer, lire_batterie

# General MIDI → pièces d'ADTOF
GM = {35: "grosse_caisse", 36: "grosse_caisse",
      37: "caisse_claire", 38: "caisse_claire", 39: "caisse_claire", 40: "caisse_claire",
      41: "toms", 43: "toms", 45: "toms", 47: "toms", 48: "toms", 50: "toms",
      42: "charleston", 44: "charleston", 46: "charleston",
      49: "cymbales", 51: "cymbales", 52: "cymbales", 53: "cymbales", 55: "cymbales", 57: "cymbales", 59: "cymbales"}

def transcrire(modele, wav):
    coups = []
    for e in modele.transcribe(wav, instruments=["drums"]):
        if type(e).__name__ == "NoteStartEvent" and e.instrument == "drums" and e.pitch in GM:
            coups.append({"instant_s": e.start_time, "piece": GM[e.pitch], "force": 0.8})
    return sorted(coups, key=lambda c: c["instant_s"])

def main():
    a = argparse.ArgumentParser()
    a.add_argument("--taille", default="medium")
    a.add_argument("ids", nargs="*")
    args = a.parse_args()
    from muscriptor.transcription_model import TranscriptionModel
    modele = TranscriptionModel.load_model(args.taille)
    conf = json.load(open(os.path.join(banc.ICI, "morceaux.json")))
    pieces = banc_batterie.PIECES
    tot = {c: dict(ref=0, nous=0, ok=0, tol=0) for c in pieces}
    for m in conf["batterie"]:
        if args.ids and m["id"] not in args.ids: continue
        d, stem, puls = banc.preparer_audio(m, "drums")
        brut = os.path.join(d, f"muscriptor-{args.taille}-coups.json")
        if not os.path.exists(brut):
            t0 = time.time()
            json.dump(transcrire(modele, stem), open(brut, "w"))
            print(f"  {m['id']} : {time.time() - t0:.0f} s", file=sys.stderr)
        tr = os.path.join(d, f"batterie-muscriptor-{args.taille}.json")
        subprocess.run([banc.BANC, "coups", brut, puls, tr], check=True, stdout=subprocess.DEVNULL, cwd=banc.RACINE)
        ref = lire_batterie.lire(os.path.join(banc_batterie.LIVRES, m["pdf"]))
        t = banc_batterie.en_evenements(json.load(open(tr))["mesures"])
        phase, N, R, chemin, st = comparer.comparer(t, ref)
        par = []
        for c in pieces:
            Nc = [[n for n in o if n[1] == c] for o in N]; Rc = [[n for n in r if n[1] == c] for r in R]
            s = comparer.noter(Nc, Rc, chemin)
            for k, v in (("ref", s["ref"]), ("nous", s["nous"]), ("ok", s["exactes"]), ("tol", s["tol"])): tot[c][k] += v
            par.append(f"{2 * s['exactes'] / max(1, s['ref'] + s['nous']):6.2f}")
        f1 = 2 * st["exactes"] / max(1, st["ref"] + st["nous"])
        print(f"{m['id']:24} F1 {f1:.2f}   " + "  ".join(par), flush=True)
    print(f"TOTAL MuScriptor {args.taille} par pièce (F1 exact / ±1) :", ", ".join(
        f"{pieces[c]} {2 * v['ok'] / max(1, v['ref'] + v['nous']):.2f}/{2 * v['tol'] / max(1, v['ref'] + v['nous']):.2f}" for c, v in tot.items()))

if __name__ == "__main__":
    main()
