#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Typographie de l'interface : la Règle 11, mesurée dans les sources.

Vérifie, dans `apps/desktop/ui/` :

- `style.css` : chaque taille de police (`font-size`, `font:`) passe par un
  jeton `--t-*` (ou hérite) ; les jetons valent l'échelle de la règle, aucun
  sous 12 px ; les graisses sont 400, 500 ou 600 ;
- `js/*.js` : aucune police de canevas (`ctx.font`) sous 12 px ;
- `index.html` et `js/*.js` : aucune taille de police en style en ligne.

Les tailles calculées de ce qui s'affiche réellement : G6 de l'audit dans
l'application (`js/audit.js`) ; les étiquettes de la carte : le test
`aucune_etiquette_sous_douze_pixels` de `crates/carto/src/style.rs`. Appelé
par `scripts/audit-interface.sh` ; seul :

    python3 scripts/typographie-interface.py

Imprime aussi l'inventaire (tailles, graisses), comme le relevé du 1ᵉʳ
novembre 2026 ; sort avec le code 1 s'il y a une violation.
"""

import re
import sys
from collections import Counter
from pathlib import Path

UI = Path(__file__).resolve().parent.parent / "apps/desktop/ui"
ECHELLE = {"legende": 12, "second": 13, "corps": 14, "intertitre": 16, "titre": 20, "fil": 26, "decor": 34}
GRAISSES = {"400", "500", "600", "normal", "inherit"}


def main():
    css = (UI / "style.css").read_text(encoding="utf-8")
    violations = []

    jetons = {n: float(v) for n, v in re.findall(r"--t-([a-z]+):\s*([\d.]+)px", css)}
    for nom, px in ECHELLE.items():
        if jetons.get(nom) != px:
            violations.append(f"style.css : --t-{nom} vaut {jetons.get(nom)} px, attendu {px}")
    for nom in set(jetons) - set(ECHELLE):
        violations.append(f"style.css : --t-{nom} n'est pas dans l'échelle de la Règle 11")

    usages = Counter()
    for numero, ligne in enumerate(css.splitlines(), 1):
        for m in re.finditer(r"font-size:\s*([^;}]+)", ligne):
            valeur = m.group(1).strip()
            jeton = re.fullmatch(r"var\(--t-([a-z]+)\)", valeur)
            if jeton:
                usages[jeton.group(1)] += 1
            # `0` : un texte masqué à dessein (repère des années), rien ne s'affiche.
            elif valeur not in ("inherit", "100%", "1em", "0"):
                violations.append(f"style.css:{numero} taille hors jeton : font-size: {valeur}")
        for m in re.finditer(r"(?<![-\w])font:\s*([^;}]+)", ligne):
            valeur = m.group(1).strip()
            jeton = re.search(r"var\(--t-([a-z]+)\)", valeur)
            if jeton:
                usages[jeton.group(1)] += 1
            elif valeur != "inherit":
                violations.append(f"style.css:{numero} taille hors jeton : font: {valeur}")
        for m in re.finditer(r"font-weight:\s*([^;}]+)", ligne):
            if m.group(1).strip() not in GRAISSES:
                violations.append(f"style.css:{numero} graisse hors règle : {m.group(1).strip()}")

    for fichier in sorted((UI / "js").glob("*.js")):
        for numero, ligne in enumerate(fichier.read_text(encoding="utf-8").splitlines(), 1):
            for px in re.findall(r"\.font\s*=\s*[\"'`][^\"'`]*?([\d.]+)px", ligne):
                if float(px) < 12:
                    violations.append(f"js/{fichier.name}:{numero} police de canevas à {px} px")
            if re.search(r"font-size\s*:|\.fontSize\s*=", ligne) and fichier.name != "audit.js":
                violations.append(f"js/{fichier.name}:{numero} taille de police en style en ligne")
    for numero, ligne in enumerate((UI / "index.html").read_text(encoding="utf-8").splitlines(), 1):
        if re.search(r'style="[^"]*font', ligne):
            violations.append(f"index.html:{numero} police en style en ligne")

    graisses = Counter(re.findall(r"font-weight:\s*(\d+)", css))
    print("Échelle (Règle 11) : " + " · ".join(f"{px} px {nom} ×{usages[nom]}" for nom, px in ECHELLE.items()))
    print("Graisses : " + " · ".join(f"{g} ×{n}" for g, n in sorted(graisses.items())))
    if violations:
        print(f"{len(violations)} violation(s) de typographie :")
        print("\n".join(f"   {v}" for v in violations))
        return 1
    print("Typographie : aucune violation.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
