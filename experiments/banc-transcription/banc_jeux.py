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
- MedleyDB 1 et 2 (Bittner et al. 2014, 2016, CC BY-NC-SA 4.0, accès sur
  demande à Zenodo ; annotations du dépôt `marl/medleydb`, MIT) : vrais
  mélanges ; la basse comme contour de hauteur — 12 stems corrigés à la main
  (`Pitch`), 91 par pYIN sur la piste isolée (`Pitch_Pyin`, automatique mais
  sûr sur une basse seule) ;
- MAESTRO v3 (Hawthorne et al. 2019, CC BY-NC-SA 4.0) : piano réel,
  Disklavier ; 20 pièces tirées du jeu de test (`choix.json`), lues à
  distance dans l'archive sans la télécharger entière.

Chaîne de l'application : les stems viennent de **notre** HTDemucs
(`rusty-music demix`), puis Basic Pitch (notes brutes, réglages par défaut),
MuScriptor (portage Burn) ou ADTOF. Mesures de mir_eval : F1 de notes à
l'attaque (±50 ms, hauteur au demi-ton), avec ou sans la fin (20 % de la
durée) ; F1 de frappes par classe (±50 ms).

Sur MedleyDB, la mesure est celle de la mélodie (mir_eval.melody), la note
la plus grave sonnant à chaque instant du contour : justesse de hauteur à
±50 cents sur les instants voisés, rappel et fausse alarme de voisement.

    <env>/bin/python banc_jeux.py slakh|mdb|guitarset|medleydb [--limite N]

