# SPDX-License-Identifier: GPL-3.0-or-later
"""Exporte le modèle de batterie ADTOF « Frame_RNN » en ONNX, et des
références de parité pour le portage Rust (`crates/transcription::batterie`).

Préparation seulement (politique du projet : Python pour préparer et
vérifier, Rust à l'exécution). Le code d'ADTOF (CC BY-NC-SA 4.0) n'est ni
copié ni lié : l'architecture est réécrite ici d'après `adtof/model/model.py`
(`_getCRNNFunctional`, hyperparamètres « Frame_RNN »), et seuls les poids
publiés sont chargés — ils restent sous leur licence d'origine.

    python exporter_adtof.py <checkpoint sans extension> <sortie.onnx> [audio_essai.wav dossier_reference]
"""
import json, sys
import numpy as np
import tensorflow as tf
import madmom
from madmom.audio.signal import SignalProcessor, FramedSignalProcessor
from madmom.audio.stft import ShortTimeFourierTransformProcessor
from madmom.audio.spectrogram import LogarithmicFilteredSpectrogramProcessor
from madmom.audio.filters import LogarithmicFilterbank
from madmom.processors import SequentialProcessor

# Hyperparamètres « Frame_RNN » (adtof/model/hyperparameters.py).
CONV = [32, 64]
GRU = [60, 60, 60]
CLASSES = ["BD", "SD", "TT", "HH", "CY+RD"]
SEUILS = [0.22, 0.24, 0.32, 0.22, 0.30]
FPS, TRAME, SR, BANDES, FMIN, FMAX = 100, 2048, 44100, 12, 20, 20000

def n_bins():
    fft = madmom.audio.stft.fft_frequencies(TRAME // 2, SR)
    cibles = madmom.audio.filters.log_frequencies(BANDES, FMIN, FMAX)
    bins = madmom.audio.filters.frequencies2bins(cibles, fft, unique_bins=True)
    return len(madmom.audio.filters.TriangularFilter.filters(bins, norm=True, overlap=True))

def modele(nb):
    x = tf.keras.Input(shape=(None, nb, 1), name="x")
    couches = []
    for i, f in enumerate(CONV):
        couches += [
            tf.keras.layers.Conv2D(f, (3, 3), activation="relu", padding="same", name="conv1" + str(i)),
            tf.keras.layers.BatchNormalization(),
            tf.keras.layers.Conv2D(f, (3, 3), activation="relu", padding="same", name="conv2" + str(i)),
            tf.keras.layers.BatchNormalization(),
            tf.keras.layers.MaxPool2D(pool_size=(1, 3), strides=(1, 3), padding="same"),
            tf.keras.layers.Dropout(0.3),
        ]
    cnn = tf.keras.models.Sequential(couches)
    y = cnn(x)
    y = tf.keras.layers.Reshape((-1, y.shape[2] * y.shape[3]))(y)
    for u in GRU:
        y = tf.keras.layers.Bidirectional(tf.keras.layers.GRU(u, stateful=False, return_sequences=True))(y)
    y = tf.keras.layers.Dense(len(CLASSES), activation="sigmoid", name="denseOutput")(y)
    return tf.keras.Model(x, y)

def caracteristiques(chemin):
    # openMadmom d'ADTOF : mono 44,1 kHz, trames de 2048 à 100 fps, STFT,
    # spectrogramme filtré logarithmique (12 bandes/octave, 20 Hz–20 kHz).
    proc = SequentialProcessor((
        SignalProcessor(num_channels=1, sample_rate=SR),
        FramedSignalProcessor(frame_size=TRAME, fps=FPS),
        ShortTimeFourierTransformProcessor(),
        LogarithmicFilteredSpectrogramProcessor(num_channels=1, sample_rate=SR, filterbank=LogarithmicFilterbank,
            frame_size=TRAME, fps=FPS, num_bands=BANDES, fmin=FMIN, fmax=FMAX, norm_filters=True),
    ))
    return np.array(proc(chemin), dtype=np.float32)

if __name__ == "__main__":
    ckpt, sortie = sys.argv[1], sys.argv[2]
    nb = n_bins()
    m = modele(nb)
    statut = m.load_weights(ckpt)
    statut.assert_existing_objects_matched()
    print(f"{nb} bandes, {m.count_params()} paramètres, poids chargés")
    import tf2onnx
    sig = (tf.TensorSpec((1, None, nb, 1), tf.float32, name="x"),)
    tf2onnx.convert.from_keras(m, input_signature=sig, opset=17, output_path=sortie)
    print("ONNX écrit :", sortie)
    if len(sys.argv) > 4:
        audio, dossier = sys.argv[3], sys.argv[4]
        S = caracteristiques(audio)
        P = m.predict(S[None, :, :, None], verbose=0)[0]
        coups = {}
        for k, (c, s) in enumerate(zip(CLASSES, SEUILS)):
            pp = madmom.features.notes.NotePeakPickingProcessor(threshold=s, smooth=0, pre_avg=0.1, post_avg=0.01,
                pre_max=0.02, post_max=0.01, combine=0.02, fps=FPS)
            coups[c] = [float(r[0]) for r in pp(P[:, k:k + 1])]
        S.astype("<f4").tofile(f"{dossier}/caracteristiques.f32")
        P.astype("<f4").tofile(f"{dossier}/predictions.f32")
        json.dump({"trames": int(S.shape[0]), "bandes": int(S.shape[1]), "coups": coups}, open(f"{dossier}/reference.json", "w"))
        print(f"référence : {S.shape[0]} trames, " + ", ".join(f"{c} {len(v)}" for c, v in coups.items()))
