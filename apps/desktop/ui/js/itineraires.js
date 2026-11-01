import "./demarrage.js"; // l'ordre d'exécution : voir main.js
import {
  $, invoke, remonter,
} from "./socle.js";
import {
  carte,
} from "./carte.js";
import {
  carteReelle, poserChemin,
} from "./maplibre.js";

/* ------------------------------------------------------------- itinéraires
 *
 * Le moteur vit dans `analysis::reseau` : un seul graphe, plusieurs fonctions
 * de coût. Cette section ne fait que poser la demande et rendre le résultat
 * — dont le **dénivelé**, la popularité le long du trajet, que le document
 * prévoyait pour le profil d'altitude.
 */

let itinProfil = "autoroute";

document.querySelectorAll("[data-profil]").forEach((b) =>
  b.addEventListener("click", () => {
    itinProfil = b.dataset.profil;
    document
      .querySelectorAll("[data-profil]")
      .forEach((s) => s.classList.toggle("segment--actif", s === b));
    retracerItineraireSiPret();
  }),
);

const majReglette = (id, sortie, rendu) => {
  const e = $(id);
  if (!e) return;
  const poser = () => ($(sortie).textContent = rendu(e.value));
  e.addEventListener("input", poser);
  e.addEventListener("change", retracerItineraireSiPret);
  poser();
};
// 0 minute = pas de contrainte de durée (il faut alors une arrivée).
majReglette("itin-minutes", "itin-minutes-val", (v) => (+v > 0 ? `${v} min` : "libre"));

const barresDenivele = (popularite) =>
  popularite.map((p) => "▁▂▃▄▅▆▇█"[Math.min(7, Math.round(p * 7))]).join("");

/// Pose l'itinéraire calculé (un seul trajet — le choix, c'est le profil).
async function poserItineraire(t) {
  const surVoirie = typeof t.distance_m === "number";
  await poserChemin(t.pistes, "aucun itinéraire", t.polyligne || null);
  const distance = surVoirie
    ? `${(t.distance_m / 1000).toFixed(1)} km`
    : `distance ${t.distance_sonique.toFixed(2)}`;
  $("itin-etat").textContent =
    `${t.pistes.length} morceaux · ${(t.duree_ms / 60000).toFixed(0)} min · ` +
    `${distance} · ${barresDenivele(t.popularite)}`;
}

/// L'itinéraire musical (graphe des voisins) — le comportement d'origine, et le
/// repli quand la voirie ne peut pas répondre (pas de ville, morceau sans
/// adresse, ou vue Points).
async function itineraireMusical(minutes) {
  const trajets = await invoke("itineraire", {
    depart: carte.depart.id,
    arrivee: carte.arrivee ? carte.arrivee.id : null,
    profil: itinProfil,
    minutes: minutes > 0 ? minutes : null,
    familles: [...carte.isolees],
  });
  if (!trajets.length) {
    $("itin-etat").textContent = "aucun itinéraire";
    return;
  }
  await poserItineraire(trajets[0]);
}

/// Trace l'itinéraire courant : sur voirie si le plan de ville est actif,
/// musical sinon (ou en repli). Appelé automatiquement quand une borne est
/// posée ou un réglage changé, comme `direct`/`dessiné` — plus besoin du
/// bouton « Tracer » sur la carte de Paris.
async function tracerItineraire() {
  const etat = $("itin-etat");
  if (!carte.depart) {
    etat.textContent = "choisir un morceau de départ sur la carte";
    return;
  }
  const minutes = Number($("itin-minutes").value);
  etat.textContent = carteReelle()
    ? "calcul de l'itinéraire…"
    : "calcul… (le réseau se construit une fois par session)";
  if ($("itin-tracer")) $("itin-tracer").disabled = true;
  try {
    if (!carteReelle()) {
      await itineraireMusical(minutes);
      return;
    }
    const reponse = await invoke("itineraire_voirie", {
      depart: carte.depart.id,
      arrivee: carte.arrivee ? carte.arrivee.id : null,
      profil: itinProfil,
      minutes: minutes > 0 ? minutes : null,
      familles: [...carte.isolees],
      rayonM: null,
    });
    if (reponse.repli) {
      // Pas de ville / tuiles pas prêtes : le musical se suffit à lui-même.
      // Sinon on montre la raison et on enchaîne sur le musical.
      const muet = /aucune ville|pas encore générée/.test(reponse.repli);
      if (!muet) etat.textContent = reponse.repli + " — itinéraire musical";
      await itineraireMusical(minutes);
      return;
    }
    await poserItineraire(reponse.trajets[0]);
  } catch (e) {
    etat.textContent =
      "échec : " + e + (carte.isolees.size ? " — essayer avec plus de familles isolées" : "");
  } finally {
    if ($("itin-tracer")) $("itin-tracer").disabled = false;
  }
}

$("itin-tracer")?.addEventListener("click", tracerItineraire);

/// Rejoue l'itinéraire quand un de ses réglages change — mais seulement s'il y
/// a déjà un départ et qu'on est sur le plan de ville (le musical est trop lent
/// pour un recalcul à chaque cran). Court délai, comme le curseur de bruit.
let attenteItin = null;
function retracerItineraireSiPret() {
  if (carte.chemin !== "itineraire" || !carte.depart || !carteReelle()) return;
  clearTimeout(attenteItin);
  attenteItin = setTimeout(() => tracerItineraire().catch((e) => remonter(e, "itinéraire")), 200);
}

export {
  retracerItineraireSiPret, tracerItineraire,
};