Modèles externes sur le stem de basse (Slakh et MedleyDB, `externes.py`) :
`EXTERNES=tsumugi,yourmt3` les ajoute, avec l'union des notes de tous les
modèles (borne haute du rappel d'un vote) et le vote à la majorité.
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

# Séparateurs hors de l'application (`experiments/roformer/separer.py`), lancés
# par leur code d'origine dans leur propre environnement (`ROFORMER_PY`).
SEPARER = os.path.join(RACINE, "experiments", "roformer", "separer.py")
ROFORMER_PY = os.path.expanduser(os.environ.get("ROFORMER_PY", os.path.join(os.environ.get("TMPDIR", "/tmp"), "roformer-env", "bin", "python")))
# Séparateur de la chaîne basse/batterie (Slakh, MedleyDB) : HTDemucs 6s par
# défaut, comme au premier banc ; `DEMIX=bsrofo_sw` en essaie un autre, avec
# ses propres stems et ses propres caches de transcription.
DEMIX = os.environ.get("DEMIX", "htdemucs_6s")

def separer(modele, src441, sortie):
    if os.path.isdir(sortie) and any(f.endswith(".wav") for f in os.listdir(sortie)):
        return
    if modele.startswith("htdemucs"):
        lancer([CLI, "demix", src441, "--out", sortie, "--modele", modele])
    else:
        liste = os.path.join(os.path.dirname(sortie), f"liste-{modele}.tsv")
        open(liste, "w").write(f"{src441}\t{sortie}\n")
        lancer([ROFORMER_PY, "-I", SEPARER, modele, liste])

def cache(nom):
    """Nom de cache propre au séparateur de `DEMIX` (inchangé pour HTDemucs 6s)."""
    return nom if DEMIX == "htdemucs_6s" else nom.replace(".json", f"@{DEMIX}.json")

def demixer(src441, dossier):
    stems = os.path.join(dossier, "stems" if DEMIX == "htdemucs_6s" else f"stems-{DEMIX}")
    separer(DEMIX, src441, stems)
    return {s: os.path.join(stems, f) for f in os.listdir(stems) if not f.startswith("._") for s in ("bass", "drums", "guitar", "vocals", "piano", "other") if f"— {s}." in f}

def adtof(stem441, dossier):
    sortie = os.path.join(dossier, cache("adtof.json"))
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

EXTERNES = [m for m in os.environ.get("EXTERNES", "").split(",") if m]

def basse_externes(stem, d, mus):
    """{nom: triplets} sur un stem de basse : chaque modèle externe, puis,
    s'il y en a, l'union et le vote à la majorité avec MuScriptor."""
    if not EXTERNES:
        return {}
    import externes
    res = {nom: triplets(externes.transcrire(nom, stem, os.path.join(d, cache(f"{nom}-basse.json")))) for nom in EXTERNES}
    listes = [[{"debut_s": a, "fin_s": b, "hauteur": h} for a, b, h in l] for l in [mus] + list(res.values())]
    res["union"] = triplets(externes.fusion(listes, quorum=1))
    if len(listes) >= 3:
        res["vote"] = triplets(externes.fusion(listes, quorum=(len(listes) + 1) // 2))
    return res

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
    # BabySlakh : 20 morceaux en WAV à 16 kHz. Slakh2100 : partie de test,
    # 151 morceaux en FLAC à 44,1 kHz (cymbales et charleston audibles).
    if args.jeu == "slakh2100":
        pistes, ext, dossier = glob.glob(os.path.join(JEUX, "slakh2100", "slakh2100_flac_redux", "test", "Track*")), "flac", "slakh2100"
    else:
        pistes, ext, dossier = glob.glob(os.path.join(JEUX, "babyslakh", "*", "Track*")), "wav", "slakh"
    T, TC = Total(), TotalCoups()
    pistes = sorted(p for p in pistes if not os.path.basename(p).startswith("._"))[:args.limite]
    for piste in pistes:
        nom = os.path.basename(piste); d = os.path.join(CACHE, dossier, nom); os.makedirs(d, exist_ok=True)
        meta = yaml.safe_load(open(os.path.join(piste, "metadata.yaml")))
        ref, ref_coups = [], []
        for s, m in meta["stems"].items():
            midi = os.path.join(piste, "MIDI", f"{s}.mid")
            # `audio_rendered` est faux partout dans BabySlakh : le stem
            # rendu fait foi.
            if not os.path.exists(os.path.join(piste, "stems", f"{s}.{ext}")) or not os.path.exists(midi):
                continue
            notes = [n for inst in pretty_midi.PrettyMIDI(midi).instruments for n in inst.notes]
            if m["is_drum"]:
                ref_coups += [(n.start, GM[n.pitch]) for n in notes if n.pitch in GM]
                continue
            # Le rendu de Slakh transpose certains instruments (les basses
            # sonnent une octave sous leur MIDI) : l'octave qui sonne se lit
            # sur le stem isolé, la même pour tous les modèles comparés.
            k = octave_rendue([(n.start, n.end, n.pitch) for n in notes], os.path.join(piste, "stems", f"{s}.{ext}"), os.path.join(d, f"octave-{s}.json"))
            ref += [(n.start, n.end, n.pitch + k, famille(m["program_num"])) for n in notes]
        mix = os.path.join(piste, f"mix.{ext}")
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
            smb = score_notes(rb, triplets(muscriptor(stems["bass"], os.path.join(d, cache("muscriptor-basse.json")))))
            sbb = score_notes(rb, [(n["debut_s"], n["fin_s"], n["hauteur"]) for n in basic_pitch(stems["bass"], os.path.join(d, cache("basic-pitch-basse.json")))])
            T.ajouter("stem basse : MuScriptor", smb); T.ajouter("stem basse : Basic Pitch", sbb)
            ligne += f"  | stem basse MuS {smb['f']:.2f} BP {sbb['f']:.2f}"
            mus = triplets(muscriptor(stems["bass"], os.path.join(d, cache("muscriptor-basse.json"))))
            for k, est in basse_externes(stems["bass"], d, mus).items():
                sc = score_notes(rb, est); T.ajouter(f"stem basse : {k}", sc); ligne += f" {k[:4]} {sc['f']:.2f}"
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

def contour(notes, temps, grave=True):
    """La note la plus grave (ou la plus aiguë) qui sonne à chaque instant, en
    Hz (0 : rien)."""
    f = np.zeros(len(temps))
    if notes:
        n = np.array(notes, dtype=float)
        n = n[np.argsort(-n[:, 2] if grave else n[:, 2])]  # la dernière écrite l'emporte
        for a, b, h in n:
            i, j = np.searchsorted(temps, [a, b])
            f[i:j] = mir_eval.util.midi_to_hz(h)
    return f

PLANCHER_PYIN = 55.0

def score_contour(ref_t, ref_f, notes, plancher=None, grave=True):
    """`plancher` : fréquence sous laquelle la référence ne descend pas (pYIN
    s'arrête à 55 Hz et note une octave trop haut le mi, le fa, le sol
    graves) ; une estimation sous le plancher, une octave pile sous la
    référence, y compte comme juste."""
    est = contour(notes, ref_t, grave)
    if plancher:
        ref_f = ref_f.copy()
        ok = (est > 0) & (est < plancher) & (ref_f > 0) & (np.abs(1200 * np.log2(np.maximum(est, 1e-9) * 2 / np.maximum(ref_f, 1e-9))) < 50)
        ref_f[ok] /= 2
    # Présence : la note annotée est-elle parmi celles qui sonnent, quelle que
    # soit la plus grave ou la plus aiguë ? Juste pour un stem qui mêle
    # plusieurs parties (deux chanteurs, l'« other » de HTDemucs), indulgent
    # pour une estimation chargée d'accords.
    trouve = np.zeros(len(ref_t), bool)
    for a, b, h in notes:
        i, j = np.searchsorted(ref_t, [a, b])
        r = ref_f[i:j]
        trouve[i:j] |= (r > 0) & (np.abs(1200 * np.log2(np.maximum(r, 1e-9) / mir_eval.util.midi_to_hz(h))) < 50)
    voise = ref_f > 0
    pres = float(trouve[voise].mean()) if voise.any() else 0.0
    rv, rc, ev, ec = mir_eval.melody.to_cent_voicing(ref_t, ref_f, ref_t, est)
    return dict(rpa=mir_eval.melody.raw_pitch_accuracy(rv, rc, ev, ec), rca=mir_eval.melody.raw_chroma_accuracy(rv, rc, ev, ec), vr=mir_eval.melody.voicing_recall(rv, ev),
                vfa=mir_eval.melody.voicing_false_alarm(rv, ev), n=int(np.sum(rv > 0)), pres=pres)

class TotalContours:
    """Moyennes pondérées par le nombre d'instants voisés de la référence."""
    def __init__(self): self.s = {}
    def ajouter(self, nom, sc):
        a = self.s.setdefault(nom, [0.0, 0.0, 0.0, 0, 0.0, 0.0])
        a[0] += sc["rpa"] * sc["n"]; a[1] += sc["vr"] * sc["n"]; a[2] += sc["vfa"] * sc["n"]; a[3] += sc["n"]; a[4] += sc["rca"] * sc["n"]; a[5] += sc["pres"] * sc["n"]
    def __str__(self):
        return "\n".join(f"  {k:36} justesse {v[0] / max(1, v[3]):.3f}  à l'octave près {v[4] / max(1, v[3]):.3f}  voisement {v[1] / max(1, v[3]):.3f}  fausse alarme {v[2] / max(1, v[3]):.3f}  présence {v[5] / max(1, v[3]):.3f}" for k, v in self.s.items())

# MedleyDB : chaque famille dit quels stems annotés elle prend, le stem de
# HTDemucs 6s où on les cherche, et à quoi MuScriptor est restreint. Les vents
# et cordes sont restreints à l'instrument exact (le programme du stem est
# connu) ; le chant, la guitare et le piano au groupe de leur stem, ce que
# l'application saurait faire. La basse se lit par la note la plus grave, les
# autres par la plus aiguë.
VENTS_CORDES = {"violin": "violin", "violin section": "violin", "viola": "viola", "viola section": "viola", "cello": "cello", "erhu": "violin",
                "flute": "flutes", "piccolo": "flutes", "dizi": "flutes", "bamboo flute": "flutes", "flute section": "flutes",
                "clarinet": "clarinet", "bass clarinet": "clarinet", "clarinet section": "clarinet", "oboe": "oboe", "bassoon": "bassoon",
                "trumpet": "trumpet", "trumpet section": "trumpet", "french horn": "french_horn", "horn section": "french_horn",
                "trombone": "trombone", "tuba": "tuba", "brass section": "brass_section",
                "alto saxophone": "soprano_and_alto_sax", "soprano saxophone": "soprano_and_alto_sax", "tenor saxophone": "tenor_sax", "baritone saxophone": "baritone_sax"}
FAMILLES = {
    "basse": ("bass", {"electric bass": None, "double bass": None}, True),
    "voix": ("vocals", {i: "voice" for i in ("male singer", "female singer", "vocalists", "male rapper", "female rapper")}, False),
    "guitare": ("guitar", {i: GUITARES for i in ("clean electric guitar", "distorted electric guitar", "acoustic guitar", "lap steel guitar")}, False),
    "piano": ("piano", {i: "acoustic_piano,electric_piano" for i in ("piano", "tack piano", "electric piano")}, False),
    "vents-cordes": ("other", VENTS_CORDES, False),
}

def medleydb_morceaux():
    racine = os.path.join(JEUX, "medleydb")
    ann = os.path.join(racine, "depot", "medleydb", "data")
    for v in ("V1", "V2"):
        for t in sorted(os.listdir(os.path.join(racine, v))):
            meta = os.path.join(ann, "Metadata", f"{t}_METADATA.yaml")
            if t.startswith("._") or not os.path.exists(meta):
                continue
            mix = os.path.join(racine, v, t, f"{t}_MIX.wav")
            # Un mélange de l'archive (AimeeNorwich_Child) est un AAC illisible
            # sous une extension .wav.
            if open(mix, "rb").read(4) != b"RIFF":
                print(f"{t:44} mélange illisible, sauté", flush=True)
                continue
            yield v, t, mix, yaml.safe_load(open(meta)), ann

def instrument(st):
    i = st["instrument"]
    return i if isinstance(i, str) else i[0]

def actif(ann, t, sid, temps):
    """1 où le stem joue d'après MedleyDB (confiance d'activation ≥ 0,5,
    leur seuil), 0 ailleurs. pYIN marque tout comme voisé, silences compris,
    où il suit le ronflement du secteur (60 Hz) ou la repisse : sans ce
    masque, la voix et les vents se noteraient sur du bruit."""
    lab = os.path.join(ann, "Annotations", "Activation_Confidence", "v2", f"{t}_ACTIVATION_CONF.lab")
    if not os.path.exists(lab):
        return np.ones(len(temps))
    a = np.genfromtxt(lab, delimiter=",", names=True)
    if sid not in a.dtype.names:
        return np.ones(len(temps))
    return (np.interp(temps, a["time"], a[sid]) >= 0.5).astype(float)

def medleydb(args):
    familles = args.famille.split(",") if args.famille else list(FAMILLES)
    T = {(f, s): TotalContours() for f in familles for s in ("main", "pyin")}
    pistes = []
    for v, t, mix, md, ann in medleydb_morceaux():
        for sid, st in sorted((md.get("stems") or {}).items()):
            inst = instrument(st)
            for f in familles:
                if inst not in FAMILLES[f][1]:
                    continue
                k = sid[1:]
                main = os.path.join(ann, "Annotations", "Pitch", f"{t}_STEM_{k}.csv")
                pyin = os.path.join(ann, "Annotations", "Pitch_Pyin", f"{t}_STEM_{k}_vamp_pyin_pyin_smoothedpitchtrack.csv")
                if os.path.exists(main):
                    pistes.append((f, t, mix, "main", main, inst, sid))
                elif os.path.exists(pyin):
                    pistes.append((f, t, mix, "pyin", pyin, inst, sid))
    for f, t, mix, sorte, fic, inst, sid in pistes[:args.limite]:
        nom_stem, table, grave = FAMILLES[f]
        d = os.path.join(CACHE, "medleydb", t); os.makedirs(d, exist_ok=True)
        ref = np.loadtxt(fic, delimiter=",", ndmin=2, usecols=(0, 1))  # quelques lignes ont une 3e colonne
        # pYIN ne rend que les instants voisés : on pose une grille régulière
        # (5,8 ms, celle des annotations) où le reste est muet.
        pas = 256 / 44100
        rt = np.arange(0, ref[-1, 0] + pas, pas)
        rf = np.zeros(len(rt))
        idx = np.clip(np.round(ref[:, 0] / pas).astype(int), 0, len(rt) - 1)
        rf[idx] = np.where(ref[:, 1] > 0, ref[:, 1], 0)
        if sorte == "pyin":
            rf *= actif(ann, t, sid, rt)
        stems = demixer(en_441(mix, os.path.join(d, "mix441.wav")), d)
        # Les noms de cache de la basse sont ceux du premier banc.
        if f == "basse":
            mus = triplets(muscriptor(stems["bass"], os.path.join(d, cache("muscriptor-basse.json"))))
            bp = basic_pitch(stems["bass"], os.path.join(d, cache("basic-pitch-basse.json")))
        else:
            groupes = table[inst]
            mus = triplets(muscriptor(stems[nom_stem], os.path.join(d, cache(f"muscriptor-{nom_stem}-{groupes.split(',')[0]}.json")), groupes))
            bp = basic_pitch(stems[nom_stem], os.path.join(d, cache(f"basic-pitch-{nom_stem}.json")))
        res = {"MuScriptor": mus, "Basic Pitch": [(n["debut_s"], n["fin_s"], n["hauteur"]) for n in bp]}
        if f == "basse":
            res.update(basse_externes(stems["bass"], d, mus))
        ligne = f"{f:12} {t:44} {sorte:4} {inst[:18]:18}"
        for k, est in res.items():
            sc = score_contour(rt, rf, est, PLANCHER_PYIN if sorte == "pyin" and f == "basse" else None, grave)
            T[(f, sorte)].ajouter(f"{f} : {k}", sc)
            ligne += f"  {k[:3]} {sc['rpa']:.2f}/{sc['vr']:.2f}/{sc['pres']:.2f}"
        print(ligne, flush=True)
    for (f, sorte), tot in T.items():
        if tot.s:
            print(f"TOTAL {f}, {'corrigés à la main' if sorte == 'main' else 'pYIN sur la piste isolée'}\n" + str(tot))

# Démixage : chaque stem de MedleyDB va dans une des six sources de HTDemucs 6s
# selon son instrument ; les modèles à quatre sources rangent guitare et piano
# dans « other ».
def source_de(inst):
    i = inst.lower()
    if i in ("electric bass", "double bass", "bass synthesizer"):
        return "bass"
    if any(m in i for m in ("singer", "vocalist", "rapper", "choir", "beatbox", "speaker", "crowd")):
        return "vocals"
    if any(m in i for m in ("drum", "percussion", "cymbal", "tabla", "bongo", "conga", "shaker", "tambourine", "cabasa", "castanet", "clap",
                            "snap", "gong", "darbuka", "doumbek", "timpani", "cowbell", "kick", "snare", "toms", "hi-hat", "triangle")):
        return "drums"
    if "guitar" in i:
        return "guitar"
    if "piano" in i:
        return "piano"
    return "other"

def sdr(ref, est):
    """SDR global de MDX (Mitsufuji et al. 2021) : pas de projection, ni de
    fenêtres ; un stem muet n'est pas noté."""
    return 10 * np.log10((np.sum(ref ** 2) + 1e-8) / (np.sum((ref - est) ** 2) + 1e-8))

# Un mélange que la somme des stems n'explique pas à 12 dB près (mastering,
# effets sur le bus, prise de son d'ensemble) fausse la référence : morceau
# affiché, mais hors des totaux.
AJUSTEMENT_MIN = 12.0

def medleydb_demix(args):
    import soundfile as sf
    modeles = args.modele.split(",")
    sources6 = ["bass", "drums", "vocals", "guitar", "piano", "other"]
    totaux = {}
    for v, t, mix, md, _ in list(medleydb_morceaux())[:args.limite]:
        d = os.path.join(CACHE, "medleydb", t); os.makedirs(d, exist_ok=True)
        m441 = en_441(mix, os.path.join(d, "mix441.wav"))
        x, _ = sf.read(m441, dtype="float32")
        # Références : les stems de MedleyDB, avec le gain qui les somme au
        # mélange (moindres carrés sur un échantillon sur 16) — le mixage
        # n'est pas une somme à gain unité.
        dossier = os.path.join(os.path.dirname(mix), md["stem_dir"])
        refs, noms = [], []
        for sid, st in sorted((md.get("stems") or {}).items()):
            w = os.path.join(dossier, st["filename"])
            if os.path.exists(w):
                y, _ = sf.read(w, dtype="float32", always_2d=True)
                y = np.repeat(y, 2, axis=1) if y.shape[1] == 1 else y[:, :2]
                n = min(len(x), len(y)); z = np.zeros_like(x); z[:n] = y[:n]
                refs.append(z); noms.append(source_de(instrument(st)))
        if not refs:
            continue
        A = np.stack([r[::16].ravel() for r in refs], 1)
        g, *_ = np.linalg.lstsq(A, x[::16].ravel(), rcond=None)
        par_source = {s: sum((g[i] * refs[i] for i in range(len(refs)) if noms[i] == s), np.zeros_like(x)) for s in sources6}
        par_source["other4"] = par_source["guitar"] + par_source["piano"] + par_source["other"]
        residu = 10 * np.log10(np.sum(x ** 2) / max(1e-8, np.sum((x - sum(par_source[s] for s in sources6)) ** 2)))
        ligne = f"{v} {t:44} ajustement {residu:5.1f} dB{'' if residu >= AJUSTEMENT_MIN else ' (écarté)'}"
        for mo in modeles:
            sortie = os.path.join(d, "stems" if mo == "htdemucs_6s" else f"stems-{mo}")
            if not mo.startswith("htdemucs") and residu < AJUSTEMENT_MIN:
                continue  # hors des totaux : inutile de payer la séparation
            separer(mo, m441, sortie)
            est = {}
            for fic in os.listdir(sortie):
                for s in sources6:
                    if not fic.startswith("._") and f"— {s}." in fic:
                        e, _ = sf.read(os.path.join(sortie, fic), dtype="float32", always_2d=True)
                        z = np.zeros_like(x); n = min(len(x), len(e)); z[:n] = e[:n, :2]; est[s] = z
            est["other4"] = sum(est.get(s, 0) for s in ("guitar", "piano", "other"))
            for s in (sources6 + ["other4"] if "guitar" in est else ["bass", "drums", "vocals", "other4"]):
                # Source absente ou presque (sous −50 dB du mélange) : pas notée.
                if s not in est or np.sum(par_source[s] ** 2) < 1e-5 * np.sum(x ** 2):
                    continue
                val = sdr(par_source[s], est[s])
                if residu >= AJUSTEMENT_MIN:
                    totaux.setdefault((v, mo, s), []).append(val)
                if s in ("bass", "drums", "vocals", "other4"):
                    ligne += f"  {mo[8:] or '4' if mo.startswith('htdemucs') else mo}:{s[:3]} {val:5.1f}"
        print(ligne, flush=True)
    print("TOTAL SDR médian par source (dB), V1 en partie dans MUSDB18 (entraînement de HTDemucs), V2 inédit")
    for (v, mo, s), vals in sorted(totaux.items()):
        print(f"  {v} {mo:12} {s:7} médiane {np.median(vals):5.2f}  moyenne {np.mean(vals):5.2f}  n {len(vals)}")

def main():
    a = argparse.ArgumentParser()
    a.add_argument("jeu", choices=["slakh", "slakh2100", "mdb", "guitarset", "maestro", "medleydb", "medleydb-demix"])
    a.add_argument("--limite", type=int, default=None)
    a.add_argument("--famille", help=f"medleydb : parmi {','.join(FAMILLES)} (toutes par défaut)")
    a.add_argument("--modele", default="htdemucs_6s,htdemucs,htdemucs_ft", help="medleydb-demix : variantes de HTDemucs, ou séparateurs de experiments/roformer/separer.py")
    args = a.parse_args()
    {"slakh": slakh, "slakh2100": slakh, "mdb": mdb, "guitarset": guitarset, "maestro": maestro, "medleydb": medleydb, "medleydb-demix": medleydb_demix}[args.jeu](args)

if __name__ == "__main__":
    main()
