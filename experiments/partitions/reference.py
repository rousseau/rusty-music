# SPDX-License-Identifier: GPL-3.0-or-later
"""Extrait la ligne de basse d'un MusicXML lu par Audiveris (`omr.py`).

Audiveris ne garde pas une partie par instrument d'un système à l'autre (une
portée de guitare en introduction, la voix qui entre plus tard…) : on suit la
clé de chaque partie, et la basse d'une mesure est la partie en clé de fa.
Hauteur réelle = écrite − 12 (la basse s'écrit une octave au-dessus).

Sortie JSON : [{"numero", "page", "duree" (en doubles croches), "notes":
[[position_16e, hauteur_midi, duree_16e], ...], "reprise": "debut"|"fin"|null,
"fin_n": [numéros de volta], "absente": pas de portée de basse,
"tablature": hauteurs lues sur la tablature}] — une note lue sur la
tablature porte en plus corde et frette : [pos, hauteur, durée, corde, frette].

    python3 reference.py <dossier du morceau | partition.mxl> > reference.json
"""
import json, os, sys, zipfile
import xml.etree.ElementTree as ET
from fractions import Fraction

PAS = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}
DIESES = "FCGDAEB"
ACCIDENTS = {"sharp": 1, "flat": -1, "natural": 0, "double-sharp": 2, "sharp-sharp": 2, "flat-flat": -2}

def armures(parties, n_mesures):
    """L'armure de chaque mesure. Audiveris oublie souvent un ou deux dièses
    d'une portée (« Under the Bridge » : 0, 2, 3 ou 4 lus pour 4) ; les
    portées d'un même système (voix, basse) partagent l'armure, on garde la
    plus chargée de celles présentes."""
    courante = {p.get("id"): 0 for p in parties}
    out = []
    for i in range(n_mesures):
        lues = []
        for p in parties:
            ms = p.findall("measure")
            if i >= len(ms): continue
            f = ms[i].find("attributes/key/fifths")
            if f is not None: courante[p.get("id")] = int(f.text)
            if ms[i].get("width") is not None: lues.append(courante[p.get("id")])
        out.append(max(lues, key=abs) if lues else (out[-1] if out else 0))
    return out

def alteration(pas, fifths):
    if fifths > 0: return 1 if pas in DIESES[:fifths] else 0
    if fifths < 0: return -1 if pas in DIESES[::-1][:-fifths] else 0
    return 0

def lire(chemin):
    z = zipfile.ZipFile(chemin)
    nom = next(n for n in z.namelist() if n.endswith(".xml") and not n.startswith("META"))
    return ET.fromstring(z.read(nom))

