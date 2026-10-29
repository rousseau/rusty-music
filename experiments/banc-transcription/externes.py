# SPDX-License-Identifier: GPL-3.0-or-later
"""Modèles de transcription externes, au banc seulement : décider s'ils
valent un portage en Rust (`docs/plan-editer-pratique-creation.md`, 29 oct.,
recherche « tsumugi / YourMT3+ »). Chacun tourne dans son propre
environnement Python, par son code d'origine ; ses notes sont mises en cache
à côté de celles de MuScriptor, au format du banc
(`[{debut_s, fin_s, hauteur}]`).

- **tsumugi** (anime-song, code et poids MIT) : modèle spécialisé basse
  `bass_v2`. `git clone https://github.com/anime-song/tsumugi` dans
  `$EXT/tsumugi`, puis `uv sync --locked` (environnement : `TSUMUGI_PY`).
- **YourMT3+** (mimbres, code GPL-3, poids Apache-2.0) : le code d'inférence
  est celui du Space Hugging Face `mimbres/YourMT3` (`git clone` avec LFS
  dans `$EXT/yourmt3-space`), lancé par `yourmt3_inferer.py`
  (environnement : `YOURMT3_PY`). Modèle « YPTF.MoE+Multi (noPS) ».

Les clones et les poids vivent sous `$RUSTY_DEV/ext` ; les environnements,
eux, ne peuvent pas vivre sur le disque externe (exFAT, pas de liens
symboliques) : leurs interpréteurs se donnent par `TSUMUGI_PY` et
`YOURMT3_PY`.
"""
import os, subprocess, sys, tempfile, json

ICI = os.path.dirname(os.path.abspath(__file__))
DEV = os.path.expanduser(os.environ.get("RUSTY_DEV", "/Volumes/LaCieDock/rusty_music-dev"))
EXT = os.path.expanduser(os.environ.get("EXT", os.path.join(DEV, "ext")))


def notes_midi(chemin, programmes=None):
    import pretty_midi
    notes = []
    for inst in pretty_midi.PrettyMIDI(chemin).instruments:
        if inst.is_drum or (programmes is not None and inst.program not in programmes):
            continue
        notes += [{"debut_s": n.start, "fin_s": n.end, "hauteur": n.pitch} for n in inst.notes]
    return sorted(notes, key=lambda n: n["debut_s"])


def _lancer(cmd, cwd, env=None):
    r = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, env={**os.environ, **(env or {})})
    if r.returncode:
        sys.exit(f"échec : {' '.join(cmd)}\n{r.stderr[-3000:]}")


def tsumugi(wav, sortie):
    depot = os.path.join(EXT, "tsumugi")
    py = os.environ.get("TSUMUGI_PY", os.path.join(depot, ".venv", "bin", "python"))
    with tempfile.TemporaryDirectory() as tmp:
        mid = os.path.join(tmp, "basse.mid")
        _lancer([py, "-m", "instrument_agnostic_amt.amt.cli.infer", "--audio", wav, "--output-midi", mid,
                 "--type", "bass_v2", "--device", os.environ.get("TSUMUGI_DEVICE", "mps")],
                depot, {"HF_HOME": os.path.join(EXT, "hf")})
        # Basse aux doigts et basse slap en deux pistes : les deux.
        return notes_midi(mid)


def yourmt3(wav, sortie):
    depot = os.path.join(EXT, "yourmt3-space")
    py = os.environ.get("YOURMT3_PY", os.path.join(depot, ".venv", "bin", "python"))
    with tempfile.TemporaryDirectory() as tmp:
        mid = os.path.join(tmp, "basse.mid")
        _lancer([py, os.path.join(ICI, "yourmt3_inferer.py"), depot, wav, mid], depot)
        # Modèle multi-instrument : sur un stem de basse démixé, il répartit
        # les notes entre basse, guitare, orgue… (« About Damn Time » : 132
        # notes de basse, 101 de guitare). Tout ce qu'il entend hors
        # batterie ; la chaîne du banc rend la ligne monophonique.
        return notes_midi(mid)


MODELES = {"tsumugi": tsumugi, "yourmt3": yourmt3}


def transcrire(nom, wav, sortie):
    """Notes du modèle `nom` sur `wav`, en cache dans `sortie`."""
    if not os.path.exists(sortie):
        json.dump(MODELES[nom](wav, sortie), open(sortie, "w"))
    return json.load(open(sortie))


def fusion(listes, tolerance=0.05, quorum=1):
    """Notes communes à au moins `quorum` des listes : même hauteur, attaques
    à moins de `tolerance` s. `quorum=1` est l'union (borne haute du rappel
    d'un vote) ; 2 sur 3, un vote à la majorité. On garde, pour chaque groupe,
    la note de la première liste qui en a une (MuScriptor en tête)."""
    toutes = sorted((n["debut_s"], n["fin_s"], n["hauteur"], i) for i, l in enumerate(listes) for n in l)
    groupes, ouverts = [], {}
    for d, f, h, i in toutes:
        g = ouverts.get(h)
        if g is not None and d - g["debut"] <= tolerance:
            g["sources"].add(i)
            g["notes"].append((i, d, f))
        else:
            g = ouverts[h] = {"debut": d, "hauteur": h, "sources": {i}, "notes": [(i, d, f)]}
            groupes.append(g)
    garde = []
    for g in groupes:
        if len(g["sources"]) >= quorum:
            _, d, f = min(g["notes"])
            garde.append({"debut_s": d, "fin_s": f, "hauteur": g["hauteur"]})
    return sorted(garde, key=lambda n: n["debut_s"])
