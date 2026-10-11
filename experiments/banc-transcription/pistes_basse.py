# SPDX-License-Identifier: GPL-3.0-or-later
"""Pistes d'amélioration de la basse, sur les notes déjà en cache du banc
BabySlakh (`banc_jeux.py slakh`) : stem de basse démixé, MuScriptor et Basic
Pitch bruts. Rien n'est relancé ; chaque variante est un post-traitement des
notes de MuScriptor, mesuré contre la même référence.

    python pistes_basse.py
"""
import glob, os
import pretty_midi, yaml
import banc_jeux as bj

def triplets(notes):
    return sorted((n["debut_s"], n["fin_s"], n["hauteur"]) for n in notes)

def une_a_la_fois(notes, fenetre=0.03):
    """Une basse joue une note à la fois : des attaques simultanées, on garde
    la plus grave (les autres sont le plus souvent des harmoniques)."""
    garde = []
    for n in notes:
        if garde and n[0] - garde[-1][0] <= fenetre:
            if n[2] < garde[-1][2]: garde[-1] = n
            continue
        garde.append(n)
    return garde

def gamme(notes, duree_max=0.12):
    """Écarte les notes brèves hors des sept classes de hauteur les plus
    présentes (pondérées par la durée) : les fausses notes passagères."""
    poids = [0.0] * 12
    for a, b, h in notes: poids[h % 12] += b - a
    sept = set(sorted(range(12), key=lambda c: -poids[c])[:7])
    return [n for n in notes if n[2] % 12 in sept or n[1] - n[0] > duree_max]

def proche(n, autres, tol=0.05, meme=lambda a, b: a == b):
    return any(abs(o[0] - n[0]) <= tol and meme(o[2], n[2]) for o in autres)

def confirmees(mus, bp):
    """Seulement les notes que Basic Pitch confirme (même classe de hauteur,
    attaque à 50 ms)."""
    return [n for n in mus if proche(n, bp, meme=lambda a, b: a % 12 == b % 12)]

def complete(mus, bp, duree_min=0.1):
    """MuScriptor, plus les notes franches de Basic Pitch là où MuScriptor
    n'a rien attaqué."""
    return sorted(mus + [n for n in bp if n[1] - n[0] >= duree_min and not proche(n, mus, meme=lambda a, b: True)])

def octave_bp(mus, bp):
    """Si Basic Pitch attaque au même instant la même classe de hauteur à
    une autre octave, et que MuScriptor est hors de la tessiture usuelle
    d'une basse (E1-G3, 28-55), on prend l'octave de Basic Pitch."""
    sortie = []
    for a, b, h in mus:
        if not 28 <= h <= 55:
            alt = [o[2] for o in bp if abs(o[0] - a) <= 0.05 and o[2] % 12 == h % 12 and o[2] != h and 28 <= o[2] <= 55]
            if alt: h = alt[0]
        sortie.append((a, b, h))
    return sortie

VARIANTES = {
    "brut": lambda m, b: m,
    "une à la fois": lambda m, b: une_a_la_fois(m),
    "gamme": lambda m, b: gamme(m),
    "octave par Basic Pitch": octave_bp,
    "confirmées par Basic Pitch": confirmees,
    "complétées par Basic Pitch": complete,
    "une à la fois + gamme": lambda m, b: gamme(une_a_la_fois(m)),
}

def main():
    T = bj.Total()
    PR = {v: [0.0, 0.0, 0] for v in VARIANTES}
    for piste in sorted(glob.glob(os.path.join(bj.JEUX, "babyslakh", "*", "Track*"))):
        nom = os.path.basename(piste); d = os.path.join(bj.CACHE, "slakh", nom)
        meta = yaml.safe_load(open(os.path.join(piste, "metadata.yaml")))
        ref = []
        for s, m in meta["stems"].items():
            midi = os.path.join(piste, "MIDI", f"{s}.mid")
            if m["is_drum"] or bj.famille(m["program_num"]) != 4: continue
            if not os.path.exists(os.path.join(piste, "stems", f"{s}.wav")) or not os.path.exists(midi): continue
            notes = [(n.start, n.end, n.pitch) for i in pretty_midi.PrettyMIDI(midi).instruments for n in i.notes]
            k = bj.octave_rendue(notes, os.path.join(piste, "stems", f"{s}.wav"), os.path.join(d, f"octave-{s}.json"))
            ref += [(a, b, h + k) for a, b, h in notes]
        mus_f, bp_f = os.path.join(d, "muscriptor-basse.json"), os.path.join(d, "basic-pitch-basse.json")
        if not ref or not os.path.exists(mus_f): continue
        mus, bp = triplets(bj.json.load(open(mus_f))), triplets(bj.json.load(open(bp_f)))
        ligne = f"{nom}"
        for v, f in VARIANTES.items():
            sc = bj.score_notes(ref, f(mus, bp))
            T.ajouter(v, sc); ligne += f"  {sc['f']:.2f}"
            a = PR[v]; a[0] += sc["p"] * sc["nref"]; a[1] += sc["r"] * sc["nref"]; a[2] += sc["nref"]
        print(ligne, flush=True)
    print("TOTAL (F1 attaque, F1 avec fin)\n" + str(T))
    for v, (sp, sr, n) in PR.items():
        print(f"  {v:28} précision {sp / n:.3f}  rappel {sr / n:.3f}")

if __name__ == "__main__":
    main()
