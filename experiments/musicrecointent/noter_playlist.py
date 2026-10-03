#!/usr/bin/env python3
"""Note les **playlists composées**, pas seulement la spec.

    python3 experiments/musicrecointent/noter_playlist.py \\
        data/compositions-X.json data/sortie-X.json

`compositions-X.json` : écrit par le test `robustesse_de_la_composition`
(`apps/desktop`, variable `RUSTY_SORTIE`) — les morceaux réellement choisis par
le moteur sur la vraie bibliothèque, plus son vocabulaire de genres et ses
artistes. `sortie-X.json` : la sortie de `robustesse` qui l'a nourri (les
specs). Les annotations viennent de `data/annotations.json`.

Mesure ce que l'utilisateur obtient : exclusions tenues, genres voulus
présents, décennie respectée, artistes de référence connus de la bibliothèque,
playlist assez longue, assouplissements. Un genre annoté qu'aucun genre de la
bibliothèque ne contient (« grindcore ») n'est pas évaluable : il est écarté
des taux de précision.
"""
import json, os, re, statistics, sys
from collections import Counter

from commun import norm, decennies

ICI = os.path.dirname(os.path.abspath(__file__))


def mots(t):
    return set(re.findall(r"[a-z0-9]+", norm(t)))


def genre_correspond(genre_piste, terme):
    """Même règle que le filtre de playlist (`genre_correspond`) : tous les mots du
    terme sont dans le genre ; rap ~ hip hop ; graphies collées (bigbeat)."""
    g, t = mots(genre_piste), mots(terme)
    if not t:
        return False
    if t <= g:
        return True
    if t == {"rap"} and {"hip", "hop"} <= g or t == {"hip", "hop"} and "rap" in g:
        return True
    return "".join(sorted(t)) in {"".join(sorted(g))} or "".join(norm(terme).split()) in "".join(norm(genre_piste).split())


def a_un_genre(piste, termes):
    return any(genre_correspond(g, t) for g in piste["genres"] for t in termes)


def artiste_nomme(piste, terme):
    a, t = " " + norm(piste.get("artiste") or "") + " ", norm(terme)
    return bool(t) and (" " + t + " ") in a


class Taux:
    def __init__(self):
        self.valeurs = []

    def add(self, v):
        self.valeurs.append(v)

    def __str__(self):
        v = self.valeurs
        return f"{100 * sum(v) / len(v):5.1f}%  (n={len(v)})" if v else "     —"


