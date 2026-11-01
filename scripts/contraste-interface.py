#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Contraste des jetons de couleur de l'interface, dans les deux thèmes.

Lit `apps/desktop/ui/style.css` : le bloc `:root` (thème sombre) et le bloc
`:root[data-theme="clair"]`. Vérifie, thème par thème :

- chaque jeton de texte sur chaque fond atteint 4,5:1 (WCAG 2, texte courant),
  3:1 pour les glyphes et le grand texte ;
- chaque jeton de couleur du sombre est redessiné dans le clair (Règle 6 : les
  deux thèmes sont dessinés, aucun n'est dérivé de l'autre) ;
- les initiales d'artiste (`--initiales-encre` sur `--initiales-fond`, une
  saturation et une clarté posées sur la teinte du nom) tiennent 4,5:1 sur
  les 360 teintes ;
- `--txt` tient 4,5:1 sur les fonds de la vue « En cours » : les couleurs
  d'album recalées en OKLCH dans `--album-clarte` (clarté min et max) et sous
  `--album-chroma`, sur toutes les teintes (`js/couleurs-album.js`) ;
- aucune déclaration `color:` hors de `:root` ne code une couleur en dur : le
  texte passe par les jetons, sans quoi ce contrôle ne le verrait pas.

Méthode reprise du test de contraste de Disc Player
(https://github.com/eudj1n/snowsky-disc-player, `tests/unit/contrast.test.mjs`,
licence MIT) : lire les jetons dans la feuille elle-même plutôt que d'en tenir
une copie. Appelé par `scripts/audit-interface.sh` ; seul :

    python3 scripts/contraste-interface.py

Sort avec le code 1 s'il y a une violation.
"""

import colorsys
import math
import re
import sys
from pathlib import Path

FEUILLE = Path(__file__).resolve().parent.parent / "apps/desktop/ui/style.css"

TEXTE, GRAND = 4.5, 3.0

# (texte, fond, seuil, rôle) — les couples que l'interface pose réellement.
FONDS = ("bg", "panel", "panel2")
COUPLES = (
    [(t, f, TEXTE, "texte") for t in ("txt", "mut", "accent", "alerte") for f in FONDS]
    + [("partie-texte", f"partie-{i}", TEXTE, "libellé de partie (Lama)") for i in range(1, 7)]
    # ▶ et ✦ posés sur la pochette : des glyphes, pas du texte courant.
    + [("bg", "txt", GRAND, "glyphe sur bouton"), ("bg", "accent", GRAND, "glyphe sur bouton survolé")]
    # La feuille de partition reste du papier dans les deux thèmes (alphaTab).
    + [("feuille-encre", "feuille", TEXTE, "partition"), ("feuille-mut", "feuille", TEXTE, "partition")]
)


def bloc(css, selecteur):
    """Les jetons `--nom: #rrggbb` du bloc de premier niveau `selecteur { … }`."""
    debut = css.find(f"{selecteur} {{")
    if debut < 0:
        sys.exit(f"bloc introuvable : {selecteur}")
    corps = css[debut : css.index("\n}", debut)]
    return {n: v.lower() for n, v in re.findall(r"--([a-z0-9-]+):\s*(#[0-9a-fA-F]{6})\b", corps)}


def sl(css, selecteur):
    """Les jetons `--nom: S% L%` (saturation, clarté) du bloc `selecteur`."""
    debut = css.find(f"{selecteur} {{")
    corps = css[debut : css.index("\n}", debut)]
    return {n: (float(s) / 100, float(l) / 100) for n, s, l in re.findall(r"--([a-z0-9-]+):\s*([\d.]+)%\s+([\d.]+)%", corps)}


def hsl(teinte, s, l):
    r, v, b = colorsys.hls_to_rgb(teinte / 360, l, s)
    return "#%02x%02x%02x" % (round(r * 255), round(v * 255), round(b * 255))


def oklch_vers_hex(L, C, h):
    """OKLCH → sRGB, chaque canal ramené dans le gamut (comme `couleurs-album.js`)."""
    a, b = C * math.cos(h), C * math.sin(h)
    l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3
    m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3
    s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3

    def canal(c):
        c = 12.92 * c if c <= 0.0031308 else 1.055 * max(c, 0) ** (1 / 2.4) - 0.055
        return round(min(1, max(0, c)) * 255)

    rgb = (
        canal(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
        canal(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
        canal(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s),
    )
    return "#%02x%02x%02x" % rgb


def plage_album(css, selecteur):
    """`--album-clarte: min max` et `--album-chroma: c` d'un bloc, ou None."""
    debut = css.find(f"{selecteur} {{")
    corps = css[debut : css.index("\n}", debut)]
    clarte = re.search(r"--album-clarte:\s*([\d.]+)\s+([\d.]+)", corps)
    chroma = re.search(r"--album-chroma:\s*([\d.]+)", corps)
    if not clarte or not chroma:
        return None
    return float(clarte.group(1)), float(clarte.group(2)), float(chroma.group(1))


