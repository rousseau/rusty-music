# SPDX-License-Identifier: GPL-3.0-or-later
"""Prépare une page de recueil « piano-chant-guitare » (epub Hal Leonard,
images d'environ 60 dpi) pour Audiveris : agrandie trois fois (l'interligne
passe de ~5 à ~15 px, sous lequel Audiveris ne trouve pas les portées) et
débarrassée des diagrammes d'accords de guitare, qu'Audiveris lit sinon comme
des accords de rondes au-dessus de la voix. Un diagramme se reconnaît à sa
grille : une composante connexe percée de nombreux trous ; on efface aussi le
nom d'accord juste au-dessus.

    python3 nettoyer_pvg.py entree.jpg sortie.png
"""
import sys
import numpy as np
from PIL import Image
from scipy import ndimage

FACTEUR = 3

def diagrammes(a):
    """Boîtes (y0, y1, x0, x1) des diagrammes, à la résolution d'origine :
    leurs traits sont gris (~100-200), on seuille haut."""
    noir = a < 225
    lab, n = ndimage.label(noir)
    boites = []
    for i, sl in enumerate(ndimage.find_objects(lab), 1):
        h, w = sl[0].stop - sl[0].start, sl[1].stop - sl[1].start
        # grille : ni une portée (bien plus large), ni une tête de note
        if not (10 <= h <= 30 and 10 <= w <= 30): continue
        comp = lab[sl] == i
        trous = ndimage.label(~comp)[0]
        # les trous ne touchent pas le bord de la boîte
        bord = set(np.unique(np.concatenate([trous[0], trous[-1], trous[:, 0], trous[:, -1]])))
        if len(set(np.unique(trous)) - bord - {0}) >= 6:
            boites.append((sl[0].start, sl[0].stop, sl[1].start, sl[1].stop))
    return boites

def nettoyer(img):
    a = np.array(img.convert("L"))
    for y0, y1, x0, x1 in diagrammes(a):
        h, w = y1 - y0, x1 - x0
        # le nom d'accord au-dessus, les ronds de cordes à vide, le doigté
        a[max(0, y0 - int(1.2 * h)):y1 + 2, max(0, x0 - w // 2):x1 + w // 2] = 255
    g = Image.fromarray(a)
    return g.resize((g.width * FACTEUR, g.height * FACTEUR), Image.BICUBIC), len(diagrammes(np.array(img.convert("L"))))

if __name__ == "__main__":
    im, n = nettoyer(Image.open(sys.argv[1]))
    im.save(sys.argv[2], dpi=(300, 300))
    print(f"{n} diagrammes effacés", file=sys.stderr)
