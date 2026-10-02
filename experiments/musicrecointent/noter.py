#!/usr/bin/env python3
"""Note les sorties de `robustesse` sur MusicRecoIntent.

    python3 experiments/musicrecointent/noter.py data/sortie-A.json [data/sortie-B.json ...]

Ne note que ce que le schéma de Lama (`ollama::InterpretationLlm`) sait
exprimer :
  Genre +/-        <-> genres / exclure_genres
  NE (+ ou ~)      <-> seed_artiste / seed_morceau / arrivee_* (ou une étape)
  NE -             <-> exclure_artistes
  Decade +         <-> annee_min / annee_max
Mood, Instrument, ListeningContext, Country n'ont pas de filtre dur : on
mesure seulement si le terme survit quelque part (étapes CLAP, genres).
Deux lectures par fichier : [brut] = ce que le modèle a dit, [spec] = ce qui
reste après le filtrage des genres par le vocabulaire de la bibliothèque.
Les annotations étant incomplètes, « inventé » veut dire : rempli alors que
rien dans l'annotation (toutes catégories) ne l'appuie — une borne basse, pas
une vérité.
"""
import difflib, json, os, re, sys, unicodedata

ICI = os.path.dirname(os.path.abspath(__file__))

def norm(s):
    s = unicodedata.normalize("NFKD", str(s)).encode("ascii", "ignore").decode().lower()
    s = re.sub(r"\b(music|musics|songs?|artists?|band)\b", " ", s)
    return re.sub(r"[^a-z0-9]+", " ", s).strip()

def proche(a, b):
    a, b = norm(a), norm(b)
    if not a or not b: return False
    if a == b or (len(a) > 2 and a in b) or (len(b) > 2 and b in a): return True
    return difflib.SequenceMatcher(None, a, b).ratio() >= 0.85

def dans(terme, liste): return any(proche(terme, x) for x in liste)

def decennies(termes):
    """Plage d'années attendue d'après les termes Decade (« 80s », « 2015 »)."""
    bornes = []
    for t in termes:
        m = re.search(r"\b(?:(19|20)?(\d)0)'?s\b", t)
        a = re.fullmatch(r"\s*((?:19|20)\d\d)\s*", t)
        if a: bornes.append((int(a[1]), int(a[1])))
        elif m:
            siecle = m[1] or ("19" if int(m[2]) >= 3 else "20")
            d = int(siecle + m[2] + "0"); bornes.append((d, d + 9))
    return (min(b[0] for b in bornes), max(b[1] for b in bornes)) if bornes else None

def texte_spec(spec):
    morceaux = list(spec.get("etapes") or []) + list(spec.get("genres") or []) + [spec.get("reformulation") or ""]
    for p in spec.get("parties") or []: morceaux += list(p.get("genres") or [])
    return morceaux

class Cpt:
    def __init__(self): self.ok = 0; self.n = 0
    def add(self, b): self.n += 1; self.ok += bool(b)
    def __str__(self): return f"{100*self.ok/self.n:5.1f}% ({self.ok}/{self.n})" if self.n else "    —"

