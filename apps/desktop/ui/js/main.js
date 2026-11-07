// Point d'entrée de l'interface : des modules ES natifs, sans bundler
// (`CLAUDE.md`), découpés dans l'ancien `app.js` par mode.
//
// L'ordre d'exécution reste celui de l'ancien fichier unique : chaque module
// importe d'abord le précédent, et l'on n'importe ici que le dernier. Le
// moteur de modules descend la chaîne jusqu'au premier (`socle.js`), puis
// exécute en remontant : socle, vues, lama… palette, en-cours, editions. Les
// importations vers un module plus loin dans la chaîne ne changent rien à cet
// ordre (il est déjà en cours de chargement) et les fonctions qu'elles nomment
// existent dès avant l'exécution. Tout s'exécute d'un seul tenant : aucun rappel
// (promesse, minuteur) ne passe avant la fin du dernier module.
//
// La chaîne, du premier exécuté au dernier :
//   socle, vues, lama, ecouter, lecteur, carte, explorer/carte-gl,
//   explorer/nuage, explorer/temps, explorer/anneau, explorer/chemins,
//   familles, ecouter-familles, fond-de-plan, modes, valise, decouvrir,
//   bibliotheque, etabli, pratiquer, creer, demarrage, itineraires, balade,
//   aura, traces, relief, lieux, autotest-carte, palette, en-cours, editions.
//
// Ajouter un module : l'insérer dans la chaîne (son `import` de tête, celui
// du suivant) ; le dernier reste celui qu'importe ce fichier. Un module qui
// ne fait que déclarer (`etat-lecture.js`) n'a pas d'ordre à tenir : il reste
// hors de la chaîne.
import "./editions.js";
