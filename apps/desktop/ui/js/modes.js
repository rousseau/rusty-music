import "./fond-de-plan.js"; // l'ordre d'exécution : voir main.js
import {
  $, ecoute, enStems, fileAttente, invoke, navigation, remonter, vue,
} from "./socle.js";
import {
  chargerPopulariteFraicheur,
} from "./vues.js";
import {
  dessinerMiniNuage, verifierLama,
} from "./lama.js";
import {
  dernierArtisteOuvert, majAutourArtiste, masquerAutourArtiste, pisteSelectionnee, poser,
} from "./ecouter.js";

import {
  carte,
} from "./carte.js";
import { majVisibiliteIntervalleAnnees } from "./explorer/carte-gl.js";
import { dessinerCarte } from "./explorer/nuage.js";
import { tracerRouteSurCarte } from "./explorer/chemins.js";
import {
  chargerFamillesParAlbum, dessinerFamillesEcoute, rafraichirGrille,
} from "./ecouter-familles.js";
import {
  chargerCarte, majAffichageAnneau, majAffichageTemps, majRailExplorer, poserModeChemin,
} from "./fond-de-plan.js";
import {
  arreterSuiviValise, entrerValise,
} from "./valise.js";
import {
  chargerAPropos, chargerArtistesDecouvrir, chargerFamillesParArtiste, decouverte,
  dessinerFamillesDecouvrir, entrerDecouvrir, rendreFilDecouvrir,
} from "./decouvrir.js";
import {
  chargerDossierDonnees, chargerDumpDiscogs, chargerParametresCarte, chargerStatsBibliotheque,
  chargerVerifications, chargerVocabulaireFamilles, dessinerRacines, majCache,
  majStockageLogiciel, reprendreActualisationEnCours,
} from "./bibliotheque.js";
import {
  arreterStems, edition, majVariantes, poserSourceEdition, prendreLaMain, varianteParDefaut,
} from "./etabli.js";
import { cacherPartition } from "./pratiquer.js";
import { lecture } from "./etat-lecture.js";
import { choisirTheme, themeChoisi } from "./theme.js";
import { appliquerEcran } from "./ecrans.js";

/* ---------------------------------------------------------- modes */

