#!/usr/bin/env python
# SPDX-License-Identifier: GPL-3.0-or-later
"""Sonde : la tour texte de CLAP sait-elle nommer nos familles ?

Trois questions, dans cet ordre — chacune ne vaut d'être posée que si la
précédente a répondu oui :

  1. `espace`   — l'espace commun tient-il avec NOS empreintes ? On classe les
                  27 000 morceaux contre quelques phrases et on regarde le haut
                  du classement. Si CLAP ne sait pas trouver une batterie ou une
                  voix féminine dans notre base, tout le reste est inutile ;
  2. `familles` — les douze familles reçoivent-elles un nom meilleur que celui
                  des genres ? Même score que `nommer_les_familles` en Rust :
                  part × log2(sur-représentation) ;
  3. `export`   — la tour texte s'exporte-t-elle en ONNX aux formes figées, sans
                  marge calculée à l'exécution ? C'est là que la tour audio
                  avait buté.

Rien ici n'entre dans le projet : l'essai doit pouvoir échouer.
"""

import argparse
import collections
import math
import sqlite3
import struct
import sys
from pathlib import Path

import numpy as np

MODELE = "laion/clap-htsat-unfused"
BASE = Path.home() / "Library/Application Support/fm.rustymusic.desktop/rusty-music.db"
EMPREINTE = "clap-htsat-unfused-5f"

# Le vocabulaire d'AudioMuse-AI, à la lettre : les 50 tags Last.fm les plus
# fréquents, qui sont ceux de MusiCNN. C'est la comparaison honnête — si CLAP
# doit remplacer leur auto-étiquetage, qu'il le fasse sur le même vocabulaire.
TAGS = [
    "rock", "pop", "alternative", "indie", "electronic", "female vocalists",
    "dance", "00s", "alternative rock", "jazz", "beautiful", "metal",
    "chillout", "male vocalists", "classic rock", "soul", "indie rock", "Mellow",
    "electronica", "80s", "folk", "90s", "chill", "instrumental", "punk",
    "oldies", "blues", "hard rock", "ambient", "acoustic", "experimental",
    "female vocalist", "guitar", "Hip-Hop", "70s", "party", "country",
    "easy listening", "sexy", "catchy", "funk", "electro", "heavy metal",
    "Progressive rock", "60s", "rnb", "indie pop", "sad", "House", "happy",
]

# Ce que les genres ne diront jamais, et qui décrit nos familles. Écrit après
# avoir lu les douze noms actuels : « Folk · Children's » pour Regina Spektor,
# Nina Simone et Agnes Obel.
DESCRIPTIONS = [
    "a female singer with a piano",
    "a male singer with an acoustic guitar",
    "a solo instrumental piano piece",
    "a distorted electric guitar riff",
    "a drum machine and a synthesizer",
    "traditional celtic music with fiddle and flute",
    "a spoken voice telling a story, no music",
    "a reggae offbeat guitar and bass",
    "a rapper over a boom bap beat",
    "a jazz double bass and brushed drums",
    "a symphony orchestra",
    "a slow downtempo track with a trip hop beat",
    "a live rock concert with a crowd",
    "a children's song",
    "a saxophone solo",
    "an accordion",
]

