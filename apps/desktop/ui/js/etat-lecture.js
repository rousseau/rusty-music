// L'état de la lecture : une seule copie de ce que joue le moteur, et, à part,
// ce que l'interface a demandé sans que le moteur l'ait encore confirmé.
//
// **Le moteur fait foi.** `lecture` n'est que son dernier relevé : seuls les
// sondages (`battement`, `battementStems`) et les fonctions qui chargent ou
// coupent les stems l'écrivent. Tenir ailleurs une copie de cet état, c'est
// le défaut du 28 oct. : l'interface croyait les stems partis, le moteur les
// tenait encore (`docs/journal.md`, phase 5).
//
// Ce module ne fait que déclarer : il peut être importé de partout sans
// entrer dans la chaîne d'exécution décrite dans `main.js`.

const lecture = {
  // Le lecteur ordinaire, relu par `battement` (`playback_state`).
  courant: null, // le chemin du morceau chargé
  fini: false, // la file est épuisée, plus rien à reprendre
  positionMs: 0,
  // Le multipiste des stems, relu par `battementStems` (`stems_state`).
  // Les champs autres que `charges` restent vides jusqu'au premier relevé.
  stems: {
    charges: false, // un multipiste est-il chargé côté moteur ?
    positionMs: undefined,
    lueA: undefined, // `performance.now()` du relevé, pour extrapoler
    ancreMs: undefined, // d'où la lecture est repartie
    enPause: undefined,
    dureeMs: undefined,
    deriveMs: undefined,
    latenceMs: undefined,
  },
};

const transport = {
  // Ce que l'utilisateur veut entendre, indépendamment de ce que le moteur a eu
  // le temps de faire. **Le DOM ne peut pas servir de mémoire ici** : le sondage
  // réécrit l'icône, et un second clic lisait alors l'état d'avant le premier —
  // il envoyait l'action inverse, et la lecture repartait au lieu de s'arrêter.
  veutJouer: false,
  // Le libellé bascule tout de suite, sans attendre l'aller-retour : sur un
  // clic, un décalage de quelques dizaines de millisecondes se voit.
  // `ignorerEtatJusqua` empêche un sondage déjà en vol de le remettre à l'ancien
  // état avec une réponse antérieure au clic.
  ignorerEtatJusqua: 0,
  // L'aperçu du transport : le morceau **sélectionné**, affiché quand rien n'est
  // chargé (voir `majApercuTransport`).
  apercu: false,
};

export { lecture, transport };