def noter(fichier, annot, source="spec"):
    res = json.load(open(fichier))
    m = {k: Cpt() for k in [
        "genre+ dans genres", "genre+ retenu (genres/étapes)", "genre- dans exclure_genres", "genre- inversé (dans genres)",
        "NE→seed/étape (≥1)", "NE- dans exclure_artistes", "NE- inversé (en seed)", "NE+~ exclu à tort",
        "décennie exacte (±1)", "décennie partielle",
        "mood/instr./contexte retenu (étapes/genres)",
        "exclusion inventée", "genre inventé", "seed inventé", "année inventée", "énergie inventée",
        "n/durée inventés", "bpm inventé", "popularité inventée", "reformulation vide"]}
    erreurs = 0; lat = []; exemples = {"genre- inversé (dans genres)": [], "exclusion inventée": [], "NE- inversé (en seed)": [], "NE+~ exclu à tort": []}
    for r in res["resultats"]:
        if "spec" not in r: erreurs += 1; continue
        sp, a = r["spec"], annot[r["id"]]; lat.append(r["s"])
        if source == "brut":   # ce que le modèle a dit, avant le filtrage par le vocabulaire de la bibliothèque
            try: sp = {**sp, **json.loads(sp["brut"])}
            except Exception: pass
        D = a["desc"]; termes = lambda cat, pol: [t for c, t, p in D if c == cat and p in pol]
        tous = [t for _, t, _ in D]
        genres = list(sp.get("genres") or []) + [g for p in sp.get("parties") or [] for g in p.get("genres") or []]
        exg, exa = sp.get("exclure_genres") or [], sp.get("exclure_artistes") or []
        seeds = [sp.get(k) for k in ("seed_artiste", "seed_morceau", "arrivee_artiste", "arrivee_morceau") if sp.get(k)]
        etapes = list(sp.get("etapes") or [])
        # genres
        for t in termes("Genre", "+"):
            m["genre+ dans genres"].add(dans(t, genres))
            m["genre+ retenu (genres/étapes)"].add(dans(t, genres + etapes))
        for t in termes("Genre", "-"):
            m["genre- dans exclure_genres"].add(dans(t, exg))
            ko = dans(t, genres)
            m["genre- inversé (dans genres)"].add(ko)
            if ko and len(exemples["genre- inversé (dans genres)"]) < 4: exemples["genre- inversé (dans genres)"].append((r["prompt"][:150], t, genres))
        # entités nommées
        ne_pos, ne_neg = termes("NE", "+~"), termes("NE", "-")
        if ne_pos: m["NE→seed/étape (≥1)"].add(any(dans(t, seeds + etapes) for t in ne_pos))
        for t in ne_neg:
            m["NE- dans exclure_artistes"].add(dans(t, exa))
            ko = dans(t, seeds)
            m["NE- inversé (en seed)"].add(ko)
            if ko and len(exemples["NE- inversé (en seed)"]) < 4: exemples["NE- inversé (en seed)"].append((r["prompt"][:150], t, seeds))
        for t in ne_pos:
            ko = dans(t, exa)
            m["NE+~ exclu à tort"].add(ko)
            if ko and len(exemples["NE+~ exclu à tort"]) < 4: exemples["NE+~ exclu à tort"].append((r["prompt"][:150], t, exa))
        # décennie
        att = decennies(termes("Decade", "+"))
        if att:
            lo, hi = sp.get("annee_min"), sp.get("annee_max")
            exact = lo is not None and hi is not None and abs(lo - att[0]) <= 1 and abs(hi - att[1]) <= 1
            m["décennie exacte (±1)"].add(exact)
            m["décennie partielle"].add(exact or (lo is not None and abs(lo - att[0]) <= 1) or (hi is not None and abs(hi - att[1]) <= 1))
        # catégories sans filtre dur
        for c in ("Mood", "Instrument", "ListeningContext", "country"):
            for t in termes(c, "+"): m["mood/instr./contexte retenu (étapes/genres)"].add(dans(t, genres + etapes))
        # inventions (rien dans l'annotation, toutes catégories, ne l'appuie)
        a_neg = any(p == "-" for _, _, p in D)
        inv_ex = (exg or exa) and not a_neg
        m["exclusion inventée"].add(bool(inv_ex))
        if inv_ex and len(exemples["exclusion inventée"]) < 4: exemples["exclusion inventée"].append((r["prompt"][:150], "", exg + exa))
        if genres: m["genre inventé"].add(any(not dans(g, tous) for g in genres))
        else: m["genre inventé"].add(False)
        m["seed inventé"].add(bool(seeds) and not any(dans(s, [t for c, t, _ in D if c == "NE"]) for s in seeds))
        m["année inventée"].add((sp.get("annee_min") is not None or sp.get("annee_max") is not None) and not termes("Decade", "+-~"))
        m["énergie inventée"].add(bool(sp.get("energie")) and not any(c in ("Mood", "ListeningContext") for c, _, _ in D))
        chiffres = bool(re.search(r"\d", r["prompt"]))
        m["n/durée inventés"].add((sp.get("n") or sp.get("duree_minutes")) is not None and not chiffres)
        m["bpm inventé"].add((sp.get("bpm_min") is not None or sp.get("bpm_max") is not None) and not re.search(r"bpm|tempo|fast|slow", r["prompt"].lower()))
        m["popularité inventée"].add(bool(sp.get("popularite")) and not re.search(r"obscur|unknown|underrated|popular|known|mainstream|hidden|rare|hit", r["prompt"].lower()))
        m["reformulation vide"].add(not (sp.get("reformulation") or "").strip())
    lat.sort()
    return res["modele"] + (" [brut]" if source == "brut" else " [spec]"), m, erreurs, lat, exemples, len(res["resultats"])

def main():
    annot = json.load(open(os.path.join(ICI, "data", "annotations.json")))
    runs = [noter(f, annot, src) for f in sys.argv[1:] for src in ("brut", "spec")]
    noms = [r[0] for r in runs]
    print(f"{'':46s}" + "".join(f"{n:>26s}" for n in noms))
    for k in runs[0][1]:
        print(f"{k:46s}" + "".join(f"{str(r[1][k]):>26s}" for r in runs))
    print(f"{'erreurs d interprétation (JSON)':46s}" + "".join(f"{r[2]:>26d}" for r in runs))
    print(f"{'latence médiane / max (s)':46s}" + "".join(f"{r[3][len(r[3])//2]:>16.1f} /{r[3][-1]:>6.1f}" for r in runs))
    for nom, (_, _, _, _, ex, _) in zip(noms, runs):
        for k, v in ex.items():
            if v:
                print(f"\n[{nom}] {k} :")
                for p, t, x in v: print(f"   « {p} » terme={t!r} -> {x}")

main()
