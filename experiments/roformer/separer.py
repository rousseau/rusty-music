# SPDX-License-Identifier: GPL-3.0-or-later
"""Séparateurs candidats (`docs/module3-roformer.md`) lancés par leur code
d'origine : `ZFTurbo/Music-Source-Separation-Training` (MIT), cloné dans
`$EXT/mss-training`, poids téléchargés à la source dans `$EXT/roformer-poids`.
Banc seulement : rien de ceci ne tourne dans l'application.

    python -I separer.py <modele> <liste.tsv>

Chaque ligne de `liste.tsv` : `entree.wav<TAB>dossier_de_sortie`. Le modèle
n'est chargé qu'une fois ; un dossier déjà rempli est sauté. Les stems sont
écrits `<nom> — <source>.wav`, comme ceux de `rusty-music demix`, pour que
`banc_jeux.py` les lise sans distinction. Une ligne JSON par morceau dans
`<dossier>/mesure.json` : durée du calcul, durée du morceau, mémoire de pointe.
"""
import json, os, resource, sys, time

DEV = os.path.expanduser(os.environ.get("RUSTY_DEV", "/Volumes/LaCieDock/rusty_music-dev"))
EXT = os.path.expanduser(os.environ.get("EXT", os.path.join(DEV, "ext")))
DEPOT = os.path.join(EXT, "mss-training")
POIDS = os.path.join(EXT, "roformer-poids")

# nom → (type de modèle MSST, configuration, poids). Réglages d'inférence
# (découpe, recouvrement, lot) : ceux de la configuration publiée.
MODELES = {
    "bsrofo_sw": ("bs_roformer", "BS-Rofo-SW-Fixed.yaml", "BS-Rofo-SW-Fixed.ckpt"),
    "bsrofo_musdb": ("bs_roformer", "config_bs_roformer_384_8_2_485100.yaml", "model_bs_roformer_ep_17_sdr_9.6568.ckpt"),
    "scnet_xl_ihf": ("scnet", "config_musdb18_scnet_xl_more_wide_v5.yaml", "model_scnet_ep_36_sdr_10.0891.ckpt"),
}


def main():
    nom, liste = sys.argv[1], sys.argv[2]
    sys.path.insert(0, DEPOT)
    import numpy as np, soundfile as sf, torch
    from utils.settings import get_model_from_config
    from utils.model_utils import demix

    type_, cfg, ckpt = MODELES[nom]
    modele, config = get_model_from_config(type_, os.path.join(POIDS, cfg))
    etat = torch.load(os.path.join(POIDS, ckpt), map_location="cpu", weights_only=True)
    for cle in ("state", "state_dict", "model_state_dict"):
        etat = etat.get(cle, etat)
    modele.load_state_dict(etat)
    # Lot plus petit que celui de la configuration (`SEP_LOT=1`) : SCNet XL
    # dépasse la mémoire du Mac avec ses 4 segments par lot.
    if os.environ.get("SEP_LOT"):
        config.inference.batch_size = int(os.environ["SEP_LOT"])
    appareil = torch.device(os.environ.get("APPAREIL", "mps" if torch.backends.mps.is_available() else "cpu"))
    modele = modele.to(appareil).eval()
    print(f"{nom} : {sum(p.numel() for p in modele.parameters()) / 1e6:.1f} M paramètres, {appareil}", flush=True)

    for ligne in open(liste):
        entree, sortie = ligne.rstrip("\n").split("\t")
        if os.path.exists(os.path.join(sortie, "mesure.json")):
            continue
        os.makedirs(sortie, exist_ok=True)
        x, sr = sf.read(entree, dtype="float32", always_2d=True)
        assert sr == 44100, entree
        if appareil.type == "mps":
            torch.mps.synchronize()
        t0 = time.time()
        stems = demix(config, modele, x.T, appareil, model_type=type_)
        if appareil.type == "mps":
            torch.mps.synchronize()
        duree = time.time() - t0
        base = os.path.splitext(os.path.basename(entree))[0]
        for s, y in stems.items():
            sf.write(os.path.join(sortie, f"{base} — {s}.wav"), y.T, 44100, subtype="FLOAT")
        mesure = {"modele": nom, "entree": entree, "calcul_s": duree, "audio_s": len(x) / sr,
                  "rss_max_mo": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 2**20,
                  "mps_pointe_mo": torch.mps.driver_allocated_memory() / 2**20 if appareil.type == "mps" else None}
        json.dump(mesure, open(os.path.join(sortie, "mesure.json"), "w"))
        print(f"{base:44} {duree:6.1f} s pour {mesure['audio_s']:6.1f} s ({mesure['audio_s'] / duree:4.1f} × le temps réel)", flush=True)


if __name__ == "__main__":
    main()
