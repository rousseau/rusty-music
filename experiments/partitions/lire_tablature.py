# SPDX-License-Identifier: GPL-3.0-or-later
"""Lit les chiffres des tablatures de basse d'une page scannée : pour chaque
portée de tablature, ses mesures (séparées par les barres) et dans chacune les
notes (x, corde, frette — ou « X » pour une note étouffée).

La notation reconnue par Audiveris donne le rythme, mais ses hauteurs sont
fragiles (armures incomplètes, bécarres manqués). La tablature, elle, dit
exactement quelle corde et quelle frette : c'est la vérité terrain des
hauteurs et des doigtés.

    python3 lire_tablature.py page.png   # affiche ce qui est lu
"""
import sys
import numpy as np
from PIL import Image
from scipy import ndimage

Image.MAX_IMAGE_PIXELS = None
BANDES = 24

def _lignes(bande):
    frac = (bande < 128).mean(axis=1)
    g = []
    for r in np.where(frac > 0.7)[0]:
        if g and r - g[-1][-1] <= 2: g[-1].append(r)
        else: g.append([r])
    return [(x[0] + x[-1]) / 2 for x in g]

def portees_tab(a):
    """Portées de tablature : [(xs des bandes, 4 ordonnées de lignes par bande)]."""
    h, w = a.shape; larg = w // BANDES
    par_bande = [_lignes(a[:, b * larg:(b + 1) * larg]) for b in range(BANDES)]
    ecarts = np.concatenate([np.diff(c) for c in par_bande if len(c) > 1])
    ecarts = ecarts[ecarts > 4]
    inter = np.bincount(np.round(ecarts).astype(int)).argmax()
    autres = np.round(ecarts[(ecarts > 1.35 * inter) & (ecarts < 3 * inter) & (np.abs(ecarts - 2 * inter) > 0.12 * inter)]).astype(int)
    if len(autres) == 0: return [], inter, None
    tab = np.bincount(autres).argmax()
    morceaux = []
    for b, c in enumerate(par_bande):
        i = 0
        while i + 3 < len(c):
            g = c[i:i + 4]
            d = np.diff(g)
            if np.all(np.abs(d - tab) < 0.12 * tab):
                morceaux.append((b, g)); i += 4
            else: i += 1
    # regrouper les morceaux de bandes voisines en portées
    portees = []
    for b, g in morceaux:
        for p in portees:
            if abs(p["g"][-1][0] - g[0]) < 1.5 * tab: p["b"].append(b); p["g"].append(g); break
        else: portees.append({"b": [b], "g": [g]})
    out = []
    for p in portees:
        if len(p["b"]) < 4: continue
        xs = np.array([(b + 0.5) * larg for b in p["b"]])
        out.append((xs, np.array(p["g"])))
    out.sort(key=lambda p: p[1][:, 0].mean())
    return out, inter, tab

TAILLE = 24

