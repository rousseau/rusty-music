#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Audit de gabarit de l'interface — cohérence des marges, des panneaux et des
# surimpressions sur tous les écrans (voir `apps/desktop/ui/audit.js` et la
# Règle 10 de `docs/interface-guidelines.md`).
#
# La webview du système ne se capture pas hors de son Espace macOS : l'interface
# se mesure de l'intérieur. L'application est lancée en mode d'essai sur une
# **copie** de la base (l'audit parcourt tous les écrans, dont certains écrivent),
# parcourt écrans × états × thèmes × tailles de fenêtre, écrit un constat par
# cellule dans son journal ; ce script les lit, imprime le rapport et sort avec
# le code 1 s'il y a des violations.
#
#   scripts/audit-interface.sh            # ~3 minutes
#   RUSTY_DONNEES_SOURCE=/chemin scripts/audit-interface.sh
#
# Variables : RUSTY_DONNEES_SOURCE (dossier de données à copier, par défaut celui
# de l'application sous macOS), RUSTY_AUDIT_DELAI (secondes, 900 par défaut).
set -euo pipefail
cd "$(dirname "$0")/.."

SOURCE="${RUSTY_DONNEES_SOURCE:-$HOME/Library/Application Support/fm.rustymusic.desktop}"
DELAI="${RUSTY_AUDIT_DELAI:-900}"
[ -f "$SOURCE/rusty-music.db" ] || { echo "base introuvable : $SOURCE/rusty-music.db" >&2; exit 2; }

TMP="$(mktemp -d)"
LOG="$TMP/audit.log"
PID=""
nettoyer() {
  [ -n "$PID" ] && kill "$PID" 2>/dev/null || true
  rm -rf "$TMP"
}
trap nettoyer EXIT

cp "$SOURCE/rusty-music.db" "$TMP/"
[ -f "$SOURCE/ville-paris.db" ] && cp "$SOURCE/ville-paris.db" "$TMP/" || true

cargo build -q -p rusty-music-desktop

RUSTY_MUSIC_DONNEES="$TMP" RUSTY_MUSIC_INCOGNITO=1 RUSTY_MUSIC_AUDIT=1 \
  ./target/debug/rusty-music-desktop >"$LOG" 2>&1 &
PID=$!
disown "$PID" 2>/dev/null || true   # pas de message « Terminated » à l'arrêt

echo "audit lancé (pid $PID, copie de la base dans $TMP)…"
debut=$SECONDS
until grep -aq "AUDIT FIN\|AUDIT ERREUR" "$LOG"; do
  if ! kill -0 "$PID" 2>/dev/null; then echo "l'application s'est arrêtée avant la fin de l'audit" >&2; tail -5 "$LOG" >&2; exit 2; fi
  if [ $((SECONDS - debut)) -gt "$DELAI" ]; then echo "délai dépassé (${DELAI} s)" >&2; exit 2; fi
  sleep 2
done

python3 - "$LOG" <<'PY'
import json, re, sys
from collections import defaultdict

lignes = [re.sub(r"\x1b\[[0-9;]*m", "", l) for l in open(sys.argv[1], encoding="utf-8", errors="replace")]
cellules, erreurs = [], []
for l in lignes:
    m = re.search(r"AUDIT (\{.*\})\s*$", l)
    if m:
        cellules.append(json.loads(m.group(1)))
    elif "AUDIT ERREUR" in l:
        erreurs.append(l.strip())

if erreurs:
    print("\n".join(erreurs))
    sys.exit(2)

par_ecran = defaultdict(lambda: defaultdict(list))
for c in cellules:
    for v in c["violations"]:
        cle = (v["regle"], v["detail"])
        contexte = f'{c["etat"]} · {c["theme"]} · {c["fenetre"][0]}×{c["fenetre"][1]}'
        par_ecran[c["ecran"]][cle].append(contexte)

total = sum(len(c["violations"]) for c in cellules)
print(f"\n{len(cellules)} cellules mesurées, {total} violations.\n")
for ecran, constats in par_ecran.items():
    print(f"■ {ecran}")
    for (regle, detail), contextes in constats.items():
        quand = contextes[0] if len(contextes) == 1 else f"{len(contextes)} cellules, dont {contextes[0]}"
        print(f"   {regle}  {detail}   [{quand}]")
    print()
if not total:
    print("Gabarit cohérent : aucune violation.")
sys.exit(1 if total else 0)
PY
