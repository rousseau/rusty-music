# SPDX-License-Identifier: GPL-3.0-or-later
"""Banc de transcription contre des jeux annotés — vérité exacte, sans OMR
(`docs/plan-editer-pratique-creation.md`, 0.4). Complète le banc contre les
partitions publiées (`experiments/partitions/`), qui classe les modèles mais
ne mesure pas leur justesse.

Jeux (téléchargés à la source, dans `$RUSTY_DEV/jeux`, jamais versionnés) :
- BabySlakh (Manilow et al. 2019, CC BY 4.0) : 20 morceaux synthétisés
  depuis le MIDI, chaque instrument annoté ;
- MDB Drums (Southall et al. 2017, CC BY-NC-SA 4.0) : 23 extraits de
  MedleyDB, frappes en 5 classes ;
- GuitarSet (Xi et al. 2018, CC BY 4.0) : 360 extraits de guitare seule,
  notes par corde ;
- MAESTRO v3 (Hawthorne et al. 2019, CC BY-NC-SA 4.0) : piano réel,
  Disklavier ; 20 pièces tirées du jeu de test (`choix.json`), lues à
  distance dans l'archive sans la télécharger entière.

Chaîne de l'application : les stems viennent de **notre** HTDemucs
(`rusty-music demix`), puis Basic Pitch (notes brutes, réglages par défaut),
MuScriptor (portage Burn) ou ADTOF. Mesures de mir_eval : F1 de notes à
l'attaque (±50 ms, hauteur au demi-ton), avec ou sans la fin (20 % de la
durée) ; F1 de frappes par classe (±50 ms).

    <env>/bin/python banc_jeux.py slakh|mdb|guitarset [--limite N]
"""
import argparse, glob, json, os, subprocess, sys
import numpy as np
import mir_eval, pretty_midi, yaml, jams

ICI = os.path.dirname(os.path.abspath(__file__))
RACINE = os.path.abspath(os.path.join(ICI, "..", ".."))
# Données de développement sur le disque externe, pas toujours branché. Il
# est en exFAT : macOS y sème des `._*` (attributs étendus), à ignorer dans
# tout listage de dossier.
DEV = os.path.expanduser(os.environ.get("RUSTY_DEV", "/Volumes/LaCieDock/rusty_music-dev"))
if not os.path.isdir(DEV):
    sys.exit(f"données de développement absentes : {DEV} (disque débranché ? sinon RUSTY_DEV=…)")
JEUX = os.path.expanduser(os.environ.get("JEUX", os.path.join(DEV, "jeux")))
CACHE = os.path.expanduser(os.environ.get("CACHE", os.path.join(DEV, "banc-jeux")))
CLI = os.path.join(RACINE, "target/release/rusty-music")
BANC = os.path.join(RACINE, "target/release/examples/banc")
MUS = os.path.join(RACINE, "target/release/examples/muscriptor")
# Taille de MuScriptor : medium par défaut ; une autre (`MUSCRIPTOR=large`)
# prend ses poids dans `$RUSTY_DEV/hf/muscriptor-<taille>` et son propre cache.
TAILLE = os.environ.get("MUSCRIPTOR", "medium")
if TAILLE != "medium":
    os.environ.setdefault("MUSCRIPTOR_POIDS", os.path.join(DEV, "hf", f"muscriptor-{TAILLE}", "model.safetensors"))
LOT = os.environ.get("LOT", "16")
BASSES = "electric_bass,acoustic_bass"
GUITARES = "acoustic_guitar,clean_electric_guitar,distorted_electric_guitar"
# General MIDI → les cinq pièces d'ADTOF.
GM = {35: "grosse_caisse", 36: "grosse_caisse", 37: "caisse_claire", 38: "caisse_claire", 40: "caisse_claire",
      41: "toms", 43: "toms", 45: "toms", 47: "toms", 48: "toms", 50: "toms",
      42: "charleston", 44: "charleston", 46: "charleston",
      49: "cymbales", 51: "cymbales", 52: "cymbales", 53: "cymbales", 55: "cymbales", 57: "cymbales", 59: "cymbales"}
