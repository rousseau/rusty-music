# SPDX-License-Identifier: GPL-3.0-or-later
"""Stems (htdemucs_6s) et pulsation de chaque morceau du banc, en avance
(`banc.py` le fait aussi à la demande)."""
import json, os, sys
sys.argv = sys.argv[:1]
import banc
conf = json.load(open(os.path.join(banc.ICI, "morceaux.json")))
for m in conf["morceaux"]:
    banc.preparer_audio(m)
    print(m["id"], "prêt", flush=True)