def luminance(hexa):
    def canal(i):
        c = int(hexa[i : i + 2], 16) / 255
        return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4

    return 0.2126 * canal(1) + 0.7152 * canal(3) + 0.0722 * canal(5)


def rapport(a, b):
    x, y = sorted((luminance(a), luminance(b)), reverse=True)
    return (x + 0.05) / (y + 0.05)


def main():
    css = FEUILLE.read_text(encoding="utf-8")
    sombre = bloc(css, ":root")
    clair = {**sombre, **bloc(css, ':root[data-theme="clair"]')}
    violations = []

    for nom in sorted(set(sombre) - set(bloc(css, ':root[data-theme="clair"]'))):
        violations.append(f"clair    --{nom} : non redessiné (repris tel quel du sombre)")

    for theme, jetons in (("sombre", sombre), ("clair", clair)):
        print(f"■ {theme}")
        for texte, fond, seuil, role in COUPLES:
            if texte not in jetons or fond not in jetons:
                violations.append(f"{theme:8} --{texte} sur --{fond} : jeton absent")
                continue
            r = rapport(jetons[texte], jetons[fond])
            marque = "ok" if r >= seuil else "ÉCHEC"
            print(f"   {marque:5} {r:5.2f}:1 ≥ {seuil}  --{texte} sur --{fond}  ({role})")
            if r < seuil:
                violations.append(f"{theme:8} --{texte} sur --{fond} : {r:.2f}:1 < {seuil}:1 ({role})")

        # Initiales d'artiste : le pire cas sur les 360 teintes.
        aplats = sl(css, ":root") if theme == "sombre" else {**sl(css, ":root"), **sl(css, ':root[data-theme="clair"]')}
        if "initiales-fond" in aplats and "initiales-encre" in aplats:
            r, pire = min(
                (rapport(hsl(t, *aplats["initiales-encre"]), hsl(t, *aplats["initiales-fond"])), t) for t in range(360)
            )
            marque = "ok" if r >= TEXTE else "ÉCHEC"
            print(f"   {marque:5} {r:5.2f}:1 ≥ {TEXTE}  --initiales-encre sur --initiales-fond  (pire teinte : {pire}°)")
            if r < TEXTE:
                violations.append(f"{theme:8} initiales d'artiste : {r:.2f}:1 < {TEXTE}:1 (teinte {pire}°)")
        else:
            violations.append(f"{theme:8} --initiales-fond / --initiales-encre : jeton absent")

        # Fonds de la vue « En cours » : le pire cas des couleurs d'album recalées.
        plage = plage_album(css, ":root" if theme == "sombre" else ':root[data-theme="clair"]')
        if plage is None:
            violations.append(f"{theme:8} --album-clarte / --album-chroma : jeton absent")
        else:
            lmin, lmax, cmax = plage
            r, fond = min(
                (rapport(jetons["txt"], f), f)
                for L in (lmin, (lmin + lmax) / 2, lmax)
                for C in (0, cmax / 2, cmax)
                for t in range(0, 360, 5)
                for f in [oklch_vers_hex(L, C, math.radians(t))]
            )
            marque = "ok" if r >= TEXTE else "ÉCHEC"
            print(f"   {marque:5} {r:5.2f}:1 ≥ {TEXTE}  --txt sur les fonds d'album  (pire : {fond})")
            if r < TEXTE:
                violations.append(f"{theme:8} --txt sur un fond d'album {fond} : {r:.2f}:1 < {TEXTE}:1")

    # Hors des deux blocs de jetons, une couleur de texte en dur échappe au contrôle.
    sans_jetons = re.sub(r":root(\[data-theme=\"clair\"\])? \{.*?\n\}", "", css, flags=re.S)
    for numero, ligne in enumerate(css.splitlines(), 1):
        # `hsl(var(--teinte) var(--initiales-encre))` passe : ses deux moitiés sont des jetons.
        if re.search(r"(?<![-\w])color:\s*(#[0-9a-fA-F]{3,8}|rgba?\(\s*\d|hsla?\(\s*\d|oklch\(\s*[\d.]|white|black)", ligne) and (
            ligne.strip() in sans_jetons
        ):
            violations.append(f"style.css:{numero} couleur de texte en dur : {ligne.strip()}")

    print()
    if violations:
        print(f"{len(violations)} violation(s) de contraste :")
        print("\n".join(f"   {v}" for v in violations))
        return 1
    print(f"Contraste : {len(COUPLES)} couples, les initiales et les fonds d'album × 2 thèmes, aucune violation.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