def extraire(racine, systeme0=0, transposition=0):
    parties = racine.findall("part")
    n_mesures = max(len(p.findall("measure")) for p in parties)
    par_mesure = armures(parties, n_mesures)
    cle = {p.get("id"): None for p in parties}
    div = {p.get("id"): 1 for p in parties}
    page = 1
    systeme = systeme0
    sortie = []
    for i in range(n_mesures):
        fifths = par_mesure[i]
        candidates = []
        reprise, voltas, duree_mesure = None, [], None
        for p in parties:
            ms = p.findall("measure")
            if i >= len(ms): continue
            m = ms[i]; pid = p.get("id")
            if p is parties[0]:
                if m.find("print[@new-page='yes']") is not None: page += 1
                if i > 0 and (m.find("print[@new-page='yes']") is not None or m.find("print[@new-system='yes']") is not None):
                    systeme += 1
            pos, notes, cle_f = Fraction(0), [], cle[pid] == "F"
            accidents = {}  # un accident vaut jusqu'à la fin de la mesure
            fin = Fraction(0)
            for e in m:
                if e.tag == "attributes":
                    d = e.find("divisions")
                    if d is not None and int(d.text) > 0: div[pid] = int(d.text)
                    c = e.find("clef/sign")
                    if c is not None:
                        cle[pid] = c.text
                        cle_f = cle_f or c.text == "F"
                    t = e.find("time")
                    if t is not None:
                        duree_mesure = Fraction(int(t.find("beats").text) * 16, int(t.find("beat-type").text))
                elif e.tag == "barline":
                    r = e.find("repeat")
                    if r is not None: reprise = "debut" if r.get("direction") == "forward" else "fin"
                    en = e.find("ending")
                    if en is not None and en.get("type") == "start":
                        voltas = [int(x) for x in en.get("number", "1").replace(",", " ").split() if x.isdigit()]
                elif e.tag == "backup":
                    pos -= Fraction(int(e.find("duration").text), div[pid])
                elif e.tag == "forward":
                    pos += Fraction(int(e.find("duration").text), div[pid])
                elif e.tag == "note":
                    if e.find("grace") is not None: continue
                    dur = Fraction(int(e.find("duration").text), div[pid]) if e.find("duration") is not None else Fraction(0)
                    accord = e.find("chord") is not None
                    debut = pos - (dernier_dur if accord else 0)
                    if not accord:
                        dernier_dur = dur
                    h = e.find("pitch")
                    lie_fin = any(t.get("type") == "stop" for t in e.findall("tie"))
                    if h is not None and not lie_fin:
                        pas, octave = h.find("step").text, int(h.find("octave").text)
                        acc = e.findtext("accidental")
                        if acc in ACCIDENTS: accidents[(pas, octave)] = ACCIDENTS[acc]
                        alter = accidents.get((pas, octave), alteration(pas, fifths))
                        midi = 12 * (octave + 1) + PAS[pas] + alter
                        notes.append([debut * 4, midi - 12 + transposition, dur * 4])
                    elif h is not None and lie_fin:
                        notes.append([debut * 4, None, dur * 4])
                    if not accord: pos += dur
                    fin = max(fin, pos)
            # mesure de remplissage (portée absente de ce système) : pas de
            # largeur, un silence d'une mesure sans position
            if m.get("width") is None: continue
            if cle[pid] == "F" or cle_f:
                candidates.append((notes, fin * 4))
        # la partie en clé de fa qui porte le plus de notes
        notes, fin = max(candidates, key=lambda c: len([n for n in c[0] if n[1] is not None]), default=([], 0))
        # accords (doubles cordes) : la note la plus grave
        par_pos = {}
        for p_, h, d in notes:
            if h is None: continue
            if p_ not in par_pos or h < par_pos[p_][1]: par_pos[p_] = [p_, h, d]
        sortie.append({
            "numero": i + 1, "page": page, "systeme": systeme,
            "duree": int(duree_mesure or fin or 16),
            "notes": [[float(p_), h, float(d)] for p_, h, d in sorted(par_pos.values())],
            "reprise": reprise, "fin_n": voltas,
            # pas de portée de basse : silence (« tacet ») ou figure rejouée
            # (« w/ Bass Fig. 1 ») — inconnu sans lire le texte
            "absente": not candidates,
        })
    return sortie

def fichiers(dossier):
    """Audiveris découpe parfois un morceau en « mouvements » (pages.mvt1.mxl…)."""
    import glob, os, re
    seul = os.path.join(dossier, "partition.mxl")
    if os.path.exists(seul): return [seul]
    cle = lambda f: int((re.search(r"mvt(\d+)", f) or [0, 0])[1])
    livre = sorted(glob.glob(os.path.join(dossier, "pages*.mxl")), key=cle)
    if livre: return livre
    # repli page par page (omr.py) : page-061.mxl, page-062.mvt1.mxl…
    cle = lambda f: (int(re.search(r"page-(\d+)", f)[1]), int((re.search(r"mvt(\d+)", f) or [0, 0])[1]))
    return sorted(glob.glob(os.path.join(dossier, "page-*.mxl")), key=cle)

def extraire_dossier(dossier, accordage=None):
    """`accordage` : cordes à vide réelles d'un morceau accordé plus bas (la
    tablature est écrite relativement à elles ; la notation, comme en
    accordage standard, d'où la transposition par la corde aiguë)."""
    accordage = accordage or ACCORDAGE
    transposition = accordage[3] - ACCORDAGE[3]
    m = []
    for f in fichiers(dossier):
        for x in extraire(lire(f), m[-1]["systeme"] + 1 if m else 0, transposition):
            x["numero"] = len(m) + 1; m.append(x)
    tab = os.path.join(dossier, "tablature.json")
    if os.path.exists(tab):
        fusionner_tablature(m, json.load(open(tab)), accordage)
    return m

ACCORDAGE = [28, 33, 38, 43]  # basse 4 cordes, corde 0 = mi grave

