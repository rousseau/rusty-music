# SPDX-License-Identifier: GPL-3.0-or-later
"""Banc F2, temps 4 : les trois classements, leurs accords, et les greffes à
écouter à l'aveugle.

    python -I classer.py <plan.json> <racine> <groove.md> <cocola.json>

- **CLAP seul** : l'ordre des voisins (`rang_clap`, cosine du morceau entier).
- **Groove de F1** : la note de `greffon::noter_groove` sans intention
  (−2,5·|Δ syncope| − |ln(densité / densité d'origine)|) ajoutée au rang
  sonore (1 − rang/n), exactement comme l'application.
- **COCOLA** : le score `BOTH` moyen sur huit fenêtres (`cocola.py`).

Écrit sous `<racine>` :

- `classement.json` et `classement.md` : les trois ordres par source, les
  accords (premier choix commun, tau de Kendall), le rang de la batterie
  d'origine pour COCOLA ;
- `ecoute/` : pour chaque source, un extrait de 30 s par premier choix
  distinct (`07-b.wav`…), lettres tirées au sort, et l'extrait d'origine
  (`07-ref.wav`) pour situer ; `ecoute/notes.tsv`, à remplir ;
- `cle.json` (hors du dossier d'écoute) : quelle lettre vient de quelle
  méthode.

L'extrait : 30 s depuis le premier temps de mesure le plus proche de 35 %
du morceau. La batterie greffée est ramenée au RMS de la batterie d'origine
sur l'extrait (le niveau ne doit pas décider), fondus de 0,5 s.
"""

import itertools
import json
import os
import random
import re
import sys

import numpy as np
import soundfile as sf

EXTRAIT = 30.0
FONDU = 0.5
GRAINE = 20261109


def lire_groove(chemin):
    g = {}
    for ligne in open(chemin):
        m = re.match(r"\| (\d+) \| ([\d.]+) \| ([\d.]+) \| ([^|]+) \|", ligne)
        if m:
            sw = m.group(4).strip()
            g[int(m.group(1))] = dict(syncope=float(m.group(2)),
                                      densite=float(m.group(3)),
                                      swing=None if sw == "—" else float(sw))
    return g


def note_groove(g, o):
    """`noter_groove` sans intention (apps/desktop/src/greffon.rs)."""
    return -abs(g["syncope"] - o["syncope"]) * 2.5 - abs(
        np.log(max(g["densite"], 0.01) / max(o["densite"], 0.01)))


def kendall(a, b):
    """Tau de Kendall entre deux ordres d'une même liste."""
    ra = {x: i for i, x in enumerate(a)}
    rb = {x: i for i, x in enumerate(b)}
    commun = [x for x in a if x in rb]
    c = d = 0
    for x, y in itertools.combinations(commun, 2):
        s = (ra[x] - ra[y]) * (rb[x] - rb[y])
        c += s > 0
        d += s < 0
    return (c - d) / max(1, c + d)


def stem(racine, sid, nom):
    d = os.path.join(racine, "stems", str(sid))
    f = [x for x in os.listdir(d) if x.endswith(f"{nom}.wav") and not x.startswith(".")]
    return os.path.join(d, f[0])


def extrait(racine, sid, n_total, sr):
    p = json.load(open(os.path.join(racine, "stems", str(sid), "pulsation.json")))
    cible = 0.35 * n_total / sr
    premiers = p.get("premiers_temps") or p.get("temps") or [cible]
    t0 = min(premiers, key=lambda t: abs(t - cible))
    a = int(t0 * sr)
    return a, min(n_total, a + int(EXTRAIT * sr))


def ecrire(chemin, x, sr):
    n = len(x)
    f = int(FONDU * sr)
    env = np.ones(n, dtype=np.float32)
    env[:f] = np.linspace(0, 1, f)
    env[-f:] = np.linspace(1, 0, f)
    x = x * env[:, None]
    x /= max(1.0, float(np.abs(x).max()) / 0.95)
    sf.write(chemin, x, sr, subtype="PCM_16")


def rms(x):
    return float(np.sqrt(np.mean(x ** 2)) + 1e-9)


