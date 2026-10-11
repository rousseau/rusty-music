# SPDX-License-Identifier: GPL-3.0-or-later
"""Référence Python d'all-in-one (Kim et Nam, WASPAA 2023, code et poids MIT)
sur les morceaux appariés à des sections annotées
(`experiments/annotations/apparier.py`), avant tout portage en Burn
(`docs/plan-editer-pratique-creation.md`, 2.4).

- Les stems sont **les nôtres** : `rusty-music demix` (HTDemucs standard,
  celui de l'application), rangés comme all-in-one les attend.
- NATTEN est remplacé par `natten_pur/` (PyTorch pur), faute de roue pour
  macOS ; c'est aussi la référence du portage.
- Mesures de mir_eval : frontières à 0,5 et 3 s, F par paires (accord des
  étiquettes, indépendant de leur nom), sur toute la durée commune.
- Calage : décalage des temps annotés s'il y en a (`pulsation.py`), sinon
  début du son (somme des stems) contre fin du silence initial annoté,
  sinon rien. Le calage par les temps n'est sûr qu'à une mesure près.
- Harmonix (base d'entraînement d'all-in-one) est noté à part, hors total.

Environnement à part (torch, madmom, demucs, allin1 sans ses dépendances) :

    PYTHONPATH=natten_pur <env>/bin/python allinone_ref.py [--limite N]
"""
import argparse, glob, json, os, subprocess, sys
import numpy as np
import mir_eval, soundfile as sf

ICI = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(ICI, "natten_pur"))
sys.path.insert(0, os.path.join(ICI, "..", "annotations"))
import apparier as ap, pulsation as pu

RACINE = os.path.abspath(os.path.join(ICI, "..", ".."))
CLI = os.path.join(RACINE, "target/release/rusty-music")
CACHE = os.path.join(ap.DEV, "banc-annotations")
# Poids d'all-in-one (8 plis, Hugging Face) sur le disque de développement.
os.environ.setdefault("HF_HUB_CACHE", os.path.join(ap.DEV, "hf", "hub"))
STEMS = {"drums": "drums", "bass": "bass", "other": "other", "vocals": "vocals"}

def sections(e):
    """Intervalles `[n, 2]` et étiquettes, en minuscules, silences compris."""
    lignes = [l.split() for l in open(e["sections"], errors="replace") if l.strip()]
    if e["jeu"] == "isophonics":
        iv = [(float(l[0]), float(l[1])) for l in lignes]
        lab = [" ".join(l[2:]).lower() or "?" for l in lignes]
    else:
        t = [float(l[0]) for l in lignes]
        lab = [" ".join(l[1:]).lower() for l in lignes]
        iv = list(zip(t[:-1], t[1:]))
        lab = lab[:-1]
    return np.array(iv), lab

def debut_du_son(dossier, seuil_db=-50):
    """Début du son du mélange, recomposé par la somme des stems (un stem seul,
    la basse par exemple, entre souvent après les autres)."""
    x, sr = 0, None
    for s in STEMS:
        y, sr = sf.read(os.path.join(dossier, f"{s}.wav"), always_2d=True)
        x = x + y.mean(1)
    trame = sr // 100
    rms = np.sqrt(np.add.reduceat(x ** 2, np.arange(0, len(x), trame)) / trame)
    fort = np.nonzero(20 * np.log10(rms + 1e-12) > seuil_db)[0]
    return fort[0] / 100 if len(fort) else 0.0

def calage(e, est_pulsation, stems):
    d = est_pulsation.get(e["chemin"])
    if e.get("temps") and d:
        ref_t, ref_p = pu.temps_annotes(e)
        dt, large = pu.caler(ref_t, ref_p, np.array(d["temps"]), np.array(d["premiers"]))
        # Même règle que pulsation.py : sous 0,5, autre édition, calage douteux.
        return dt, "temps" if large >= 0.5 else "douteux"
    iv, lab = sections(e)
    if lab and lab[0] == "silence":
        return debut_du_son(stems) - iv[0][1], "silence"
    return 0.0, "aucun"

def demixer(e, dossier):
    """Nos stems, sous le nom qu'attend all-in-one (`htdemucs/<nom>/bass.wav`…)."""
    if all(os.path.exists(os.path.join(dossier, f"{s}.wav")) for s in STEMS): return
    tmp = dossier + ".tmp"
    os.makedirs(tmp, exist_ok=True)
    subprocess.run([CLI, "demix", e["chemin"], "--out", tmp], check=True, capture_output=True, cwd=RACINE)
    for s in STEMS:
        f = [g for g in glob.glob(os.path.join(tmp, f"*— {s}.wav")) if not os.path.basename(g).startswith("._")]
        os.replace(f[0], os.path.join(tmp, f"{s}.wav"))
    os.replace(tmp, dossier)