async function basculerMode(mode) {
  const explorer = mode === "explorer";
  // Pratiquer et Créer : deux modes du rail sur les mêmes stems du morceau
  // ouvert. Le mode fixe l'usage (la partition au centre, ou la pile) ;
  // passer de l'un à l'autre garde les stems chargés.
  const editer = enStems(mode);
  const bibliotheque = mode === "bibliotheque";
  const decouvrir = mode === "decouvrir";
  const valise = mode === "valise";
  // Quitter Découvrir éteint les pastilles « nouveau » : ce qu'on vient de
  // voir n'est plus une nouveauté au prochain passage.
  if (navigation.mode === "decouvrir" && mode !== "decouvrir") {
    invoke("decouvrir_tout_vu").catch((e) => remonter(e, "découvrir"));
  }
  document.querySelectorAll(".mode").forEach((b) =>
    b.classList.toggle("mode--actif", b.dataset.mode === mode),
  );
  navigation.mode = mode;
  majVisibiliteIntervalleAnnees();
  if (editer) {
    edition.usage = mode;
    varianteParDefaut();
    majVariantes();
  }
  // Entrer dans Pratiquer ou Créer avec des stems déjà affichés doit les rendre
  // audibles : c'est ce qu'on vient y faire.
  if (editer) prendreLaMain().catch((e) => remonter(e, "stems"));
  // Ce que l'écran montre (blocs du rail, contrôles de l'en-tête, vues du
  // centre) : la table de `ecrans.js`, appliquée ici ; les fonctions d'état
  // de chaque écran y demandent ensuite ce qui dépend de leur état.
  const enLama = explorer && carte.affichage === "lama";
  appliquerEcran();
  if (!valise) arreterSuiviValise();
  // Le bandeau « autour de cet artiste » suit l'artiste consulté dans
  // Écouter : Pratiquer et Créer reprennent le navigateur, pas ce bandeau.
  // Revenir à l'Écoute le restaure (`dernierArtisteOuvert`, plus bas).
  if (mode !== "ecoute") masquerAutourArtiste();
  // Le rail d'Explorer selon l'affichage : Lama ou carte, familles, chemin.
  majRailExplorer();
  if (!editer) {
    cacherPartition();
  } else {
    // Entrer dans le mode : on repart du sélecteur (ou de l'établi si des
    // stems sont déjà chargés — `majEtatEditer` le voit).
    // Un morceau déjà sélectionné (dans Écouter, sur la carte…) est la source :
    // on entre directement en « séparer », sans avoir à le lire ni à le
    // re-choisir. Sans sélection, on repart du sélecteur.
    edition.montrerChoix = false;
    edition.sourceChoisie = pisteSelectionnee !== null;
  }

  // Sortir de Pratiquer et de Créer rend la sortie au lecteur ordinaire : garder les
  // stems chargés tiendrait 186 Mo et une sortie audio pour rien.
  if (!editer && lecture.stems.charges) await arreterStems();

  if (explorer) {
    // `false` : ré-affirme juste l'état du rail en entrant dans Explorer, ne
    // doit pas relancer un chemin depuis un vieux départ/arrivée — la
    // visualisation doit rester indépendante de la playlist en cours.
    poserModeChemin(carte.chemin, false);
    majAffichageTemps();
    majAffichageAnneau();
    await chargerCarte();
    // Rattrape une file lancée depuis l'Écoute avant la première visite
    // d'Explorer : la carte n'avait alors pas encore ses points pour y
    // tracer quoi que ce soit.
    tracerRouteSurCarte(fileAttente.pistes);
    if (enLama) {
      $("fil-titre").textContent = "Lama";
      dessinerMiniNuage();
      verifierLama();
    }
  } else if (bibliotheque) {
    $("fil-titre").textContent = "Bibliothèque";
    $("fil-compte").textContent = "";
    await dessinerRacines();
    majCache().catch((e) => remonter(e, "cache"));
    chargerDossierDonnees().catch((e) => remonter(e, "dossier de données"));
    majStockageLogiciel().catch((e) => remonter(e, "stockage du logiciel"));
    chargerDumpDiscogs().catch((e) => remonter(e, "dump Discogs"));
    chargerStatsBibliotheque().catch((e) => remonter(e, "statistiques"));
    reprendreActualisationEnCours().catch((e) => remonter(e, "actualisation"));
    chargerVerifications().catch((e) => remonter(e, "vérifications"));
    chargerParametresCarte().catch((e) => remonter(e, "paramètres de la carte"));
    chargerVocabulaireFamilles().catch((e) => remonter(e, "vocabulaire des familles"));
    chargerPopulariteFraicheur().catch((e) => remonter(e, "popularité"));
    chargerAPropos().catch((e) => remonter(e, "à propos"));
  } else if (valise) {
    $("fil-titre").textContent = "Valise";
    $("fil-compte").textContent = "";
    entrerValise().catch((e) => remonter(e, "valise"));
  } else if (decouvrir) {
    $("fil-titre").textContent = "Découvrir";
    $("fil-compte").textContent = "";
    chargerArtistesDecouvrir().catch((e) => remonter(e, "découvrir"));
    entrerDecouvrir().catch((e) => remonter(e, "découvrir"));
    // Filtre par famille du fil : les données au premier passage, le rendu à
    // chaque entrée (les familles ont pu être renommées depuis).
    {
      const rendre = () => {
        dessinerFamillesDecouvrir();
        rendreFilDecouvrir();
      };
      if (decouverte.famillesParArtiste) rendre();
      else
        chargerFamillesParArtiste()
          .then(rendre)
          .catch((e) => remonter(e, "familles des artistes"));
    }
  } else {
    poser(vue.quoi, vue.titre, vue.lignes, vue.retour);
    // Pratiquer et Créer travaillent sur la sélection courante : on la relit à
    // chaque entrée plutôt que de la mémoriser, elle a pu changer depuis.
    // `poserSourceEdition` appelle `majEtatEditer`, qui range le centre.
    if (editer) await poserSourceEdition();
    // Filtre par famille de l'Écoute : les données au premier passage, le
    // rendu à chaque entrée (les familles ont pu être renommées depuis).
    if (mode === "ecoute") {
      const rendre = () => {
        dessinerFamillesEcoute();
        rafraichirGrille();
      };
      if (ecoute.famillesParAlbum) rendre();
      else chargerFamillesParAlbum().then(rendre).catch((e) => remonter(e, "familles des albums"));
      // Revenir à l'Écoute sur la même grille d'artiste restaure son
      // bandeau — `poser` ci-dessus venait de le masquer sans rien savoir de
      // l'artiste (voir `dernierArtisteOuvert`).
      if (vue.quoi === "albums" && vue.retour !== null && vue.titre === dernierArtisteOuvert.nom) {
        majAutourArtiste(dernierArtisteOuvert.nom, dernierArtisteOuvert.mbid);
      }
    }
  }
}

document.querySelectorAll(".mode").forEach((b) => {
  if (!b.disabled) b.addEventListener("click", () => basculerMode(b.dataset.mode));
});

/* ---------------------------------------------------------- thème */

// Le choix de Bibliothèque (Sombre / Clair / Système) ; la palette (⌘K) bascule
// aussi, d'où l'écoute de `theme-choisi` pour garder le segment juste.
function majChoixTheme(choix = themeChoisi()) {
  document.querySelectorAll("[data-choix-theme]").forEach((b) =>
    b.classList.toggle("segment--actif", b.dataset.choixTheme === choix),
  );
}
document.querySelectorAll("[data-choix-theme]").forEach((b) =>
  b.addEventListener("click", () => choisirTheme(b.dataset.choixTheme)),
);
document.addEventListener("theme-choisi", (e) => majChoixTheme(e.detail));
majChoixTheme();

// Les canevas lisent leurs couleurs dans les jetons au moment de dessiner : un
// changement de thème les redessine (le HTML, lui, suit la feuille seul).
document.addEventListener("theme", () => {
  if (navigation.mode === "explorer") {
    if (carte.affichage === "lama") dessinerMiniNuage();
    else dessinerCarte();
  }
  if (navigation.mode === "ecoute") dessinerFamillesEcoute();
  if (navigation.mode === "decouvrir") dessinerFamillesDecouvrir();
});

export {
  basculerMode,
};
