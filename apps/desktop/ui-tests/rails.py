#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Assemble les captures de `rails.spec.js` : une rangée par étiquette, le rail
de chaque écran côte à côte, son nom et sa hauteur de contenu au-dessus, un
trait rouge à la hauteur visible du rail (la limite de G8, Règle 13).

    python3 rails.py DOSSIER SORTIE.png avant apres
"""
import json
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

dossier, sortie, etiquettes = Path(sys.argv[1]), sys.argv[2], sys.argv[3:]
POLICE = "/System/Library/Fonts/Helvetica.ttc"
try:
    police, petite = ImageFont.truetype(POLICE, 20), ImageFont.truetype(POLICE, 16)
except OSError:
    police = petite = ImageFont.load_default()

ECART, TETE, MARGE = 14, 56, 120
rangees = []
for e in etiquettes:
    releve = json.loads((dossier / f"{e}.json").read_text())
    images = [Image.open(dossier / f"{e}-{i + 1:02d}.png").convert("RGB") for i in range(len(releve))]
    rangees.append((e, releve, images))

largeur = MARGE + max(sum(i.width + ECART for i in imgs) for _, _, imgs in rangees)
hauteurs = [TETE + max(max(i.height for i in imgs), max(r["hauteur"] for r in rel)) + 30 for _, rel, imgs in rangees]
toile = Image.new("RGB", (largeur, sum(hauteurs)), "#F4F1EA")
d = ImageDraw.Draw(toile)
y0 = 0
for (e, releve, images), h in zip(rangees, hauteurs):
    d.text((10, y0 + TETE), e, fill="#222", font=police)
    x = MARGE
    for r, img in zip(releve, images):
        d.text((x, y0 + 6), r["nom"], fill="#222", font=petite)
        trop = r["bas"] - r["hauteur"]
        d.text((x, y0 + 28), f"{r['bas']} px" + (f"  +{trop}" if trop > 0 else ""), fill="#B3261E" if trop > 0 else "#555", font=petite)
        toile.paste(img, (x, y0 + TETE))
        yl = y0 + TETE + r["hauteur"]
        d.line([(x - 4, yl), (x + img.width + 4, yl)], fill="#D0302A", width=2)
        x += img.width + ECART
    y0 += h
toile.save(sortie)
print(sortie, toile.size)
