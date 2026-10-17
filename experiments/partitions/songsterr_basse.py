# SPDX-License-Identifier: GPL-3.0-or-later
"""Parties de basse Songsterr des morceaux les plus populaires de la bibliothèque
(au plus trois par artiste), pour le banc des doigtés (`corpus_doigtes.py`).
Usage local et d'évaluation seulement : rien n'est versionné. Une requête toutes
les 1,2 s ; reprend là où il s'est arrêté (`index.json`).

    python3 songsterr_basse.py [nombre de parties]
"""
import json, os, sqlite3, time, gzip, urllib.request, urllib.parse, sys
OUT = os.path.join(os.path.expanduser(os.environ.get("RUSTY_DEV", "/Volumes/LaCieDock/rusty_music-dev")), "jeux", "songsterr-basse")
DB = os.path.expanduser("~/Library/Application Support/fm.rustymusic.desktop/rusty-music.db")
CIBLE = int(sys.argv[1]) if len(sys.argv) > 1 else 300
c = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
rows = c.execute("""select coalesce(nullif(t.album_artist,''), t.artist), t.title, max(p.relative) r from tracks t join track_popularite p on p.track_id=t.id
  where t.title is not null and t.artist is not null group by lower(t.artist), lower(t.title) order by r desc""").fetchall()
par_artiste, cand = {}, []
for a, t, r in rows:
    if a is None or par_artiste.get(a.lower(), 0) >= 3: continue
    par_artiste[a.lower()] = par_artiste.get(a.lower(), 0) + 1; cand.append((a, t))
def get(url):
    time.sleep(1.2)
    req = urllib.request.Request(url, headers={"User-Agent": "rusty-music-eval/0.1"})
    with urllib.request.urlopen(req, timeout=30) as f: b = f.read()
    return json.loads(gzip.decompress(b) if b[:2] == b"\x1f\x8b" else b)
index_f = os.path.join(OUT, "index.json")
index = json.load(open(index_f)) if os.path.exists(index_f) else {"vus": [], "tabs": []}
vus = set(index["vus"])
for a, t in cand:
    if len(index["tabs"]) >= CIBLE: break
    cle = f"{a} — {t}".lower()
    if cle in vus: continue
    vus.add(cle); index["vus"].append(cle)
    try:
        res = get("https://www.songsterr.com/api/songs?" + urllib.parse.urlencode({"pattern": f"{a} {t.split('(')[0]}", "size": 5}))
        s = next((s for s in res if s["artist"].lower() == a.lower() and s["title"].lower().split("(")[0].strip() == t.lower().split("(")[0].strip()), None)
        if not s: continue
        bas = [(i, tr) for i, tr in enumerate(s["tracks"]) if 32 <= tr.get("instrumentId", -1) <= 39 and tr.get("views", 0) >= 300]
        if not bas: continue
        i, tr = max(bas, key=lambda x: x[1]["views"])
        meta = get(f"https://www.songsterr.com/api/meta/{s['songId']}")
        if meta.get("aiGenerated"): continue
        part = get(f"https://dqsljvtekg760.cloudfront.net/{s['songId']}/{meta['revisionId']}/{meta['image']}/{i}.json")
        nom = f"{s['songId']}.json"
        json.dump(part, open(os.path.join(OUT, nom), "w"))
        index["tabs"].append({"fichier": nom, "artiste": s["artist"], "titre": s["title"], "vues": tr["views"], "piste": tr["name"]})
        print(len(index["tabs"]), s["artist"], "—", s["title"], tr["views"], flush=True)
    except Exception as e:
        print("erreur", a, t, e, flush=True)
    finally:
        json.dump(index, open(index_f, "w"), ensure_ascii=False)
