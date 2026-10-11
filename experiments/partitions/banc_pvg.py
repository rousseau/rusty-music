# SPDX-License-Identifier: GPL-3.0-or-later
"""Banc multi-instrument contre les recueils « piano-chant-guitare »
(`omr_pvg.py`, `reference_pvg.py`) : ce que MuScriptor (portage Burn) sait
transcrire, partie par partie.

- **voix** : MuScriptor « voice » sur le stem vocal, monophonie et
  quantification (exemple `banc -- notes`), contre la mélodie écrite. La voix
  d'homme sonne une octave sous la portée, et un recueil n'est pas toujours
  dans la tonalité du disque : la transposition est choisie par morceau.
- **basse** : MuScriptor basse sur le stem de basse, contre la main gauche du
  piano (indicatif : un arrangement pour piano suit la basse sans la recopier).
- **harmonie** : MuScriptor sans restriction sur le mélange, contre le piano
  et la voix — classes de hauteur sonnant à chaque temps (F1), mesures
  alignées comme `comparer.py`.

Chaque partie aussi par Basic Pitch aux réglages par défaut (exemple
`banc -- brutes`), sauf la basse (comparée au banc de basse, `banc.py`).

    python3 banc_pvg.py [--parties voix,basse,harmonie] [--modeles muscriptor,basic-pitch] [--livre beatles] [id ...]
"""
import argparse, bisect, json, os, subprocess, sys
import banc, comparer, reference, reference_pvg

ICI = banc.ICI
MUSCRIPTOR = os.path.join(banc.RACINE, "target/release/examples/muscriptor")

def muscriptor(audio, sortie, instruments):
    if not os.path.exists(sortie):
        r = subprocess.run([MUSCRIPTOR, audio, sortie, "16", "1000", instruments], check=True, capture_output=True, text=True, cwd=banc.RACINE)
        print(f"    {os.path.basename(sortie)} : {r.stdout.strip()}", file=sys.stderr)
    return json.load(open(sortie))

def vers_notes(brutes, garder=lambda n: True):
    return [{"debut_s": n["debut_s"], "fin_s": n["fin_s"], "hauteur": n["hauteur"], "amplitude": 1.0, "corde": None, "frette": None}
            for n in brutes if garder(n)]

def quantifier(d, nom, notes, puls, stem=None):
    brut = os.path.join(d, f"{nom}-notes.json"); json.dump(notes, open(brut, "w"))
    tr = os.path.join(d, f"transcription-{nom}.json")
    subprocess.run([banc.BANC, "notes", brut, puls, tr] + ([stem] if stem else []), check=True, stdout=subprocess.DEVNULL, cwd=banc.RACINE)
    return json.load(open(tr))

def transposer(r, k):
    return [dict(m, notes=[[p, h + k, *x] for p, h, *x in m["notes"]]) for m in r]

def decalage_tonal(nous, ref):
    """Le décalage (0 à 11 demi-tons) à ajouter à la référence pour qu'elle
    ressemble le plus à nos notes : corrélation circulaire des histogrammes
    de classes de hauteur. Un recueil n'est pas toujours dans la tonalité du
    disque (« Penny Lane » : si majeur sur le disque, un demi-ton plus bas
    dans le recueil)."""
    hn, hr = [0.0] * 12, [0.0] * 12
    for h in nous: hn[h % 12] += 1
    for h in ref: hr[h % 12] += 1
    return max(range(12), key=lambda k: sum(hn[(i + k) % 12] * hr[i] for i in range(12)))

def hauteurs_transcription(t):
    return [e["jeu"]["hauteur"] for m in t["mesures"] for e in m["evenements"] if e["jeu"] and not e["lie"]]

def meilleure_octave(t, r, octaves):
    """Comparaison à la transposition qui explique le plus de notes justes :
    le décalage tonal, puis l'octave parmi `octaves`."""
    k0 = decalage_tonal(hauteurs_transcription(t), [n[1] for m in r for n in m["notes"]])
    k0 = k0 - 12 if k0 > 6 else k0
    best = None
    for k in (k0 + o for o in octaves):
        res = comparer.comparer(t, transposer(r, k))
        if best is None or res[-1]["exactes"] > best[1][-1]["exactes"]: best = (k, res)
    return best

# --- harmonie -------------------------------------------------------------

def grille(puls):
    """Temps (s) et indice du premier temps de la première mesure."""
    p = json.load(open(puls)); temps = p["temps"]
    premier = p["premiers_temps"][0] if p["premiers_temps"] else temps[0]
    return temps, min(range(len(temps)), key=lambda i: abs(temps[i] - premier)) % 4

def en_seiziemes(t, temps):
    k = max(0, min(len(temps) - 2, bisect.bisect_right(temps, t) - 1))
    return 4 * k + 4 * (t - temps[k]) / max(1e-6, temps[k + 1] - temps[k])

