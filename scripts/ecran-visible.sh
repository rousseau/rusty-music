#!/usr/bin/env bash
# Sortie 0 si l'écran est visible : session déverrouillée, au premier plan
# (console) et affichage principal allumé ; 1 sinon (ou hors macOS). Avec -v,
# dit pourquoi sur la sortie d'erreur.
#
# Sert aux étapes qui doivent voir quelque chose (captures de la carte,
# relief) : macOS ne donne plus aucune image à une fenêtre quand la session
# est verrouillée (V13) ni quand l'affichage s'est mis en veille (V9).
#
# Lu par CoreGraphics, par un seul appel JXA (osascript, ~0,1 s, pas de
# compilation) : `CGSessionCopyCurrentDictionary` (CGSSessionScreenIsLocked,
# kCGSSessionOnConsoleKey) et `CGDisplayIsAsleep` de l'écran principal.
# Repli si osascript échoue : `ioreg` pour le verrou, sans savoir l'affichage.
set -uo pipefail

bavard=0
[ "${1:-}" = "-v" ] && bavard=1
dire() { [ "$bavard" = 1 ] && echo "$*" >&2; return 0; }

[ "$(uname)" = Darwin ] || { dire "hors macOS"; exit 1; }

etat="$(osascript -l JavaScript -e '
ObjC.import("CoreGraphics");
const r = $.CGSessionCopyCurrentDictionary();
const s = r ? ObjC.deepUnwrap(ObjC.castRefToObject(r)) : {};
const verrou = s.CGSSessionScreenIsLocked ? 1 : 0;
const console_ = s.kCGSSessionOnConsoleKey ? 1 : 0;
const dort = $.CGDisplayIsAsleep($.CGMainDisplayID()) ? 1 : 0;
`${verrou} ${console_} ${dort}`;
' 2>/dev/null)"

if [ -z "$etat" ]; then
  # Pas de tube vers `grep -q` : sous pipefail, ioreg tué par SIGPIPE (V13).
  case "$(ioreg -n Root -d1)" in
    *'"CGSSessionScreenIsLocked"=Yes'*) dire "session verrouillée (ioreg)"; exit 1 ;;
  esac
  dire "état de l'affichage illisible (osascript) ; session non verrouillée selon ioreg"
  exit 1
fi

read -r verrou console_ dort <<<"$etat"
[ "$verrou" = 1 ] && { dire "session verrouillée"; exit 1; }
[ "$console_" = 1 ] || { dire "session hors console (autre utilisateur au premier plan)"; exit 1; }
[ "$dort" = 1 ] && { dire "affichage éteint (veille)"; exit 1; }
dire "écran visible"
exit 0
