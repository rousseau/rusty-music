#!/usr/bin/env python3
"""Bilan de la sonde de popularité — lit `echantillon.json` + `cache.json`.

Répond aux trois questions de la phase 0 :

  1. couverture de chaque source (globale, et ventilée par effectif d'artiste) ;
  2. fiabilité du rapprochement Deezer (taux de non-concordance + exemples à
     vérifier à la main) ;
  3. accord ListenBrainz ↔ Deezer (corrélation de rang de Spearman).

    python3 rapport.py
"""

import json
import os
import statistics

ICI = os.path.dirname(os.path.abspath(__file__))
ech = json.load(open(os.path.join(ICI, "echantillon.json"), encoding="utf-8"))
cache = json.load(open(os.path.join(ICI, "cache.json"), encoding="utf-8"))

N = len(ech)


def lb_rec(e):
    v = cache["lb_recording"].get(e["recording_mbid"])
    return v if v and v.get("ecoutes", 0) > 0 else None


def lb_rg(e):
    if not e["rg_mbid"]:
        return None
    v = cache["lb_release_group"].get(e["rg_mbid"])
    return v if v and v.get("ecoutes", 0) > 0 else None


def dz_track(e):
    v = cache["deezer_track"].get(e["recording_mbid"]) or {}
    return v if v.get("trouve") and v.get("concordant") and v.get("rank", 0) > 0 else None


def dz_album(e):
    v = cache["deezer_album"].get(e["recording_mbid"]) or {}
    return v if v.get("trouve") and v.get("concordant") and v.get("fans", 0) > 0 else None


def pc(n, d=N):
    return f"{n:3d}/{d}  {100 * n / d:5.1f} %" if d else "   —"


def spearman(paires):
    xs = [a for a, _ in paires]
    ys = [b for _, b in paires]
    if len(paires) < 5 or len(set(xs)) < 2 or len(set(ys)) < 2:
        return None
    return statistics.correlation(xs, ys, method="ranked")


# --------------------------------------------------------------------- 1. couverture
print(f"\n=== Couverture — échantillon de {N} morceaux ===\n")
n_rg = sum(1 for e in ech if e["rg_mbid"])
print(f"release-group résolu (lien titre normalisé)   {pc(n_rg)}")
print()
print(f"ListenBrainz — enregistrement                 {pc(sum(1 for e in ech if lb_rec(e)))}")
print(f"ListenBrainz — release-group  (sur {n_rg})       "
      f"{pc(sum(1 for e in ech if lb_rg(e)), n_rg)}")
print(f"ListenBrainz — enreg. OU album (échelon réel)  "
      f"{pc(sum(1 for e in ech if lb_rec(e) or lb_rg(e)))}")
print()
dz_tr_found = sum(1 for e in ech if (cache['deezer_track'].get(e['recording_mbid']) or {}).get('trouve'))
dz_al_found = sum(1 for e in ech if (cache['deezer_album'].get(e['recording_mbid']) or {}).get('trouve'))
print(f"Deezer — piste : un résultat                   {pc(dz_tr_found)}")
print(f"Deezer — piste : résultat concordant + rank>0  {pc(sum(1 for e in ech if dz_track(e)))}")
print(f"Deezer — album : un résultat                   {pc(dz_al_found)}")
print(f"Deezer — album : résultat concordant + fans>0  {pc(sum(1 for e in ech if dz_album(e)))}")
print()
avec_jauge = sum(1 for e in ech if lb_rec(e) or lb_rg(e) or dz_track(e) or dz_album(e))
print(f"AU MOINS UNE SOURCE (→ jauge affichable)       {pc(avec_jauge)}")
print(f"AUCUNE source (→ grisé)                        {pc(N - avec_jauge)}")

