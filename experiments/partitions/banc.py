# SPDX-License-Identifier: GPL-3.0-or-later
"""Banc de la transcription de basse contre des partitions publiées.

Pour chaque morceau de `morceaux.json` : six stems (htdemucs_6s), pulsation
(Beat This!, sur le mélange), transcription du stem de basse (exemple `banc`),
ligne de basse de la partition (`omr.py` puis `reference.py`), comparaison
(`comparer.py`). Tout se met en cache dans $CACHE ; rien n'est versionné.

    CACHE=~/Exp/tmp/banc-partitions python3 banc.py [--refaire-transcription] [id ...]
"""
import json, os, sqlite3, subprocess, sys
import reference, comparer

ICI = os.path.dirname(os.path.abspath(__file__))
RACINE = os.path.abspath(os.path.join(ICI, "..", ".."))
CACHE = os.path.expanduser(os.environ.get("CACHE", "~/Exp/tmp/banc-partitions"))
MUSIQUE = os.path.expanduser(os.environ.get("MUSIQUE", "~/Music/MyMusic"))
BASE = os.path.expanduser(os.environ.get("RUSTY_DB", "~/Library/Application Support/fm.rustymusic.desktop/rusty-music.db"))
CLI = os.path.join(RACINE, "target/release/rusty-music")
PULS = os.path.join(RACINE, "target/release/examples/verif_pulsation")
BANC = os.path.join(RACINE, "target/release/examples/banc")

def audio(m):
    if "fichier" in m: return os.path.join(MUSIQUE, m["fichier"])
    mots = m["requete"].split()
    db = sqlite3.connect(f"file:{BASE}?mode=ro", uri=True)
    # le titre d'abord, puis l'album pour départager (studio plutôt que live)
    lignes = db.execute("select path, title, album from tracks where title like ? and path not like '%Live%'",
                        (f"%{' '.join(mots[:2]) if len(mots) > 2 else m['requete']}%",)).fetchall()
    lignes.sort(key=lambda l: -sum(w.lower() in (l[2] or "").lower() for w in mots))
    return lignes[0][0]

def preparer_audio(m):
    """Stems et pulsation (en cache). Rend (dossier, stem de basse, pulsation)."""
    d = os.path.join(CACHE, m["id"]); os.makedirs(d, exist_ok=True)
    src = audio(m)
    stems = os.path.join(d, "stems")
    basse = [f for f in os.listdir(stems) if "bass" in f] if os.path.isdir(stems) else []
    if not basse:
        subprocess.run([CLI, "demix", src, "--out", stems, "--modele", "htdemucs_6s"], check=True, cwd=RACINE,
                       stdout=subprocess.DEVNULL)
        basse = [f for f in os.listdir(stems) if "bass" in f]
    puls = os.path.join(d, "pulsation.json")
    if not os.path.exists(puls):
        out = subprocess.run([PULS, src], capture_output=True, text=True, check=True, cwd=RACINE).stdout
        j = json.loads(next(l for l in out.splitlines() if l.startswith("{")))
        json.dump({"temps": j["temps"], "premiers_temps": j["premiers"]}, open(puls, "w"))
    return d, os.path.join(stems, basse[0]), puls

def etape(m, refaire):
    d, basse, puls = preparer_audio(m)
    tr = os.path.join(d, "transcription.json")
    if refaire or not os.path.exists(tr):
        subprocess.run([BANC, basse, puls, tr], check=True, cwd=RACINE, stdout=subprocess.DEVNULL)
    ref = reference.extraire_dossier(d)
    json.dump(ref, open(os.path.join(d, "reference.json"), "w"))
    return comparer.comparer(json.load(open(tr)), ref)

def main():
    conf = json.load(open(os.path.join(ICI, "morceaux.json")))
    refaire = "--refaire-transcription" in sys.argv
    voulus = {a for a in sys.argv[1:] if not a.startswith("--")}
    tot = dict(ref=0, nous=0, exactes=0, tol=0, octave=0, autres=0, vides=0, vides_notes=0, notes_silence=0, hors=0, mesures=0,
               doigtes_ref=0, doigtes=0, ref_tab=0, nous_tab=0, exactes_tab=0)
    print(f"{'morceau':22} {'mes.':>4} {'hors':>4} {'réf':>5} {'nous':>5} {'F1':>5} {'F1±1':>5} {'F1tab':>5} {'oct.':>5} {'doigté':>6} {'silences':>9}")
    for m in conf["morceaux"]:
        if voulus and m["id"] not in voulus: continue
        if not reference.fichiers(os.path.join(CACHE, m["id"])):
            print(f"{m['id']:22} (partition pas encore lue)"); continue
        phase, N, R, chemin, st = etape(m, refaire)
        st["mesures"] = len(N)
        for k in tot: tot[k] += st[k]
        f1 = lambda a, s=st: 2 * a / max(1, s["ref"] + s["nous"])
        print(f"{m['id']:22} {len(N):4d} {st['hors']:4d} {st['ref']:5d} {st['nous']:5d} {f1(st['exactes']):5.2f} {f1(st['tol']):5.2f} {2 * st['exactes_tab'] / max(1, st['ref_tab'] + st['nous_tab']):5.2f} "
              f"{st['octave'] / max(1, st['tol'] + st['octave'] + st['autres']):5.2f} {st['doigtes'] / max(1, st['doigtes_ref']):6.2f} {st['vides_notes']:3d}/{st['vides']:<3d}")
    f1 = lambda a: 2 * a / max(1, tot["ref"] + tot["nous"])
    print(f"{'TOTAL':22} {tot['mesures']:4d} {tot['hors']:4d} {tot['ref']:5d} {tot['nous']:5d} {f1(tot['exactes']):5.2f} {f1(tot['tol']):5.2f} {2 * tot['exactes_tab'] / max(1, tot['ref_tab'] + tot['nous_tab']):5.2f} "
          f"{tot['octave'] / max(1, tot['tol'] + tot['octave'] + tot['autres']):5.2f} {tot['doigtes'] / max(1, tot['doigtes_ref']):6.2f} {tot['vides_notes']:3d}/{tot['vides']:<3d}")
    json.dump(tot, open(os.path.join(CACHE, "dernier-banc.json"), "w"))

if __name__ == "__main__":
    main()
