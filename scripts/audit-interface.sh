#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Audit de l'interface — contraste des couleurs dans les deux thèmes
# (`scripts/contraste-interface.py`) et typographie (`scripts/typographie-interface.py`),
# puis, dans l'application : gabarit (marges, panneaux, surimpressions), tailles
# réellement affichées et écritures, sur tous les écrans (voir
# `apps/desktop/ui/js/audit.js`, Règles 6, 10 et 11 de `docs/interface-guidelines.md`).
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

# Contraste des jetons de couleur dans les deux thèmes (Règle 6) et typographie
# (Règle 11) : statiques, lus dans les sources, avant de lancer quoi que ce soit.
echo "contraste, typographie…"
for controle in contraste typographie; do
  if sortie="$(python3 "scripts/$controle-interface.py")"; then
    echo "$sortie" | tail -n 1
  else
    echo "$sortie"
    exit 1
  fi
done

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

# Session verrouillée : WebKit tient la fenêtre pour invisible et suspend la
# page au bout de ~30 s ; l'audit attendrait alors jusqu'au délai, sans un mot.
verrouillee() { [ "$(uname)" = Darwin ] && ioreg -n Root -d1 | grep -q '"CGSSessionScreenIsLocked"=Yes'; }
if verrouillee; then
  echo "session verrouillée : l'audit a besoin de l'écran déverrouillé et de sa fenêtre visible" >&2
  exit 2
fi

echo "compilation…"
cargo build -q -p rusty-music-desktop

RUSTY_MUSIC_DONNEES="$TMP" RUSTY_MUSIC_INCOGNITO=1 RUSTY_MUSIC_AUDIT=1 \
  ./target/debug/rusty-music-desktop >"$LOG" 2>&1 &
PID=$!
disown "$PID" 2>/dev/null || true   # pas de message « Terminated » à l'arrêt

echo "audit lancé (pid $PID, copie de la base dans $TMP)…"
debut=$SECONDS
until grep -aq "AUDIT FIN\|AUDIT ERREUR" "$LOG"; do
  if ! kill -0 "$PID" 2>/dev/null; then echo "l'application s'est arrêtée avant la fin de l'audit" >&2; tail -5 "$LOG" >&2; exit 2; fi
  # Verrouillée en cours de route, la page est suspendue : le dire plutôt
  # qu'attendre le délai sans un mot.
  if verrouillee; then echo "session verrouillée pendant l'audit : relancer écran déverrouillé" >&2; exit 2; fi
  if [ $((SECONDS - debut)) -gt "$DELAI" ]; then
    # Le journal part avec le dossier temporaire : dire au moins où l'audit
    # s'est arrêté (la dernière cellule mesurée, les dernières lignes).
    echo "délai dépassé (${DELAI} s) ; $(grep -ac 'AUDIT {' "$LOG") cellules mesurées, la dernière :" >&2
    grep -a 'AUDIT {' "$LOG" | tail -1 | cut -c1-200 >&2
    tail -4 "$LOG" | cut -c1-200 >&2
    exit 2
  fi
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

# G8 (Règle 13) : la hauteur du rail de chaque écran, qu'il tienne ou non —
# la mesure dont part le désencombrement.
rails = {}
for c in cellules:
    r = c["mesures"].get("rail")
    if r and c["etat"] == "base" and c["theme"] == "sombre":
        rails[c["ecran"]] = (r, c["mesures"].get("familles"))
if rails:
    fam = {f for _, f in rails.values()}
    print(f"\nRail à 1 400 × 900 (G8), {'/'.join(map(str, sorted(fam)))} familles : contenu / hauteur, dépassement")
    for ecran, ((bas, haut), _) in rails.items():
        print(f"   {ecran:<24} {bas:>5} / {haut} px   {'+' + str(bas - haut) if bas > haut else 'tient'}")
sys.exit(1 if total else 0)
PY
