# /// script
# requires-python = ">=3.10,<3.13"
# dependencies = ["numpy<2", "scipy", "librosa", "resampy", "pretty_midi", "mir_eval", "onnxruntime"]
# ///
# SPDX-License-Identifier: GPL-3.0-or-later
"""Parité du port Rust de Basic Pitch avec le code d'origine (Spotify).

Lit ce que vide `cargo run -p rusty-music-transcription --example transcrire --
<stem> <dossier>` : l'audio exact donné au réseau (22 050 Hz), les activations
et les notes brutes calculées en Rust. Rejoue en Python le découpage de
`inference.py` avec ONNX Runtime, puis `note_creation.output_to_notes_polyphonic`
du dépôt d'origine, et compare.

    uv run experiments/transcription/parite_basic_pitch.py <dossier> <basic_pitch_src> <nmp.onnx>

`basic_pitch_src` : un dossier contenant `basic_pitch/{__init__,constants,note_creation}.py`
copiés du dépôt spotify/basic-pitch (révision fa5997a).
"""
import json, sys
import numpy as np
import onnxruntime as ort

dossier, src, modele = sys.argv[1:4]
sys.path.insert(0, src)
from basic_pitch import constants as C
from basic_pitch import note_creation as NC

audio = np.fromfile(f"{dossier}/audio22k.f32", dtype="<f4")
note_rs = np.fromfile(f"{dossier}/note.f32", dtype="<f4").reshape(-1, 88)
onset_rs = np.fromfile(f"{dossier}/onset.f32", dtype="<f4").reshape(-1, 88)
notes_rs = json.load(open(f"{dossier}/notes.json"))

# Le découpage de `inference.run_inference`, sur le même signal.
n_olap = 30
overlap_len = n_olap * C.FFT_HOP
hop = C.AUDIO_N_SAMPLES - overlap_len
padded = np.concatenate([np.zeros(overlap_len // 2, dtype=np.float32), audio])
fen = []
for i in range(0, padded.shape[0], hop):
    w = padded[i:i + C.AUDIO_N_SAMPLES]
    if len(w) < C.AUDIO_N_SAMPLES:
        w = np.pad(w, [0, C.AUDIO_N_SAMPLES - len(w)])
    fen.append(w)
sess = ort.InferenceSession(modele)
sorties = {"note": [], "onset": []}
for w in fen:
    n, o = sess.run(["StatefulPartitionedCall:1", "StatefulPartitionedCall:2"], {"serving_default_input_2:0": w[None, :, None].astype(np.float32)})
    sorties["note"].append(n); sorties["onset"].append(o)

def unwrap(x):
    x = np.concatenate(x)[:, n_olap // 2:-(n_olap // 2), :]
    x = x.reshape(-1, x.shape[2])
    return x[: int(audio.shape[0] / hop * (C.AUDIO_WINDOW_LENGTH * C.ANNOTATIONS_FPS - n_olap)), :]

note_py, onset_py = unwrap(sorties["note"]), unwrap(sorties["onset"])
print(f"trames : rust {note_rs.shape[0]}, python {note_py.shape[0]}")
m = min(note_rs.shape[0], note_py.shape[0])
print(f"écart max des activations : note {np.abs(note_rs[:m] - note_py[:m]).max():.2e}, attaque {np.abs(onset_rs[:m] - onset_py[:m]).max():.2e}")

# La création de notes d'origine, réglages « basse » (30–400 Hz).
evts = NC.output_to_notes_polyphonic(note_py.copy(), onset_py.copy(), onset_thresh=0.5, frame_thresh=0.3,
    min_note_len=11, infer_onsets=True, max_freq=400.0, min_freq=30.0, melodia_trick=True)
t = NC.model_frames_to_time(note_py.shape[0])
py = sorted((round(float(t[a]), 4), round(float(t[b]), 4), int(p)) for a, b, p, _ in evts)
rs = sorted((round(n["debut_s"], 4), round(n["fin_s"], 4), n["hauteur"]) for n in notes_rs)
print(f"notes : rust {len(rs)}, python {len(py)}")
# Appariement à 1 ms près : le Rust calcule les instants en f32, le Python en f64.
restants = list(rs)
appariees = 0
for a in py:
    k = next((j for j, b in enumerate(restants) if b[2] == a[2] and abs(b[0] - a[0]) < 1e-3 and abs(b[1] - a[1]) < 1e-3), None)
    if k is not None:
        restants.pop(k); appariees += 1
print(f"identiques (début, fin à 1 ms près, même hauteur) : {appariees} / {len(py)} — {100 * appariees / max(1, len(py)):.1f} %")
for b in restants[:5]: print("  sans pendant", b)
