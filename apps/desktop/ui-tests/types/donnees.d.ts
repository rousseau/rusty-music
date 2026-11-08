// SPDX-License-Identifier: GPL-3.0-or-later
// `window.__DONNEES__` : la bibliothèque fictive que `outils.js` injecte dans
// la page, et que lisent les réponses calculées de `fixtures.js`. Typée
// d'après le module lui-même, pour que ces réponses soient contrôlées comme
// les autres (sans cette déclaration, TypeScript leur donne un type
// d'erreur, que `controle.ts` ne sait pas écarter).
interface Window {
  __DONNEES__: typeof import("../fixtures.js").donnees;
}
