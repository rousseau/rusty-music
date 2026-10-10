# SPDX-License-Identifier: GPL-3.0-or-later
"""Lit une transcription de batterie gravée par Sibelius (PDF vectoriel,
police Opus) : chaque glyphe a sa position exacte, les hampes, ligatures et
barres sont des traits. Pas de reconnaissance d'image — Audiveris rate les
têtes en croix (charleston, cymbales).

Sortie : la liste des mesures écrites, comme `reference.py` :
[{"numero", "page", "systeme", "duree": 16, "notes": [[pos_16e, code, 0]],
"absente": false, "repetition": 0|1|2}] — les mesures « % » (et « %% » sur
deux mesures) reçoivent le contenu des mesures répétées. Codes : pièces
d'ADTOF (36 grosse caisse, 38 caisse claire, 45 toms, 42 charleston,
49 cymbales — ride comprise).

Usage local et d'évaluation seulement (transcriptions protégées).

    python3 lire_batterie.py transcription.pdf     # affiche ce qui est lu
"""
import sys
import fitz

NOIRE, CROIX, CROIX_OUVERTE = 0xF0CF, 0xF0C0, 0xF04F
FANTOME = 0xF065  # note fantôme (police OpusSpecial), une noire pour le rythme
PETITE_NOTE, CROCHET = 0xF0DE, 0xF06A
SILENCES = {0xF0B7: 16, 0xF0EE: 8, 0xF0CE: 4, 0xF0E4: 2, 0xF0C5: 1}  # en doubles croches
REPETE_1, REPETE_2 = 0xF0D4, 0xF0A5
POINT = 0xF06B  # point d'augmentation (inconnu dans ces fichiers : à confirmer)
CHIFFRES = {0xF030 + i: i for i in range(10)}

GC, CC, TOMS, HH, CYM = 36, 38, 45, 42, 49

def piece(degre, glyphe):
    """Clé de percussion standard (Drum Ninja) : degré 0 = 1ʳᵉ ligne (mi4)."""
    if glyphe in (CROIX, CROIX_OUVERTE):
        if degre <= -1: return HH          # charleston au pied
        if degre == 5: return CC           # cross-stick
        if degre == 8: return CYM          # ride
        if degre == 9: return HH
        return CYM                         # crash, china
    if degre <= 1: return GC
    if degre == 5: return CC
    return TOMS

def glyphes(page):
    out = []
    for b in page.get_text("rawdict")["blocks"]:
        for l in b.get("lines", []):
            for s in l["spans"]:
                if "Opus" not in s["font"] or "Text" in s["font"]: continue
                for c in s["chars"]:
                    out.append((ord(c["c"]), c["origin"][0], c["origin"][1], c["bbox"]))
    return out

def traits(page):
    lignes, poutres = [], []
    for d in page.get_drawings():
        for it in d["items"]:
            if it[0] == "l":
                (x0, y0), (x1, y1) = (it[1].x, it[1].y), (it[2].x, it[2].y)
                lignes.append((min(x0, x1), min(y0, y1), max(x0, x1), max(y0, y1)))
        if d["type"] == "f":
            r = d["rect"]
            if r.width > 2: poutres.append((r.x0, r.y0, r.x1, r.y1))
    return lignes, poutres

def portees(lignes):
    """Groupes de 5 lignes horizontales régulièrement espacées."""
    hz = sorted({(round(y0, 1), round(x0), round(x1)) for x0, y0, x1, y1 in lignes if y1 - y0 < 0.3 and x1 - x0 > 40})
    ys = sorted({y for y, _, _ in hz})
    out = []
    i = 0
    while i + 4 < len(ys):
        g = ys[i:i + 5]; e = [b - a for a, b in zip(g, g[1:])]
        if max(e) - min(e) < 0.4 and 3 < e[0] < 8:
            xs = [(x0, x1) for y, x0, x1 in hz if abs(y - g[0]) < 0.2]
            out.append({"haut": g[0], "bas": g[4], "ecart": (g[4] - g[0]) / 4,
                        "x0": min(a for a, _ in xs), "x1": max(b for _, b in xs)})
            i += 5
        else: i += 1
    return out

def lire_page(page):
    gl = glyphes(page); lignes, poutres = traits(page)
    systemes = []
    for st in portees(lignes):
        h, b, e = st["haut"], st["bas"], st["ecart"]
        dans = lambda y: h - 6 * e < y < b + 5 * e
        # barres : verticales qui couvrent la portée
        barres = sorted({round((x0 + x1) / 2, 1) for x0, y0, x1, y1 in lignes
                         if x1 - x0 < 0.5 and y0 <= h + 0.5 and y1 >= b - 0.5 and y1 - y0 < 6 * e})
        # le début de la portée vaut une barre (premier système, systèmes sans
        # barre initiale : la mesure commence après la clé)
        if not barres or barres[0] > st["x0"] + 10: barres = [st["x0"]] + barres
        fus = []
        for x in barres:
            if fus and x - fus[-1] < 6: fus[-1] = x
            else: fus.append(x)
        hampes = [(x0, y0, y1) for x0, y0, x1, y1 in lignes if x1 - x0 < 0.5 and y1 - y0 > 1.5 * e and dans((y0 + y1) / 2)
                  and not any(abs(x0 - f) < 1 for f in barres)]
        g_st = [g for g in gl if dans(g[2])]
        mesures = []
        for ga, dr in zip(fus, fus[1:]):
            if dr - ga < 10: continue
            mg = [g for g in g_st if ga < g[1] < dr]
            codes = {g[0] for g in mg}
            rep = 2 if REPETE_2 in codes else 1 if REPETE_1 in codes else 0
            mesures.append(lire_mesure(mg, hampes, poutres, b, e, ga, dr) | {"repetition": rep})
        # un « %% » posé sur la barre entre deux mesures
        for g in g_st:
            if g[0] == REPETE_2:
                for k, (ga, dr) in enumerate([(a, c) for a, c in zip(fus, fus[1:]) if c - a >= 10]):
                    if abs(g[1] - dr) < 15 or ga < g[1] < dr:
                        mesures[k]["repetition"] = 2
                        if k + 1 < len(mesures): mesures[k + 1]["repetition"] = 2
                        break
        systemes.append(mesures)
    return systemes

