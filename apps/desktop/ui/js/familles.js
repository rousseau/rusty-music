import "./explorer/chemins.js"; // l'ordre d'exécution : voir main.js
// Les familles : leurs couleurs (sur le canevas et sur le plan) et leur légende,
// partagée par Écouter, Explorer et Découvrir (`rendreFamilles`).
import {
  $, invoke, remonter,
} from "./socle.js";
import { carte, temps } from "./carte.js";
import { tracerItineraire } from "./itineraires.js";
import { GRIS_BATI, majFiltreGL } from "./explorer/carte-gl.js";
import { dessinerCarte, modeTemps } from "./explorer/nuage.js";
import { rejouerChemin } from "./explorer/chemins.js";

/// Les douze teintes de famille, lues dans la feuille de style.
///
/// Sert au **nuage de points**, à la **légende** et au mode Écoute — tout ce
/// qui suit le thème clair/sombre de l'application. Sur la carte MapLibre,
/// voir `couleursFamillesCarte` : les familles s'y calent sur le fond de plan.
function couleursFamilles() {
  return getComputedStyle(document.documentElement)
    .getPropertyValue("--familles")
    .split(",")
    .map((c) => c.trim())
    .filter(Boolean);
}

/// Palettes de familles **de la carte**, calées sur chaque fond de plan.
/// Miroir de `crates/carto/src/palette.rs` — les deux doivent nommer une
/// famille de la même couleur (comme `--familles` ↔ `style.rs` auparavant) :
/// Rust les cuit dans `style-<id>.json`, le JS les rejoue après `setStyle`
/// pour rester cohérent avec l'isolement d'une famille (`majFiltreGL`).
const FAMILLES_VIVES = [
  "#EF8891", "#EC9066", "#D99E46", "#B7AF47", "#88BC6A", "#4EC497",
  "#0CC3C3", "#38BBE6", "#73AEF8", "#A39FF6", "#C892E1", "#E289BD",
];
const FAMILLES_CARTE = {
  sepia: {
    familles: [
      "#B24B58", "#B05323", "#9E6300", "#7E7400", "#4C8227", "#00895D",
      "#00888A", "#0080AC", "#3472BE", "#6B63BC", "#8F56A7", "#A74D83",
    ],
    autres: "#8A7A60",
    bati: "#EADBC4",
  },
  encre: {
    familles: [
      "#B06A70", "#AE7455", "#A08346", "#7C8347", "#579156", "#2E9480",
      "#33908F", "#4C86A0", "#6981AC", "#7D74A6", "#94709E", "#B06C94",
    ],
    autres: "#8C867A",
    bati: "#ECE6DB",
  },
  nuit: { familles: FAMILLES_VIVES, autres: "#8C8C90", bati: "#333333" },
  "bleu-plan": {
    familles: [
      "#E6A6AD", "#E3AB8A", "#D4B47C", "#BCC088", "#9CCA94", "#74CDB4",
      "#6CC9D2", "#86C6E2", "#A2C8F5", "#C0BEF5", "#D6B0E7", "#E6ADD0",
    ],
    autres: "#7C93AE",
    bati: "#234870",
  },
};

/// Le jeu de familles pour la carte : celui du fond de plan actif, ou celui de
/// la feuille de style (`osm-clair`, qui suit le thème de l'appli).
function couleursFamillesCarte() {
  return FAMILLES_CARTE[carte.theme]?.familles ?? couleursFamilles();
}
/// Le gris « fourre-tout » (`#6E6656` par défaut) accordé au fond de plan.
function autresCarte() {
  return FAMILLES_CARTE[carte.theme]?.autres ?? "#6E6656";
}
/// Le gris du bâti (vacant, ou d'une famille non isolée) accordé au fond.
function grisBatiCarte() {
  return FAMILLES_CARTE[carte.theme]?.bati ?? GRIS_BATI;
}

/// Charge `carte.familles` (`[[cluster, nom, effectif]]`) au premier besoin —
/// mêmes données pour la légende d'Explorer et le filtre de l'Écoute.
///
/// Les noms viennent du moteur. Ni le genre le plus fréquent — « Rock » domine
/// six familles sur douze et ne les distinguerait pas — ni le plus
/// caractéristique, qui nommait « Ska Rock » une famille de 4 321 morceaux
/// menée par Bob Marley. Les deux à la fois : voir `nommer_les_familles`.
async function chargerFamilles() {
  if (!carte.familles) {
    try {
      carte.familles = await invoke("families");
    } catch (e) {
      remonter(e, "familles");
      carte.familles = [];
    }
  }
  // Détail du vote à trois sources (`docs/nommage-familles.md`), pour le
  // bloc « Famille » de l'inspecteur — best-effort : son échec n'empêche pas
  // la légende de la carte, déjà servie par `carte.familles` ci-dessus.
  // Rechargé dès qu'il est absent, indépendamment de `carte.familles` : une
  // passe Last.fm invalide le vote sans toucher aux noms de la légende
  // (`carte.familleVotes = null` seul, voir `passeLastfm`).
  if (!carte.familleVotes) {
    try {
      carte.familleVotes = await invoke("families_detail");
    } catch {
      carte.familleVotes = [];
    }
  }
}

