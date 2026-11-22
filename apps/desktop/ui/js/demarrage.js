import "./creer.js"; // l'ordre d'exécution : voir main.js
import {
  $, ecoute, enStems, invoke, navigation, remonter, vue,
} from "./socle.js";
import {
  interpreterIntention,
} from "./lama.js";
import {
  poser,
} from "./ecouter.js";
import { journalCarte } from "./explorer/carte-gl.js";
import {
  chargerFamillesParAlbum, dessinerFamillesEcoute, rafraichirGrille,
} from "./ecouter-familles.js";
import {
  basculerMode,
} from "./modes.js";
import { chargerAccueil } from "./accueil.js";

/* ---------------------------------------------------------- démarrage */

/// Recharge une vue de premier niveau — l'Accueil à l'ouverture, ou
/// Artistes/Albums depuis le commutateur du rail. Aussi le point de retour
/// après un scan, l'oubli d'une racine, ou l'effacement de la recherche : dans
/// ces trois cas `quoi` est omis, et c'est la vue déjà affichée qui revient.
async function charger(quoi = ecoute.sommet.quoi) {
  if (quoi === "accueil") {
    // Au lancement, `charger` part pendant l'exécution de ce module, avant
    // celle d'`accueil.js` (plus loin dans la chaîne, `main.js`) : attendre
    // un tour laisse la chaîne finir, et l'état de l'Accueil exister.
    await null;
    // La page s'affiche sans attendre le sommaire du rail.
    const [, [artistes, racines]] = await Promise.all([
      chargerAccueil(),
      Promise.all([invoke("artists"), invoke("roots")]),
    ]);
    majSommaire(artistes, racines);
    return;
  }
  const jeton = ++ecoute.jetonVue;
  const [artistes, racines] = await Promise.all([invoke("artists"), invoke("roots")]);
  majSommaire(artistes, racines);
  if (quoi === "albums") {
    const albums = await invoke("albums", { artist: null, mbid: null });
    if (jeton === ecoute.jetonVue) poser("albums", "Albums", albums);
  } else if (jeton === ecoute.jetonVue) {
    poser("artistes", "Artistes", artistes);
  }
}

/// Le compte de la bibliothèque, dans le bouton qui y mène ; le détail au survol.
function majSommaire(artistes, racines) {
  const total = racines.reduce((n, r) => n + r.tracks, 0);
  $("sommaire").textContent = total.toLocaleString("fr-FR");
  $("sommaire").parentElement.title =
    `${total.toLocaleString("fr-FR")} morceaux, ${artistes.length.toLocaleString("fr-FR")} artistes`;
}

/// Relit la bibliothèque après un changement (fin d'un scan, fichiers ajoutés
/// ou retirés sous une racine surveillée) sans rien déranger : contrairement à
/// `charger`, ne change pas de vue. Les listes de premier niveau (Artistes ou
/// Albums) sont remplacées **dans** l'objet `ecoute.sommet`, que les vues
/// descendues (albums d'un artiste, pistes d'un album) gardent comme
/// `retour` : y revenir montre la liste fraîche. La grille n'est redessinée
/// que si c'est elle qui est affichée, à la même hauteur de défilement ; la
/// piste choisie, la file et la lecture ne sont pas touchées.
async function rechargerEcoute() {
  const sommet = ecoute.sommet;
  const [artistes, racines] = await Promise.all([invoke("artists"), invoke("roots")]);
  majSommaire(artistes, racines);
  // L'Accueil affiché se recompose (le moteur sait que la bibliothèque a
  // changé) ; ailleurs, il le sera à son prochain affichage.
  if (sommet.quoi === "accueil") {
    if (vue.quoi === "accueil") await chargerAccueil();
    return;
  }
  if (sommet.quoi !== "albums" && sommet.quoi !== "artistes") return;
  sommet.lignes =
    sommet.quoi === "albums" ? await invoke("albums", { artist: null, mbid: null }) : artistes;
  if (vue.retour === null && vue.quoi === sommet.quoi) {
    vue.lignes = sommet.lignes;
    rafraichirGrille();
  }
}

/// Changements signalés par la surveillance (`bibliotheque-modifiee`, émis par
/// `demarrer_surveillance`). Une copie de 200 fichiers arrive en plusieurs
/// regroupements : on attend que la rafale se calme avant de relire. Hors de
/// la grille (Explorer, Bibliothèque…), la relecture attend le retour dans
/// Écouter, Pratiquer ou Créer (`mode-change`, émis par `basculerMode`).
const CALME_BIBLIOTHEQUE_MS = 1500;
let attenteBibliotheque = null;
let bibliothequePerimee = false;

const grilleAffichable = () => navigation.mode === "ecoute" || enStems();

function relireSiAffichee() {
  if (!grilleAffichable()) {
    bibliothequePerimee = true;
    return;
  }
  bibliothequePerimee = false;
  rechargerEcoute().catch((e) => remonter(e, "rechargement d'Écouter"));
}

window.__TAURI__.event.listen("bibliotheque-modifiee", () => {
  clearTimeout(attenteBibliotheque);
  attenteBibliotheque = setTimeout(relireSiAffichee, CALME_BIBLIOTHEQUE_MS);
});

document.addEventListener("mode-change", () => {
  if (bibliothequePerimee) relireSiAffichee();
});

charger("accueil")
  .then(() => chargerFamillesParAlbum())
  .then(() => {
    dessinerFamillesEcoute();
    rafraichirGrille();
  })
  .catch((e) => {
    $("fil-titre").textContent = "Erreur";
    $("sommaire").parentElement.title = String(e);
  });


// Mode imposé par l'environnement, s'il y en a un. Voir `mode_initial`.
invoke("mode_initial")
  .then(async (m) => {
    if (m) await basculerMode(m);
    // Essais sans pilotage clavier (`essai_lama`) : l'affichage, le thème et le
    // prompt viennent de l'environnement, l'application se conduit seule.
    const essai = await invoke("essai_lama");
    if (essai.theme) document.documentElement.dataset.theme = essai.theme;
    if (essai.affichage) document.querySelector(`[data-affichage="${essai.affichage}"]`)?.click();
    if (essai.prompt) {
      $("intention-texte").value = essai.prompt;
      interpreterIntention();
    }
    // Audit de gabarit (`audit.js`, `scripts/audit-interface.sh`) : chargé à la
    // demande, jamais en usage normal.
    if (essai.audit) {
      import("./audit.js")
        .then((m) => m.auditerInterface())
        .catch((e) => journalCarte("AUDIT ERREUR " + (e?.stack ?? e), "warn"));
    }
  })
  .catch(() => {});

export {
  charger, rechargerEcoute,
};