PIECES = ["grosse_caisse", "caisse_claire", "toms", "charleston", "cymbales"]
MDB = {"KD": "grosse_caisse", "SD": "caisse_claire", "TT": "toms", "HH": "charleston", "CY": "cymbales"}

def lancer(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True, cwd=RACINE)
    if r.returncode:
        sys.exit(f"échec : {' '.join(cmd)}\n{r.stderr[-2000:]}")
    return r.stdout

def muscriptor(audio, sortie, instruments=None):
    """Notes brutes `{debut_s, fin_s, hauteur, programme?}` ; sans liste, les
    basses au format de l'application."""
    if TAILLE != "medium":
        sortie = os.path.join(os.path.dirname(sortie), os.path.basename(sortie).replace("muscriptor", f"muscriptor-{TAILLE}", 1))
    if not os.path.exists(sortie):
        lancer([MUS, audio, sortie, LOT, "1000"] + ([instruments] if instruments else []))
    return json.load(open(sortie))

def basic_pitch(audio, sortie):
    if not os.path.exists(sortie):
        lancer([BANC, "brutes", audio, sortie])
    return json.load(open(sortie))

def en_441(src, dest):
    """HTDemucs et ADTOF attendent du 44,1 kHz stéréo."""
    if not os.path.exists(dest):
        subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", src, "-ar", "44100", "-ac", "2", dest], check=True)
    return dest

def demixer(src441, dossier):
    stems = os.path.join(dossier, "stems")
    if not os.path.isdir(stems) or not os.listdir(stems):
        lancer([CLI, "demix", src441, "--out", stems, "--modele", "htdemucs_6s"])
    return {s: os.path.join(stems, f) for f in os.listdir(stems) if not f.startswith("._") for s in ("bass", "drums", "guitar", "vocals", "piano", "other") if f"— {s}." in f}

def adtof(stem441, dossier):
    sortie = os.path.join(dossier, "adtof.json")
    if not os.path.exists(sortie):
        # La pulsation ne sert qu'à la mise en mesure, ignorée ici.
        puls = os.path.join(dossier, "pulsation-neutre.json")
        json.dump({"temps": [i * 0.5 for i in range(2000)], "premiers_temps": [i * 2.0 for i in range(500)]}, open(puls, "w"))
        lancer([BANC, "batterie", stem441, puls, sortie])
    return [(c["instant_s"], c["piece"]) for c in json.load(open(sortie))["coups"]]

def coups_muscriptor(notes):
    return [(n["debut_s"], GM[n["hauteur"]]) for n in notes if n.get("programme") == 128 and n["hauteur"] in GM]

def tableaux(notes):
    if not notes:
        return np.zeros((0, 2)), np.zeros(0)
    iv = np.array([[n[0], max(n[1], n[0] + 0.01)] for n in notes])
    return iv, mir_eval.util.midi_to_hz(np.array([n[2] for n in notes], dtype=float))

def score_notes(ref, est):
    """(précision, rappel, F1 attaque, F1 attaque+fin) sur des triplets
    (début, fin, hauteur MIDI)."""
    ri, rp = tableaux(ref); ei, ep = tableaux(est)
    if not len(rp) or not len(ep):
        return dict(p=0.0, r=0.0, f=0.0, ff=0.0, nref=len(rp), nest=len(ep))
    p, r, f, _ = mir_eval.transcription.precision_recall_f1_overlap(ri, rp, ei, ep, onset_tolerance=0.05, offset_ratio=None)
    _, _, ff, _ = mir_eval.transcription.precision_recall_f1_overlap(ri, rp, ei, ep, onset_tolerance=0.05, offset_ratio=0.2)
    return dict(p=p, r=r, f=f, ff=ff, nref=len(rp), nest=len(ep))

def score_coups(ref, est):
    out = {}
    for c in PIECES:
        r = np.array(sorted(t for t, p in ref if p == c)); e = np.array(sorted(t for t, p in est if p == c))
        if not len(r):
            continue
        out[c] = (mir_eval.onset.f_measure(r, e, window=0.05)[0] if len(e) else 0.0, len(r))
    return out