# Le même, élargi : c'est le vocabulaire qui décide, pas le modèle — le sondage
# aux 50 tags Last.fm l'a montré. Écrit pour couvrir ce que nos douze familles
# contiennent réellement, une entrée par chose qu'on saurait reconnaître à
# l'oreille en trois secondes.
LARGE = DESCRIPTIONS + [
    "a heavy metal band with screamed vocals",
    "a punk rock band playing fast",
    "a funk band with a slap bass",
    "a brass band",
    "a string quartet",
    "a solo classical guitar",
    "an electronic dance track with a four on the floor kick",
    "a drum and bass breakbeat",
    "a dub track with heavy reverb and echo",
    "a soul singer with a gospel choir",
    "a blues guitar with a harmonica",
    "a country song with a pedal steel guitar",
    "an ambient drone with no rhythm",
    "a field recording of birds and water",
    "a bagpipe",
    "a banjo and a fiddle playing a reel",
    "an african percussion ensemble",
    "a latin band with congas and trumpets",
    "a solo cello",
    "a harpsichord",
    "a female choir singing in harmony",
    "a man rapping in french",
    "a woman singing in french",
    "a chiptune with square waves",
    "a jazz big band with a swing rhythm",
    "a psychedelic rock song with an organ",
    "a lullaby sung softly",
    "a marching drum line",
    "applause and cheering",
    "a distorted bass and industrial noise",
    "a sitar and tabla",
    "a flamenco guitar with handclaps",
    "a slow ballad with strings",
    "an upbeat pop song with a catchy chorus",
]

# Les entrées que le sondage a réclamées, et c'est là l'enseignement : une
# famille sans entrée qui lui corresponde ne reste pas sans nom, elle en reçoit
# un faux. Le ska français est parti sous « an african percussion ensemble »,
# le chant breton sous « a man rapping in french » — dans les deux cas
# l'affinité la plus inhabituelle d'un morceau que rien ne décrivait.
COUVRANT = LARGE + [
    "a ska band with a horn section and offbeat guitar",
    "a french chanson with an accordion",
    "a man singing in french over a rock band",
    "traditional singing without instruments",
    "a celtic song sung in a breton or gaelic language",
    "a film score with strings and brass",
    "a solo acoustic guitar instrumental",
    "a live band recorded in a small room",
    "a spoken introduction before the music starts",
    "a woman singing a jazz standard",
    "an organ and a hammond in a groove band",
    "a turntablist scratching records",
]

PROMPT = "This is a sound of "


def charger_base(limite=0):
    """Les empreintes, avec de quoi les reconnaître. Lecture seule : l'appli
    peut tourner en même temps."""
    con = sqlite3.connect(f"file:{BASE}?mode=ro", uri=True)
    req = """
        select f.track_id, f.cluster, f.vector, t.artist, t.title
        from features f join tracks t on t.id = f.track_id
        where f.model = ? and f.cluster is not null
    """
    if limite:
        req += f" limit {limite}"
    lignes = con.execute(req, (EMPREINTE,)).fetchall()
    con.close()

    n = len(lignes)
    vecteurs = np.empty((n, 512), dtype=np.float32)
    familles = np.empty(n, dtype=np.int32)
    noms = []
    for i, (_, cluster, blob, artiste, titre) in enumerate(lignes):
        vecteurs[i] = np.frombuffer(blob, dtype="<f4", count=512)
        familles[i] = cluster
        noms.append(f"{artiste or '?'} — {titre or '?'}")
    # Elles sortent normalisées de `projection::normaliser`, mais on ne le
    # suppose pas : un cosinus calculé sur des vecteurs non unitaires ne serait
    # plus un cosinus, et l'erreur serait silencieuse.
    normes = np.linalg.norm(vecteurs, axis=1)
    print(f"{n} empreintes · norme {normes.min():.4f} à {normes.max():.4f}", file=sys.stderr)
    vecteurs /= normes[:, None]
    return vecteurs, familles, noms


def tenseur_de(sortie):
    """`get_text_features` rend un tenseur ou un objet de sortie selon la
    version de transformers. On ne fige pas la version d'un essai jetable."""
    import torch

    if isinstance(sortie, torch.Tensor):
        return sortie
    for champ in ("text_embeds", "pooler_output", "last_hidden_state"):
        v = getattr(sortie, champ, None)
        if v is not None:
            return v
    raise TypeError(f"sortie texte inattendue : {type(sortie)}")