def _normaliser(binaire):
    """Glyphe noir sur blanc → carré TAILLE×TAILLE, centré, proportions gardées."""
    ys, xs = np.nonzero(binaire)
    if len(ys) == 0: return None
    b = binaire[ys.min():ys.max() + 1, xs.min():xs.max() + 1]
    h, w = b.shape; c = max(h, w)
    carre = np.zeros((c, c), bool); carre[(c - h) // 2:(c - h) // 2 + h, (c - w) // 2:(c - w) // 2 + w] = b
    im = Image.fromarray((carre * 255).astype(np.uint8)).resize((TAILLE, TAILLE), Image.BILINEAR)
    v = np.asarray(im, np.float32); v -= v.mean(); n = np.linalg.norm(v)
    return v / n if n else None

_MODELES = None
def _modeles():
    """Chiffres et X dessinés dans des polices sans empattement grasses, proches
    de la gravure des livres Hal Leonard."""
    global _MODELES
    if _MODELES is None:
        from PIL import ImageDraw, ImageFont
        polices = ["/System/Library/Fonts/Supplemental/Arial Bold.ttf", "/System/Library/Fonts/Helvetica.ttc",
                   "/System/Library/Fonts/Supplemental/Arial.ttf"]
        _MODELES = []
        for chemin in polices:
            for indice in ([0, 1] if chemin.endswith(".ttc") else [0]):
                try: f = ImageFont.truetype(chemin, 60, index=indice)
                except OSError: continue
                for car in "0123456789X":
                    im = Image.new("L", (90, 90), 0); ImageDraw.Draw(im).text((10, 5), car, fill=255, font=f)
                    b = np.asarray(im) > 128
                    v = _normaliser(b)
                    if v is not None: _MODELES.append((car, v))
                    # La ligne de la tablature passe au milieu du chiffre : la
                    # réserve blanche l'efface autour, pas dans les boucles
                    # (« 0 » lu « 8 »). Variante avec la ligne dans les trous.
                    trous = ndimage.binary_fill_holes(b) & ~b
                    if trous.any():
                        ys = np.nonzero(b.any(axis=1))[0]; ym = (ys.min() + ys.max()) // 2
                        for dy in (-3, 0, 3):
                            avec = b.copy(); bande = np.zeros_like(b); bande[ym + dy - 1:ym + dy + 2, :] = True
                            avec |= bande & trous
                            v = _normaliser(avec)
                            if v is not None: _MODELES.append((car, v))
    return _MODELES

def classer(binaire):
    """Le caractère le plus ressemblant et sa corrélation."""
    v = _normaliser(binaire)
    if v is None: return None, 0.0
    h, w = binaire.shape
    meilleur = max(((float((v * m).sum()), c) for c, m in _modeles()))
    # « 1 » : très étroit, la forme carrée normalisée le confond avec tout
    if w < 0.4 * h: return "1", 1.0
    return meilleur[1], meilleur[0]

def lire(a):
    portees, inter, tab = portees_tab(a)
    noir = a < 128
    systemes = []
    for xs, g in portees:
        x0, x1 = int(xs.min() - 1.5 * (xs[1] - xs[0])), int(xs.max() + 1.5 * (xs[1] - xs[0]))
        x0, x1 = max(0, x0), min(a.shape[1], x1)
        y0, y1 = int(g[:, 0].min() - 0.8 * tab), int(g[:, 3].max() + 0.8 * tab)
        zone = noir[y0:y1, x0:x1].copy()
        cols = np.arange(x0, x1)
        lignes_y = np.stack([np.interp(cols, xs, g[:, k]) for k in range(4)]) - y0  # 4 × largeur
        gris = a[y0:y1, x0:x1]
        # Segmentation : on efface les rangées des lignes (les chiffres y
        # perdent une bande, qu'une fermeture verticale recoud). La lecture se
        # fait ensuite sur l'image d'origine, dans la boîte du chiffre.
        sombre = zone.copy()
        for k in range(4):
            for j, yl in enumerate(lignes_y[k]):
                sombre[max(0, int(yl) - 2):int(yl) + 3, j] = False
        sombre = ndimage.binary_closing(sombre, structure=np.ones((7, 1)))
        # Ce qui reste des lignes forme des composantes bien plus larges qu'un
        # chiffre, qu'on ignore.
        lab, n = ndimage.label(sombre)
        glyphes = []
        for sl in ndimage.find_objects(lab):
            hh = sl[0].stop - sl[0].start; ww = sl[1].stop - sl[1].start
            if 0.4 * tab <= hh <= 1.25 * tab and 0.05 * tab <= ww <= 1.3 * tab:
                glyphes.append(sl)
        # barres de mesure : colonnes noires d'un bout à l'autre de la portée
        barres = []
        for j in range(zone.shape[1]):
            ya, yb = int(lignes_y[0, j]) - 1, int(lignes_y[3, j]) + 2
            if zone[ya:yb, j].mean() > 0.95:
                if barres and j - barres[-1][-1] <= 2: barres[-1].append(j)
                else: barres.append([j])
        barres = [(b[0] + b[-1]) / 2 for b in barres]
        # jetons : glyphes voisins sur la même corde (frettes à deux chiffres)
        glyphes.sort(key=lambda s: s[1].start)
        def etiquette(sl):
            v = lab[sl]; v = v[v > 0]
            return int(np.bincount(v).argmax())
        jetons = []
        for sl in glyphes:
            yc = (sl[0].start + sl[0].stop) / 2; xc = int((sl[1].start + sl[1].stop) / 2)
            corde_ = int(np.argmin(np.abs(lignes_y[:, min(xc, lignes_y.shape[1] - 1)] - yc)))
            if abs(lignes_y[corde_, min(xc, lignes_y.shape[1] - 1)] - yc) > 0.35 * tab: continue
            if jetons and jetons[-1]["corde"] == corde_ and sl[1].start - jetons[-1]["x1"] < 0.3 * tab:
                j = jetons[-1]; j["x1"] = sl[1].stop; j["y0"] = min(j["y0"], sl[0].start); j["y1"] = max(j["y1"], sl[0].stop)
                j["ids"].append(etiquette(sl))
            else:
                jetons.append({"corde": corde_, "x0": sl[1].start, "x1": sl[1].stop, "y0": sl[0].start, "y1": sl[0].stop,
                               "ids": [etiquette(sl)]})
        for j in jetons:
            texte = ""
            for i_ in sorted(j["ids"], key=lambda i_: ndimage.find_objects((lab == i_).astype(int))[0][1].start):
                m_ = lab == i_
                sl = ndimage.find_objects(m_.astype(int))[0]
                b = gris[sl] < 128
                car, score = classer(b)
                if car is None or score < 0.5: texte = ""; break
                texte += car
            j["texte"] = texte
        notes = []
        for j in jetons:
            t = j["texte"]
            if not t: continue
            val = "X" if "X" in t else (int(t) if t.isdigit() and int(t) <= 24 else None)
            if val is None: continue
            # corde 0 = la plus grave (mi), comme dans crates/transcription
            notes.append({"x": x0 + (j["x0"] + j["x1"]) / 2, "corde": 3 - j["corde"], "frette": val})
        barres = sorted(x0 + b for b in barres)
        # mesures : entre barres successives
        # avant la première barre : la clé « TAB », sauf si la portée commence
        # sans barre (notes loin de la marge)
        bornes = ([x0 + 3 * tab] if not barres or barres[0] > x0 + 6 * tab else []) + barres
        mesures = []
        for ga, dr in zip(bornes, bornes[1:]):
            dans = [n for n in notes if ga <= n["x"] < dr]
            if dr - ga > 2 * tab: mesures.append(dans)
        systemes.append({"y": float(g[:, 0].mean()), "barres": len(barres), "mesures": mesures})
    return systemes

if __name__ == "__main__":
    a = np.array(Image.open(sys.argv[1]).convert("L"))
    for s in lire(a):
        print(f"y={s['y']:.0f} barres={s['barres']}")
        for m in s["mesures"]:
            print("   |", " ".join(f"{n['corde']}:{n['frette']}" for n in m))