def triplets(notes, programmes=None):
    return [(n["debut_s"], n["fin_s"], n["hauteur"]) for n in notes
            if n.get("programme", 33) != 128 and (programmes is None or n.get("programme", 33) in programmes)]

class Total:
    """Moyenne pondérée par le nombre de notes de référence."""
    def __init__(self): self.s = {}
    def ajouter(self, nom, sc):
        a = self.s.setdefault(nom, [0.0, 0.0, 0])
        a[0] += sc["f"] * sc["nref"]; a[1] += sc["ff"] * sc["nref"]; a[2] += sc["nref"]
    def __str__(self):
        return "\n".join(f"  {k:32} F1 {v[0] / max(1, v[2]):.3f}  F1 avec fin {v[1] / max(1, v[2]):.3f}  ({v[2]} notes)" for k, v in self.s.items())

class TotalCoups:
    def __init__(self): self.s = {}
    def ajouter(self, nom, sc):
        for c, (f, n) in sc.items():
            a = self.s.setdefault(nom, {}).setdefault(c, [0.0, 0]); a[0] += f * n; a[1] += n
    def __str__(self):
        return "\n".join(f"  {k:28} " + "  ".join(f"{c} {v[0] / max(1, v[1]):.2f}" for c, v in d.items()) for k, d in self.s.items())

def octave_rendue(notes, stem, cache):
    """Décalage (−12, 0, +12) qui accorde le MIDI d'un stem à son rendu,
    d'après Basic Pitch sur le stem isolé."""
    if not os.path.exists(cache):
        est = [(n["debut_s"], n["fin_s"], n["hauteur"]) for n in basic_pitch(stem, cache + ".bp.json")]
        f = {k: score_notes([(a, b, h + k) for a, b, h in notes], est)["f"] for k in (-12, 0, 12)}
        json.dump({"decalage": max(f, key=f.get), "f1": f}, open(cache, "w"))
    return json.load(open(cache))["decalage"]

def famille(programme):
    return programme // 8  # familles General MIDI : 4 basse, 3 guitare, 0 piano…