def tour_texte():
    """Le modèle texte + sa projection, tel que `get_text_features` l'emploie."""
    import torch
    from transformers import AutoTokenizer, ClapModel

    tok = AutoTokenizer.from_pretrained(MODELE)
    modele = ClapModel.from_pretrained(MODELE).eval()

    def encoder(phrases, prompt=True):
        textes = [PROMPT + p if prompt else p for p in phrases]
        lots = tok(textes, padding=True, return_tensors="pt")
        with torch.no_grad():
            v = tenseur_de(modele.get_text_features(**lots))
        v = v / torch.linalg.norm(v, dim=-1, keepdim=True)
        return v.numpy().astype(np.float32)

    return encoder, tok, modele


def cmd_espace(args):
    vecteurs, _, noms = charger_base(args.limite)
    encoder, _, _ = tour_texte()
    phrases = VOCABULAIRES[args.vocabulaire]
    T = encoder(phrases, prompt=args.prompt)
    scores = vecteurs @ T.T          # (morceaux, phrases)
    for j, phrase in enumerate(phrases):
        haut = np.argsort(-scores[:, j])[:args.top]
        print(f"\n« {phrase} »")
        for i in haut:
            print(f"   {scores[i, j]:+.3f}  {noms[i]}")


def score_familles(etiquettes, familles):
    """part × log2(sur-représentation) — le score de `nommer_les_familles`.

    Repris tel quel pour que la comparaison porte sur la *source* des
    étiquettes et non sur la manière de les compter.
    """
    global_ = collections.Counter(etiquettes)
    total = len(etiquettes)
    par_famille = collections.defaultdict(collections.Counter)
    for e, f in zip(etiquettes, familles):
        par_famille[int(f)][e] += 1

    sortie = {}
    for fam, comptes in par_famille.items():
        dedans = sum(comptes.values())
        plancher = max(5, round(dedans * 0.01))
        classe = []
        for tag, n in comptes.items():
            if n < plancher:
                continue
            part = n / dedans
            ailleurs = global_[tag] / total
            s = part * math.log2(part / ailleurs)
            if s > 0:
                classe.append((s, tag, n))
        classe.sort(reverse=True)
        sortie[fam] = (dedans, classe)
    return sortie


VOCABULAIRES = {"tags": TAGS, "descriptions": DESCRIPTIONS, "large": LARGE, "couvrant": COUVRANT}


def calibrer(scores, mode):
    """Rendre les colonnes comparables entre elles.

    **Un cosinus CLAP ne se compare pas d'une phrase à l'autre.** « a children's
    song » sort à +0,73 sur son meilleur morceau, « a reggae offbeat guitar and
    bass » à +0,52 sur le sien : la première l'emporte partout, y compris sur du
    reggae. L'argmax brut ne classe donc pas les morceaux, il classe les
    phrases — et toujours dans le même ordre.

    Ce qui a du sens, c'est le rang d'un morceau **dans** une phrase. Centrer
    puis réduire chaque colonne le rétablit sans rien supposer du vocabulaire.
    """
    if mode == "brut":
        return scores
    centre = scores - scores.mean(axis=0, keepdims=True)
    if mode == "centre":
        return centre
    return centre / (scores.std(axis=0, keepdims=True) + 1e-9)


def artistes_de(noms, indices, k=5):
    c = collections.Counter(noms[i].split(" — ")[0] for i in indices)
    return ", ".join(a for a, _ in c.most_common(k))


def par_centroide(vecteurs, familles, T, mots, k=2):
    """Nommer la famille par son centre, pas par un vote de ses morceaux.

    L'autre voie compte les étiquettes gagnées morceau par morceau, comme
    `nommer_les_familles` compte les genres. Mais un genre est posé par un
    humain, une étiquette CLAP est un argmax sur un vocabulaire : le vote
    hérite du bruit de chaque morceau, et le score de sur-représentation
    l'amplifie — une phrase rare qui gagne cent fois par accident nomme une
    famille de quatre mille.

    Le centre, lui, moyenne le bruit avant de décider. Le centrage reste
    nécessaire, mais il se fait sur douze familles au lieu de 27 000 morceaux :
    on retire à chaque phrase ce qu'elle dit de toutes les familles, il ne
    reste que ce qu'elle dit de celle-ci.
    """
    fams = sorted(set(int(f) for f in familles))
    centres = np.vstack([
        vecteurs[familles == f].mean(axis=0) for f in fams
    ])
    centres /= np.linalg.norm(centres, axis=1, keepdims=True)
    s = centres @ T.T
    s = s - s.mean(axis=0, keepdims=True)
    sortie = {}
    for i, f in enumerate(fams):
        haut = np.argsort(-s[i])[:k]
        sortie[f] = ([mots[j] for j in haut], s[i][haut], int((familles == f).sum()))
    return sortie


