import "./pratiquer.js"; // l'ordre d'exécution : voir main.js
import {
  $, invoke, sommet,
} from "./socle.js";
import {
  interpreterIntention,
} from "./lama.js";
import {
  poser,
} from "./ecouter.js";
import {
  journalCarte,
} from "./maplibre.js";
import {
  chargerFamillesParAlbum, dessinerFamillesEcoute, rafraichirGrille,
} from "./ecouter-familles.js";
import {
  basculerMode,
} from "./modes.js";

/* ---------------------------------------------------------- démarrage */

/// Recharge une vue de premier niveau — Albums par défaut à l'ouverture, ou
/// Artistes/Albums depuis le commutateur du rail. Aussi le point de retour
/// après un scan, l'oubli d'une racine, ou l'effacement de la recherche : dans
/// ces trois cas `quoi` est omis, et c'est la vue déjà affichée qui revient.
async function charger(quoi = sommet.quoi) {
  const [artistes, racines] = await Promise.all([invoke("artists"), invoke("roots")]);
  if (quoi === "albums") {
    poser("albums", "Albums", await invoke("albums", { artist: null, mbid: null }));
  } else {
    poser("artistes", "Artistes", artistes);
  }

  const total = racines.reduce((n, r) => n + r.tracks, 0);
  $("sommaire").textContent = `${total.toLocaleString("fr-FR")} morceaux\n${artistes.length.toLocaleString("fr-FR")} artistes`;
  $("sommaire").style.whiteSpace = "pre-line";
}

charger("albums")
  .then(() => chargerFamillesParAlbum())
  .then(() => {
    dessinerFamillesEcoute();
    rafraichirGrille();
  })
  .catch((e) => {
    $("fil-titre").textContent = "Erreur";
    $("sommaire").textContent = String(e);
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
  charger,
};
