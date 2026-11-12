// SPDX-License-Identifier: GPL-3.0-or-later
/* ------------------------------------- pratiquer : l'accordage de la basse */

// V37 : la tablature est écrite pour l'accordage du morceau (déduit des notes
// par le moteur, `tablature::Accord`, ou choisi à la main) et ne bouge pas
// avec la hauteur des stems. Ce qui change avec la hauteur, c'est l'accordage
// à utiliser pour jouer avec : celui du disque décalé d'autant. Un morceau
// en ½ ton dessous, monté d'½ ton, se joue en standard sur les mêmes frettes.
// Ces fonctions nomment l'un et l'autre ; aucune ne touche à l'interface.

/// Les choix à la main (`tablature::Accord`, kebab-case), dans l'ordre du menu.
const CHOIX = [
  ["standard", "standard"],
  ["demi-ton", "½ ton dessous"],
  ["ton", "1 ton dessous"],
  ["drop-d", "drop D"],
  ["cinq", "5 cordes"],
];

const LETTRES = ["C", "D♭", "D", "E♭", "E", "F", "G♭", "G", "A♭", "A", "B♭", "B"];
const NOMS_FR = ["do", "ré♭", "ré", "mi♭", "mi", "fa", "sol♭", "sol", "la♭", "la", "si♭", "si"];

/// Les formes connues : écarts entre cordes voisines, et la corde grave de
/// référence (MIDI).
const FORMES = [
  { ecarts: "5,5,5", grave: 28, nom: "standard" },
  { ecarts: "7,5,5", grave: 26, nom: "drop" },
  { ecarts: "5,5,5,5", grave: 23, nom: "5 cordes" },
];

/// Le décalage en demi-tons dit en mots : « ½ ton dessous », « 1 ton
/// dessous », « 1 ton ½ au-dessus ».
function decalage(k) {
  const n = Math.abs(k);
  const tons = Math.floor(n / 2);
  const mot = tons === 0 ? "½ ton" : n % 2 ? `${tons} ton${tons > 1 ? "s" : ""} ½` : `${tons} ton${tons > 1 ? "s" : ""}`;
  return `${mot} ${k < 0 ? "dessous" : "au-dessus"}`;
}

/// Le nom d'un accordage (cordes à vide MIDI, de la grave à l'aiguë) :
/// « standard », « ½ ton dessous », « drop D », « 5 cordes, ½ ton dessous »…
function nommer(cordes) {
  if (!cordes?.length) return "standard";
  const ecarts = cordes.slice(1).map((c, i) => c - cordes[i]).join(",");
  const forme = FORMES.find((f) => f.ecarts === ecarts);
  if (!forme) return "particulier";
  const k = cordes[0] - forme.grave;
  if (forme.nom === "drop") return `drop ${LETTRES[((cordes[0] % 12) + 12) % 12]}`;
  if (forme.nom === "standard") return k ? decalage(k) : "standard";
  return k ? `5 cordes, ${decalage(k)}` : "5 cordes";
}

/// Les cordes en lettres, de la grave à l'aiguë : « E♭ A♭ D♭ G♭ ».
function lettres(cordes) {
  return cordes.map((c) => LETTRES[((c % 12) + 12) % 12]).join(" ");
}

/// « ½ ton dessous (E♭ A♭ D♭ G♭) ».
function dire(cordes) {
  return `${nommer(cordes)} (${lettres(cordes)})`;
}

/// L'accordage à utiliser avec la hauteur des stems décalée de `demi`
/// demi-tons : chaque corde décalée d'autant.
function aJouer(cordes, demi) {
  return cordes.map((c) => c + (demi | 0));
}

/// Le nom d'une corde à vide pour l'accordeur, parmi `cordes` : « mi♭ »,
/// « ré grave » quand deux cordes portent la même note (drop D).
function nomCorde(h, cordes) {
  const nom = NOMS_FR[((h % 12) + 12) % 12];
  const memes = cordes.filter((c) => c % 12 === h % 12);
  if (memes.length < 2) return nom;
  return h === Math.min(...memes) ? `${nom} grave` : `${nom} aigu`;
}

export { aJouer, CHOIX, dire, lettres, nomCorde, nommer };
