# SPDX-License-Identifier: GPL-3.0-or-later
"""Banc G2 : super-résolution, Apollo contre AERO.

Étapes (chacune relit ce que la précédente a écrit dans RACINE) :

  preparer  extraits de 30 s : 20 MP3 de la bibliothèque (selection.json),
            et des FLAC encodés en MP3 128 et 192 kbit/s (lameenc), dont la
            source sert de référence
  apollo    Apollo (JusperLee/Apollo, poids CC BY-SA 4.0) sur chaque extrait,
            par torch ; --device mps|cpu ; temps par fichier
  aero      se lance à part : le binaire Rust du banc (`src/main.rs`),
            chemin de l'application
  mesurer   contre la référence : LSD (log-spectral distance) pleine bande
            et 16-22 kHz, SI-SDR après alignement ; sur les MP3 de la
            bibliothèque : énergie au-dessus de 16 kHz ; coût
  ecoute    un dossier à l'aveugle : par extrait, l'original, AERO et
            Apollo, au même niveau RMS, noms neutres ; la clé à part

Python sert au banc seulement (CLAUDE.md : vérifier, pas exécuter).
"""

import json
import math
import random
import sys
import time
from pathlib import Path

import numpy as np
import soundfile as sf

RACINE = Path(sys.argv[2] if len(sys.argv) > 2 else "/Volumes/LaCieDock/rusty_music-dev/banc-superres")
SR = 44_100
DUREE = 30.0
EXTRAITS = RACINE / "extraits"
SORTIES = RACINE / "sorties"


def lire(chemin):
    x, sr = sf.read(str(chemin), dtype="float32", always_2d=True)
    assert sr == SR, (chemin, sr)
    return x.T  # canaux × échantillons


def ecrire(chemin, x):
    chemin.parent.mkdir(parents=True, exist_ok=True)
    sf.write(str(chemin), np.ascontiguousarray(x.T), SR, subtype="PCM_16")