def cmd_centroide(args):
    vecteurs, familles, noms = charger_base(args.limite)
    encoder, _, _ = tour_texte()
    mots = VOCABULAIRES[args.vocabulaire]
    T = encoder(mots, prompt=True)
    table = par_centroide(vecteurs, familles, T, mots, k=args.k)

    par_famille = collections.defaultdict(list)
    for i, f in enumerate(familles):
        par_famille[int(f)].append(i)

    for fam, (tete, scores, n) in sorted(table.items(), key=lambda kv: -kv[1][2]):
        print(f"\n{n:>6}  " + " · ".join(tete))
        if args.detail:
            print("        " + "  ".join(f"{v:+.3f}" for v in scores))
        print(f"        {artistes_de(noms, par_famille[fam])}")


def cmd_comparer(args):
    """Le tableau qui tranche : ce que les genres disent, ce que CLAP dit, et
    qui est dans la famille. Sans la troisième colonne les deux premières ne
    se jugent pas."""
    vecteurs, familles, noms = charger_base(args.limite)
    encoder, _, _ = tour_texte()
    mots = VOCABULAIRES[args.vocabulaire]
    scores = calibrer(vecteurs @ encoder(mots, prompt=True).T, args.calibrage)
    etiquettes = [mots[j] for j in np.argmax(scores, axis=1)]
    tables = score_familles(etiquettes, familles)

    par_famille = collections.defaultdict(list)
    for i, f in enumerate(familles):
        par_famille[int(f)].append(i)

    for fam, (dedans, classe) in sorted(tables.items(), key=lambda kv: -kv[1][0]):
        tete = " · ".join(t for _, t, _ in classe[:2]) or "—"
        print(f"\n{dedans:>6}  {tete}")
        print(f"        {artistes_de(noms, par_famille[fam])}")


def cmd_familles(args):
    vecteurs, familles, _ = charger_base(args.limite)
    encoder, _, _ = tour_texte()
    mots = VOCABULAIRES[args.vocabulaire]
    T = encoder(mots, prompt=args.prompt)
    scores = calibrer(vecteurs @ T.T, args.calibrage)
    # Une étiquette par morceau, la meilleure — comme un genre par artiste.
    etiquettes = [mots[j] for j in np.argmax(scores, axis=1)]

    tables = score_familles(etiquettes, familles)
    for fam, (dedans, classe) in sorted(tables.items(), key=lambda kv: -kv[1][0]):
        tete = " · ".join(t for _, t, _ in classe[:2]) or "—"
        print(f"{dedans:>6}  {tete}")
        if args.detail:
            for s, tag, n in classe[:5]:
                print(f"          {s:+.3f}  {tag}  ({n})")


