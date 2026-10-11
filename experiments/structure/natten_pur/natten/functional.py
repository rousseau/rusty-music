# SPDX-License-Identifier: GPL-3.0-or-later
"""Les quatre opérations de NATTEN qu'appelle all-in-one, en PyTorch pur :
NATTEN n'a ni roue pour macOS ni export ONNX, et c'est aussi la référence
du portage en Burn (gather + softmax, sans noyau dédié).

Attention de voisinage dilatée, à la NATTEN 0.14 : avec une dilatation d,
la séquence se lit comme d sous-suites entrelacées (i ≡ r mod d) ; dans sa
sous-suite, chaque position voit k voisins consécutifs, fenêtre centrée
puis repoussée contre les bords (jamais tronquée). Le biais de position
relative vaut `rpb[(j - i) / d + k - 1]`.
"""
import torch

def voisins(longueur, k, d, device=None):
    """Indices `[longueur, k]` des voisins, et indices du biais relatif."""
    i = torch.arange(longueur, device=device)
    r, u = i % d, i // d
    long_r = (longueur - r + d - 1) // d
    debut = torch.clamp(u - k // 2, min=0)
    debut = torch.minimum(debut, torch.clamp(long_r - k, min=0))
    j = (debut[:, None] + torch.arange(k, device=device)[None, :]) * d + r[:, None]
    return j, (j - i[:, None]) // d + k - 1

def natten1dqkrpb(q, k, rpb, kernel_size, dilation):
    """q, k : [B, H, L, D] ; rpb : [H, 2k-1] → scores [B, H, L, k]."""
    j, b = voisins(q.shape[2], kernel_size, dilation, q.device)
    kv = k[:, :, j]                                  # [B, H, L, k, D]
    s = (q.unsqueeze(3) * kv).sum(-1)
    return s if rpb is None else s + rpb[:, b].unsqueeze(0)

def natten1dav(a, v, kernel_size, dilation):
    """a : [B, H, L, k] ; v : [B, H, L, D] → [B, H, L, D]."""
    j, _ = voisins(v.shape[2], kernel_size, dilation, v.device)
    return (a.unsqueeze(-1) * v[:, :, j]).sum(3)

def _index_2d(x, y, k, d, device):
    jx, bx = voisins(x, k, d, device)
    jy, by = voisins(y, k, d, device)
    # Voisins dans l'ordre ligne par ligne : (ki, kj) → ki * k + kj.
    lin = (jx[:, None, :, None] * y + jy[None, :, None, :]).reshape(x, y, k * k)
    return lin, bx, by

def natten2dqkrpb(q, k, rpb, kernel_size, dilation):
    """q, k : [B, H, X, Y, D] ; rpb : [H, 2k-1, 2k-1] → [B, H, X, Y, k²]."""
    B, H, X, Y, D = q.shape
    lin, bx, by = _index_2d(X, Y, kernel_size, dilation, q.device)
    kv = k.reshape(B, H, X * Y, D)[:, :, lin]        # [B, H, X, Y, k², D]
    s = (q.unsqueeze(4) * kv).sum(-1)
    if rpb is not None:
        biais = rpb[:, bx[:, None, :, None], by[None, :, None, :]]   # [H, X, Y, k, k]
        s = s + biais.reshape(H, X, Y, kernel_size * kernel_size).unsqueeze(0)
    return s

def natten2dav(a, v, kernel_size, dilation):
    """a : [B, H, X, Y, k²] ; v : [B, H, X, Y, D] → [B, H, X, Y, D]."""
    B, H, X, Y, D = v.shape
    lin, _, _ = _index_2d(X, Y, kernel_size, dilation, v.device)
    return (a.unsqueeze(-1) * v.reshape(B, H, X * Y, D)[:, :, lin]).sum(4)
