# SPDX-License-Identifier: GPL-3.0-or-later
"""Extrait d'un MusicXML de recueil « piano-chant-guitare » (`omr_pvg.py`)
les parties de chaque mesure : la voix (portée unique au-dessus du piano ;
une clé de fa y est une clé de sol mal lue), le piano entier (deux portées) et sa main gauche (la note la plus
grave de chaque attaque, souvent la ligne de basse de l'enregistrement).

Audiveris ne garde pas les mêmes parties d'un système à l'autre (introduction
au piano seul, voix qui entre plus tard) : on reconnaît le piano à ses deux
portées et la voix à sa portée unique en clé de sol.

Hauteurs écrites (la voix d'homme sonne souvent une octave plus bas : le banc
choisit l'octave). Sortie : [{"numero", "systeme", "duree" (doubles croches),
"voix", "piano", "gauche": [[position_16e, hauteur, duree_16e], ...] ou null
si la portée manque, "reprise", "fin_n"}].

    python3 reference_pvg.py <dossier du morceau> > reference-pvg.json
"""
import json, sys
from fractions import Fraction
from reference import ACCIDENTS, PAS, alteration, armures, fichiers, lire

def mesures_partie(p, n_mesures, par_mesure):
    """Pour chaque mesure : None (partie absente) ou {portée: [[pos, h, dur]]}."""
    div, portees, cles = 1, 1, {}
    out = []
    for i, m in enumerate(p.findall("measure")[:n_mesures]):
        if m.get("width") is None:
            # mesure de remplissage : ses attributs (clé, portées) valent pour la suite
            for e in m.findall("attributes"):
                s = e.findtext("staves")
                if s: portees = int(s)
                for c in e.findall("clef"): cles[int(c.get("number", 1))] = c.findtext("sign")
            out.append(None); continue
        fifths = par_mesure[i]
        pos, fin, accidents, notes = Fraction(0), Fraction(0), {}, {}
        dernier_dur = Fraction(0)
        for e in m:
            if e.tag == "attributes":
                d = e.find("divisions")
                if d is not None and int(d.text) > 0: div = int(d.text)
                s = e.findtext("staves")
                if s: portees = int(s)
                for c in e.findall("clef"): cles[int(c.get("number", 1))] = c.findtext("sign")
            elif e.tag == "backup": pos -= Fraction(int(e.findtext("duration")), div)
            elif e.tag == "forward": pos += Fraction(int(e.findtext("duration")), div)
            elif e.tag == "note":
                if e.find("grace") is not None: continue
                dur = Fraction(int(e.findtext("duration")), div) if e.find("duration") is not None else Fraction(0)
                accord = e.find("chord") is not None
                debut = pos - (dernier_dur if accord else 0)
                if not accord: dernier_dur = dur
                st = int(e.findtext("staff") or 1)
                notes.setdefault(st, [])
                h = e.find("pitch")
                lie_fin = any(t.get("type") == "stop" for t in e.findall("tie"))
                if h is not None and not lie_fin:
                    pas, octave = h.findtext("step"), int(h.findtext("octave"))
                    if portees == 1 and cles.get(st) == "F":
                        # Portée seule en clé de fa : la clé de sol de la voix
                        # mal lue (une portée de piano-chant n'est jamais
                        # seule en fa) — même ligne, douze degrés plus haut.
                        d = octave * 7 + "CDEFGAB".index(pas) + 12
                        pas, octave = "CDEFGAB"[d % 7], d // 7
                    acc = e.findtext("accidental")
                    if acc in ACCIDENTS: accidents[(st, pas, octave)] = ACCIDENTS[acc]
                    alter = accidents.get((st, pas, octave), alteration(pas, fifths))
                    notes[st].append([float(debut * 4), 12 * (octave + 1) + PAS[pas] + alter, float(dur * 4)])
                if not accord: pos += dur
                fin = max(fin, pos)
        out.append({"portees": portees, "cles": dict(cles), "notes": notes, "fin": float(fin * 4)})
    return out

def extraire(racine, systeme0=0):
    parties = racine.findall("part")
    n = max(len(p.findall("measure")) for p in parties)
    par_mesure = armures(parties, n)
    lues = [mesures_partie(p, n, par_mesure) for p in parties]
    sortie, systeme, duree = [], systeme0, 16
    for i in range(n):
        m0 = parties[0].findall("measure")[i] if i < len(parties[0].findall("measure")) else None
        if i > 0 and m0 is not None and (m0.find("print[@new-page='yes']") is not None or m0.find("print[@new-system='yes']") is not None):
            systeme += 1
        reprise, voltas = None, []
        for p in parties:
            ms = p.findall("measure")
            if i >= len(ms): continue
            t = ms[i].find("attributes/time")
            if t is not None and t.findtext("beats", "").isdigit():
                duree = int(t.findtext("beats")) * 16 // int(t.findtext("beat-type"))
            for b in ms[i].findall("barline"):
                r = b.find("repeat")
                if r is not None: reprise = "debut" if r.get("direction") == "forward" else "fin"
                en = b.find("ending")
                if en is not None and en.get("type") == "start":
                    voltas = [int(x) for x in en.get("number", "1").replace(",", " ").split() if x.isdigit()]
        voix = piano = gauche = None
        for l in lues:
            x = l[i] if i < len(l) else None
            if x is None: continue
            if x["portees"] >= 2 and piano is None:
                piano = sorted(x["notes"].get(1, []) + x["notes"].get(2, []))
                par_pos = {}
                for p_, h, d in x["notes"].get(2, []):
                    if p_ not in par_pos or h < par_pos[p_][1]: par_pos[p_] = [p_, h, d]
                gauche = sorted(par_pos.values())
            elif x["portees"] == 1 and voix is None:
                voix = sorted({(p_, h): [p_, h, d] for p_, h, d in x["notes"].get(1, [])}.values())
                # la voix chante une note à la fois : la plus haute d'un accord
                par_pos = {}
                for p_, h, d in voix:
                    if p_ not in par_pos or h > par_pos[p_][1]: par_pos[p_] = [p_, h, d]
                voix = sorted(par_pos.values())
        sortie.append({"numero": i + 1, "systeme": systeme, "duree": duree, "voix": voix, "piano": piano,
                       "gauche": gauche, "reprise": reprise, "fin_n": voltas})
    return sortie

def extraire_dossier(dossier):
    m = []
    for f in fichiers(dossier):
        for x in extraire(lire(f), m[-1]["systeme"] + 1 if m else 0):
            x["numero"] = len(m) + 1; m.append(x)
    return m

def vers_banc(mesures, partie):
    """Au format de `reference.py` (pour `comparer.py`) : une partie absente
    de la mesure est « absente »."""
    return [{"numero": x["numero"], "systeme": x["systeme"], "duree": x["duree"], "notes": x[partie] or [],
             "absente": x[partie] is None, "reprise": x["reprise"], "fin_n": x["fin_n"]} for x in mesures]

if __name__ == "__main__":
    m = extraire_dossier(sys.argv[1])
    json.dump(m, sys.stdout)
    for p in ("voix", "piano", "gauche"):
        print(f"{p}: {sum(1 for x in m if x[p] is not None)}/{len(m)} mesures, {sum(len(x[p] or []) for x in m)} notes", file=sys.stderr)