def cmd_export(args):
    """La tour texte s'exporte-t-elle proprement en ONNX ?"""
    import torch
    import onnx

    encoder, tok, modele = tour_texte()
    longueur = args.longueur

    class Tour(torch.nn.Module):
        """`get_text_features` + normalisation, comme la tour audio embarque
        déjà sa projection."""

        def __init__(self, m):
            super().__init__()
            self.m = m

        def forward(self, input_ids, attention_mask):
            v = tenseur_de(
                self.m.get_text_features(input_ids=input_ids, attention_mask=attention_mask)
            )
            return v / torch.linalg.norm(v, dim=-1, keepdim=True)

    tour = Tour(modele).eval()
    ids = torch.zeros((1, longueur), dtype=torch.int64)
    masque = torch.ones((1, longueur), dtype=torch.int64)

    sortie = Path(args.sortie)
    sortie.parent.mkdir(parents=True, exist_ok=True)
    torch.onnx.export(
        tour, (ids, masque), str(sortie),
        input_names=["input_ids", "attention_mask"], output_names=["embeddings"],
        opset_version=18, dynamo=False,
    )

    # Le même repliage que pour la tour audio : ORT ne produit pas un graphe
    # qu'il refuserait ensuite de relire. `BASIC` et pas `EXTENDED` — les
    # fusions poussées introduisent des opérateurs `com.microsoft`.
    if args.replier:
        import onnxruntime as ort_

        brut = sortie.with_suffix(".brut.onnx")
        sortie.rename(brut)
        o = ort_.SessionOptions()
        o.graph_optimization_level = ort_.GraphOptimizationLevel.ORT_ENABLE_BASIC
        o.optimized_model_filepath = str(sortie)
        ort_.InferenceSession(str(brut), o, providers=["CPUExecutionProvider"])
        avant = len(onnx.load(str(brut), load_external_data=False).graph.node)
        print(f"repliage ORT : {avant} nœuds →", end=" ")
        # Un demi-gigaoctet d'intermédiaire n'a pas à rester sur le disque.
        brut.unlink()

    m = onnx.load(str(sortie), load_external_data=False)
    onnx.checker.check_model(m)
    ops = collections.Counter(n.op_type for n in m.graph.node)
    connus = {i.name for i in m.graph.initializer}
    connus |= {n.output[0] for n in m.graph.node if n.op_type == "Constant"}
    calcules = [n.name for n in m.graph.node
                if n.op_type == "Pad" and len(n.input) > 1 and n.input[1] not in connus]
    domaines = {n.domain for n in m.graph.node if n.domain}
    print(f"{sum(ops.values())} nœuds · {len(ops)} types")
    print(f"marges calculées : {calcules or 'aucune'}")
    print(f"domaines non standard : {domaines or 'aucun'}")
    print("types :", ", ".join(f"{k}×{v}" for k, v in ops.most_common()))

    # Le seul contrôle qui compte : ONNX rend-il ce que PyTorch rendait ?
    import onnxruntime as ort
    s = ort.InferenceSession(str(sortie), providers=["CPUExecutionProvider"])
    phrases = DESCRIPTIONS[:4]
    lots = tok([PROMPT + p for p in phrases], padding="max_length",
               max_length=longueur, truncation=True, return_tensors="np")
    attendu = encoder(phrases)
    obtenu = np.vstack([
        s.run(None, {"input_ids": lots["input_ids"][i:i+1].astype(np.int64),
                     "attention_mask": lots["attention_mask"][i:i+1].astype(np.int64)})[0]
        for i in range(len(phrases))
    ])
    cos = (attendu * obtenu).sum(axis=1)
    ecart = np.abs(attendu - obtenu).max()
    print(f"cosinus torch/ONNX : {cos.min():.10f} · écart absolu max {ecart:.2e}")


def cmd_reference(args):
    """Écrit de quoi vérifier un import Burn sans tokeniseur : les identifiants
    de jetons en entrée, le vecteur attendu en sortie."""
    import json

    encoder, tok, _ = tour_texte()
    phrases = VOCABULAIRES[args.vocabulaire]
    lots = tok([PROMPT + p for p in phrases], padding="max_length",
               max_length=args.longueur, truncation=True, return_tensors="np")
    vecteurs = encoder(phrases)
    Path(args.sortie).parent.mkdir(parents=True, exist_ok=True)
    with open(args.sortie, "w", encoding="utf-8") as f:
        json.dump({
            "longueur": args.longueur,
            "phrases": phrases,
            "input_ids": lots["input_ids"].astype(int).tolist(),
            "attention_mask": lots["attention_mask"].astype(int).tolist(),
            "vecteurs": vecteurs.tolist(),
        }, f)
    print(f"{len(phrases)} phrases · {args.longueur} jetons → {args.sortie}")


