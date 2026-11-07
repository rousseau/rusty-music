// SPDX-License-Identifier: GPL-3.0-or-later
// Essai C4 (synthèse de novembre, O7) : chaque commande dont le type est
// exporté de Rust (`commandes.ts`, écrit par le test `types_commandes` du
// bureau) doit avoir dans `fixtures.js` une réponse figée de la même forme —
// une valeur, ou une fonction qui en rend une. `npm run types`.
import type { commands } from "./commandes";
import fx = require("../fixtures.js");

type Commandes = typeof commands;
// Un littéral JS `["Essai", 3]` s'infère en `(string | number)[]`, jamais en
// tuple : côté attendu, les tuples s'élargissent de même (seule leur longueur
// échappe au contrôle).
type Large<T> = T extends readonly (infer U)[]
  ? Large<U>[]
  : T extends object
    ? { [P in keyof T]: Large<T[P]> }
    : T;
type Retour<K extends keyof Commandes> = Large<Awaited<ReturnType<Commandes[K]>>>;
type Figee<K extends keyof Commandes> = Retour<K> | ((a: any, e: any) => Retour<K>);

export const controle: { [K in keyof Commandes]: Figee<K> } = fx.fixtures;