/// Rendu commun de la légende des familles : une pastille teintée, un nom, un
/// effectif par famille. `estActive(cluster)` décide du filet d'accent,
/// `auClic(cluster)` réagit au clic. Même geste de multi-sélection (cocher/
/// décocher) dans les trois modes : Explorer isole un ensemble de familles
/// sur la carte, l'Écoute et Découvrir cochent/décochent une famille de leur
/// filtre — cohérence d'interface (`docs/interface-guidelines.md`, principe
/// Nielsen).
///
/// `extension`, optionnel : sur la frise des filiations seulement (§ 3), un
/// second contrôle par ligne — étendre/réduire cette bande verticalement
/// (`estActive`/`auClic` propres à ce contrôle, indépendants de l'isolement).
/// Toujours une ligne (`.famille-ligne`) même sans `extension`, pour que
/// l'ajout du second bouton ne redistribue jamais la mise en page existante.
function rendreFamilles(hote, estActive, auClic, extension) {
  const teintes = couleursFamilles();
  hote.replaceChildren();
  for (const [c, nom, n] of carte.familles ?? []) {
    const ligne = document.createElement("div");
    ligne.className = "famille-ligne";

    const el = document.createElement("button");
    el.className = "famille" + (estActive(c) ? " famille--isolee" : "");
    el.innerHTML = `<span class="famille__pastille"></span>
                    <span></span><span class="famille__n"></span>`;
    el.children[0].style.background = teintes[c % teintes.length] ?? "currentColor";
    // Une famille dont aucun genre ne ressort garde son numéro : mieux vaut un
    // nom neutre qu'un nom faux.
    el.children[1].textContent = nom || `famille ${c + 1}`;
    el.children[2].textContent = n.toLocaleString("fr-FR");
    // Deux colonnes dans le rail (Règle 13) : le compte et le nom coupé se
    // lisent au survol.
    el.title = `${nom || `famille ${c + 1}`} · ${n.toLocaleString("fr-FR")} morceau${n > 1 ? "x" : ""}`;
    el.addEventListener("click", () => auClic(c));
    ligne.appendChild(el);

    if (extension) {
      const actif = extension.estActive(c);
      const bouton = document.createElement("button");
      bouton.className = "famille__etendre" + (actif ? " famille__etendre--actif" : "");
      bouton.title = actif ? "Réduire cette bande" : "Étendre cette bande verticalement";
      bouton.textContent = "↕";
      bouton.addEventListener("click", () => extension.auClic(c));
      ligne.appendChild(bouton);
    }

    hote.appendChild(ligne);
  }
}

/// Le filtre par famille borne aussi le trajet : quand les familles isolées
/// changent, ce qui est déjà tracé se recalcule. Un itinéraire (voirie ou
/// musical) n'a pas de `carte.refaire` : on ne le rejoue que s'il y a un départ
/// et un tracé à l'écran, pour ne pas lancer un calcul que personne n'a demandé.
function rejouerSelonFamilles() {
  if (carte.chemin === "itineraire") {
    if (carte.depart && carte.route) tracerItineraire().catch((e) => remonter(e, "itinéraire"));
  } else if (carte.refaire) {
    rejouerChemin().catch((e) => remonter(e, "chemin"));
  }
}

async function dessinerFamilles() {
  await chargerFamilles();
  rendreFamilles(
    $("familles"),
    (c) => carte.isolees.has(c),
    (c) => {
      if (carte.isolees.has(c)) carte.isolees.delete(c);
      else carte.isolees.add(c);
      dessinerFamilles();
      dessinerCarte();
      majFiltreGL();
      // Le filtre par famille borne aussi le chemin : un chemin déjà tracé se
      // recalcule pour ne garder que les familles isolées (ou les relâcher).
      rejouerSelonFamilles();
    },
    // Zoom vertical ciblé (§ 3) : seulement sur la frise, où une bande peut
    // compresser plusieurs milliers d'albums (Rock · Grunge, par exemple).
    // Ailleurs (Nuage, Carte, Anneau), aucune notion de « bande » n'existe.
    modeTemps()
      ? {
          estActive: (c) => temps.bandeEtendue === c,
          auClic: (c) => {
            temps.bandeEtendue = temps.bandeEtendue === c ? null : c;
            dessinerFamilles();
            dessinerCarte();
          },
        }
      : undefined,
  );
  $("familles-tout").hidden = carte.isolees.size === 0;
}

$("familles-tout").addEventListener("click", () => {
  carte.isolees.clear();
  dessinerFamilles();
  dessinerCarte();
  majFiltreGL();
  rejouerSelonFamilles();
});

export {
  autresCarte, chargerFamilles, couleursFamilles, couleursFamillesCarte, dessinerFamilles,
  grisBatiCarte, rendreFamilles,
};