def cmd_table(args):
    """La table d'empreintes du vocabulaire — ce qu'on embarquerait au lieu du
    modèle. `f32` petit-boutiste, comme les empreintes de la base."""
    encoder, _, _ = tour_texte()
    phrases = VOCABULAIRES[args.vocabulaire]
    v = encoder(phrases)
    chemin = Path(args.sortie)
    chemin.parent.mkdir(parents=True, exist_ok=True)
    with open(chemin, "wb") as f:
        f.write(v.astype("<f4").tobytes())
    with open(chemin.with_suffix(".txt"), "w", encoding="utf-8") as f:
        f.write("\n".join(phrases) + "\n")
    print(f"{len(phrases)} × 512 → {chemin} ({chemin.stat().st_size} octets)")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--limite", type=int, default=0, help="n'utiliser que N morceaux")
    sous = p.add_subparsers(dest="cmd", required=True)

    a = sous.add_parser("espace", help="top morceaux pour quelques phrases")
    a.add_argument("--top", type=int, default=5)
    a.add_argument("--vocabulaire", choices=list(VOCABULAIRES), default="descriptions")
    a.add_argument("--prompt", action="store_true", default=True)
    a.add_argument("--brut", dest="prompt", action="store_false")
    a.set_defaults(fn=cmd_espace)

    b = sous.add_parser("familles", help="nommer les douze familles")
    b.add_argument("--vocabulaire", choices=list(VOCABULAIRES), default="large")
    b.add_argument("--calibrage", choices=["brut", "centre", "reduit"], default="reduit")
    b.add_argument("--detail", action="store_true")
    b.add_argument("--prompt", action="store_true", default=True)
    b.add_argument("--brut", dest="prompt", action="store_false")
    b.set_defaults(fn=cmd_familles)

    c = sous.add_parser("export", help="exporter la tour texte en ONNX")
    c.add_argument("--longueur", type=int, default=32)
    c.add_argument("--sortie", default="modeles/clap-text-encoder.onnx")
    c.add_argument("--replier", action="store_true", default=True)
    c.add_argument("--sans-repliage", dest="replier", action="store_false")
    c.set_defaults(fn=cmd_export)

    d = sous.add_parser("reference", help="jetons + vecteurs attendus, pour l'import Burn")
    d.add_argument("--vocabulaire", choices=list(VOCABULAIRES), default="descriptions")
    d.add_argument("--longueur", type=int, default=32)
    d.add_argument("--sortie", default="reference.json")
    d.add_argument("--prompt", action="store_true", default=True)
    d.set_defaults(fn=cmd_reference)

    e = sous.add_parser("table", help="la table d'empreintes du vocabulaire")
    e.add_argument("--vocabulaire", choices=list(VOCABULAIRES), default="large")
    e.add_argument("--sortie", default="modeles/vocabulaire.bin")
    e.add_argument("--prompt", action="store_true", default=True)
    e.set_defaults(fn=cmd_table)

    f_ = sous.add_parser("comparer", help="nom CLAP + artistes dominants, famille par famille")
    f_.add_argument("--vocabulaire", choices=list(VOCABULAIRES), default="large")
    f_.add_argument("--calibrage", choices=["brut", "centre", "reduit"], default="centre")
    f_.set_defaults(fn=cmd_comparer)

    g = sous.add_parser("centroide", help="nommer par le centre de la famille")
    g.add_argument("--vocabulaire", choices=list(VOCABULAIRES), default="large")
    g.add_argument("--k", type=int, default=2)
    g.add_argument("--detail", action="store_true")
    g.set_defaults(fn=cmd_centroide)

    args = p.parse_args()
    args.fn(args)


if __name__ == "__main__":
    main()