def fusionner_tablature(mesures, tablature, accordage=None):
    """Remplace les hauteurs reconnues par celles de la tablature, et ajoute
    corde et frette. Les mesures de la notation (celles qui ont une portée de
    basse) et celles de la tablature se suivent dans le même ordre, mais leurs
    découpages diffèrent parfois (chiffres de mesure, clé « TAB » lus comme une
    mesure) : on aligne les deux suites par programmation dynamique, sur la
    ressemblance des hauteurs, puis on fusionne les paires assez ressemblantes."""
    omr = [x for x in mesures if not x["absente"] and x["notes"]]
    accordage = accordage or ACCORDAGE
    tabs = [[dict(n, accordage=accordage) for n in mt] for p in sorted(tablature, key=int) for s in tablature[p] for mt in s["mesures"] if mt]
    n, k = len(omr), len(tabs)
    G = -0.2
    S = [[0.0] * (k + 1) for _ in range(n + 1)]
    for i in range(1, n + 1): S[i][0] = i * G
    for j in range(1, k + 1): S[0][j] = j * G
    sims = {}
    def sim(i, j):
        if (i, j) not in sims: sims[(i, j)] = ressemblance(omr[i]["notes"], notes_tab(tabs[j]))[0]
        return sims[(i, j)]
    for i in range(1, n + 1):
        for j in range(max(1, i - 40), min(k, i + 40) + 1):
            S[i][j] = max(S[i - 1][j - 1] + sim(i - 1, j - 1) - 0.4, S[i - 1][j] + G, S[i][j - 1] + G)
        for j in list(range(1, max(1, i - 40))) + list(range(min(k, i + 40) + 1, k + 1)):
            S[i][j] = -1e9
    i, j = n, k
    while i > 0 and j > 0:
        if S[i][j] == S[i - 1][j - 1] + sim(i - 1, j - 1) - 0.4:
            if sim(i - 1, j - 1) >= 0.5: fusionner_mesure(omr[i - 1], tabs[j - 1])
            i -= 1; j -= 1
        elif S[i][j] == S[i - 1][j] + G: i -= 1
        else: j -= 1

def notes_tab(mt):
    return [(None if n["frette"] == "X" else n.get("accordage", ACCORDAGE)[n["corde"]] + n["frette"], n["corde"], n["frette"]) for n in mt]

def _sc(a, b):
    if b[0] is None: return 0.5
    d = abs(a[1] - b[0])
    return 1.0 if d == 0 else 0.8 if d % 12 == 0 or d <= 2 else 0.2

def ressemblance(o, t):
    """Needleman-Wunsch entre notes reconnues et notes de tablature : score
    normalisé et paires (i, j)."""
    G = -0.3
    n, k = len(o), len(t)
    if n == 0 or k == 0: return 0.0, []
    S = [[0.0] * (k + 1) for _ in range(n + 1)]
    for i in range(1, n + 1): S[i][0] = i * G
    for j in range(1, k + 1): S[0][j] = j * G
    for i in range(1, n + 1):
        for j in range(1, k + 1):
            S[i][j] = max(S[i - 1][j - 1] + _sc(o[i - 1], t[j - 1]), S[i - 1][j] + G, S[i][j - 1] + G)
    i, j, paires = n, k, []
    while i > 0 and j > 0:
        if S[i][j] == S[i - 1][j - 1] + _sc(o[i - 1], t[j - 1]): paires.append((i - 1, j - 1)); i -= 1; j -= 1
        elif S[i][j] == S[i - 1][j] + G: i -= 1
        else: j -= 1
    return S[n][k] / max(n, k), paires

def fusionner_mesure(mo, mt):
    t = notes_tab(mt); o = mo["notes"]
    acc = mt[0].get("accordage", ACCORDAGE) if mt else ACCORDAGE
    for i, j in ressemblance(o, t)[1]:
        h, corde, frette = t[j]
        if h is None: o[i].append("etouffee"); continue
        # Un chiffre rattaché à la corde voisine : la tablature et la notation
        # diffèrent alors d'une quarte juste (4 % des notes au banc du
        # 10 oct.). Même frette, corde d'à côté.
        if corde > 0 and h - o[i][1] == acc[corde] - acc[corde - 1]: h -= acc[corde] - acc[corde - 1]; corde -= 1
        elif corde < 3 and o[i][1] - h == acc[corde + 1] - acc[corde]: h += acc[corde + 1] - acc[corde]; corde += 1
        o[i][1] = h; o[i] += [corde, frette]
    # les notes étouffées (X) n'ont pas de hauteur : hors évaluation
    mo["notes"] = [x for x in o if "etouffee" not in x]
    mo["tablature"] = True

if __name__ == "__main__":
    import os
    a = sys.argv[1]
    m = extraire_dossier(a) if os.path.isdir(a) else extraire(lire(a))
    json.dump(m, sys.stdout)
    print(f"{sum(1 for x in m if x.get('tablature'))} mesures avec tablature, " 
          f"{len(m)} mesures, {sum(len(x['notes']) for x in m)} notes, "
          f"{sum(1 for x in m if x['absente'])} sans portée de basse, "
          f"{sum(1 for x in m if not x['notes'] and not x['absente'])} en silences", file=sys.stderr)