def nos_mesures_poly(notes, puls, phase):
    temps, k0 = grille(puls)
    debut = 4 * (k0 - phase)
    fin = 4 * len(temps)
    n = int((fin - debut) // 16) + 1
    out = [[] for _ in range(max(0, n))]
    for x in notes:
        p = en_seiziemes(x["debut_s"], temps) - debut
        q = en_seiziemes(x["fin_s"], temps) - debut
        k = int(p // 16)
        if 0 <= k < n: out[k].append((round(p - 16 * k), x["hauteur"], max(1, round(q - p))))
    return out

def chromas(ms, duree=16):
    """Classes de hauteur sonnant pendant chaque temps : [set] × 4."""
    t = [set() for _ in range(4)]
    for p, h, d, *_ in ms:
        for q in range(4):
            if p < 4 * q + 4 and p + d > 4 * q: t[q].add(h % 12)
    return t

def sim_chroma(o, r):
    a, b = chromas(o), chromas(r)
    inter = sum(len(x & y) for x, y in zip(a, b)); tot = sum(len(x) + len(y) for x, y in zip(a, b))
    return 1.0 if tot == 0 else 2 * inter / tot

def harmonie(notes, ref, puls):
    R = [[(round(p), h, round(dd)) for p, h, dd in (m["piano"] or []) + (m["voix"] or []) if p < 16] for m in ref]
    k = decalage_tonal([n["hauteur"] for n in notes], [n[1] for m in R for n in m])
    R = [[(p, h + k, dd) for p, h, dd in m] for m in R]
    best = None
    for phase in range(4):
        N = nos_mesures_poly(notes, puls, phase)
        anc = comparer.sim
        comparer.sim = sim_chroma
        try: chemin, total = comparer.aligner(N, R)
        finally: comparer.sim = anc
        if best is None or total > best[0]: best = (total, N, chemin)
    _, N, chemin = best
    inter = tot_o = tot_r = 0
    for o, j in zip(N, chemin):
        if j == len(R): continue
        a, b = chromas(o), chromas(R[j])
        inter += sum(len(x & y) for x, y in zip(a, b)); tot_o += sum(map(len, a)); tot_r += sum(map(len, b))
    return dict(inter=inter, nous=tot_o, ref=tot_r)

# --------------------------------------------------------------------------

def basic_pitch(audio, sortie):
    if not os.path.exists(sortie):
        subprocess.run([banc.BANC, "brutes", audio, sortie], check=True, stdout=subprocess.DEVNULL, cwd=banc.RACINE)
    return json.load(open(sortie))

def main():
    a = argparse.ArgumentParser()
    a.add_argument("--parties", default="voix,basse,harmonie")
    a.add_argument("--modeles", default="muscriptor,basic-pitch")
    a.add_argument("--morceaux", default="morceaux_pvg.json")
    a.add_argument("--livre", help="un seul livre")
    a.add_argument("ids", nargs="*")
    args = a.parse_args()
    parties, modeles = args.parties.split(","), args.modeles.split(",")
    conf = json.load(open(os.path.join(ICI, args.morceaux)))
    cles = [(p, mo) for p in parties for mo in modeles if not (p == "basse" and mo == "basic-pitch")]
    tot = {k: dict(ref=0, nous=0, exactes=0, tol=0, inter=0) for k in cles}
    f1 = lambda s, k="exactes": 2 * s[k] / max(1, s["ref"] + s["nous"])
    court = {"muscriptor": "MuS", "basic-pitch": "BP"}
    for m in conf["morceaux"]:
        if args.ids and m["id"] not in args.ids: continue
        if args.livre and m["livre"] != args.livre: continue
        d = os.path.join(banc.CACHE, m["id"])
        if not reference.fichiers(d): print(f"{m['id']:44} (partition pas encore lue)"); continue
        ref = reference_pvg.extraire_dossier(d)
        json.dump(ref, open(os.path.join(d, "reference-pvg.json"), "w"))
        ligne = f"{m['id'][:44]:44}"
        for p, mo in cles:
            if p == "voix":
                _, voc, puls = banc.preparer_audio(m, "vocals")
                brutes = (vers_notes(muscriptor(voc, os.path.join(d, "muscriptor-voix.json"), "voice")) if mo == "muscriptor"
                          else basic_pitch(voc, os.path.join(d, "basic-pitch-voix.json")))
                t = quantifier(d, f"voix-{mo}", brutes, puls, voc)
                k, (_, _, _, _, st) = meilleure_octave(t, reference_pvg.vers_banc(ref, "voix"), (-12, 0))
                ligne += f"  voix {court[mo]} {f1(st):.2f}"
            elif p == "basse":
                _, basse, puls = banc.preparer_audio(m, "bass")
                brut = os.path.join(d, "muscriptor-rust-lot16-notes.json")
                if not os.path.exists(brut):
                    subprocess.run([MUSCRIPTOR, basse, brut, "16", "1000"], check=True, capture_output=True, cwd=banc.RACINE)
                t = quantifier(d, "basse-muscriptor", json.load(open(brut)), puls, basse)
                k, (_, _, _, _, st) = meilleure_octave(t, reference_pvg.vers_banc(ref, "gauche"), (-12, 0, 12))
                ligne += f"  basse {court[mo]} {f1(st):.2f}"
            else:
                src = banc.audio(m)
                _, _, puls = banc.preparer_audio(m, "bass")
                notes = ([n for n in muscriptor(src, os.path.join(d, "muscriptor-tous.json"), "tous") if n["programme"] != 128] if mo == "muscriptor"
                         else basic_pitch(src, os.path.join(d, "basic-pitch-tous.json")))
                h = harmonie(notes, ref, puls)
                st = dict(ref=h["ref"], nous=h["nous"], exactes=h["inter"], tol=h["inter"], inter=h["inter"])
                ligne += f"  harm. {court[mo]} {f1(st):.2f}"
            for c in ("ref", "nous", "exactes", "tol"): tot[(p, mo)][c] += st[c]
        print(ligne, flush=True)
    print("TOTAL  " + "  ".join(f"{p} {mo} F1 {f1(tot[(p, mo)]):.3f}" for p, mo in cles))
    json.dump({f"{p}/{mo}": v for (p, mo), v in tot.items()}, open(os.path.join(banc.CACHE, f"dernier-banc-pvg{'-' + args.livre if args.livre else ''}.json"), "w"))

if __name__ == "__main__":
    main()
