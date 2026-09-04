# SPDX-License-Identifier: GPL-3.0-or-later
"""Références PyTorch pour l'essai `burn-aero`.

Rejoue le réseau seul (comme `scripts/preparer-aero.sh`) et dumpe, en binaire
f32 brut (little-endian, row-major), tout ce qu'il faut pour valider le
pipeline Rust : STFT, iSTFT, réseau, et le forward complet d'AERO.

    PYTHONPATH=/tmp/rusty-music-aero-src python3 reference_aero.py \
        --checkpoint musdb-hl256.th --out /tmp/aero-ref --segment 5

Produit, sous <out>.* :
  lr.f32          audio basse résolution, [Nlr]           (11 025 Hz)
  spec.f32        _spec + move_to_channels, [2, 256, T]   (réel, imag)
  spec_hr.f32     réseau(spec), [2, 256, T]
  hr.f32          model(lr) complet, [Nhr]                (44 100 Hz)
  meta.txt        Nlr T Nhr nfft hop_in win_in hop_out win_out lr_sr hr_sr
"""
import argparse
import numpy as np
import torch
import src.models.modules as M
from src.models.aero import Aero

PARAMS = set("""in_channels out_channels audio_channels channels growth nfft
  hop_length end_iters cac rewrite hybrid hybrid_old freq_emb emb_scale
  emb_smooth kernel_size strides context context_enc freq_ends enc_freq_attn
  norm_starts norm_groups dconv_mode dconv_depth dconv_comp dconv_time_attn
  dconv_lstm dconv_init rescale lr_sr hr_sr spec_upsample act_func debug""".split())


def local_forward(self, x):                       # EyeLike -> (delta == 0)
    B, C, T = x.shape
    heads = self.heads
    idx = torch.arange(T, device=x.device, dtype=x.dtype)
    delta = idx[:, None] - idx[None, :]
    q = self.query(x).view(B, heads, -1, T)
    k = self.key(x).view(B, heads, -1, T)
    dots = torch.einsum("bhct,bhcs->bhts", k, q) / (k.shape[2] ** 0.5)
    if self.ndecay:
        d = torch.arange(1, self.ndecay + 1, device=x.device, dtype=x.dtype)
        dq = torch.sigmoid(self.query_decay(x).view(B, heads, -1, T)) / 2
        dk = -d.view(-1, 1, 1) * delta.abs() / self.ndecay ** 0.5
        dots = dots + torch.einsum("fts,bhfs->bhts", dk, dq)
    dots = dots.masked_fill(delta == 0, -100)
    w = torch.softmax(dots, dim=2)
    c = self.content(x).view(B, heads, -1, T)
    r = torch.einsum("bhts,bhct->bhcs", w, c).reshape(B, -1, T)
    return x + self.proj(r)


class Reseau(torch.nn.Module):
    def __init__(self, m):
        super().__init__()
        self.m = m

    def forward(self, x):
        B, _, Fq, T = x.shape
        mean = x.mean(dim=(1, 2, 3), keepdim=True)
        std = x.std(dim=(1, 2, 3), keepdim=True)
        h = (x - mean) / (1e-5 + std)
        saved, lengths = [], []
        for idx, enc in enumerate(self.m.encoder):
            lengths.append(h.shape[-1])
            h = enc(h, None)
            if idx == 0 and self.m.freq_emb is not None:
                frs = torch.arange(h.shape[-2], device=h.device)
                emb = self.m.freq_emb(frs).t()[None, :, :, None].expand_as(h)
                h = h + self.m.freq_emb_scale * emb
            saved.append(h)
        h = torch.zeros_like(h)
        for dec in self.m.decoder:
            h = dec(h, saved.pop(-1), lengths.pop(-1))
        h = h.view(B, self.m.out_channels, -1, Fq, T)
        h = h * std[:, None] + mean[:, None]
        return h[:, 0]


def dump(path, arr):
    arr.detach().cpu().numpy().astype("<f4").tofile(path)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--checkpoint", required=True)
    ap.add_argument("--segment", type=float, default=5.0)
    ap.add_argument("--out", default="/tmp/aero-ref")
    a = ap.parse_args()

    M.LocalState.forward = local_forward
    pkg = torch.load(a.checkpoint, map_location="cpu", weights_only=False)
    g = pkg["models"]["generator"]
    kw = {k: v for k, v in dict(g["kwargs"]).items() if k in PARAMS}
    model = Aero(**kw)
    model.load_state_dict(g["state"])
    model.eval()

    hop, lr_sr, hr_sr, nfft = model.hop_length, kw["lr_sr"], kw["hr_sr"], kw["nfft"]
    scale = int(model.scale)
    hop_in, win_in = hop, model.win_length                 # 64, 128
    hop_out, win_out = hop * scale, model.win_length * scale  # 256, 512
    lr_len = int(lr_sr * a.segment // hop) * hop

    torch.manual_seed(0)
    t = torch.arange(lr_len) / lr_sr
    audio = sum(torch.sin(2 * torch.pi * f * t) * (0.3 / (1 + f / 800))
                for f in [110, 175, 330, 660, 1200, 2500, 4000])
    audio = audio + 0.02 * torch.randn(lr_len)
    audio = (audio / audio.abs().max() * 0.7)[None, None]   # [1,1,Nlr]

    with torch.no_grad():
        z = model._spec(audio)                              # [1,1,256,T] complex
        spec = model._move_complex_to_channels_dim(z)       # [1,2,256,T]
        spec_hr = Reseau(model).eval()(spec)                # [1,2,256,T]
        hr = model(audio)                                   # [1,1,Nhr]

    T = spec.shape[-1]
    dump(f"{a.out}.lr.f32", audio[0, 0])
    dump(f"{a.out}.spec.f32", spec[0])
    dump(f"{a.out}.spec_hr.f32", spec_hr[0])
    dump(f"{a.out}.hr.f32", hr[0, 0])
    with open(f"{a.out}.meta.txt", "w") as f:
        f.write(f"Nlr {lr_len}\nT {T}\nNhr {hr.shape[-1]}\n"
                f"nfft {nfft}\nhop_in {hop_in}\nwin_in {win_in}\n"
                f"hop_out {hop_out}\nwin_out {win_out}\n"
                f"lr_sr {lr_sr}\nhr_sr {hr_sr}\n")
    print(f"Nlr={lr_len} T={T} Nhr={hr.shape[-1]}  hop_in={hop_in} win_in={win_in}"
          f"  hop_out={hop_out} win_out={win_out}")


if __name__ == "__main__":
    main()
