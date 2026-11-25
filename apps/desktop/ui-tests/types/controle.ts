// SPDX-License-Identifier: GPL-3.0-or-later
// Essai C4 (synthèse de novembre, O7), étendu par V5 : chaque commande dont
// le type est exporté de Rust (`commandes.ts`, écrit par le test
// `types_commandes` du bureau) doit avoir dans `fixtures.js` une réponse
// figée de la même forme — une valeur, ou une fonction qui en rend une.
// `npm run types`.
import type { commands, PhaseTrackpad } from "./commandes";
import fx = require("../fixtures.js");

type Commandes = typeof commands;
type Fixtures = typeof fx.fixtures;

// Un littéral JS `["Essai", 3]` s'infère en `(string | number)[]`, jamais en
// tuple : côté attendu, les tuples s'élargissent de même (seule leur longueur
// échappe au contrôle).
type Large<T> = T extends readonly (infer U)[]
  ? Large<U>[]
  : T extends object
    ? { [P in keyof T]: Large<T[P]> }
    : T;
type Attendu<K extends keyof Commandes> = Awaited<ReturnType<Commandes[K]>>;
type Retour<K extends keyof Commandes> = Large<Attendu<K>>;
// Une `Error` figée : le faux moteur la lève (`faux-tauri.js`), comme une
// commande qui échoue.
type Figee<K extends keyof Commandes> = Retour<K> | ((a: any, e: any) => Retour<K>) | Error;

// 1. Ce qui manque ou n'a pas le bon type : la réponse figée doit être
//    assignable au type de Rust.
export const controle: { [K in keyof Commandes]: Figee<K> } = fx.fixtures;

// 2. Ce qui est en trop : un champ que Rust a retiré (ou renommé) mais que
//    `fixtures.js` garde. L'assignation ci-dessus ne le voit pas — pas de
//    contrôle d'excédent hors d'un littéral —, d'où ce parcours : chaque
//    champ de la réponse figée doit exister dans le type attendu, à toute
//    profondeur. L'erreur nomme le chemin du champ en trop
//    (`"balade_passee.pistes"`). Une réponse calculée qui rend une valeur
//    de l'état partagé (`e.balade`) est `any` : elle échappe au contrôle.
type Rendu<K extends keyof Fixtures> = Fixtures[K] extends (...a: any[]) => infer R ? R : Fixtures[K];
type EstAny<T> = 0 extends 1 & T ? true : false;
// Les clés et les champs d'une union, membre à membre (`T | null` compris).
type Cles<T> = T extends unknown ? keyof T : never;
type Champ<T, C extends PropertyKey> = T extends unknown ? (C extends keyof T ? T[C] : never) : never;
type Element<T> = T extends readonly (infer U)[] ? U : never;
type Surplus<A, R, Chemin extends string> =
  EstAny<R> extends true
    ? never
    : R extends readonly (infer U)[]
      ? Surplus<Element<A>, U, `${Chemin}[]`>
      : R extends object
        ? {
            [C in keyof R & string]-?: C extends Cles<A>
              ? Surplus<Champ<A, C>, R[C], `${Chemin}.${C}`>
              : `${Chemin}.${C}`;
          }[keyof R & string]
        : never;
// Commande par commande. Une réponse calculée tirée de `window.__DONNEES__`
// ou de l'état partagé est `any` : écartée ici, sans quoi elle noierait
// l'union. Et indexer `ParCommande` par l'union de ses clés ne suffit pas :
// TypeScript le réécrit en `Surplus<…union…>`, qui mêle les commandes ; une
// condition distributive les évalue une à une.
type ParCommande = {
  [K in keyof Commandes & keyof Fixtures & string]: EstAny<Rendu<K>> extends true
    ? never
    : Surplus<Attendu<K>, Rendu<K>, K>;
};
type UneAUne<K> = K extends keyof ParCommande ? ParCommande[K] : never;
type EnTrop = UneAUne<keyof ParCommande>;

export const sansChampEnTrop: [EnTrop] extends [never] ? true : EnTrop = true;

// 3. Les événements figés (`fixtures.js`, `evenements`) : de la forme que
//    Rust émet.
export const evenements: { scratch_phase: Record<string, PhaseTrackpad> } = fx.evenements;
