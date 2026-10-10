# SPDX-License-Identifier: GPL-3.0-or-later
"""Reconnaissance optique d'une partition de basse scannée (livres « Bass
Recorded Versions » : portée de notation + tablature). Les pages sont rendues à
300 dpi, débarrassées de leurs tablatures (`nettoyer_page.py`, sans quoi
Audiveris prend l'interligne de la tablature pour celui des portées et rate les
ligatures), assemblées en un PDF, puis lues par Audiveris → MusicXML. Les tablatures,
lues avant d'être effacées (`lire_tablature.py`), vont dans
`tablature.json` : page → systèmes → mesures → (corde, frette).

Usage local et d'évaluation seulement : les partitions sont des œuvres
protégées, rien de ce qui en sort n'est versionné.

    AUDIVERIS=/chemin/Audiveris.app/Contents/MacOS/Audiveris \
    LIVRES=~/Exp/tmp CACHE=~/Exp/tmp/banc-partitions \
    python3 omr.py [id ...]
"""
import json, os, subprocess, sys, glob
import fitz, numpy as np
from PIL import Image
from nettoyer_page import nettoyer
import lire_tablature

ICI = os.path.dirname(os.path.abspath(__file__))
LIVRES = os.path.expanduser(os.environ.get("LIVRES", "~/Exp/tmp"))
CACHE = os.path.expanduser(os.environ.get("CACHE", "~/Exp/tmp/banc-partitions"))
AUDIVERIS = os.environ["AUDIVERIS"]

def main():
    conf = json.load(open(os.path.join(ICI, "morceaux.json")))
    voulus = set(sys.argv[1:])
    for m in conf["morceaux"]:
        if voulus and m["id"] not in voulus: continue
        dossier = os.path.join(CACHE, m["id"]); os.makedirs(dossier, exist_ok=True)
        mxl = os.path.join(dossier, "partition.mxl")
        if os.path.exists(mxl) or glob.glob(os.path.join(dossier, "page*.mxl")):
            print(m["id"], "déjà lu"); continue
        livre = fitz.open(os.path.join(LIVRES, conf["livres"][m["livre"]]))
        sortie = fitz.open()
        tablatures = {}
        for p in range(m["pages"][0], m["pages"][1] + 1):
            pix = livre[p].get_pixmap(dpi=300, colorspace=fitz.csGRAY)
            a = np.frombuffer(pix.samples, np.uint8).reshape(pix.height, pix.width)
            if m.get("tablature", True):
                tablatures[p] = lire_tablature.lire(a)
            propre, inter, n = nettoyer(a)
            chemin = os.path.join(dossier, f"page-{p:03d}.png")
            Image.fromarray(propre).save(chemin)
            page = sortie.new_page(width=livre[p].rect.width, height=livre[p].rect.height)
            page.insert_image(page.rect, filename=chemin)
        pdf = os.path.join(dossier, "pages.pdf"); sortie.save(pdf)
        json.dump(tablatures, open(os.path.join(dossier, "tablature.json"), "w"))
        subprocess.run([AUDIVERIS, "-batch", "-export", "-output", dossier, pdf],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        lus = sorted(glob.glob(os.path.join(dossier, "pages*.mxl")))
        if not lus:
            # une page qu'Audiveris ne sait pas lire fait échouer tout le
            # livre : on reprend page par page
            for p in range(m["pages"][0], m["pages"][1] + 1):
                subprocess.run([AUDIVERIS, "-batch", "-export", "-output", dossier, os.path.join(dossier, f"page-{p:03d}.png")],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            lus = sorted(glob.glob(os.path.join(dossier, "page-*.mxl")))
        print(m["id"], "→", [os.path.basename(x) for x in lus] or "échec")
        if len(lus) == 1: os.rename(lus[0], mxl)
        # plusieurs « mouvements » : reference.py les enchaîne

main()