def lire_mesure(mg, hampes, poutres, bas, e, ga, dr):
    tetes = []
    petites = [g for g in mg if g[0] == PETITE_NOTE]
    # les hampes des petites notes (ornements) ne comptent pas : collées
    # devant une caisse claire, elles passaient pour la sienne
    hampes = [h for h in hampes if not any(p[3][0] - 1 <= h[0] <= p[3][2] + 1 and h[1] - 2 <= p[2] <= h[2] + 2 for p in petites)]
    for code, x, y, bb in mg:
        if code not in (NOIRE, CROIX, CROIX_OUVERTE, FANTOME): continue
        degre = round((bas - y) / (e / 2))
        tetes.append({"x": x, "x1": bb[2], "y": y, "piece": piece(degre, code)})
    if not tetes:
        return {"notes": [], "duree": 16}
    # hampe de chaque tête : verticale qui part de la tête, à droite (montante)
    # ou à gauche (descendante)
    for t in tetes:
        cands = [(hx, y0, y1) for hx, y0, y1 in hampes if (t["x"] - 1 <= hx <= t["x1"] + 1) and y0 - 2 <= t["y"] <= y1 + 2]
        if cands:
            hx, y0, y1 = min(cands, key=lambda c: min(abs(c[0] - t["x"]), abs(c[0] - t["x1"])))
            t["hampe"] = (round(hx, 1), "haut" if t["y"] > (y0 + y1) / 2 else "bas", y0, y1)
    # durée d'une hampe : nombre de ligatures qui la touchent à son bout libre
    def duree(hampe, voisins):
        hx, sens, y0, y1 = hampe
        bout = y0 if sens == "haut" else y1
        n = sum(1 for px0, py0, px1, py1 in poutres if px0 - 0.6 <= hx <= px1 + 0.6 and abs((py0 + py1) / 2 - bout) < 3.2 * e)
        crochet = any(g[0] == CROCHET and abs(g[1] - hx) < 3 and not any(abs(p[1] - g[1]) < 4 for p in petites) for g in voisins)
        n = max(n, 1 if crochet else 0)
        return 4 / (2 ** n)
    voix = {"haut": {}, "bas": {}}
    for t in tetes:
        if "hampe" in t: voix[t["hampe"][1]].setdefault(t["hampe"][0], t["hampe"])
    silences = [(x, y, SILENCES[c]) for c, x, y, _ in mg if c in SILENCES]
    # position (en doubles croches) de chaque hampe, voix par voix
    reperes = []
    for sens, hs in voix.items():
        evts = [(hx, duree(h, mg)) for hx, h in hs.items()]
        milieu = bas - 2 * e
        evts += [(x, d) for x, y, d in silences if (y < milieu) == (sens == "haut") or len(voix["bas"]) == 0 or len(voix["haut"]) == 0]
        evts.sort()
        pos, cumul = 0, []
        for x, d in evts:
            cumul.append((x, pos)); pos += d
        if abs(pos - 16) < 0.01 and cumul:
            reperes += cumul
    reperes.sort()
    def position(x):
        if not reperes:  # repli : proportionnel à la largeur de la mesure
            return round(16 * (x - ga - 8) / max(1, dr - ga - 12))
        k = min(range(len(reperes)), key=lambda i: abs(reperes[i][0] - x))
        return round(reperes[k][1])
    notes = set()
    for t in tetes:
        x = t["hampe"][0] if "hampe" in t else t["x"]
        p = position(x)
        if 0 <= p < 16: notes.add((p, t["piece"]))
    return {"notes": [[p, c, 0] for p, c in sorted(notes)], "duree": 16, "fiable": bool(reperes)}

def lire(chemin):
    d = fitz.open(chemin)
    sortie = []
    for np_, page in enumerate(d):
        for k, sys_ in enumerate(lire_page(page)):
            for m in sys_:
                m.update(numero=len(sortie) + 1, page=np_ + 1, systeme=f"{np_}-{k}", absente=False)
                sortie.append(m)
    # répétitions : « % » reprend la mesure d'avant, « %% » les deux d'avant
    i = 0
    while i < len(sortie):
        m = sortie[i]
        if m["repetition"] == 1 and i >= 1 and not m["notes"]:
            m["notes"] = [list(n) for n in sortie[i - 1]["notes"]]
        elif m["repetition"] == 2 and i >= 2 and not m["notes"]:
            m["notes"] = [list(n) for n in sortie[i - 2]["notes"]]
        i += 1
    return sortie

if __name__ == "__main__":
    noms = {GC: "GC", CC: "CC", TOMS: "T", HH: "HH", CYM: "CY"}
    for m in lire(sys.argv[1]):
        grille = ["." * 1 for _ in range(16)]
        par = {}
        for p, c, _ in m["notes"]: par.setdefault(p, []).append(noms[c])
        txt = " ".join(f"{p}:{'+'.join(v)}" for p, v in sorted(par.items()))
        print(f"{m['numero']:3d} p{m['page']} {'%' * m['repetition']:2} {'' if m.get('fiable', True) else '~'} {txt}")