def slakh(args):
    T, TC = Total(), TotalCoups()
    pistes = sorted(glob.glob(os.path.join(JEUX, "babyslakh", "*", "Track*")))[:args.limite]
    for piste in pistes:
        nom = os.path.basename(piste); d = os.path.join(CACHE, "slakh", nom); os.makedirs(d, exist_ok=True)
        meta = yaml.safe_load(open(os.path.join(piste, "metadata.yaml")))
        ref, ref_coups = [], []
        for s, m in meta["stems"].items():
            midi = os.path.join(piste, "MIDI", f"{s}.mid")
            # `audio_rendered` est faux partout dans BabySlakh : le stem
            # rendu fait foi.
            if not os.path.exists(os.path.join(piste, "stems", f"{s}.wav")) or not os.path.exists(midi):
                continue
            notes = [n for inst in pretty_midi.PrettyMIDI(midi).instruments for n in inst.notes]
            if m["is_drum"]:
                ref_coups += [(n.start, GM[n.pitch]) for n in notes if n.pitch in GM]
                continue
            # Le rendu de Slakh transpose certains instruments (les basses
            # sonnent une octave sous leur MIDI) : l'octave qui sonne se lit
            # sur le stem isolé, la même pour tous les modèles comparés.
            k = octave_rendue([(n.start, n.end, n.pitch) for n in notes], os.path.join(piste, "stems", f"{s}.wav"), os.path.join(d, f"octave-{s}.json"))
            ref += [(n.start, n.end, n.pitch + k, famille(m["program_num"])) for n in notes]
        mix = os.path.join(piste, "mix.wav")
        # Tous les instruments, sur le mélange.
        mt = muscriptor(mix, os.path.join(d, "muscriptor-tous.json"), "tous")
        bp = basic_pitch(mix, os.path.join(d, "basic-pitch-mix.json"))
        tous = [r[:3] for r in ref]
        sc_m, sc_b = score_notes(tous, triplets(mt)), score_notes(tous, [(n["debut_s"], n["fin_s"], n["hauteur"]) for n in bp])
        T.ajouter("mélange : MuScriptor", sc_m); T.ajouter("mélange : Basic Pitch", sc_b)
        ligne = f"{nom}  mélange MuS {sc_m['f']:.2f}  BP {sc_b['f']:.2f}"
        # Par famille, instrument compris (MuScriptor seul sait le dire).
        for fam, lib in ((4, "basse"), (3, "guitare"), (0, "piano")):
            rf = [r[:3] for r in ref if r[3] == fam]
            if rf:
                sc = score_notes(rf, [(n["debut_s"], n["fin_s"], n["hauteur"]) for n in mt if n["programme"] != 128 and famille(n["programme"]) == fam])
                T.ajouter(f"{lib} (instrument nommé) : MuScriptor", sc); ligne += f"  {lib} {sc['f']:.2f}"
        # Basse et batterie par la chaîne de l'application : nos stems.
        stems = demixer(en_441(mix, os.path.join(d, "mix441.wav")), d)
        rb = [r[:3] for r in ref if r[3] == 4]
        if rb:
            smb = score_notes(rb, triplets(muscriptor(stems["bass"], os.path.join(d, "muscriptor-basse.json"))))
            sbb = score_notes(rb, [(n["debut_s"], n["fin_s"], n["hauteur"]) for n in basic_pitch(stems["bass"], os.path.join(d, "basic-pitch-basse.json"))])
            T.ajouter("stem basse : MuScriptor", smb); T.ajouter("stem basse : Basic Pitch", sbb)
            ligne += f"  | stem basse MuS {smb['f']:.2f} BP {sbb['f']:.2f}"
        if ref_coups:
            TC.ajouter("stem batterie : ADTOF", score_coups(ref_coups, adtof(stems["drums"], d)))
            TC.ajouter("mélange : MuScriptor", score_coups(ref_coups, coups_muscriptor(mt)))
        print(ligne, flush=True)
    print("TOTAL notes\n" + str(T)); print("TOTAL frappes\n" + str(TC))

def mdb(args):
    TC = TotalCoups()
    racine = os.path.join(JEUX, "mdbdrums", "MDBDrums", "MDB Drums")
    for mix in sorted(glob.glob(os.path.join(racine, "audio", "full_mix", "*_MIX.wav")))[:args.limite]:
        nom = os.path.basename(mix)[:-8]; d = os.path.join(CACHE, "mdb", nom); os.makedirs(d, exist_ok=True)
        ref = []
        for l in open(os.path.join(racine, "annotations", "class", f"{nom}_class.txt")):
            t, c = l.split()
            if c in MDB: ref.append((float(t), MDB[c]))
        stems = demixer(mix, d)
        res = {"stem batterie : ADTOF": adtof(stems["drums"], d),
               "stem batterie : MuScriptor": coups_muscriptor(muscriptor(stems["drums"], os.path.join(d, "muscriptor-stem.json"), "drums")),
               "mélange : MuScriptor": coups_muscriptor(muscriptor(mix, os.path.join(d, "muscriptor-mix.json"), "drums"))}
        ligne = f"{nom:32}"
        for k, est in res.items():
            sc = score_coups(ref, est); TC.ajouter(k, sc)
            n = sum(v[1] for v in sc.values())
            ligne += f"  {k.split(' : ')[1][:3]}({k[:5]}) {sum(f * m for f, m in sc.values()) / max(1, n):.2f}"
        print(ligne, flush=True)
    print("TOTAL frappes (F1 par classe, ±50 ms)\n" + str(TC))