# ventilation par effectif d'artiste (quartiles)
effs = sorted(e["effectif_artiste"] for e in ech)
q1, q2, q3 = effs[N // 4], effs[N // 2], effs[3 * N // 4]
print(f"\n--- par effectif d'artiste (quartiles : ≤{q1}, ≤{q2}, ≤{q3}, +) ---")
seuils = [("≤%d" % q1, lambda x: x <= q1),
          ("≤%d" % q2, lambda x: q1 < x <= q2),
          ("≤%d" % q3, lambda x: q2 < x <= q3),
          (">%d" % q3, lambda x: x > q3)]
for nom, f in seuils:
    grp = [e for e in ech if f(e["effectif_artiste"])]
    lb = sum(1 for e in grp if lb_rec(e) or lb_rg(e))
    dz = sum(1 for e in grp if dz_track(e) or dz_album(e))
    any_ = sum(1 for e in grp if lb_rec(e) or lb_rg(e) or dz_track(e) or dz_album(e))
    print(f"  {nom:>6}  n={len(grp):3d}   LB {pc(lb, len(grp))}   "
          f"Deezer {pc(dz, len(grp))}   une source {pc(any_, len(grp))}")

# --------------------------------------------------- 2. fiabilité Deezer
print("\n\n=== Fiabilité du rapprochement Deezer ===\n")
for kind in ("deezer_track", "deezer_album"):
    trouves = [(e, cache[kind][e["recording_mbid"]]) for e in ech
               if (cache[kind].get(e["recording_mbid"]) or {}).get("trouve")]
    disc = [(e, v) for e, v in trouves if not v.get("concordant")]
    print(f"{kind:13s} : {len(trouves)} résultats, "
          f"{len(disc)} artiste non concordant ({100 * len(disc) / max(len(trouves), 1):.0f} %)")

print("\n--- 20 rapprochements 'piste' concordants, à vérifier à l'œil "
      "(attendu → rendu) ---")
conc = [(e, cache["deezer_track"][e["recording_mbid"]]) for e in ech
        if (cache["deezer_track"].get(e["recording_mbid"]) or {}).get("concordant")]
for e, v in conc[:20]:
    print(f"  {e['artiste'][:22]:22s} — {e['titre'][:32]:32s}  →  "
          f"{v['artiste_rendu'][:22]:22s} — {v['titre_rendu'][:32]:32s}  rank={v.get('rank')}")

print("\n--- non concordants (piste) ---")
for e, v in [(e, cache["deezer_track"][e["recording_mbid"]]) for e in ech
             if (cache["deezer_track"].get(e["recording_mbid"]) or {}).get("trouve")
             and not (cache["deezer_track"].get(e["recording_mbid"]) or {}).get("concordant")][:15]:
    print(f"  {e['artiste'][:24]:24s} — {e['titre'][:30]:30s}  →  "
          f"{v['artiste_rendu'][:24]:24s} — {v['titre_rendu'][:30]:30s}")

# --------------------------------------------------- 3. accord entre sources
print("\n\n=== Accord entre sources (Spearman sur les rangs) ===\n")

pa = [(lb_rec(e)["ecoutes"], dz_track(e)["rank"]) for e in ech if lb_rec(e) and dz_track(e)]
r = spearman(pa)
print(f"LB enregistrement  ↔  Deezer rank piste     n={len(pa):3d}   "
      f"ρ = {r:.3f}" if r is not None else
      f"LB enregistrement  ↔  Deezer rank piste     n={len(pa):3d}   (trop peu)")

pb = [(lb_rg(e)["ecoutes"], dz_album(e)["fans"]) for e in ech if lb_rg(e) and dz_album(e)]
r = spearman(pb)
print(f"LB release-group   ↔  Deezer fans album     n={len(pb):3d}   "
      f"ρ = {r:.3f}" if r is not None else
      f"LB release-group   ↔  Deezer fans album     n={len(pb):3d}   (trop peu)")

pc_ = [(lb_rec(e)["ecoutes"], lb_rg(e)["ecoutes"]) for e in ech if lb_rec(e) and lb_rg(e)]
r = spearman(pc_)
print(f"LB enregistrement  ↔  LB release-group      n={len(pc_):3d}   "
      f"ρ = {r:.3f}" if r is not None else
      f"LB enregistrement  ↔  LB release-group      n={len(pc_):3d}   (trop peu)")

# --------------------------------------------------- échelles brutes
print("\n\n=== Échelles brutes (min / médiane / max) ===\n")


def stats(vals):
    vals = sorted(v for v in vals if v is not None)
    if not vals:
        return "—"
    return f"{vals[0]:>10,d} / {int(statistics.median(vals)):>10,d} / {vals[-1]:>12,d}   (n={len(vals)})"


print("LB enreg. écoutes   ", stats([lb_rec(e)["ecoutes"] for e in ech if lb_rec(e)]))
print("LB enreg. auditeurs ", stats([lb_rec(e)["auditeurs"] for e in ech if lb_rec(e)]))
print("LB r-g   écoutes    ", stats([lb_rg(e)["ecoutes"] for e in ech if lb_rg(e)]))
print("Deezer piste rank   ", stats([dz_track(e)["rank"] for e in ech if dz_track(e)]))
print("Deezer album fans   ", stats([dz_album(e)["fans"] for e in ech if dz_album(e)]))
print()
