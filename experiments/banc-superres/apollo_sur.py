# SPDX-License-Identifier: GPL-3.0-or-later
"""Banc G2 : Apollo par chargement sûr (décision du 10 nov.) — ni le dépôt
cloné exécuté, ni le pickle d'origine déroulé par le chargeur complet.

    python -I apollo_sur.py charger <pytorch_model.bin> <poids.safetensors>
    python -I apollo_sur.py traiter <racine> <poids.safetensors> <cpu|mps> [noms,…]
    python -I apollo_sur.py ecoute <racine>   # AERO contre Apollo, à l'aveugle

Réseau réécrit ici d'après JusperLee/Apollo (code CC BY-SA 4.0, © Kai Li et
al. ; commit e84bcac, `look2hear/models/apollo.py`) : STFT 20 ms (882
points, pas 441) à 44,1 kHz, 80 bandes (79 × 5 cases + 47), par bande
RMSNorm + projection 1 × 1 vers 256 ; 6 blocs (Roformer à travers les
bandes, RoPE, 8 têtes ; puis trois convolutions séparables le long du
temps) ; par bande, RMSNorm, projection, GLU → partie réelle et imaginaire ;
ISTFT. L'adaptation reste sous CC BY-SA 4.0, compatible dans un sens avec
la GPL-3 (liste de Creative Commons) ; elle ne quitte pas `experiments/`.

Poids (CC BY-SA 4.0, fiche Hugging Face JusperLee/Apollo) : archive torch
`apollo/data.pkl`. Ses globales hors torch, relevées par `pickletools` :
`omegaconf` (DictConfig, ListConfig, Metadata, ContainerMetadata,
AnyNode), `typing.Any`, `torch.torch_version.TorchVersion`,
`collections.defaultdict`, et `dict`, `list`, `long` du protocole 2 —
les hyperparamètres d'entraînement, pas le
réseau. Elles sont remplacées par une classe inerte ; c'est le chargeur
restreint de torch (`weights_only=True`) qui lit le fichier. Chargement
strict du `state_dict`.
"""

import json
import sys
import time
from pathlib import Path

import numpy as np
import torch
import torch._weights_only_unpickler as wou
import torch.nn.functional as F
from torch import nn

SR = 44_100


class Inerte:
    """Tient la place d'un objet de configuration : garde ce qu'on lui donne."""

    def __init__(self, *a, **k):
        self.args = a

    def __setstate__(self, etat):
        self.etat = etat

    def __call__(self, *a, **k):
        return Inerte(*a)


INERTES = [
    "omegaconf.dictconfig.DictConfig",
    "omegaconf.listconfig.ListConfig",
    "omegaconf.base.Metadata",
    "omegaconf.base.ContainerMetadata",
    "omegaconf.nodes.AnyNode",
    "typing.Any",
    "torch.torch_version.TorchVersion",
    "collections.defaultdict",
]


def charger_state_dict(chemin):
    autorises = wou._get_allowed_globals

    def elargis():
        g = dict(autorises())
        for nom in INERTES:
            g[nom] = Inerte
        # `__builtin__ dict/list/long` du protocole 2 : les types eux-mêmes,
        # sans effet de bord.
        g.update({"builtins.dict": dict, "builtins.list": list, "builtins.int": int})
        return g

    wou._get_allowed_globals = elargis
    try:
        d = torch.load(chemin, map_location="cpu", weights_only=True)
    finally:
        wou._get_allowed_globals = autorises
    return d["model_name"], d["state_dict"]


# ---- réseau -------------------------------------------------------------


class RMSNorm(nn.Module):
    def __init__(self, n):
        super().__init__()
        self.weight = nn.Parameter(torch.ones(n))

    def forward(self, x):  # B, N, T
        y = x.float() * torch.rsqrt(x.float().pow(2).mean(1, keepdim=True) + 1e-5)
        return y.type_as(x) * self.weight.view(1, -1, 1)


