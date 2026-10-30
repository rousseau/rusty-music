# SPDX-License-Identifier: GPL-3.0-or-later
"""Références de parité pour le port Burn de BS-RoFormer
(`crates/editor/src/roformer/`), écrites par le code d'origine (MSST) sur
processeur, en float32 :

    python -I reference.py <modele> <entree.wav> <dossier> [debut_s] [duree_s]

Écrit dans `dossier` (float32 petit-boutiste, à plat) :
- `audio.f32` : l'extrait, gauche puis droite ;
- `spectre.f32`, `decoupe.f32`, `bloc<i>.f32`, `masques.f32` : les étages
  du réseau sur le premier segment (pour bissecter une divergence) ;
- `segment.f32` : la sortie de `BSRoformer.forward` sur ce segment,
  `[stems][canal][échantillon]` ;
- `demix.f32` : `demix` sur l'extrait entier (segments recouvrants) ;
- `infos.json` : formes et noms des stems.
"""
import json, os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from separer import DEPOT, MODELES, POIDS  # noqa: E402


def main():
    nom, entree, dossier = sys.argv[1:4]
    debut = float(sys.argv[4]) if len(sys.argv) > 4 else 60.0
    duree = float(sys.argv[5]) if len(sys.argv) > 5 else 30.0
    sys.path.insert(0, DEPOT)
    import numpy as np, soundfile as sf, torch
    from utils.settings import get_model_from_config
    from utils.model_utils import demix

    torch.set_grad_enabled(False)
    type_, cfg, ckpt = MODELES[nom]
    modele, config = get_model_from_config(type_, os.path.join(POIDS, cfg))
    etat = torch.load(os.path.join(POIDS, ckpt), map_location="cpu", weights_only=True)
    for cle in ("state", "state_dict", "model_state_dict"):
        etat = etat.get(cle, etat)
    modele.load_state_dict(etat)
    modele = modele.eval()

    x, sr = sf.read(entree, dtype="float32", always_2d=True)
    assert sr == 44100
    x = x[int(debut * sr):int((debut + duree) * sr)].T.copy()
    os.makedirs(dossier, exist_ok=True)
    ecrire = lambda n, t: np.ascontiguousarray(t, dtype="<f4").tofile(os.path.join(dossier, n))
    ecrire("audio.f32", x)

    taille = config.audio.chunk_size
    seg = torch.from_numpy(x[:, :taille].copy())[None]
    etages = {}
    crochets = [
        modele.band_split.register_forward_hook(lambda m, e, s: etages.update(spectre=e[0][0], decoupe=s[0])),
        modele.final_norm.register_forward_hook(lambda m, e, s: etages.update(final=e[0][0])),
    ]
    for i, bloc in enumerate(modele.layers):
        crochets.append(bloc[-1].register_forward_hook(lambda m, e, s, i=i: etages.update({f"bloc{i}": s})))
    masques = []
    for m in modele.mask_estimators:
        crochets.append(m.register_forward_hook(lambda m, e, s: masques.append(s[0])))
    sortie = modele(seg)[0]
    for c in crochets:
        c.remove()
    ecrire("spectre.f32", etages["spectre"])
    ecrire("decoupe.f32", etages["decoupe"])
    for i in range(len(modele.layers)):
        ecrire(f"bloc{i}.f32", etages[f"bloc{i}"])  # [(trames), bandes, dim]
    ecrire("masques.f32", torch.stack(masques))
    ecrire("segment.f32", sortie)

    config.inference.batch_size = 1
    stems = demix(config, modele, x, torch.device("cpu"), model_type=type_)
    noms = list(stems)
    ecrire("demix.f32", np.stack([stems[s] for s in noms]))
    json.dump({"stems": noms, "segment": int(seg.shape[-1]), "echantillons": int(x.shape[1]),
               "trames": int(etages["spectre"].shape[0]), "dim": int(etages["decoupe"].shape[-1])},
              open(os.path.join(dossier, "infos.json"), "w"))
    print(f"références écrites dans {dossier} : {noms}, {x.shape[1]} échantillons")


if __name__ == "__main__":
    main()