def main():
    a = argparse.ArgumentParser()
    a.add_argument("--limite", type=int)
    args = a.parse_args()
    import allin1
    morceaux = morceaux_uniques(args.limite)
    demix, spec, sortie = (os.path.join(CACHE, x) for x in ("demix", "spec", "allinone"))
    os.makedirs(sortie, exist_ok=True)
    for e in morceaux:
        demixer(e, os.path.join(demix, "htdemucs", nom(e)))
    chemins = [e["chemin"] for e in morceaux]
    allin1.analyze(chemins, out_dir=sortie, demix_dir=demix, spec_dir=spec, device="cpu",
                   keep_byproducts=True, multiprocess=False)
    noter(morceaux, lambda e: json.load(open(os.path.join(sortie, nom(e) + ".json")))["segments"])

def nom(e):
    return os.path.splitext(os.path.basename(e["chemin"]))[0]

def morceaux_uniques(limite=None):
    """Morceaux à sections annotées. all-in-one nomme tout d'après le nom du
    fichier (liens résolus) : deux fichiers de même nom sont écartés."""
    morceaux = [e for e in json.load(open(os.path.join(ap.ANN, "appariement.json"))) if e.get("sections")]
    if limite: morceaux = morceaux[:limite]
    noms = {}
    for e in morceaux: noms.setdefault(nom(e), []).append(e)
    return [v[0] for v in noms.values() if len(v) == 1]

def positifs(iv, lab):
    """Retire les intervalles de durée nulle (bornes en double), que mir_eval refuse."""
    garde = iv[:, 1] - iv[:, 0] > 1e-6
    return iv[garde], [l for l, g in zip(lab, garde) if g]

def noter(morceaux, segments):
    """`segments(e)` → liste de `{start, end, label}` ; imprime les mesures."""
    puls = os.path.join(CACHE, "pulsation.jsonl")
    est_pulsation = {d["chemin"]: d for d in map(json.loads, open(puls))} if os.path.exists(puls) else {}
    res = []
    for e in morceaux:
        dt, comment = calage(e, est_pulsation, os.path.join(CACHE, "demix", "htdemucs", nom(e)))
        if comment == "douteux":
            print(f"{e['jeu']:10} {e['artiste'][:16]:16} {e['titre'][:26]:26} écarté (calage douteux)")
            continue
        segs = segments(e)
        ref_iv, ref_lab = sections(e)
        ref_iv = ref_iv + dt
        est_iv = np.array([(s["start"], s["end"]) for s in segs])
        est_lab = [s["label"] for s in segs]
        # Durée commune : mir_eval veut deux découpages du même intervalle.
        debut, fin = max(ref_iv[0, 0], est_iv[0, 0], 0), min(ref_iv[-1, 1], est_iv[-1, 1])
        ref_iv, ref_lab = mir_eval.util.adjust_intervals(ref_iv, ref_lab, t_min=debut, t_max=fin)
        est_iv, est_lab = mir_eval.util.adjust_intervals(est_iv, est_lab, t_min=debut, t_max=fin)
        ref_iv, est_iv = ref_iv - debut, est_iv - debut  # mir_eval veut partir de 0
        (ref_iv, ref_lab), (est_iv, est_lab) = positifs(ref_iv, ref_lab), positifs(est_iv, est_lab)
        f05 = mir_eval.segment.detection(ref_iv, est_iv, window=0.5, trim=True)[2]
        f3 = mir_eval.segment.detection(ref_iv, est_iv, window=3, trim=True)[2]
        pf = mir_eval.segment.pairwise(ref_iv, ref_lab, est_iv, est_lab)[2]
        res.append((e["jeu"], f05, f3, pf))
        print(f"{e['jeu']:10} {e['artiste'][:16]:16} {e['titre'][:26]:26} F0,5 {f05:.2f} F3 {f3:.2f} "
              f"paires {pf:.2f}  (calage {comment} {dt:+.2f} s)")
    print()
    # Harmonix est la base d'entraînement d'all-in-one, et son calage par les
    # temps n'est sûr qu'à une mesure près (pas de silence annoté) : hors total.
    for jeu in sorted({r[0] for r in res}) + ["tous"]:
        v = np.array([r[1:] for r in res if r[0] == jeu or (jeu == "tous" and r[0] != "harmonix")])
        print(f"{jeu:10} {len(v):3} morceaux — F0,5 {v[:, 0].mean():.3f} · F3 {v[:, 1].mean():.3f} · paires {v[:, 2].mean():.3f}")

if __name__ == "__main__":
    main()