def guitarset(args):
    T = Total()
    fichiers = sorted(glob.glob(os.path.join(JEUX, "guitarset", "audio_mono-mic", "*_mic.wav")))[:args.limite]
    for audio in fichiers:
        nom = os.path.basename(audio)[:-8]; d = os.path.join(CACHE, "guitarset", nom); os.makedirs(d, exist_ok=True)
        j = jams.load(os.path.join(JEUX, "guitarset", "annotation", f"{nom}.jams"))
        ref = [(o.time, o.time + o.duration, int(round(o.value))) for a in j.search(namespace="note_midi") for o in a.data]
        sm = score_notes(ref, triplets(muscriptor(audio, os.path.join(d, "muscriptor.json"), GUITARES)))
        sb = score_notes(ref, [(n["debut_s"], n["fin_s"], n["hauteur"]) for n in basic_pitch(audio, os.path.join(d, "basic-pitch.json"))])
        genre = "solo" if nom.endswith("solo") else "accomp."
        T.ajouter(f"{genre} : MuScriptor", sm); T.ajouter(f"{genre} : Basic Pitch", sb)
        print(f"{nom:28} MuS {sm['f']:.2f}  BP {sb['f']:.2f}", flush=True)
    print("TOTAL notes\n" + str(T))

def pedale(pm):
    """Notes de référence, fin prolongée tant que la pédale forte (CC64 ≥ 64)
    tient : convention d'Onsets and Frames pour MAESTRO."""
    inst = pm.instruments[0]
    appuis, debut = [], None
    for c in sorted(inst.control_changes, key=lambda c: c.time):
        if c.number != 64: continue
        if c.value >= 64 and debut is None: debut = c.time
        elif c.value < 64 and debut is not None: appuis.append((debut, c.time)); debut = None
    if debut is not None: appuis.append((debut, pm.get_end_time()))
    notes = sorted(inst.notes, key=lambda n: n.start)
    ref = []
    for i, n in enumerate(notes):
        fin = n.end
        for a, b in appuis:
            if a <= fin < b: fin = b; break
        # Une note tenue s'arrête quand la même touche est rejouée.
        suivante = next((m.start for m in notes[i + 1:] if m.pitch == n.pitch), None)
        if suivante is not None: fin = min(fin, suivante)
        ref.append((n.start, max(fin, n.start + 0.01), n.pitch))
    return ref

def maestro(args):
    T = Total()
    base = os.path.join(JEUX, "maestro")
    for x in json.load(open(os.path.join(base, "choix.json")))[:args.limite]:
        audio = os.path.join(base, os.path.basename(x["audio_filename"]))
        nom = os.path.basename(audio)[:-4]; d = os.path.join(CACHE, "maestro", nom); os.makedirs(d, exist_ok=True)
        ref = pedale(pretty_midi.PrettyMIDI(os.path.join(base, os.path.basename(x["midi_filename"]))))
        sm = score_notes(ref, triplets(muscriptor(audio, os.path.join(d, "muscriptor.json"), "acoustic_piano")))
        sb = score_notes(ref, [(n["debut_s"], n["fin_s"], n["hauteur"]) for n in basic_pitch(audio, os.path.join(d, "basic-pitch.json"))])
        T.ajouter("piano : MuScriptor", sm); T.ajouter("piano : Basic Pitch", sb)
        ligne = f"{x['canonical_composer'][:22]:22} {x['canonical_title'][:30]:30} MuS {sm['f']:.2f}  BP {sb['f']:.2f}"
        # Modèles spécialisés, s'ils sont passés (`piano_specialises.py`).
        for nom_m, court in (("transkun", "TK"), ("bytedance", "BD")):
            mid = os.path.join(d, f"{nom_m}.mid")
            if os.path.exists(mid):
                s = score_notes(ref, [(n.start, n.end, n.pitch) for i in pretty_midi.PrettyMIDI(mid).instruments for n in i.notes])
                T.ajouter(f"piano : {nom_m}", s); ligne += f"  {court} {s['f']:.2f}"
        print(ligne, flush=True)
    print("TOTAL notes\n" + str(T))

def main():
    a = argparse.ArgumentParser()
    a.add_argument("jeu", choices=["slakh", "mdb", "guitarset", "maestro"])
    a.add_argument("--limite", type=int, default=None)
    args = a.parse_args()
    {"slakh": slakh, "mdb": mdb, "guitarset": guitarset, "maestro": maestro}[args.jeu](args)

if __name__ == "__main__":
    main()