def main():
    plan_p, racine, groove_p, cocola_p = sys.argv[1:5]
    plan = json.load(open(plan_p))
    groove = lire_groove(groove_p)
    cocola = json.load(open(cocola_p))
    journal = {}
    for ligne in open(os.path.join(racine, "greffes.jsonl")):
        j = json.loads(ligne)
        journal[(j["source"], j["candidat"])] = j
    titres = {m["id"]: f"{m['artist']} — {m['title']}" for m in plan["morceaux"]}

    rng = random.Random(GRAINE)
    ecoute = os.path.join(racine, "ecoute")
    if cocola:
        os.makedirs(ecoute, exist_ok=True)
    cle, resultats, notes_tsv = {}, [], ["source\tfichier\tnote (1 à 5)\tremarque"]
    for k, s in enumerate(plan["sources"], 1):
        sid = s["id"]
        cands = [c["id"] for c in s["candidats"]
                 if os.path.isfile(os.path.join(racine, "greffes", str(sid), f"{c['id']}.wav"))]
        n = len(s["candidats"])
        rang = {c["id"]: i for i, c in enumerate(s["candidats"])}
        o = groove.get(sid)
        sc_groove = {}
        for c in cands:
            sonore = 1 - rang[c] / n
            g = groove.get(c)
            sc_groove[c] = sonore + (note_groove(g, o) if (g and o) else -10)
        co = cocola.get(str(sid), {})
        ordres = {
            "clap": sorted(cands, key=lambda c: rang[c]),
            "groove": sorted(cands, key=lambda c: -sc_groove[c]),
            "cocola": sorted([c for c in cands if str(c) in co],
                             key=lambda c: -co[str(c)]["both"]),
        }
        rang_orig = None
        if "original" in co:
            rang_orig = 1 + sum(co[str(c)]["both"] > co["original"]["both"]
                                for c in ordres["cocola"])
        premiers = {m: v[0] for m, v in ordres.items() if v}
        r = dict(source=sid, titre=titres[sid], ordres=ordres, premiers=premiers,
                 cocola_original=rang_orig,
                 cocola_percussif_premier=max(
                     (c for c in cands if str(c) in co),
                     key=lambda c: co[str(c)]["percussive"], default=None),
                 tau=dict(clap_groove=kendall(ordres["clap"], ordres["groove"]),
                          clap_cocola=kendall(ordres["clap"], ordres["cocola"]),
                          groove_cocola=kendall(ordres["groove"], ordres["cocola"])),
                 modes={c: journal.get((sid, c), {}).get("mode") for c in cands})
        resultats.append(r)

        # Les extraits à l'aveugle — seulement avec les trois méthodes : une
        # écoute à deux méthodes serait à refaire.
        if not cocola:
            continue
        ctx, sr = sf.read(os.path.join(racine, "greffes", str(sid), "contexte.wav"),
                          dtype="float32", always_2d=True)
        orig, _ = sf.read(stem(racine, sid, "drums"), dtype="float32", always_2d=True)
        a, b = extrait(racine, sid, len(ctx), sr)
        ref_rms = rms(orig[a:b])
        ecrire(os.path.join(ecoute, f"{k:02d}-ref.wav"), ctx[a:b] + orig[a:b], sr)
        distincts = list(dict.fromkeys(premiers.values()))
        lettres = rng.sample("abc", len(distincts))
        cle[f"{k:02d}"] = dict(source=titres[sid], fichiers={})
        for lettre, c in sorted(zip(lettres, distincts)):
            dr, _ = sf.read(os.path.join(racine, "greffes", str(sid), f"{c}.wav"),
                            dtype="float32", always_2d=True)
            seg = dr[a:b]
            seg = seg * (ref_rms / rms(seg)) if rms(seg) > 1e-4 else seg
            ecrire(os.path.join(ecoute, f"{k:02d}-{lettre}.wav"), ctx[a:b] + seg, sr)
            cle[f"{k:02d}"]["fichiers"][lettre] = dict(
                candidat=c, titre=titres[c],
                methodes=[m for m, p in premiers.items() if p == c])
            notes_tsv.append(f"{k:02d}\t{k:02d}-{lettre}.wav\t\t")
        print(f"{k:02d} {titres[sid]} : {len(distincts)} extrait(s), "
              f"original {rang_orig}/{len(ordres['cocola']) + 1} pour COCOLA", flush=True)

    if cocola:
        with open(os.path.join(ecoute, "notes.tsv"), "w") as f:
            f.write("\n".join(notes_tsv) + "\n")
    json.dump(cle, open(os.path.join(racine, "cle.json"), "w"), ensure_ascii=False, indent=1)
    json.dump(resultats, open(os.path.join(racine, "classement.json"), "w"),
              ensure_ascii=False, indent=1)

    # Le résumé.
    L = ["| # | source | CLAP | groove F1 | COCOLA | τ CLAP·groove | τ CLAP·COCOLA | τ groove·COCOLA | original (COCOLA) |",
         "|---|---|---|---|---|---|---|---|---|"]
    for k, r in enumerate(resultats, 1):
        p = r["premiers"]
        L.append(f"| {k:02d} | {r['titre']} | {titres.get(p.get('clap'), '—')} | "
                 f"{titres.get(p.get('groove'), '—')} | {titres.get(p.get('cocola'), '—')} | "
                 f"{r['tau']['clap_groove']:+.2f} | {r['tau']['clap_cocola']:+.2f} | "
                 f"{r['tau']['groove_cocola']:+.2f} | {r['cocola_original']} |")
    m = lambda cle_: np.mean([r["tau"][cle_] for r in resultats])
    accord = lambda a, b: sum(r["premiers"].get(a) == r["premiers"].get(b) for r in resultats)
    orig = [r["cocola_original"] for r in resultats if r["cocola_original"]]
    L += ["",
          f"Sources : {len(resultats)}. Premier choix commun : CLAP·groove {accord('clap', 'groove')}, "
          f"CLAP·COCOLA {accord('clap', 'cocola')}, groove·COCOLA {accord('groove', 'cocola')}.",
          f"Tau de Kendall moyen : CLAP·groove {m('clap_groove'):+.2f}, CLAP·COCOLA "
          f"{m('clap_cocola'):+.2f}, groove·COCOLA {m('groove_cocola'):+.2f}.",
          f"Batterie d'origine chez COCOLA : rang médian {np.median(orig) if orig else float('nan'):.0f} sur 11, "
          f"première {sum(x == 1 for x in orig)} fois sur {len(orig)}.",
          f"COCOLA percussif d'accord avec COCOLA sur le premier : "
          f"{sum(r['cocola_percussif_premier'] == r['premiers'].get('cocola') for r in resultats)} fois.",
          f"Greffes d'un bloc (métrique refusée) : "
          f"{sum(1 for r in resultats for v in r['modes'].values() if v and v.startswith('bloc'))}."]
    open(os.path.join(racine, "classement.md"), "w").write("\n".join(L) + "\n")
    print("\n".join(L[-6:]))


if __name__ == "__main__":
    main()
