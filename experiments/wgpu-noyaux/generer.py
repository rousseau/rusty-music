#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Fabrique un modèle ONNX par opérateur suspect, et sa sortie de référence.

Les suspects sont les opérateurs que HTDemucs emploie et que CLAP n'employait
pas — CLAP passant sur wgpu au cosinus 1,0000000000, le défaut est
nécessairement dans ce qui les sépare.

Chaque modèle ne contient qu'un nœud. L'entrée est calculée par une formule
que le Rust reproduit à l'identique, pour n'avoir aucun fichier d'entrée à
transporter. La référence vient d'ONNX Runtime : on veut savoir non seulement
que les deux backends divergent, mais lequel a tort.

    python3 generer.py <dossier de sortie>
"""

import json
import sys
from pathlib import Path

import numpy as np
import onnx
from onnx import TensorProto, helper
import onnxruntime as ort


def entree(taille):
    """Doit rester rigoureusement identique à `entree()` de src/main.rs."""
    i = np.arange(taille, dtype=np.float32)
    return (np.sin(i * np.float32(0.017)) * np.float32(0.8)).astype(np.float32)


def poids(taille, decalage):
    """Paramètres appris, eux aussi calculés plutôt que tirés au sort."""
    i = np.arange(taille, dtype=np.float32)
    return (np.cos(i * np.float32(0.031) + np.float32(decalage)) * np.float32(0.5)
            + np.float32(1.0)).astype(np.float32)


CAS = {}


def cas(nom, forme, noeud, initialiseurs=(), sortie_forme=None):
    CAS[nom] = dict(forme=forme, noeud=noeud, init=list(initialiseurs),
                    sortie=sortie_forme)


# Formes proches de celles de HTDemucs : canaux nombreux, plans temps-fréquence.
cas("instancenormalization", [1, 32, 32, 64],
    helper.make_node("InstanceNormalization", ["x", "scale", "b"], ["y"], epsilon=1e-5),
    [("scale", [32], 0.0), ("b", [32], 1.7)])

cas("layernormalization", [1, 64, 128],
    helper.make_node("LayerNormalization", ["x", "scale", "b"], ["y"], axis=-1, epsilon=1e-5),
    [("scale", [128], 0.0), ("b", [128], 1.7)])

cas("convtranspose", [1, 16, 16, 32],
    helper.make_node("ConvTranspose", ["x", "w"], ["y"], kernel_shape=[3, 3],
                     strides=[2, 2], pads=[1, 1, 1, 1], output_padding=[1, 1]),
    [("w", [16, 8, 3, 3], 0.4)])

# `num_outputs` n'existe qu'à partir de l'opset 18 ; en 17 la découpe se
# déclare par un second entrant.
cas("split", [1, 64, 128],
    helper.make_node("Split", ["x", "parts"], ["y", "y2"], axis=1),
    [("parts", None, None)])

cas("sigmoid", [1, 32, 32, 64], helper.make_node("Sigmoid", ["x"], ["y"]))

# Les deux oubliés du premier tour : la liste de suspects avait été tirée du
# graphe *non replié*, où ils se noyaient dans des centaines d'occurrences de
# calcul de formes. Après repliage, ce sont de vrais opérateurs de données.
cas("gather", [1, 64, 128],
    helper.make_node("Gather", ["x", "idx"], ["y"], axis=1), [("idx", None, None)])
cas("unsqueeze", [1, 64, 128],
    helper.make_node("Unsqueeze", ["x", "axes"], ["y"]), [("axes", None, None)])

# Les mêmes opérateurs à l'échelle de HTDemucs. Un noyau GPU peut être juste
# sur un petit tenseur et faux sur un grand : le découpage en tuiles change,
# et c'est là que vivent les erreurs de bord.
cas("instancenormalization_grand", [1, 48, 512, 336],
    helper.make_node("InstanceNormalization", ["x", "scale", "b"], ["y"], epsilon=1e-5),
    [("scale", [48], 0.0), ("b", [48], 1.7)])

cas("convtranspose_grand", [1, 48, 256, 168],
    helper.make_node("ConvTranspose", ["x", "w"], ["y"], kernel_shape=[8, 1],
                     strides=[4, 1], pads=[2, 0, 2, 0]),
    [("w", [48, 24, 8, 1], 0.4)])

cas("layernormalization_grand", [1, 336, 512],
    helper.make_node("LayerNormalization", ["x", "scale", "b"], ["y"], axis=-1, epsilon=1e-5),
    [("scale", [512], 0.0), ("b", [512], 1.7)])
cas("clip", [1, 32, 32, 64],
    helper.make_node("Clip", ["x", "lo", "hi"], ["y"]),
    [("lo", [], -0.3), ("hi", [], 0.45)])
cas("tile", [1, 8, 16],
    helper.make_node("Tile", ["x", "reps"], ["y"]), [("reps", None, None)])
cas("sin", [1, 64, 128], helper.make_node("Sin", ["x"], ["y"]))
cas("cos", [1, 64, 128], helper.make_node("Cos", ["x"], ["y"]))


def construire(nom, spec, dossier):
    forme = spec["forme"]
    inits = []
    for n, f, d in spec["init"]:
        if n == "reps":
            inits.append(helper.make_tensor("reps", TensorProto.INT64, [3], [1, 2, 3]))
        elif n == "idx":
            inits.append(helper.make_tensor("idx", TensorProto.INT64, [5], [0, 17, 33, 48, 63]))
        elif n == "axes":
            inits.append(helper.make_tensor("axes", TensorProto.INT64, [1], [2]))
        elif n == "parts":
            inits.append(helper.make_tensor("parts", TensorProto.INT64, [2], [32, 32]))
        elif f == []:
            inits.append(helper.make_tensor(n, TensorProto.FLOAT, [], [d]))
        else:
            v = poids(int(np.prod(f)), d).reshape(f)
            inits.append(helper.make_tensor(n, TensorProto.FLOAT, f, v.flatten()))

    # Formes de sortie laissées à l'inférence : les déclarer à la main
    # obligerait à réimplémenter la sémantique de chaque opérateur.
    sorties = [helper.make_empty_tensor_value_info(s) for s in spec["noeud"].output]
    graphe = helper.make_graph(
        [spec["noeud"]], nom,
        [helper.make_tensor_value_info("x", TensorProto.FLOAT, forme)],
        sorties, initializer=inits)
    modele = helper.make_model(graphe, opset_imports=[helper.make_operatorsetid("", 17)])
    modele.ir_version = 10
    modele = onnx.shape_inference.infer_shapes(modele)
    onnx.checker.check_model(modele)

    chemin = dossier / f"{nom}.onnx"
    onnx.save(modele, chemin)

    x = entree(int(np.prod(forme))).reshape(forme)
    s = ort.InferenceSession(str(chemin), providers=["CPUExecutionProvider"])
    y = s.run(None, {"x": x})[0]
    return dict(forme=forme, sortie=list(y.shape),
                somme=float(y.astype(np.float64).sum()),
                norme=float(np.linalg.norm(y.astype(np.float64))),
                premiers=[float(v) for v in y.flatten()[:4]])


def main():
    dossier = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
    dossier.mkdir(parents=True, exist_ok=True)
    reference = {}
    for nom, spec in CAS.items():
        reference[nom] = construire(nom, spec, dossier)
        print(f"  {nom:<24} entrée {spec['forme']} → sortie {reference[nom]['sortie']}")
    (dossier / "reference.json").write_text(json.dumps(reference, indent=2))

    # Le Rust inclut ce fichier tel quel : pas de dépendance JSON côté Rust
    # pour neuf triplets de nombres.
    lignes = [
        "// Généré par generer.py — ne pas modifier à la main.",
        "// Sommes et normes relevées sous ONNX Runtime.",
        "pub const REFERENCE: &[(&str, f64, f64)] = &[",
    ]
    for nom in sorted(reference):
        r = reference[nom]
        lignes.append(f'    ("{nom}", {r["somme"]:.9e}, {r["norme"]:.9e}),')
    lignes.append("];")
    (dossier / "reference.rs").write_text("\n".join(lignes) + "\n")
    print(f"\n{len(CAS)} modèles, leur référence et reference.rs dans {dossier}")


if __name__ == "__main__":
    main()
