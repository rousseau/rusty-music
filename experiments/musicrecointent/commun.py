"""Utilitaires partagés par noter.py et noter_playlist.py."""
import difflib, re, unicodedata

def norm(s):
    s = unicodedata.normalize("NFKD", str(s)).encode("ascii", "ignore").decode().lower()
    s = re.sub(r"\b(music|musics|songs?|artists?|band)\b", " ", s)
    return re.sub(r"[^a-z0-9]+", " ", s).strip()

def proche(a, b):
    a, b = norm(a), norm(b)
    if not a or not b: return False
    # « rap » ~ « hip hop » ; « bigbeat » ~ « big beat » (graphies collées)
    a, b = re.sub(r"\bhip hop\b|\bhiphop\b", "rap", a), re.sub(r"\bhip hop\b|\bhiphop\b", "rap", b)
    if a.replace(" ", "") == b.replace(" ", ""): return True
    if a == b or (len(a) > 2 and a in b) or (len(b) > 2 and b in a): return True
    return difflib.SequenceMatcher(None, a, b).ratio() >= 0.85

def dans(terme, liste): return any(proche(terme, x) for x in liste)

def decennies(termes):
    """Plage d'années attendue d'après les termes Decade (« 80s », « 2015 »)."""
    bornes = []
    for t in termes:
        m = re.search(r"\b(?:(19|20)?(\d)0)'?s\b", t)
        a = re.fullmatch(r"\s*((?:19|20)\d\d)\s*", t)
        if a: bornes.append((int(a[1]), int(a[1])))
        elif m:
            siecle = m[1] or ("19" if int(m[2]) >= 3 else "20")
            d = int(siecle + m[2] + "0"); bornes.append((d, d + 9))
    return (min(b[0] for b in bornes), max(b[1] for b in bornes)) if bornes else None
