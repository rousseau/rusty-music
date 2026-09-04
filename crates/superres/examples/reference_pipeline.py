# SPDX-License-Identifier: GPL-3.0-or-later
"""Référence PyTorch pour `examples/verifier.rs` : model() sur un fichier entier.

  PYTHONPATH=/tmp/rusty-music-aero-src python3 reference_pipeline.py \
      --checkpoint musdb-hl256.th --wav /tmp/sr_in.wav --out /tmp/sr_ref

Écrit  <out>.lr.f32  (11 025 Hz, torchaudio)  et  <out>.hr.f32  (44 100 Hz).
"""
import argparse, numpy as np, torch
import src.models.modules as M
from src.models.aero import Aero
from torchaudio.functional import resample

PARAMS = set("""in_channels out_channels audio_channels channels growth nfft
  hop_length end_iters cac rewrite hybrid hybrid_old freq_emb emb_scale
  emb_smooth kernel_size strides context context_enc freq_ends enc_freq_attn
  norm_starts norm_groups dconv_mode dconv_depth dconv_comp dconv_time_attn
  dconv_lstm dconv_init rescale lr_sr hr_sr spec_upsample act_func debug""".split())


def local_forward(self, x):
    B, C, T = x.shape
    h = self.heads
    i = torch.arange(T, device=x.device, dtype=x.dtype)
    d = i[:, None] - i[None, :]
    q = self.query(x).view(B, h, -1, T)
    k = self.key(x).view(B, h, -1, T)
    dt = torch.einsum("bhct,bhcs->bhts", k, q) / (k.shape[2] ** 0.5)
    if self.ndecay:
        de = torch.arange(1, self.ndecay + 1, device=x.device, dtype=x.dtype)
        dq = torch.sigmoid(self.query_decay(x).view(B, h, -1, T)) / 2
        dt = dt + torch.einsum("fts,bhfs->bhts", -de.view(-1, 1, 1) * d.abs() / self.ndecay ** 0.5, dq)
    dt = dt.masked_fill(d == 0, -100)
    w = torch.softmax(dt, dim=2)
    c = self.content(x).view(B, h, -1, T)
    return x + self.proj(torch.einsum("bhts,bhct->bhcs", w, c).reshape(B, -1, T))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--checkpoint", required=True)
    ap.add_argument("--wav", required=True)
    ap.add_argument("--out", default="/tmp/sr_ref")
    a = ap.parse_args()

    M.LocalState.forward = local_forward
    pkg = torch.load(a.checkpoint, map_location="cpu", weights_only=False)
    g = pkg["models"]["generator"]
    kw = {k: v for k, v in dict(g["kwargs"]).items() if k in PARAMS}
    model = Aero(**kw)
    model.load_state_dict(g["state"])
    model.eval()

    import wave
    w = wave.open(a.wav, "rb")
    n, ch = w.getnframes(), w.getnchannels()
    raw = np.frombuffer(w.readframes(n), "<i2").astype("f4") / 32768.0
    x = torch.from_numpy(raw.reshape(-1, ch).mean(axis=1))

    lr = resample(x[None], w.getframerate(), 11025)[0]
    with torch.no_grad():
        hr = model(lr[None, None])[0, 0]
    np.asarray(lr).astype("<f4").tofile(f"{a.out}.lr.f32")
    np.asarray(hr).astype("<f4").tofile(f"{a.out}.hr.f32")
    print(f"lr {tuple(lr.shape)} -> hr {tuple(hr.shape)}")


if __name__ == "__main__":
    main()
