# SPDX-License-Identifier: GPL-3.0-or-later
"""Modèles de piano spécialisés, code Python de référence, sur les pièces
MAESTRO du banc (`banc_jeux.py maestro` les lit ensuite) :
- Transkun 2 (Yan et Duan 2024, MIT ; poids inclus dans le paquet) ;
- piano_transcription_inference (Kong et al. 2021, ByteDance, Apache-2.0 ;
  poids téléchargés depuis Zenodo au premier lancement).

Environnement à part : `pip install transkun piano_transcription_inference`.
Sortie : `$RUSTY_DEV/banc-jeux/maestro/<pièce>/{transkun,bytedance}.mid`.
"""
import json, os, subprocess, sys

# Données de développement sur le disque externe, pas toujours branché.
DEV = os.path.expanduser(os.environ.get("RUSTY_DEV", "/Volumes/LaCieDock/rusty_music-dev"))
if not os.path.isdir(DEV):
    sys.exit(f"données de développement absentes : {DEV} (disque débranché ? sinon RUSTY_DEV=…)")
JEUX = os.path.expanduser(os.environ.get("JEUX", os.path.join(DEV, "jeux")))
CACHE = os.path.expanduser(os.environ.get("CACHE", os.path.join(DEV, "banc-jeux")))
base = os.path.join(JEUX, "maestro")
choix = json.load(open(os.path.join(base, "choix.json")))
limite = int(sys.argv[1]) if len(sys.argv) > 1 else None
bin_ = os.path.dirname(sys.executable)
pt = None
for x in choix[:limite]:
    audio = os.path.join(base, os.path.basename(x["audio_filename"]))
    d = os.path.join(CACHE, "maestro", os.path.basename(audio)[:-4]); os.makedirs(d, exist_ok=True)
    tk = os.path.join(d, "transkun.mid")
    if not os.path.exists(tk):
        subprocess.run([os.path.join(bin_, "transkun"), audio, tk, "--device", "mps"], check=True)
    bd = os.path.join(d, "bytedance.mid")
    if not os.path.exists(bd):
        if pt is None:
            import librosa
            from piano_transcription_inference import PianoTranscription, sample_rate
            pt = PianoTranscription(device="cpu")
        # `load_audio` du paquet appelle une API retirée de librosa.
        a, _ = librosa.load(audio, sr=sample_rate, mono=True)
        pt.transcribe(a, bd)
    print(os.path.basename(d), flush=True)