def main():
    compo = json.load(open(sys.argv[1]))
    specs = {r["id"]: r.get("spec") for r in json.load(open(sys.argv[2]))["resultats"]}
    annot = json.load(open(os.path.join(ICI, "data", "annotations.json")))
    vocab = compo["vocabulaire"]
    artistes = [norm(a) for a in compo["artistes"]]
    connu = lambda terme: any(genre_correspond(v, terme) for v in vocab)
    artiste_connu = lambda terme: any((" " + norm(terme) + " ") in (" " + a + " ") for a in artistes)

    m = {k: Taux() for k in [
        "playlist complète (≥ n voulus)", "playlist non vide (≥ 5 morceaux)", "sans assouplissement",
        "genre+ : précision, genre filtré dur", "genre+ : précision, CLAP seul", "genre+ : ≥ 80 % de morceaux conformes",
        "genre- : exclusion tenue (requêtes)", "genre- : morceaux fautifs",
        "NE- : exclusion tenue (requêtes)",
        "décennie : ≥ 90 % de morceaux dans la plage", "NE+~ : artiste connu de la bibliothèque", "NE+~ : artiste présent dans la playlist",
        "diversité : artistes distincts / morceaux"]}
    non_evaluable = 0
    adm = []
    exemples = {"genre- : exclusion tenue (requêtes)": [], "genre+ : ≥ 80 % de morceaux conformes": []}
    erreurs = 0
    for c in compo["compositions"]:
        if "route" not in c:
            erreurs += 1
            continue
        a, spec, route = annot[c["id"]], specs.get(c["id"]) or {}, c["route"]
        D = lambda cat, pol: [t for k, t, p in a["desc"] if k == cat and p in pol]
        n = len(route)
        m["playlist complète (≥ n voulus)"].add(n >= c["n_voulu"])
        m["playlist non vide (≥ 5 morceaux)"].add(n >= 5)
        m["sans assouplissement"].add(not c["relaches"])
        adm.append(min(c["admissibles"]) if c["admissibles"] else 0)
        if n:
            m["diversité : artistes distincts / morceaux"].add(len({p["artiste"] for p in route}) / n)
        # genres voulus
        gp = [t for t in D("Genre", "+") if connu(t)]
        if D("Genre", "+") and not gp:
            non_evaluable += 1
        if gp and n:
            prec = sum(a_un_genre(p, gp) for p in route) / n
            dur = bool(spec.get("genres")) or any(pa.get("genres") for pa in spec.get("parties") or [])
            m["genre+ : précision, genre filtré dur" if dur else "genre+ : précision, CLAP seul"].add(prec)
            m["genre+ : ≥ 80 % de morceaux conformes"].add(prec >= 0.8)
            if prec < 0.8 and len(exemples["genre+ : ≥ 80 % de morceaux conformes"]) < 6:
                exemples["genre+ : ≥ 80 % de morceaux conformes"].append((a["query"][:90], gp, f"{prec:.0%}", bool(dur), c["admissibles"]))
        # exclusions
        gm = [t for t in D("Genre", "-") if connu(t)]
        if gm and n:
            fautifs = sum(a_un_genre(p, gm) for p in route)
            m["genre- : exclusion tenue (requêtes)"].add(fautifs == 0)
            m["genre- : morceaux fautifs"].add(fautifs / n)
            if fautifs and len(exemples["genre- : exclusion tenue (requêtes)"]) < 6:
                exemples["genre- : exclusion tenue (requêtes)"].append((a["query"][:90], gm, f"{fautifs}/{n}", spec.get("exclure_genres"), spec.get("genres")))
        nm = D("NE", "-")
        if nm and n:
            m["NE- : exclusion tenue (requêtes)"].add(not any(artiste_nomme(p, t) for p in route for t in nm))
        # décennie
        att = decennies(D("Decade", "+"))
        if att and n:
            avec = [p for p in route if p["annee"]]
            if avec:
                m["décennie : ≥ 90 % de morceaux dans la plage"].add(sum(att[0] <= p["annee"] <= att[1] for p in avec) / len(avec) >= 0.9)
        # références
        for t in D("NE", "+~")[:1]:
            k = artiste_connu(t)
            m["NE+~ : artiste connu de la bibliothèque"].add(k)
            if k:
                m["NE+~ : artiste présent dans la playlist"].add(any(artiste_nomme(p, t) for p in route))

    def moy(t):
        return f"{100 * statistics.mean(t.valeurs):5.1f}%  (n={len(t.valeurs)})" if t.valeurs else "     —"
    print(f"{len(compo['compositions'])} compositions ({erreurs} sans playlist), bibliothèque : {compo['morceaux_placables']} morceaux\n")
    for k, t in m.items():
        pct = k.startswith(("genre+ : précision", "diversité", "genre- : morceaux"))
        print(f"  {k:46s} {moy(t) if pct else t}")
    print(f"  {'genres annotés non évaluables (inconnus)':46s} {non_evaluable} requêtes")
    adm.sort()
    print(f"  {'morceaux admissibles (min des parties)':46s} médiane {adm[len(adm)//2]}, 10ᵉ percentile {adm[len(adm)//10]}, < 50 : {sum(x < 50 for x in adm)} requêtes")
    for k, v in exemples.items():
        if v:
            print(f"\n{k} — exemples :")
            for e in v:
                print("   ", e)


if __name__ == "__main__":
    main()
