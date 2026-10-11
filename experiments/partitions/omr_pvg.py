# SPDX-License-Identifier: GPL-3.0-or-later
"""Reconnaissance optique des recueils « piano-chant-guitare » (epub Hal
Leonard) : les images d'un morceau, dans l'ordre de son chapitre, préparées
par `nettoyer_pvg.py` (agrandies, sans diagrammes d'accords), assemblées en un
PDF puis lues par Audiveris → MusicXML (voix + piano).

Usage local et d'évaluation seulement : les recueils sont des œuvres
protégées, rien de ce qui en sort n'est versionné.

    AUDIVERIS=/chemin/Audiveris.app/Contents/MacOS/Audiveris \
    python3 omr_pvg.py [id ...]
"""
import glob, io, json, os, posixpath, re, subprocess, sys, zipfile
from concurrent.futures import ThreadPoolExecutor
import fitz
from PIL import Image
from nettoyer_pvg import nettoyer

ICI = os.path.dirname(os.path.abspath(__file__))
# Données de développement sur le disque externe, pas toujours branché.
DEV = os.path.expanduser(os.environ.get("RUSTY_DEV", "/Volumes/LaCieDock/rusty_music-dev"))
if not os.path.isdir(DEV):
    sys.exit(f"données de développement absentes : {DEV} (disque débranché ? sinon RUSTY_DEV=…)")
LIVRES = os.path.expanduser(os.environ.get("LIVRES", DEV))
CACHE = os.path.expanduser(os.environ.get("CACHE", os.path.join(DEV, "banc-partitions")))
AUDIVERIS = os.environ["AUDIVERIS"]

def images(epub, chapitre):
    z = zipfile.ZipFile(epub)
    html = next(n for n in z.namelist() if n.endswith("/" + chapitre) or n == chapitre)
    dossier = posixpath.dirname(html)
    for src in re.findall(r'<img[^>]+src="([^"]+)"', z.read(html).decode("utf-8", "replace")):
        yield Image.open(io.BytesIO(z.read(posixpath.normpath(posixpath.join(dossier, src)))))

def lire(m, livres):
    dossier = os.path.join(CACHE, m["id"]); os.makedirs(dossier, exist_ok=True)
    if os.path.exists(os.path.join(dossier, "partition.mxl")) or glob.glob(os.path.join(dossier, "pages*.mxl")):
        return f"{m['id']} déjà lu"
    pdf = fitz.open()
    for k, im in enumerate(images(os.path.join(LIVRES, livres[m["livre"]]), m["chapitre"])):
        propre, _ = nettoyer(im)
        chemin = os.path.join(dossier, f"page-{k:03d}.png")
        propre.save(chemin, dpi=(300, 300))
        page = pdf.new_page(width=propre.width * 72 / 300, height=propre.height * 72 / 300)
        page.insert_image(page.rect, filename=chemin)
    sortie = os.path.join(dossier, "pages.pdf"); pdf.save(sortie)
    subprocess.run([AUDIVERIS, "-batch", "-export", "-output", dossier, sortie],
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    lus = sorted(glob.glob(os.path.join(dossier, "pages*.mxl")))
    if not lus:
        for chemin in sorted(glob.glob(os.path.join(dossier, "page-*.png"))):
            subprocess.run([AUDIVERIS, "-batch", "-export", "-output", dossier, chemin],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        lus = sorted(glob.glob(os.path.join(dossier, "page-*.mxl")))
    if len(lus) == 1 and lus[0].endswith("pages.mxl"): os.rename(lus[0], os.path.join(dossier, "partition.mxl"))
    return f"{m['id']} → {len(lus) or 'échec'}"

def main():
    conf = json.load(open(os.path.join(ICI, sys.argv[1] if len(sys.argv) > 1 and sys.argv[1].endswith(".json") else "morceaux_pvg.json")))
    voulus = {a for a in sys.argv[1:] if not a.endswith(".json")}
    ms = [m for m in conf["morceaux"] if not voulus or m["id"] in voulus]
    # Audiveris est mono-fil pour l'essentiel : quatre morceaux à la fois
    with ThreadPoolExecutor(4) as ex:
        for r in ex.map(lambda m: lire(m, conf["livres"]), ms): print(r, flush=True)

main()
