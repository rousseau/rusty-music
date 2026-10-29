# SPDX-License-Identifier: GPL-3.0-or-later
"""YourMT3+ (« YPTF.MoE+Multi (noPS) ») sur un fichier audio, vers un MIDI :
les réglages et l'appel du Space Hugging Face `mimbres/YourMT3` (`app.py`,
`model_helper.py`), sans Gradio, sur Metal quand il est là. Lancé par `externes.py` dans
l'environnement du Space, depuis son dossier.

    python yourmt3_inferer.py <dossier du Space> <audio> <sortie.mid>
"""
import os, shutil, sys

depot, audio, sortie = sys.argv[1:4]
sys.path.insert(0, os.path.join(depot, "amt", "src"))
sys.path.insert(0, depot)
os.chdir(depot)

from collections import Counter  # noqa: E402

import torch  # noqa: E402
import torchaudio  # noqa: E402
from model_helper import load_model_checkpoint  # noqa: E402
from utils.audio import slice_padded_array  # noqa: E402
from utils.event2note import merge_zipped_note_events_and_ties_to_notes  # noqa: E402
from utils.note2event import mix_notes  # noqa: E402
from utils.utils import write_model_output_as_midi  # noqa: E402

CHECKPOINT = "mc13_256_g4_all_v7_mt3f_sqr_rms_moe_wf4_n8k2_silu_rope_rp_b36_nops@last.ckpt"
ARGS = [CHECKPOINT, "-p", "2024", "-tk", "mc13_full_plus_256", "-dec", "multi-t5",
        "-nl", "26", "-enc", "perceiver-tf", "-sqr", "1", "-ff", "moe",
        "-wf", "4", "-nmoe", "8", "-kmoe", "2", "-act", "silu", "-epe", "rope",
        "-rp", "1", "-ac", "spec", "-hop", "300", "-atc", "1", "-pr", "32"]
# `transcribe` du Space ne connaît que CUDA ou le CPU (5 min par morceau) :
# la même chose, sur l'appareil demandé (Metal par défaut).
APPAREIL = torch.device(os.environ.get("YOURMT3_DEVICE", "mps" if torch.backends.mps.is_available() else "cpu"))

modele = load_model_checkpoint(args=ARGS, device="cpu").to(APPAREIL)
son, sr = torchaudio.load(audio)
son = torchaudio.functional.resample(torch.mean(son, dim=0).unsqueeze(0), sr, modele.audio_cfg["sample_rate"])
n = modele.audio_cfg["input_frames"]
segments = torch.from_numpy(slice_padded_array(son, n, n).astype("float32")).to(APPAREIL).unsqueeze(1)
with torch.no_grad():
    jetons, _ = modele.inference_file(bsz=8, audio_segments=segments)
debuts = [n * i / modele.audio_cfg["sample_rate"] for i in range(segments.shape[0])]
par_canal, erreurs = [], Counter()
for canal in range(modele.task_manager.num_decoding_channels):
    zippes, _, _ = modele.task_manager.detokenize_list_batches([a[:, canal, :] for a in jetons], debuts, return_events=True)
    notes, e = merge_zipped_note_events_and_ties_to_notes(zippes)
    par_canal.append(notes)
    erreurs += e
nom = "rusty-banc"
write_model_output_as_midi(mix_notes(par_canal), "./", nom, modele.midi_output_inverse_vocab)
shutil.move(os.path.join("model_output", nom + ".mid"), sortie)
