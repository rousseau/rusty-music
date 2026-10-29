# SPDX-License-Identifier: GPL-3.0-or-later
"""Modèles de transcription externes au banc des livres
(`experiments/banc-transcription/externes.py` : tsumugi, YourMT3+), par la
même chaîne que MuScriptor (`muscriptor_banc.py`) : notes du stem de basse,
puis l'exemple `banc -- notes` (monophonie, accordage, doigtés,
quantification) et la même comparaison à la partition.

Avec `--union`, en plus : l'union des notes du modèle et de MuScriptor
(portage Burn, lot 16 — lancer `muscriptor_banc.py --rust 16` avant), borne
haute du rappel d'un vote ; avec deux modèles externes ou plus, le vote à
la majorité.

    python3 externe_banc.py tsumugi[,yourmt3] [--union] [id ...]
"""
import argparse, json, os, subprocess, sys
import banc, comparer, reference

sys.path.insert(0, os.path.join(banc.ICI, "..", "banc-transcription"))
import externes  # noqa: E402


def evaluer(m, d, puls, notes, nom):
    """F1, F1tab et doigtés d'une liste de notes, par la chaîne de l'application."""
    propre = os.path.join(d, f"{nom}-notes-propres.json")
    json.dump([{**n, "amplitude": 0.8, "corde": None, "frette": None} for n in notes], open(propre, "w"))
    tr = os.path.join(d, f"transcription-{nom}.json")
    subprocess.run([banc.BANC, "notes", propre, puls, tr], check=True, stdout=subprocess.DEVNULL, cwd=banc.RACINE)
    ref = reference.extraire_dossier(d, m.get("accordage"))
    return comparer.comparer(json.load(open(tr)), ref)[4]


def main():
    a = argparse.ArgumentParser()
    a.add_argument("modeles", help="parmi " + ",".join(externes.MODELES))
    a.add_argument("--union", action="store_true")
    a.add_argument("ids", nargs="*")
    args = a.parse_args()
    modeles = args.modeles.split(",")
    variantes = modeles + (["union"] + (["vote"] if len(modeles) >= 2 else []) if args.union else [])
    cles = ("ref", "nous", "exactes", "ref_tab", "nous_tab", "exactes_tab", "doigtes", "doigtes_ref")
    tot = {v: dict.fromkeys(cles, 0) for v in variantes}
    suffixe = "" if banc.DEMIX == "htdemucs_6s" else f"-{banc.DEMIX}"
    conf = json.load(open(os.path.join(banc.ICI, "morceaux.json")))
    for m in conf["morceaux"]:
        if args.ids and m["id"] not in args.ids: continue
        d = os.path.join(banc.CACHE, m["id"])
        if not reference.fichiers(d) or not os.path.isdir(os.path.join(d, "stems")): continue
        d, basse, puls = banc.preparer_audio(m)
        notes = {nom: externes.transcrire(nom, basse, os.path.join(d, f"{nom}{suffixe}-notes.json")) for nom in modeles}
        if args.union:
            mus = os.path.join(d, f"muscriptor-rust-lot16{suffixe}-notes.json")
            if not os.path.exists(mus):
                sys.exit(f"{mus} absent : lancer d'abord muscriptor_banc.py --rust 16")
            listes = [json.load(open(mus))] + [notes[n] for n in modeles]
            notes["union"] = externes.fusion(listes, quorum=1)
            if "vote" in variantes:
                notes["vote"] = externes.fusion(listes, quorum=(len(listes) + 1) // 2)
        ligne = f"{m['id']:24}"
        for v in variantes:
            st = evaluer(m, d, puls, notes[v], f"{v}{suffixe}")
            for k in cles: tot[v][k] += st[k]
            ligne += f"  {v[:7]} F1 {2 * st['exactes'] / max(1, st['ref'] + st['nous']):.2f}/{2 * st['exactes_tab'] / max(1, st['ref_tab'] + st['nous_tab']):.2f}"
        print(ligne, flush=True)
    for v, t in tot.items():
        print(f"TOTAL {v}{suffixe} : F1 {2 * t['exactes'] / max(1, t['ref'] + t['nous']):.3f}  "
              f"F1tab {2 * t['exactes_tab'] / max(1, t['ref_tab'] + t['nous_tab']):.3f}  "
              f"doigtés {t['doigtes'] / max(1, t['doigtes_ref']):.3f}")


if __name__ == "__main__":
    main()
