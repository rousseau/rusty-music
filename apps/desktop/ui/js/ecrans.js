// Ce que chaque écran montre : la table des blocs du rail, des contrôles de
// l'en-tête et des vues du centre (plus le bloc du stem dans l'inspecteur et
// la barre de l'établi en bas), et la seule fonction qui en écrit `hidden`.
//
// Un élément de la table n'est décidé qu'ici. Avant elle, `basculerMode`
// tenait 26 affectations au cas par cas, et chaque fonction d'état (`poser`,
// `majEtatEditer`, le Lama…) les siennes : un élément posé par l'une restait
// affiché dans l'écran suivant, que l'autre ne savait pas devoir masquer
// (bandeau d'artiste, repère A–Z, tri, « Choisir l'édition… » :
// `docs/synthese-novembre-2026.md`, O2).
//
// Deux valeurs : `TOUJOURS`, l'élément est là dès que l'écran l'est ;
// `SELON`, il en fait partie mais dépend de l'état interne de l'écran
// (sélecteur, séparer ou établi ; Lama ou carte ; vue d'Écouter…). La
// fonction qui gère cet état ne touche pas à l'élément : elle le *demande*
// (`demander`), pour l'écran courant, et la table décide. Un élément absent
// de l'écran est masqué, quoi qu'on ait demandé.
//
// Ce module ne fait que déclarer : il peut être importé de partout sans entrer
// dans la chaîne d'exécution décrite dans `main.js`.
import { navigation } from "./socle.js";

const TOUJOURS = "toujours";
const SELON = "selon";

/// Le navigateur d'Écouter (grille ou liste, repères, retour), que Pratiquer
/// et Créer reprennent dans leur état « choisir ».
const NAVIGATEUR = {
  retour: SELON, liste: SELON, grille: SELON, "index-alpha": SELON, "index-annees": SELON,
  "autour-artiste": SELON,
};

/// Pratiquer et Créer : les mêmes stems, le même parcours (choisir, séparer,
/// établi), décidé par `majEtatEditer`.
const ETABLI = {
  ...NAVIGATEUR,
  // rail
  "bloc-chercher": SELON, "bloc-demix": TOUJOURS,
  // en-tête
  "editer-candidat": SELON,
  // centre, inspecteur, bas
  "editer-separer": SELON, "editer-etabli": SELON, "bloc-stem": SELON, dock: SELON,
};

const ECRANS = {
  ecoute: {
    ...NAVIGATEUR,
    "bloc-chercher": TOUJOURS, "bloc-vue-lib": TOUJOURS, "bloc-familles-ecoute": SELON,
    "choisir-edition": SELON, "tri-albums": SELON,
  },
  pratiquer: { ...ETABLI, "bloc-usage": SELON, "bloc-partitions": SELON },
  creer: ETABLI,
  explorer: {
    "bloc-chercher": SELON, "bloc-colorer": TOUJOURS, "bloc-familles": SELON,
    "carte-vue": SELON, "lama-vue": SELON,
  },
  decouvrir: {
    "bloc-decouvrir": TOUJOURS, "bloc-familles-decouvrir": SELON, "decouvrir-vue": TOUJOURS,
  },
  valise: { "bloc-valise": TOUJOURS, "valise-vue": TOUJOURS },
  bibliotheque: { "bibliotheque-vue": TOUJOURS },
};

/// Tous les éléments que la table décide, quel que soit l'écran.
const ELEMENTS = [...new Set(Object.values(ECRANS).flatMap(Object.keys))];

/// Ce que les fonctions d'état ont demandé, écran par écran. Rien demandé :
/// masqué.
const demandes = Object.fromEntries(Object.keys(ECRANS).map((e) => [e, new Map()]));

function decider(id, ecran = navigation.mode) {
  const place = ECRANS[ecran]?.[id];
  const montre = place === TOUJOURS || (place === SELON && demandes[ecran].get(id) === true);
  const el = document.getElementById(id);
  if (el) el.hidden = !montre;
}

/// Une fonction d'état demande (ou retire) un élément de l'écran courant ;
/// hors de cet écran, la demande ne change rien.
function demander(id, oui) {
  demandes[navigation.mode]?.set(id, !!oui);
  decider(id);
}

/// Applique la table à l'écran courant : `basculerMode` à chaque entrée.
function appliquerEcran() {
  for (const id of ELEMENTS) decider(id);
}

export {
  ECRANS, SELON, TOUJOURS, appliquerEcran, demander,
};
