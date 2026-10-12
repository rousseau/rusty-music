# SPDX-License-Identifier: GPL-3.0-or-later
"""Convertit les 8 plis d'all-in-one (`taejunkim/allinone`, poids MIT) de
`.pth` en safetensors, pour le portage Burn (`crates/transcription/src/sections`).
Seul le `state_dict` est gardé ; la configuration, identique d'un pli à
l'autre, est écrite dans le code Rust.

    <env>/bin/python exporter_safetensors.py [sortie]   # défaut : $RUSTY_DEV/modeles/allinone
"""
import glob, os, sys
import torch
from huggingface_hub import hf_hub_download
from safetensors.torch import save_file

DEV = os.path.expanduser(os.environ.get("RUSTY_DEV", "/Volumes/LaCieDock/rusty_music-dev"))
os.environ.setdefault("HF_HUB_CACHE", os.path.join(DEV, "hf", "hub"))
FICHIERS = ["harmonix-fold0-0vra4ys2.pth", "harmonix-fold1-3ozjhtsj.pth", "harmonix-fold2-gmgo0nsy.pth",
            "harmonix-fold3-i92b7m8p.pth", "harmonix-fold4-1bql5qo0.pth", "harmonix-fold5-x4z5zeef.pth",
            "harmonix-fold6-x7t226rq.pth", "harmonix-fold7-qwwskhg6.pth"]

def main():
    sortie = sys.argv[1] if len(sys.argv) > 1 else os.path.join(DEV, "modeles", "allinone")
    os.makedirs(sortie, exist_ok=True)
    for i, f in enumerate(FICHIERS):
        c = torch.load(hf_hub_download("taejunkim/allinone", f), map_location="cpu", weights_only=False)
        etat = {k: v.contiguous() for k, v in c["state_dict"].items()}
        dest = os.path.join(sortie, f"harmonix-fold{i}.safetensors")
        save_file(etat, dest, metadata={"source": f"taejunkim/allinone/{f}", "licence": "MIT"})
        print(dest, len(etat), "tenseurs")

if __name__ == "__main__":
    main()
