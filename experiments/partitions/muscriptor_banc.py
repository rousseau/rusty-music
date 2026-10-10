# SPDX-License-Identifier: GPL-3.0-or-later
"""MuScriptor (Kyutai × Mirelo, code MIT, poids CC BY-NC 4.0 + conditions)
au banc de la basse, par son code Python de référence — évaluation
seulement : le banc décide si le modèle vaut un portage en Rust/Burn
(`docs/plan-editer-pratique-creation.md`, 0.4).

Pour chaque morceau déjà préparé par `banc.py` : transcription du stem de
basse, restreinte aux basses (`instruments`), puis la même suite que pour
Basic Pitch (exemple `banc -- notes` : monophonie, accordage, doigtés,
quantification) et la même comparaison.

Conditions d'usage des poids (à accepter sur Hugging Face, compte requis) :
transcrire seulement ce sur quoi on a les droits, recherche, pas d'usage
commercial. Ici : bibliothèque personnelle, évaluation locale.

    <venv>/bin/python muscriptor_banc.py [--taille small|medium|large] [id ...]
"""
import argparse, json, os, subprocess, sys, time
import banc, comparer, reference

def transcrire(modele, wav, instruments):
    debuts, notes = {}, []
    for e in modele.transcribe(wav, instruments=instruments):
        nom = type(e).__name__
        if nom == "NoteStartEvent":
            debuts[e.index] = e
        elif nom == "NoteEndEvent":
            s = e.start_event
            notes.append({"debut_s": s.start_time, "fin_s": e.end_time, "hauteur": s.pitch,
                          "amplitude": 0.8, "corde": None, "frette": None})
    return sorted(notes, key=lambda n: n["debut_s"])

def nettoyer(notes, tranche=5.0, max_notes=150, max_doublons=10):
    """Écarte les tranches de 5 s (celles du modèle) où le décodage glouton
    boucle sans jamais finir : mêmes notes répétées toutes les 10-20 ms, des
    centaines par tranche — surtout dans le silence (tacet d'« Under the
    Bridge ») et les fins bruitées. Un morceau sain reste sous ~110 notes."""
    par = {}
    for n in notes: par.setdefault(int(n["debut_s"] // tranche), []).append(n)
    garde = []
    for k, ns in sorted(par.items()):
        cles = {(round(n["debut_s"], 2), n["hauteur"]) for n in ns}
        if len(ns) > max_notes or len(ns) - len(cles) > max_doublons: continue
        vues = set()
        for n in ns:
            c = (round(n["debut_s"], 2), n["hauteur"])
            if c not in vues: vues.add(c); garde.append(n)
    return garde

def main():
    a = argparse.ArgumentParser()
    a.add_argument("--taille", default="medium")
    a.add_argument("--instruments", default="electric_bass,acoustic_bass")
    a.add_argument("--porte", action="store_true", help="porte d'énergie, comme Basic Pitch")
    a.add_argument("ids", nargs="*")
    args = a.parse_args()
    modele = None
    def charger():
        from muscriptor.transcription_model import TranscriptionModel
        return TranscriptionModel.load_model(args.taille)
    conf = json.load(open(os.path.join(banc.ICI, "morceaux.json")))
    tot = dict(ref=0, nous=0, exactes=0, ref_tab=0, nous_tab=0, exactes_tab=0, doigtes=0, doigtes_ref=0)
    for m in conf["morceaux"]:
        if args.ids and m["id"] not in args.ids: continue
        d = os.path.join(banc.CACHE, m["id"])
        if not reference.fichiers(d) or not os.path.isdir(os.path.join(d, "stems")): continue
        d, basse, puls = banc.preparer_audio(m)
        brut = os.path.join(d, f"muscriptor-{args.taille}-notes.json")
        if not os.path.exists(brut):
            modele = modele or charger()
            t0 = time.time()
            json.dump(transcrire(modele, basse, args.instruments.split(",")), open(brut, "w"))
            print(f"  {m['id']} : {time.time() - t0:.0f} s", file=sys.stderr)
        propre = os.path.join(d, f"muscriptor-{args.taille}-notes-propres.json")
        json.dump(nettoyer(json.load(open(brut))), open(propre, "w"))
        tr = os.path.join(d, f"transcription-muscriptor-{args.taille}.json")
        cmd = [banc.BANC, "notes", propre, puls, tr] + ([basse] if args.porte else [])
        subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, cwd=banc.RACINE)
        ref = reference.extraire_dossier(d, m.get("accordage"))
        phase, N, R, chemin, st = comparer.comparer(json.load(open(tr)), ref)
        for k in tot: tot[k] += st[k]
        f1 = 2 * st["exactes"] / max(1, st["ref"] + st["nous"])
        f1t = 2 * st["exactes_tab"] / max(1, st["ref_tab"] + st["nous_tab"])
        print(f"{m['id']:24} F1 {f1:.2f}  F1tab {f1t:.2f}  doigté {st['doigtes'] / max(1, st['doigtes_ref']):.2f}", flush=True)
    print(f"TOTAL MuScriptor {args.taille} : F1 {2 * tot['exactes'] / max(1, tot['ref'] + tot['nous']):.3f}  "
          f"F1tab {2 * tot['exactes_tab'] / max(1, tot['ref_tab'] + tot['nous_tab']):.3f}  "
          f"doigtés {tot['doigtes'] / max(1, tot['doigtes_ref']):.3f}")

if __name__ == "__main__":
    main()