def extrait(x):
    """30 s à partir du quart du morceau (ni l'intro ni la fin)."""
    n = x.shape[1]
    debut = min(max(0, n // 4), max(0, n - int(DUREE * SR)))
    return x[:, debut : debut + int(DUREE * SR)]


def mp3(x, debit):
    """Encode en MP3 (LAME, CBR `debit` kbit/s), puis relit."""
    import lameenc

    enc = lameenc.Encoder()
    enc.set_bit_rate(debit)
    enc.set_in_sample_rate(SR)
    enc.set_channels(2)
    enc.set_quality(2)
    pcm = (np.clip(x.T, -1, 1) * 32767).astype("<i2").tobytes()
    octets = enc.encode(pcm) + enc.flush()
    # Décodé en mémoire : sur le LaCie (exFAT), libsndfile refuse les MP3
    # (« bad data offset »), alors qu'il les lit sur l'APFS.
    import io

    y, sr = sf.read(io.BytesIO(octets), dtype="float32", always_2d=True)
    assert sr == SR
    y = y.T
    return y


def aligner(ref, y, max_dec=4096):
    """Décale `y` pour qu'il tombe sur `ref` (délai du codeur MP3)."""
    a = ref.mean(0)[: SR * 10]
    b = y.mean(0)[: SR * 10 + max_dec]
    n = 1 << (len(a) + len(b)).bit_length()
    c = np.fft.irfft(np.fft.rfft(b, n) * np.conj(np.fft.rfft(a, n)), n)
    lags = np.concatenate([c[: max_dec + 1], c[-max_dec:]])
    idx = int(np.argmax(lags))
    d = idx if idx <= max_dec else idx - len(lags)
    if d >= 0:
        y = y[:, d:]
    else:
        y = np.pad(y, ((0, 0), (-d, 0)))
    m = min(ref.shape[1], y.shape[1])
    return ref[:, :m], y[:, :m], d


def spectre_db(x, nfft=2048, hop=512):
    """Spectre de puissance en dB, mono, trames × raies."""
    m = x.mean(0)
    fen = np.hanning(nfft).astype(np.float32)
    trames = 1 + (len(m) - nfft) // hop
    idx = np.arange(nfft)[None, :] + hop * np.arange(trames)[:, None]
    S = np.abs(np.fft.rfft(m[idx] * fen, axis=1)) ** 2
    return 10 * np.log10(S + 1e-10)


def lsd(ref, y, fmin=0.0, fmax=SR / 2):
    """Log-spectral distance (dB), moyenne des trames, entre fmin et fmax."""
    R, Y = spectre_db(ref), spectre_db(y)
    f = np.fft.rfftfreq(2048, 1 / SR)
    b = (f >= fmin) & (f < fmax)
    return float(np.mean(np.sqrt(np.mean((R[:, b] - Y[:, b]) ** 2, axis=1))))


def si_sdr(ref, y):
    r, e = ref.reshape(-1).astype(np.float64), y.reshape(-1).astype(np.float64)
    a = np.dot(e, r) / (np.dot(r, r) + 1e-12)
    s = a * r
    return float(10 * np.log10(np.dot(s, s) / (np.dot(e - s, e - s) + 1e-12)))


def energie_haute(x, fc=16_000):
    """Part de l'énergie au-dessus de fc, en dB sous l'énergie totale."""
    S = 10 ** (spectre_db(x) / 10)
    f = np.fft.rfftfreq(2048, 1 / SR)
    return float(10 * np.log10(S[:, f >= fc].sum() / S.sum() + 1e-12))


def coupure(x):
    """Fréquence au-dessus de laquelle il reste moins de −60 dB du pic du
    spectre moyen (la bande passante du codeur)."""
    S = np.mean(10 ** (spectre_db(x) / 10), axis=0)
    db = 10 * np.log10(S / S.max() + 1e-20)
    f = np.fft.rfftfreq(2048, 1 / SR)
    au_dessus = np.where(db > -60)[0]
    return float(f[au_dessus.max()]) if len(au_dessus) else 0.0


def lire_extrait(chemin):
    """Lit seulement les 30 s utiles (libsndfile échoue parfois au-delà
    d'une trame MP3 abîmée : on lit alors par blocs jusqu'à l'erreur)."""
    with sf.SoundFile(str(chemin)) as f:
        assert f.samplerate == SR, (chemin, f.samplerate)
        n, m = f.frames, int(DUREE * SR)
        debut = min(max(0, n // 4), max(0, n - m))
        try:
            f.seek(debut)
            x = f.read(m, dtype="float32", always_2d=True)
        except Exception:
            f.seek(0)
            blocs = []
            try:
                while sum(len(b) for b in blocs) < debut + m:
                    b = f.read(SR, dtype="float32", always_2d=True)
                    if not len(b):
                        break
                    blocs.append(b)
            except Exception:
                pass
            x = np.concatenate(blocs)
            x = x[max(0, min(debut, len(x) - m)) :][:m]
    x = x.T
    if x.shape[0] == 1:
        x = np.repeat(x, 2, axis=0)
    return x[:2]


def preparer():
    sel = json.load(open(RACINE / "selection.json"))
    meta = []
    for i, t in enumerate(sel):
        x = lire_extrait(t["path"])
        nom = f"bib{i + 1:02d}"
        ecrire(EXTRAITS / f"{nom}.wav", x)
        meta.append({"nom": nom, "genre": t["genre"], "debit": t["bitrate"], "ref": None})
    # Des FLAC de la bibliothèque, encodés ici : la source sert de référence.
    import sqlite3

    base = sqlite3.connect(sys.argv[3]) if len(sys.argv) > 3 else None
    flacs = [r[0] for r in base.execute("select path from tracks where codec='FLAC' order by path")] if base else []
    random.seed(20261110)
    flacs = random.sample(flacs, min(6, len(flacs)))
    for i, p in enumerate(flacs):
        x = lire_extrait(p)
        nom = f"ref{i + 1:02d}"
        ecrire(EXTRAITS / f"{nom}-source.wav", x)
        for d in (128, 192):
            y = mp3(x, d)
            ecrire(EXTRAITS / f"{nom}-mp3{d}.wav", y)
            meta.append({"nom": f"{nom}-mp3{d}", "genre": "FLAC encodé", "debit": d, "ref": f"{nom}-source.wav"})
    json.dump(meta, open(RACINE / "extraits.json", "w"), ensure_ascii=False, indent=1)
    print(f"{len(meta)} extraits")


def apollo(device):
    import torch

    sys.path.insert(0, str(RACINE / "apollo-src"))
    import look2hear.models
    from huggingface_hub import hf_hub_download

    poids = hf_hub_download(repo_id="JusperLee/Apollo", filename="pytorch_model.bin", cache_dir=str(RACINE / "hf"))
    dev = torch.device(device)
    t = time.perf_counter()
    model = look2hear.models.BaseModel.from_pretrain(poids, sr=SR, win=20, feature_dim=256, layer=6).to(dev).eval()
    print(f"Apollo chargé en {time.perf_counter() - t:.1f} s sur {device}", flush=True)
    meta = json.load(open(RACINE / "extraits.json"))
    temps = {}
    seuls = sys.argv[4].split(",") if len(sys.argv) > 4 else None
    for m in meta:
        if seuls and m["nom"] not in seuls:
            continue
        x = torch.from_numpy(lire(EXTRAITS / f"{m['nom']}.wav")).unsqueeze(0)
        with torch.inference_mode():
            if device == "mps":
                torch.mps.synchronize()
            t = time.perf_counter()
            # Par morceaux de 10 s, chevauchement 1 s (mémoire bornée).
            y = morceaux(model, x, dev)
            if device == "mps":
                torch.mps.synchronize()
            dt = time.perf_counter() - t
        temps[m["nom"]] = dt
        ecrire(SORTIES / f"{m['nom']}-apollo.wav", np.clip(y[0].numpy(), -1, 1))
        print(f"{m['nom']}  {dt:.1f} s", flush=True)
    json.dump(temps, open(RACINE / f"temps-apollo-{device}.json", "w"), indent=1)


def morceaux(model, x, dev, seg=10.0, chev=1.0):
    n = x.shape[-1]
    L, H = int(seg * SR), int((seg - chev) * SR)
    out = np.zeros(x.shape[1:], dtype=np.float32)
    poids = np.zeros(n, dtype=np.float32)
    debut = 0
    while True:
        bout = x[..., debut : debut + L]
        y = model(bout.to(dev)).detach().to("cpu")[0].numpy()[:, : bout.shape[-1]]
        w = np.ones(y.shape[-1], dtype=np.float32)
        f = min(int(chev * SR), y.shape[-1])
        if debut > 0:
            w[:f] = np.linspace(0, 1, f)
        if debut + L < n:
            w[-f:] = np.linspace(1, 0, f)
        out[:, debut : debut + y.shape[-1]] += y * w
        poids[debut : debut + y.shape[-1]] += w
        if debut + L >= n:
            break
        debut += H
    return (out / np.maximum(poids, 1e-6))[None]


def mesurer():
    meta = json.load(open(RACINE / "extraits.json"))
    lignes = []
    for m in meta:
        x = lire(EXTRAITS / f"{m['nom']}.wav")
        sorties = {"mp3": x}
        for v in ("aero", "apollo"):
            p = SORTIES / f"{m['nom']}-{v}.wav"
            if p.exists():
                sorties[v] = lire(p)
        ligne = {"nom": m["nom"], "genre": m["genre"], "debit": m["debit"]}
        for v, y in sorties.items():
            ligne[f"coupure_{v}"] = coupure(y)
            ligne[f"haut_{v}"] = energie_haute(y)
        if m["ref"]:
            ref = lire(EXTRAITS / m["ref"])
            ligne["haut_ref"] = energie_haute(ref)
            for v, y in sorties.items():
                r, yy, d = aligner(ref, y)
                ligne[f"lsd_{v}"] = lsd(r, yy)
                ligne[f"lsd16_{v}"] = lsd(r, yy, 16_000)
                ligne[f"lsdbas_{v}"] = lsd(r, yy, 0, 16_000)
                ligne[f"sisdr_{v}"] = si_sdr(r, yy)
                ligne[f"decalage_{v}"] = d
        lignes.append(ligne)
        print(json.dumps(ligne, ensure_ascii=False), flush=True)
    json.dump(lignes, open(RACINE / "mesures.json", "w"), ensure_ascii=False, indent=1)


def rms(x):
    return float(np.sqrt(np.mean(x.astype(np.float64) ** 2)) + 1e-12)


def ecoute(n=10):
    """Un dossier à l'aveugle : n extraits de la bibliothèque, trois versions
    chacun, au même RMS, lettres tirées au hasard ; la clé dans un fichier à
    part."""
    meta = [m for m in json.load(open(RACINE / "extraits.json")) if not m["ref"]]
    random.seed(1110)
    choisis = random.sample(meta, min(n, len(meta)))
    dossier = RACINE / "ecoute"
    dossier.mkdir(exist_ok=True)
    cle = {}
    for i, m in enumerate(choisis):
        versions = {"original": lire(EXTRAITS / f"{m['nom']}.wav")}
        for v in ("aero", "apollo"):  # celles qui existent
            if (SORTIES / f"{m['nom']}-{v}.wav").exists():
                versions[v] = lire(SORTIES / f"{m['nom']}-{v}.wav")
        cible = rms(versions["original"])
        lettres = ["A", "B", "C"][: len(versions)]
        random.shuffle(lettres)
        for (v, x), l in zip(versions.items(), lettres):
            y = x * (cible / rms(x))
            y = y / max(1.0, float(np.abs(y).max()) / 0.98)
            ecrire(dossier / f"{i + 1:02d}-{l}.wav", y)
            cle[f"{i + 1:02d}-{l}"] = {"version": v, "extrait": m["nom"], "genre": m["genre"], "debit": m["debit"]}
    (RACINE / "ecoute-cle.json").write_text(json.dumps(cle, ensure_ascii=False, indent=1))
    print(f"{len(choisis)} extraits dans {dossier}")


if __name__ == "__main__":
    etape = sys.argv[1]
    if etape == "preparer":
        preparer()
    elif etape == "apollo":
        apollo(sys.argv[3] if len(sys.argv) > 3 else "mps")
    elif etape == "mesurer":
        mesurer()
    elif etape == "ecoute":
        ecoute()