class Roformer(nn.Module):
    """Attention à travers les bandes, RoPE (paires entrelacées), MLP à porte."""

    def __init__(self, n, tetes=8, fenetre=100, theta=10000):
        super().__init__()
        self.tetes, self.d = tetes, n // tetes
        freq = 1.0 / theta ** (torch.arange(0, self.d, 2)[: self.d // 2] / self.d)
        pos = torch.arange(fenetre).view(-1, 1) * freq.view(1, -1)
        self.register_buffer("cos_freq", torch.stack([pos.cos()] * 2, -1).reshape(fenetre, self.d))
        self.register_buffer("sin_freq", torch.stack([pos.sin()] * 2, -1).reshape(fenetre, self.d))
        self.input_norm = RMSNorm(n)
        self.weight = nn.Conv1d(n, 3 * n, 1, bias=False)
        self.output = nn.Conv1d(n, n, 1, bias=False)
        self.MLP = nn.Sequential(RMSNorm(n), nn.Conv1d(n, 8 * n, 1, bias=False), nn.SiLU())
        self.MLP_output = nn.Conv1d(4 * n, n, 1, bias=False)

    def rope(self, x):  # ..., T, d
        T, d = x.shape[-2:]
        paires = x.reshape(*x.shape[:-1], d // 2, 2)
        tourne = torch.stack([-paires[..., 1], paires[..., 0]], -1).reshape(x.shape)
        return x * self.cos_freq[:T] + tourne * self.sin_freq[:T]

    def forward(self, x):  # B, N, T
        B, _, T = x.shape
        w = self.weight(self.input_norm(x)).reshape(B, self.tetes, 3 * self.d, T).mT
        q, k, v = torch.split(w, self.d, dim=-1)
        a = F.scaled_dot_product_attention(self.rope(q).contiguous(), self.rope(k).contiguous(), v.contiguous())
        y = self.output(a.mT.reshape(B, -1, T)) + x
        porte, z = self.MLP(y).chunk(2, dim=1)
        return y + self.MLP_output(F.silu(porte) * z)


class ConvActNorm(nn.Module):
    def __init__(self, n, k=7):
        super().__init__()
        self.conv = nn.Sequential(
            nn.Conv1d(n, n, k, padding=(k - 1) // 2, groups=n), RMSNorm(n),
            nn.Conv1d(n, 4 * n, 1), nn.SiLU(), nn.Conv1d(4 * n, n, 1))

    def forward(self, x):
        return x + self.conv(x)


class BSNet(nn.Module):
    def __init__(self, n):
        super().__init__()
        self.band_net = Roformer(n)
        self.seq_net = nn.Module()
        self.seq_net.blocks = nn.Sequential(ConvActNorm(n), ConvActNorm(n), ConvActNorm(n))

    def forward(self, x):  # B, nband, N, T
        B, nb, N, T = x.shape
        y = self.band_net(x.permute(0, 3, 2, 1).reshape(B * T, N, nb))
        y = y.reshape(B, T, N, nb).permute(0, 3, 2, 1)
        return self.seq_net.blocks(y.reshape(B * nb, N, T)).reshape(B, nb, N, T)


class Apollo(nn.Module):
    def __init__(self, sr=SR, win_ms=20, n=256, couches=6):
        super().__init__()
        self.win = int(sr * win_ms // 1000)
        self.pas = self.win // 2
        cases = self.win // 2 + 1
        self.largeurs = [self.win // 160] * 79
        self.largeurs.append(cases - sum(self.largeurs))
        self.BN = nn.ModuleList([nn.Sequential(RMSNorm(2 * l + 1), nn.Conv1d(2 * l + 1, n, 1)) for l in self.largeurs])
        self.net = nn.Sequential(*[BSNet(n) for _ in range(couches)])
        self.output = nn.ModuleList([
            nn.Sequential(RMSNorm(n), nn.Conv1d(n, 4 * l, 1), nn.GLU(dim=1)) for l in self.largeurs])

    def forward(self, x):  # B, canaux, échantillons
        B, C, L = x.shape
        fen = torch.hann_window(self.win, device=x.device)
        spec = torch.stft(x.reshape(B * C, L), self.win, self.pas, window=fen, return_complex=True)
        eps = torch.finfo(torch.float32).eps
        feats, i = [], 0
        for l, bn in zip(self.largeurs, self.BN):
            s = spec[:, i : i + l]
            p = (s.abs().pow(2).sum(1, keepdim=True) + eps).sqrt()
            feats.append(bn(torch.cat([s.real / p, s.imag / p, p.log()], 1)))
            i += l
        h = self.net(torch.stack(feats, 1))
        est = []
        for k, (l, sortie) in enumerate(zip(self.largeurs, self.output)):
            ri = sortie(h[:, k]).view(B * C, 2, l, -1)
            est.append(torch.complex(ri[:, 0], ri[:, 1]))
        y = torch.istft(torch.cat(est, 1), self.win, self.pas, window=fen, length=L)
        return y.view(B, C, -1)


def construire(state):
    m = Apollo()
    res = m.load_state_dict(state, strict=True)
    return m.eval(), res


# ---- étapes -------------------------------------------------------------


def charger(bin_, sortie):
    from safetensors.torch import save_file

    nom, state = charger_state_dict(bin_)
    assert nom == "Apollo", nom
    m, _ = construire(state)
    formes = {k: tuple(v.shape) for k, v in m.state_dict().items()}
    assert all(formes[k] == tuple(v.shape) for k, v in state.items())
    n = sum(v.numel() for k, v in state.items() if not k.endswith("_freq"))
    save_file({k: v.contiguous() for k, v in state.items()}, sortie)
    print(f"chargement strict : {len(state)} tenseurs, {n / 1e6:.2f} M paramètres ; {sortie}")


def traiter(racine, poids, device, seuls=None):
    from safetensors.torch import load_file

    sys.path.insert(0, str(Path(__file__).parent))
    import banc  # lire, ecrire, morceaux (même découpe que le banc)

    racine = Path(racine)
    m, _ = construire(load_file(poids))
    dev = torch.device(device)
    m = m.to(dev)
    meta = json.load(open(racine / "extraits.json"))
    temps = {}
    for e in meta:
        if seuls and e["nom"] not in seuls:
            continue
        x = torch.from_numpy(banc.lire(racine / "extraits" / f"{e['nom']}.wav")).unsqueeze(0)
        with torch.inference_mode():
            if device == "mps":
                torch.mps.synchronize()
            t = time.perf_counter()
            y = banc.morceaux(m, x, dev)
            if device == "mps":
                torch.mps.synchronize()
            dt = time.perf_counter() - t
        temps[e["nom"]] = dt
        banc.ecrire(racine / "sorties" / f"{e['nom']}-apollo.wav", np.clip(y[0], -1, 1))
        print(f"{e['nom']}  {dt:.1f} s", flush=True)
    json.dump(temps, open(racine / f"temps-apollo-{device}.json", "w"), indent=1)


def ecoute(racine, n=10):
    """À l'aveugle, AERO contre Apollo seulement (décision du 10 nov.) : les
    mêmes extraits que `banc.py ecoute` (graine 1110), même RMS que le MP3,
    lettres tirées au hasard ; la clé dans un fichier à part."""
    import random

    sys.path.insert(0, str(Path(__file__).parent))
    import banc

    racine = Path(racine)
    meta = [m for m in json.load(open(racine / "extraits.json")) if not m["ref"]]
    random.seed(1110)
    choisis = random.sample(meta, min(n, len(meta)))
    dossier = racine / "ecoute-aero-apollo"
    dossier.mkdir(exist_ok=True)
    cle = {}
    for i, m in enumerate(choisis):
        cible = banc.rms(banc.lire(racine / "extraits" / f"{m['nom']}.wav"))
        lettres = ["A", "B"]
        random.shuffle(lettres)
        for v, l in zip(("aero", "apollo"), lettres):
            x = banc.lire(racine / "sorties" / f"{m['nom']}-{v}.wav")
            y = x * (cible / banc.rms(x))
            y = y / max(1.0, float(np.abs(y).max()) / 0.98)
            banc.ecrire(dossier / f"{i + 1:02d}-{l}.wav", y)
            cle[f"{i + 1:02d}-{l}"] = {"version": v, "extrait": m["nom"], "debit": m["debit"]}
    (racine / "ecoute-aero-apollo-cle.json").write_text(json.dumps(cle, ensure_ascii=False, indent=1))
    print(f"{len(choisis)} extraits dans {dossier}")


if __name__ == "__main__":
    if sys.argv[1] == "charger":
        charger(sys.argv[2], sys.argv[3])
    elif sys.argv[1] == "traiter":
        traiter(sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5].split(",") if len(sys.argv) > 5 else None)
    elif sys.argv[1] == "ecoute":
        ecoute(sys.argv[2])
