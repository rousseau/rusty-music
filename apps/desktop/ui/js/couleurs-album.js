// Deux couleurs d'une pochette, pour les fonds de la vue « En cours » — la
// seule exception à la Règle 3 (`docs/interface-guidelines.md` : couleur =
// famille), tranchée le 1ᵉʳ novembre 2026 : fonds seulement, jamais un
// contrôle, un trait ni un point, jamais ailleurs.
//
// Méthode de Disc Player (https://github.com/eudj1n/snowsky-disc-player,
// `src/domain/coverColours.ts`, licence MIT, © eudj1n), réécrite ici. La
// moyenne des pixels rend grises les pochettes sombres ou bariolées ; on range
// plutôt une copie 48 × 48 en cases de couleur : le quasi-noir, le quasi-blanc
// et le gris pèsent peu, la couleur fréquente et saturée gagne, la seconde s'en
// écarte nettement. Chacune est ensuite recalée en OKLCH dans les bornes que
// le thème affiché pose (`--album-clarte`, `--album-chroma` dans `style.css`),
// calculées pour que `--txt` y tienne 4,5:1 sur toutes les teintes
// (`scripts/contraste-interface.py`). Hors de la chaîne d'exécution (voir
// `main.js`) : ce module ne fait que déclarer.

const lineaire = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const gamma = (c) => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);

/// sRGB (0..255) → OKLab (Björn Ottosson).
function versOklab([r, g, b]) {
  const R = lineaire(r / 255), G = lineaire(g / 255), B = lineaire(b / 255);
  const l = Math.cbrt(0.4122214708 * R + 0.5363325363 * G + 0.0514459929 * B);
  const m = Math.cbrt(0.2119034982 * R + 0.6806995451 * G + 0.1073969566 * B);
  const s = Math.cbrt(0.0883024619 * R + 0.2817188376 * G + 0.6299787005 * B);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

/// OKLCH → `rgb(…)`, chaque canal ramené dans le gamut.
function versRgb({ L, C, h }) {
  const a = C * Math.cos(h), b = C * Math.sin(h);
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  const canal = (c) => Math.round(Math.min(1, Math.max(0, gamma(c))) * 255);
  const r = canal(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s);
  const g = canal(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s);
  const bl = canal(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s);
  return `rgb(${r} ${g} ${bl})`;
}

const versLch = (rgb) => {
  const [L, a, b] = versOklab(rgb);
  return { L, C: Math.hypot(a, b), h: Math.atan2(b, a) };
};
const ecart = (x, y) => Math.hypot(x[0] - y[0], x[1] - y[1], x[2] - y[2]);

/// Les deux couleurs (OKLCH) de pixels RGBA : `{ premiere, seconde }`.
function couleursDePixels(pixels) {
  const cases = new Map();
  for (let i = 0; i + 3 < pixels.length; i += 4) {
    if (pixels[i + 3] < 128) continue;
    const r = pixels[i], g = pixels[i + 1], b = pixels[i + 2];
    const cle = ((r >> 4) << 8) | ((g >> 4) << 4) | (b >> 4);
    const c = cases.get(cle) ?? { n: 0, r: 0, g: 0, b: 0 };
    c.n++; c.r += r; c.g += g; c.b += b;
    cases.set(cle, c);
  }
  const total = [...cases.values()].reduce((s, c) => s + c.n, 0) || 1;
  const candidates = [...cases.values()].map((c) => {
    const rgb = [c.r / c.n, c.g / c.n, c.b / c.n];
    const lch = versLch(rgb);
    const terne = lch.C < 0.035 || lch.L < 0.16 || lch.L > 0.94;
    return { lch, lab: versOklab(rgb), poids: (c.n / total) * (0.15 + Math.min(lch.C, 0.25) * 4) * (terne ? 0.08 : 1) };
  }).sort((x, y) => y.poids - x.poids);
  // Une même couleur répartie sur des cases voisines ne compte qu'une fois.
  const fondues = [];
  for (const c of candidates) {
    const proche = fondues.find((f) => ecart(f.lab, c.lab) < 0.06);
    if (proche) proche.poids += c.poids;
    else fondues.push({ ...c });
  }
  fondues.sort((x, y) => y.poids - x.poids);
  const premiere = fondues[0]?.lch ?? { L: 0.6, C: 0, h: 0 };
  const lab = fondues[0]?.lab ?? [0.6, 0, 0];
  // Une pochette d'une seule couleur : la même, plus claire et plus douce.
  const seconde = fondues.find((f) => ecart(f.lab, lab) > 0.13)?.lch
    ?? { L: Math.min(0.9, premiere.L + 0.14), C: premiere.C * 0.6, h: premiere.h + 0.12 };
  return { premiere, seconde };
}

/// Les bornes du thème affiché : clarté [min, max] et chroma maximal (OKLCH).
function bornesDuTheme() {
  const css = getComputedStyle(document.documentElement);
  const [min, max] = css.getPropertyValue("--album-clarte").trim().split(/\s+/).map(Number);
  const chroma = Number(css.getPropertyValue("--album-chroma").trim());
  return { min, max, chroma };
}

/// Une couleur recalée dans les bornes du thème : sa teinte et ce qu'elle a de
/// clarté et de chroma, ramenés dans la plage où `--txt` reste lisible.
function recaler(c, { min, max, chroma }) {
  const L = Math.min(max, Math.max(min, c.L));
  return versRgb({ L, C: Math.min(c.C, chroma), h: c.h });
}

/// Les deux couleurs de fond d'une pochette (`data:` URI), recalées pour le
/// thème affiché : `{ haut, bas }` en `rgb(…)`, ou `null` sans image lisible.
async function couleursPochette(src) {
  if (!src) return null;
  const img = new Image();
  img.src = src;
  try {
    await img.decode();
  } catch {
    return null;
  }
  const cnv = document.createElement("canvas");
  cnv.width = cnv.height = 48;
  const ctx = cnv.getContext("2d", { willReadFrequently: true });
  ctx.drawImage(img, 0, 0, 48, 48);
  const { premiere, seconde } = couleursDePixels(ctx.getImageData(0, 0, 48, 48).data);
  const bornes = bornesDuTheme();
  return { haut: recaler(premiere, bornes), bas: recaler(seconde, bornes) };
}

export {
  couleursDePixels, couleursPochette,
};
