import "./fond-de-plan.js"; // l'ordre d'exécution : voir main.js
import {
  $, ecoute, enStems, fileAttente, invoke, navigation, remonter, vue, vueEnGrille,
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
import {
  dessinerCarte, majVisibiliteIntervalleAnnees, tracerRouteSurCarte,
} from "./maplibre.js";
import {
  chargerFamillesParAlbum, dessinerFamillesEcoute, majBarreTri, rafraichirGrille,
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
  edition,
} from "./editer.js";
import {
  arreterStems, cacherPartition, majVariantes, poserSourceEdition, prendreLaMain, varianteParDefaut,
} from "./pratiquer.js";
import { lecture } from "./etat-lecture.js";
import { choisirTheme, themeChoisi } from "./theme.js";

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
  // Le mode courant, lisible par la feuille de style : le repère A–Z (propre aux
  // listes de l'Écoute) s'y masque par une règle CSS, que le code asynchrone qui
  // réaffiche `#index-alpha` (rendu de la grille, familles chargées…) ne peut
  // plus contourner.
  document.body.dataset.mode = mode;
  majVisibiliteIntervalleAnnees();
  if (editer) {
    edition.usage = mode;
    varianteParDefaut();
    majVariantes();
  }
  // Entrer dans Pratiquer ou Créer avec des stems déjà affichés doit les rendre
  // audibles : c'est ce qu'on vient y faire.
  if (editer) prendreLaMain().catch((e) => remonter(e, "stems"));
  // Explorer a deux centres : la carte (nuage, carte, temps, anneau — un canevas)
  // et le Lama (le panneau dédié à l'interprète).
  const enLama = explorer && carte.affichage === "lama";
  $("carte-vue").hidden = !explorer || enLama;
  $("lama-vue").hidden = !enLama;
  $("bibliotheque-vue").hidden = !bibliotheque;
  $("decouvrir-vue").hidden = !decouvrir;
  $("valise-vue").hidden = !valise;
  $("bloc-valise").hidden = !valise;
  if (!valise) arreterSuiviValise();
  $("liste").hidden = explorer || bibliotheque || decouvrir || valise || vueEnGrille();
  $("grille").hidden = explorer || bibliotheque || decouvrir || valise || !vueEnGrille();
  // Propre à l'Écoute : les branches Explorer/Bibliothèque/Découvrir
  // ci-dessous ne rappellent pas `poser` (qui le masquerait sinon) — sans
  // cette ligne, le bandeau restait affiché en quittant l'artiste consulté.
  if (mode !== "ecoute") masquerAutourArtiste();
  // L'ordre de la grille d'albums n'existe qu'en Écoute : `poser` le rétablit
  // en y revenant, mais ne court pas pour les autres modes.
  majBarreTri(mode !== "ecoute");
  $("retour").hidden = explorer || bibliotheque || decouvrir || valise || vue.retour === null;
  // Le repère A–Z est celui des listes d'artistes et d'albums de l'Écoute :
  // resté affiché en passant à Explorer, il se dessinait par-dessus la carte
  // et le Lama, dans la marge de droite — qui paraissait alors plus étroite que
  // celle de gauche. `poser` le rétablit en revenant à l'Écoute.
  $("index-alpha").hidden = $("index-alpha").hidden || explorer || bibliotheque || decouvrir || valise;
  $("index-annees").hidden = $("index-annees").hidden || explorer || bibliotheque || decouvrir || valise;
  // « Choisir l'édition… » appartient aux pistes d'un album ouvert en Écouter :
  // posé par `poser`, il restait dans l'en-tête des autres écrans. `poser` le
  // rétablit en revenant à l'Écoute.
  if (mode !== "ecoute") $("choisir-edition").hidden = true;
  $("bloc-vue-lib").hidden = explorer || editer || bibliotheque || decouvrir || valise;
  // « Filtrer » estompe ce qui est affiché — la carte en Explorer, la grille ou
  // la liste en Écouter, en Pratiquer et en Créer. Bibliothèque, Découvrir et Valise n'ont
  // rien qu'il filtrerait : masqué (la palette, ⌘K, reste partout).
  // (`majRailExplorer` cache aussi ce champ dans le Lama, `majEtatEditer` en
  // Pratiquer et Créer hors de l'état « choisir ».)
  $("bloc-chercher").hidden = bibliotheque || decouvrir || valise || enLama;
  $("bloc-colorer").hidden = !explorer;
  $("bloc-demix").hidden = !editer;
  if (!editer) {
    $("bloc-usage").hidden = true;
    $("bloc-partitions").hidden = true;
    cacherPartition();
  }
  $("bloc-chemin").hidden = !explorer;
  // Le champ d'intention vit dans le panneau Lama (`#lama-vue`) : il n'est
  // visible qu'avec lui, plus sur les quatre autres affichages où il n'avait
  // aucun lien avec ce qu'on y voyait.
  majRailExplorer();
  // Le filtre par famille de l'Écoute : `majBlocFamillesEcoute` le rallume si
  // la grille d'albums est à l'écran et la carte calculée.
  $("bloc-familles-ecoute").hidden = true;
  // Idem pour le filtre par famille de Découvrir (`majBlocFamillesDecouvrir`).
  if (!decouvrir) $("bloc-familles-decouvrir").hidden = true;
  $("bloc-decouvrir").hidden = !decouvrir;
  // La carte de séparation, l'établi et la barre d'outils des stems : `majEtatEditer`
  // en décide selon l'avancement (`docs/ui-spec-editeur.md`). Hors Pratiquer et
  // Créer, tout est masqué.
  if (!editer) {
    $("editer-separer").hidden = true;
    $("editer-etabli").hidden = true;
    $("dock").hidden = true;
    $("bloc-stem").hidden = true;
    $("editer-candidat").hidden = true;
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
