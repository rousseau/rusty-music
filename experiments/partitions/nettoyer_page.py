# SPDX-License-Identifier: GPL-3.0-or-later
"""Efface les tablatures d'une page scannée, pour que la reconnaissance ne voie
que les portées de 5 lignes. Les lignes se cherchent par bandes verticales (le
scan est de travers et courbé) ; une tablature se reconnaît à son interligne,
nettement plus grand que celui des portées.
usage: nettoyer_page.py entree.png sortie.png"""
import sys, numpy as np
from PIL import Image
Image.MAX_IMAGE_PIXELS=None
BANDES=24
def lignes(bande):
    frac=(bande<128).mean(axis=1)
    g=[]
    for r in np.where(frac>0.7)[0]:
        if g and r-g[-1][-1]<=2: g[-1].append(r)
        else: g.append([r])
    return [(x[0]+x[-1])/2 for x in g]
def groupes(c, inter):
    """Suites de lignes régulièrement espacées."""
    res=[]; cur=[]
    for y in c:
        if cur and (len(cur)<2 and y-cur[-1] < 2.2*inter or len(cur)>=2 and abs((y-cur[-1])-(cur[-1]-cur[-2]))<0.2*(cur[-1]-cur[-2])):
            cur.append(y)
        else:
            if cur: res.append(cur)
            cur=[y]
    if cur: res.append(cur)
    return res
def nettoyer(a):
    h,w=a.shape; larg=w//BANDES
    par_bande=[lignes(a[:, b*larg:(b+1)*larg]) for b in range(BANDES)]
    ecarts=np.concatenate([np.diff(c) for c in par_bande if len(c)>1])
    ecarts=ecarts[ecarts>4]
    # interligne des portées : l'écart le plus fréquent
    inter=np.bincount(np.round(ecarts).astype(int)).argmax()
    # interligne des tablatures : l'écart fréquent suivant, hors double de l'interligne
    autres=np.round(ecarts[(ecarts>1.35*inter)&(ecarts<3*inter)&(np.abs(ecarts-2*inter)>0.12*inter)]).astype(int)
    if len(autres)==0: return a.copy(), inter, 0
    tab=np.bincount(autres).argmax()
    out=a.copy(); n_tab=0
    for b,c in enumerate(par_bande):
        for g in groupes(c, inter):
            if len(g)<2: continue
            sp=(g[-1]-g[0])/(len(g)-1)
            if abs(sp-tab)<0.1*tab and len(g)>=3:
                n_tab+=1
                x0=max(0,b*larg-larg//2); x1=min(w,(b+1)*larg+larg//2)
                out[int(g[0]-sp*0.8):int(g[-1]+sp*0.8)+1, x0:x1]=255
    return out, inter, n_tab
if __name__=="__main__":
    a=np.array(Image.open(sys.argv[1]).convert("L"))
    out,inter,n=nettoyer(a)
    Image.fromarray(out).save(sys.argv[2])
    print(f"interligne {inter}px, {n} morceaux de tablature effacés", file=sys.stderr)
