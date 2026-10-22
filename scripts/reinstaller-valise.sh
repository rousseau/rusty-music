#!/usr/bin/env bash
# Reconstruit et réinstalle Rusty Valise sur l'iPhone — à relancer tous les
# 7 jours avec un compte Apple gratuit (le profil expire, les Documents restent).
# Voir docs/plan-ios-compagnon.md, « Installer sur l'iPhone ».
# Usage : ./scripts/reinstaller-valise.sh [UDID]
#   sans argument : $IPHONE_UDID, sinon le seul iPhone physique appairé.
set -euo pipefail
cd "$(dirname "$0")/../apps/mobile"

ipa="gen/apple/build/arm64/Rusty Valise.ipa"

udid="${1:-${IPHONE_UDID:-}}"
if [ -z "$udid" ]; then
  # Une ligne par appareil ; on garde les physiques (pas le simulateur).
  # (bash 3.2 de macOS : pas de mapfile)
  trouves=()
  while read -r u; do trouves+=("$u"); done < <(xcrun devicectl list devices 2>/dev/null \
    | awk '/physical/ { for (i = 1; i < NF; i++) if ($(i+1) == "(UDID)") print $i }')
  case ${#trouves[@]} in
    1) udid="${trouves[0]}" ;;
    0) echo "Aucun iPhone appairé : brancher l'iPhone (ou même Wi-Fi), le déverrouiller." >&2
       exit 1 ;;
    *) echo "Plusieurs iPhone : passer l'UDID en argument." >&2
       printf '  %s\n' "${trouves[@]}" >&2
       exit 1 ;;
  esac
fi

# Piège 4 : la construction ne remplace pas une sortie existante — sans ce
# ménage, on réinstallerait l'ancienne version sans le savoir.
rm -rf gen/apple/build/arm64

# Xcode réutilise le profil de signature tant qu'il n'a pas expiré : la
# réinstallation ne repousserait pas l'échéance. Sans profil local de l'app,
# -allowProvisioningUpdates en demande un neuf (7 jours à partir de maintenant).
profils="$HOME/Library/Developer/Xcode/UserData/Provisioning Profiles"
for p in "$profils"/*.mobileprovision; do
  [ -f "$p" ] || continue
  if security cms -D -i "$p" 2>/dev/null | grep -q '\.fr\.rousseau\.rustyvalise<'; then
    rm -f "$p"
  fi
done

echo "── construction (--debug : la version optimisée plante au lancement, piège 5)"
cargo tauri ios build --debug --target aarch64 --export-method debugging

[ -f "$ipa" ] || { echo "Construction sans $ipa" >&2; exit 1; }

echo "── installation sur $udid"
xcrun devicectl device install app --device "$udid" "$ipa"

# L'échéance est celle du profil de signature installé sur l'iPhone.
echeance=$(xcrun devicectl device profile list --device "$udid" --json-output - 2>/dev/null \
  | /usr/bin/python3 -c '
import json, sys
p = [x["expirationDate"] for x in json.load(sys.stdin)["result"]["provisioningProfiles"]
     if x["name"].endswith("fr.rousseau.rustyvalise")]
print(max(p) if p else "")' || true)
if [ -n "$echeance" ]; then
  jour=$(LC_TIME=fr_FR.UTF-8 date -j -u -f '%Y-%m-%dT%H:%M:%S' "${echeance%%.*}" '+%A %-d %B à %H:%M UTC')
  echo "── installé ; l'app expire le $jour"
else
  echo "── installé"
fi
