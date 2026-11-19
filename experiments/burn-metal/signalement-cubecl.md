# Brouillon de signalement — à poster (ou non) par François

Destinataire proposé : https://github.com/tracel-ai/cubecl/issues (ou burn, si
on préfère partir de `Tensor::matmul`). Rien n'a été envoyé.

---

**Title:** Metal (MSL) matmul returns wrong values for `[.., s, 48] × [.., 48, s]` with s ≈ 1280–1536

**Versions:** burn 0.21.0 (`wgpu` + `metal` feature), cubecl 0.10/0.11 (as
resolved by burn 0.21), macOS, Apple M4 Pro.

**What happens.** `q.matmul(k.swap_dims(2, 3))` with `q`, `k` of shape
`[1, h, s, 48]` gives wrong results on the MSL backend when `s` is between
about 1280 and 1536 (1344 in our model): max abs error up to ~70 % of the
max of the result. Same code is exact (≈ 3e-7 relative) on `ndarray` and on
the default WGSL backend, and on MSL for `s` = 672, 1000, 1024, 2688, or for
head widths 32 or 64.

- Reproduces at every `CUBECL_AUTOTUNE_LEVEL` (minimal → full) with
  `CUBECL_AUTOTUNE_CACHE=false`.
- Reproduces without the `fusion` feature (then even freshly uploaded,
  contiguous `q` fails); with fusion, it fails whenever `q` is a view
  (`reshape([1, s, h, d]).swap_dims(1, 2)`, as in `burn::nn::MultiHeadAttention`).
- Zero-padding the last dim from 48 to 64 makes it exact.

Separately, on WGSL:
- `Conv1d` with kernel 1, 24 input channels, 384 output channels, batch 1 and
  5375 or 5376 samples has up to ~69 % max error (4096 samples: exact; 32 input channels: exact;
  batch 32 × 336 samples: exact). Reproducer:
  `crates/editor/examples/formes_conv.rs`.
- `w.matmul(v)` with `w: [1, 8, 1000, 1000]` (contiguous) and
  `v: [1, 8, 1000, 48]` has a ~27 % max error.

**Reproducer:** a self-contained example computing these products against an
f64 reference written by hand —
`crates/editor/examples/formes_attention.rs` in
https://github.com/rousseau/rusty-music (only depends on burn; the backend
type is `burn::backend::Wgpu<f32, i32>`, built with `--features burn/metal`).

**Impact.** HTDemucs separates wrong (16–40 dB from PyTorch instead of
70 dB): 6-source on MSL through the attention, all three variants on WGSL
through the 1×1 convolution. We work around both by zero-padding the inner
dimension to a multiple of 32 (https://github.com/rousseau/demucs-rs, branch
`attention-tetes-completees`).
