import os, sys, warnings; warnings.filterwarnings("ignore")
for v in ('OMP_NUM_THREADS','OPENBLAS_NUM_THREADS','NUMBA_NUM_THREADS'): os.environ[v]='1'
import numpy as np, librosa
from multiprocessing import Pool
def est(p):
    try:
        y,sr=librosa.load(p,sr=22050,mono=True)
        return p, float(librosa.feature.tempo(y=y,sr=sr,aggregate=np.mean)[0])
    except Exception: return p,None
if __name__=="__main__":
    fs=sorted(os.path.join(r,f) for r,_,fl in os.walk("genres") for f in fl if f.endswith(".wav"))
    with Pool(8) as pool: res=pool.map(est,fs,chunksize=8)
    with open("librosa.csv","w") as o:
        o.write("chemin,bpm\n")
        for p,b in res: o.write(f"{os.path.relpath(p,'genres')},{'' if b is None else f'{b:.3f}'}\n")
