# SPDX-License-Identifier: GPL-3.0-or-later
"""Apprend l'a priori de position du doigté (`crates/transcription/src/positions.rs`)
sur le corpus du banc des doigtés (`corpus_doigtes.py`) : à quelle frette, et
sur quelle corde (comptée depuis l'aiguë), les bassistes des tablatures de
référence jouent. Seuls des comptes agrégés sont écrits, rien des
tablatures elles-mêmes.

    python3 apprendre_positions.py <corpus.json> [--exclure nom,...] [--json sortie.json]

Sans `--json`, réécrit `positions.rs`.
"""
import argparse, collections, json, math, os

FRETTES, CORDES = 21, 6   # frettes 0..20 (au-delà : 20), cordes depuis l'aiguë

def table(corpus, exclure=()):
    n = collections.Counter()
    for m in corpus:
        if m["nom"] in exclure: continue
        k = len(m["cordes"])
        for c, f in m["verite"]:
            n[(min(k - 1 - c, CORDES - 1), min(f, FRETTES - 1))] += 1
    tot = sum(n.values()) + FRETTES * CORDES
    cout = [[-math.log((n[(c, f)] + 1) / tot) for f in range(FRETTES)] for c in range(CORDES)]
    bas = min(min(l) for l in cout)
    return [[round(x - bas, 3) for x in l] for l in cout], sum(n.values()), len({m["nom"] for m in corpus} - set(exclure))

if __name__ == "__main__":
    a = argparse.ArgumentParser(); a.add_argument("corpus"); a.add_argument("--exclure", default=""); a.add_argument("--json")
    a = a.parse_args()
    corpus = json.load(open(a.corpus))
    t, notes, morceaux = table(corpus, set(filter(None, a.exclure.split(","))))
    if a.json:
        json.dump(t, open(a.json, "w")); raise SystemExit
    livres = sum(m["nom"].startswith("livre:") for m in corpus); ss = morceaux - livres
    lignes = ",\n".join("    [" + ", ".join(f"{x:.3}" for x in l) + "]" for l in t)
    sortie = os.path.join(os.path.dirname(__file__), "..", "..", "crates", "transcription", "src", "positions.rs")
    open(sortie, "w").write(f"""// SPDX-License-Identifier: GPL-3.0-or-later
//! A priori de position du doigté : le coût (− log de la fréquence, 0 pour
//! la plus courante) de jouer une note à telle frette sur telle corde, comptée
//! depuis l'aiguë. Appris sur {notes} notes de {morceaux} tablatures de
//! référence ({livres} livres « Bass Recorded Versions », {ss} parties de
//! basse Songsterr) — seuls ces comptes agrégés sont gardés.
//!
//! Écrit par `experiments/partitions/apprendre_positions.py` ; ne pas
//! modifier à la main.

/// Frettes de la table (0 à 20 ; au-delà, la dernière ligne vaut).
pub const FRETTES: usize = {FRETTES};
/// Cordes de la table, depuis l'aiguë (au-delà, la dernière vaut).
pub const CORDES: usize = {CORDES};

/// `POSITIONS[corde depuis l'aiguë][frette]`.
pub const POSITIONS: [[f32; FRETTES]; CORDES] = [
{lignes},
];
""")
    print(f"{notes} notes, {morceaux} morceaux → {os.path.normpath(sortie)}")
