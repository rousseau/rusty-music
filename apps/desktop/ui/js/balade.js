import "./itineraires.js"; // l'ordre d'exécution : voir main.js
import {
  $, fileAttente, invoke, navigation, remonter,
} from "./socle.js";
import {
  carte, cnv, ctx,
} from "./carte.js";
import {
  carteGL, couleursFamilles, couleursFamillesCarte, dessinerCarte, pointSous,
  tracerRouteSurCarte, versEcran,
} from "./maplibre.js";
import {
  demarrerLecture, dessinerFile,
} from "./lecteur.js";
import { lecture } from "./etat-lecture.js";
import { marquerApercu } from "./aura.js";
import { rafraichirTraces } from "./traces.js";
import { relief } from "./relief.js";

/* ------------------------------------------------- la balade (P1)
 *
 * `docs/plan-balade.md`, P1 : la lecture est une marche. Le moteur choisit le
 * morceau suivant depuis la position courante (`explorer/balade.rs`) ; ce
 * module ne fait que l'entrée (« Flâner d'ici », au clic droit sur un morceau
 * d'Explorer), la sortie (« Arrêter la balade ») et le dessin : le
 * promeneur, qui avance le long de la rue au rythme de la lecture, et la trace
 * de ce qui a déjà été parcouru.
 *
 * Rien n'est calculé à chaque battement côté moteur : le sondage de lecture
 * existant (`battement`) donne la position, `balade_etat` n'est relu qu'au
 * changement de morceau et toutes les 2 s.
 *
 * Le carrefour (P2) : quand plusieurs voies s'offrent au bout du morceau, un
 * panneau de deux ou trois cartes, ancré à côté du promeneur, pendant les 30
 * dernières secondes (dès le début pour un morceau de moins de 40 s). La
 * pente par défaut est marquée d'un anneau qui se vide jusqu'à ce qu'elle
 * soit prise, 10 s avant la fin : là, le suivant est figé dans la file du
 * lecteur, et le panneau se grise en le disant. Gestes : 1, 2, 3, les flèches
 * (la voie dans cette direction sur l'écran), un clic sur la carte ou sur la
 * rue, Échap pour masquer le panneau de ce carrefour. Survoler une carte fait
 * entendre 5 s de son morceau.
 */

/// Le panneau se présente pendant les dernières secondes du morceau…
const CARREFOUR_AVANT_FIN_MS = 30_000;
/// … le suivant est figé à 10 s de la fin (`FIGE_AVANT_FIN` côté moteur)…
const FIGE_AVANT_FIN_MS = 10_000;
/// … et un morceau plus court se voit proposer le choix dès son début.
const MORCEAU_COURT_MS = 40_000;
/// La jauge proche ↔ ailleurs : distance cosinus CLAP ramenée sur [0, 1]. Un
/// voisin à l'oreille est vers 0,03, deux morceaux pris au hasard vers 0,19
/// (`continuite_balade`, P0).
const DISTANCE_AILLEURS = 0.25;
/// Le tracé de chaque voie sur la carte : ses premiers mètres.
const VOIE_TRACEE_M = 60;
/// Un clic à moins de tant de pixels d'une voie tracée la choisit.
const VOIE_CLIC_PX = 10;
/// Délai de survol avant l'extrait : un passage de la souris n'est pas une
/// demande.
const EXTRAIT_DELAI_MS = 350;

const INTERVENTIONS = ["porter", "proposer", "demander"];
/// L'allure de la balade : la pente par défaut au carrefour (P3).
const ALLURES = ["rester", "deriver", "aventurer", "niveau", "gravir", "redescendre"];

const balade = {
  vue: null, // dernière réponse de `balade_etat`, `null` sans balade
  luA: 0, // `performance.now()` de la dernière lecture
  courantVu: null, // le chemin joué lors de la dernière lecture
  dessineA: 0,
  battuA: 0, // dernier appel de `majBalade` par le sondage du lecteur
  // Le degré d'intervention et l'allure (la pente de P3), gardés avec les
  // autres réglages de la carte.
  intervention: lireReglage("balade-intervention", INTERVENTIONS, "proposer"),
  allure: lireReglage("balade-allure", ALLURES, "deriver"),
  masque: null, // numéro du carrefour masqué par Échap
  cle: "", // ce que montrent les cartes du panneau, pour ne les refaire qu'au changement
  voiesEcran: [], // [{ i, points: [[x, y]…] }] : le tracé des voies, pour le clic
  survol: null, // minuterie de l'extrait
};

function lireReglage(cle, valeurs, defaut) {
  try {
    const v = localStorage.getItem(cle);
    return valeurs.includes(v) ? v : defaut;
  } catch {
    return defaut;
  }
}

/// Lance la balade depuis le morceau `id`.
async function flanerDici(id) {
  if (!Number.isFinite(id)) return;
  fermerMenuBalade();
  try {
    await demarrerLecture(async () => {
      balade.vue = await invoke("balade_demarrer", {
        depart: id,
        intervention: balade.intervention,
        allure: balade.allure,
        grandeur: relief.grandeur,
      });
    });
  } catch (err) {
    remonter(err, "balade_demarrer");
    return;
  }
  balade.luA = performance.now();
  // La route d'un itinéraire précédent n'a plus rien à dire : la balade trace
  // sa propre ligne.
  tracerRouteSurCarte(null);
  try {
    fileAttente.pistes = await invoke("file_courante");
    dessinerFile();
  } catch (err) {
    remonter(err, "file courante (balade)");
  }
  majBandeau();
  redessiner(true);
}

async function arreterBalade() {
  try {
    await invoke("balade_arreter");
  } catch (err) {
    remonter(err, "balade_arreter");
  }
  balade.vue = null;
  rafraichirTraces();
  majBandeau();
  majCarrefour();
  redessiner(true);
}

async function relireBalade() {
  balade.luA = performance.now();
  balade.courantVu = lecture.courant;
  const avant = balade.vue?.pistes.length ?? 0;
  try {
    balade.vue = await invoke("balade_etat");
  } catch (err) {
    remonter(err, "balade_etat");
    return;
  }
  // Un pas de plus, ou la balade finie : les balades passées et les sentiers
  // battus le montrent (P5).
  if ((balade.vue?.pistes.length ?? 0) !== avant) rafraichirTraces();
  majBandeau();
  majCarrefour();
}

/// Appelé à chaque battement du lecteur.
function majBalade(parLeSondage = true) {
  const maintenant = performance.now();
  if (parLeSondage) balade.battuA = maintenant;
  const relire =
    lecture.courant !== balade.courantVu || (balade.vue && maintenant - balade.luA > 2000);
  if (relire) relireBalade().then(() => redessiner(true));
  else {
    majCarrefour();
    redessiner(false);
  }
}

// Le sondage du lecteur s'arrête quand rien ne joue : en mode « demander »,
// la balade attend dans le silence, et le panneau doit rester vivant.
setInterval(() => {
  if (balade.vue && performance.now() - balade.battuA > 1500) majBalade(false);
}, 1000);

/// La carte, au plus une fois par seconde : le promeneur avance de quelques
/// mètres par minute, pas besoin de plus.
function redessiner(force) {
  if (navigation.mode !== "explorer") return;
  const maintenant = performance.now();
  if (!force && (!balade.vue || maintenant - balade.dessineA < 1000)) return;
  balade.dessineA = maintenant;
  dessinerCarte();
}

/// Le bandeau de la carte : où en est la balade, et de quoi l'arrêter.
function majBandeau() {
  const bandeau = $("carte-balade");
  const v = balade.vue;
  bandeau.hidden = !v;
  if (!v) return;
  const n = v.pistes.length;
  $("carte-balade-texte").textContent = v.fin
    ? `Balade finie : ${v.fin}`
    : `Balade · ${n} morceau${n > 1 ? "x" : ""}${v.terrain === "voisins" ? " · par les voisins" : ""}`;
  // Le compteur d'usage (`balade-mesure.json`), pour qui veut le voir.
  const m = v.mesure;
  $("carte-balade-texte").title = m
    ? `Carrefours : ${m.carrefours} · choix : ${m.choix} · ailleurs : ${m.ailleurs} · retours : ${m.retours}`
    : "";
  $("carte-balade-revenir").hidden = !v.revenir || !!v.fin;
}

/* ------------------------------------------------- le carrefour (P2) */

const parIdCarte = () => new Map(carte.points.map((p) => [p.id, p]));

/// Le temps restant du morceau en cours, d'après le dernier relevé.
function restantMs() {
  const v = balade.vue;
  if (!v) return null;
  const courant = parIdCarte().get(v.pistes[v.pistes.length - 1]);
  if (!courant?.duration_ms || lecture.courant !== courant.path) return null;
  return { restant: courant.duration_ms - lecture.positionMs, duree: courant.duration_ms };
}

function nomFamille(cluster) {
  return (carte.familles ?? []).find(([c]) => c === cluster)?.[1] ?? "";
}

/// Sur le plan de ville, la balade suit les rues ; ailleurs (le nuage, ou le
/// graphe des voisins), elle va d'un point à l'autre.
const surLesRues = () => !!carteGL() && carte.villeReelle && balade.vue?.terrain === "ville";

/// La teinte d'une famille, celle du fond où l'on est : la même sur la carte
/// du panneau et sur le tracé de la voie.
function teinte(cluster) {
  const t = surLesRues() ? couleursFamillesCarte() : couleursFamilles();
  return cluster >= 0 && t.length ? t[cluster % t.length] : "var(--mut)";
}

/// Le panneau se montre-t-il, et dans quel état.
function etatCarrefour() {
  const v = balade.vue;
  const c = v?.carrefour;
  if (!c || v.fin || navigation.mode !== "explorer") return null;
  if (balade.intervention === "porter" || c.numero === balade.masque) return null;
  // Rien ne joue (le mode « demander » attend dans le silence), un carrefour
  // rejoué, un morceau court : tout de suite. Sinon, à la fin du morceau.
  const r = restantMs();
  const proche =
    c.retour || !r || r.duree < MORCEAU_COURT_MS || r.restant <= CARREFOUR_AVANT_FIN_MS;
  if (!proche) return null;
  return { c, r, fige: v.fige, attend: v.attend };
}

function majCarrefour() {
  const panneau = $("carte-carrefour");
  const e = etatCarrefour();
  panneau.hidden = !e;
  if (!e) {
    balade.cle = "";
    return;
  }
  const { c, r, fige, attend } = e;
  const cle = `${c.numero}|${c.prise}|${c.voies.map((x) => +x.lue).join("")}|${carte.theme}`;
  if (cle !== balade.cle) {
    balade.cle = cle;
    remplirVoies(c);
  }
  panneau.classList.toggle("carrefour--fige", fige);
  // L'anneau de la voie prise : ce qui reste avant qu'elle le soit.
  const part =
    fige || attend || !r
      ? fige ? 0 : 1
      : Math.max(0, Math.min(1, (r.restant - FIGE_AVANT_FIN_MS) /
          (Math.min(CARREFOUR_AVANT_FIN_MS, r.duree) - FIGE_AVANT_FIN_MS)));
  panneau.querySelectorAll(".voie").forEach((b) => {
    const i = Number(b.dataset.voie);
    b.disabled = fige || c.voies[i].lue;
    const anneau = b.querySelector(".voie__anneau circle");
    if (anneau) anneau.style.strokeDashoffset = String(1 - (i === c.prise ? part : 0));
  });
  const prise = c.voies[c.prise];
  const titre = parIdCarte().get(prise.morceau)?.title ?? "";
  let texte;
  if (fige) texte = "Trop tard pour changer : le morceau suivant est déjà en route.";
  else if (attend) texte = "La balade attend votre choix.";
  else if (c.choisie) texte = `Vous prenez la voie ${c.prise + 1}.`;
  else if (r) {
    const s = Math.max(0, Math.ceil((r.restant - FIGE_AVANT_FIN_MS) / 1000));
    texte = `Sans geste, la voie ${c.prise + 1} dans ${s} s.`;
  } else texte = `Sans geste, la voie ${c.prise + 1}.`;
  $("carrefour-etat").textContent = texte;
  $("carrefour-etat").title = titre;
  placerPanneau();
}

/// Les cartes du panneau : nom de la rue (l'artiste), titre du premier
/// morceau, famille, jauge proche ↔ ailleurs.
function remplirVoies(c) {
  const index = parIdCarte();
  const liste = $("carrefour-voies");
  liste.replaceChildren(
    ...c.voies.map((voie, i) => {
      const p = index.get(voie.morceau);
      const li = document.createElement("li");
      const b = document.createElement("button");
      b.type = "button";
      b.className = "voie";
      b.dataset.voie = String(i);
      b.classList.toggle("voie--prise", i === c.prise);
      b.classList.toggle("voie--lue", voie.lue);
      const cluster = p?.cluster ?? -1;
      const famille = nomFamille(cluster);
      const jauge = Math.min(1, voie.distance / DISTANCE_AILLEURS);
      b.innerHTML = `
        <span class="voie__touche">
          <svg class="voie__anneau" viewBox="0 0 24 24" aria-hidden="true">
            <circle cx="12" cy="12" r="10.5" pathLength="1"></circle>
          </svg>
          <span>${i + 1}</span>
        </span>
        <span class="voie__corps">
          <span class="voie__rue"></span>
          <span class="voie__titre"></span>
          <span class="voie__famille"><i class="voie__pastille"></i><span></span></span>
          <span class="voie__jauge" title="Distance à l'oreille au morceau en cours">
            <span>proche</span><span class="voie__barre"><i></i></span><span>ailleurs</span>
          </span>
        </span>
        <svg class="voie__ecoute" viewBox="0 0 20 20" width="16" height="16" aria-hidden="true">
          <path d="M3 8h3l4-3.5v11L6 12H3z" fill="currentColor"/>
          <path d="M13 7.2a4 4 0 0 1 0 5.6M15.4 5a7 7 0 0 1 0 10" fill="none"
                stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        </svg>`;
      b.title = "Survoler pour entendre 5 s de ce morceau, sous celui qui joue";
      b.querySelector(".voie__rue").textContent = voie.lue
        ? `${p?.artist ?? "?"} · déjà prise`
        : p?.artist ?? "?";
      b.querySelector(".voie__titre").textContent = p?.title ?? "";
      b.querySelector(".voie__pastille").style.background = teinte(cluster);
      b.querySelector(".voie__famille span").textContent = famille || "famille inconnue";
      b.querySelector(".voie__barre i").style.left = `${(jauge * 100).toFixed(0)}%`;
      if (i !== c.prise) b.querySelector(".voie__anneau").remove();
      b.addEventListener("click", () => choisir(i));
      b.addEventListener("mouseenter", () => entendre(voie.morceau));
      b.addEventListener("mouseleave", () => entendre(null));
      li.append(b);
      return li;
    }),
  );
}

/// Le panneau à côté du promeneur, jamais dessus : à droite s'il y a la
/// place, sinon à gauche ; en hauteur, centré sur lui dans les limites de la
/// carte.
function placerPanneau() {
  const panneau = $("carte-carrefour");
  const zone = cnv.parentElement.getBoundingClientRect();
  const r = cnv.getBoundingClientRect();
  const [x, y] = balade.promeneur ?? [r.width / 2, r.height / 2];
  const l = panneau.offsetWidth;
  const h = panneau.offsetHeight;
  const ecart = 28;
  const ox = r.left - zone.left;
  const oy = r.top - zone.top;
  let gauche = ox + x + ecart;
  if (gauche + l > zone.width - 8) gauche = ox + x - ecart - l;
  gauche = Math.max(8, gauche);
  const haut = Math.max(56, Math.min(zone.height - h - 8, oy + y - h / 2));
  panneau.style.left = `${gauche}px`;
  panneau.style.top = `${haut}px`;
}

async function choisir(i) {
  const v = balade.vue;
  const c = v?.carrefour;
  if (!c || v.fige || !c.voies[i] || c.voies[i].lue) return;
  entendre(null);
  try {
    balade.vue = await invoke("balade_choisir", { numero: c.numero, voie: i });
  } catch (err) {
    remonter(err, "balade_choisir");
    await relireBalade();
  }
  majCarrefour();
  redessiner(true);
}

async function revenir() {
  try {
    balade.vue = await invoke("balade_revenir");
  } catch (err) {
    remonter(err, "balade_revenir");
    await relireBalade();
  }
  balade.masque = null;
  majBandeau();
  majCarrefour();
  redessiner(true);
}

/// L'extrait de 5 s au survol : le moteur le joue par la source de l'aura
/// (P4), sous le morceau en cours, puis rend la main à l'aura. La voie
/// entendue le montre (`voie--entendue`), et la carte entoure son morceau.
function entendre(morceau) {
  clearTimeout(balade.survol);
  clearTimeout(balade.finExtrait);
  balade.entendu = morceau;
  const marquer = (id) => {
    document.querySelectorAll("#carrefour-voies .voie").forEach((b) => {
      const voie = balade.vue?.carrefour?.voies[Number(b.dataset.voie)];
      b.classList.toggle("voie--entendue", id != null && voie?.morceau === id);
    });
    marquerApercu(id);
  };
  if (morceau == null) {
    marquer(null);
    invoke("balade_extrait", { morceau: null }).catch((err) => remonter(err, "balade_extrait"));
    return;
  }
  balade.survol = setTimeout(() => {
    invoke("balade_extrait", { morceau })
      .then(() => {
        if (balade.entendu !== morceau) return;
        marquer(morceau);
        balade.finExtrait = setTimeout(() => marquer(null), 5000);
      })
      .catch((err) => remonter(err, "balade_extrait"));
  }, EXTRAIT_DELAI_MS);
}

/// Les réglages de la balade, dans le rail d'Explorer : l'intervention et
/// l'allure, un groupe de segments chacun (`data-intervention`, `data-allure`).
const REGLAGES = [
  { groupe: "#balade-intervention", champ: "intervention", cle: "balade-intervention" },
  { groupe: "#bloc-balade", champ: "allure", cle: "balade-allure" },
];

function majReglage() {
  for (const { groupe, champ } of REGLAGES)
    document
      .querySelectorAll(`${groupe} [data-${champ}]`)
      .forEach((b) => b.classList.toggle("segment--actif", b.dataset[champ] === balade[champ]));
}
majReglage();

for (const { groupe, champ, cle } of REGLAGES)
  document.querySelectorAll(`${groupe} [data-${champ}]`).forEach((b) =>
    b.addEventListener("click", async () => {
      balade[champ] = b.dataset[champ];
      try {
        localStorage.setItem(cle, balade[champ]);
      } catch {
        // Sans stockage, le réglage vaut pour la session.
      }
      majReglage();
      try {
        // Une autre allure redécide le pas suivant s'il est encore libre.
        const vue = await invoke("balade_regler", { [champ]: balade[champ] });
        if (vue) balade.vue = vue;
      } catch (err) {
        remonter(err, "balade_regler");
      }
      majCarrefour();
      redessiner(true);
    }),
  );

/// Les gestes du clavier au carrefour : 1, 2, 3, les flèches, Échap. Écoutés
/// avant ceux du transport (← / → y changent de piste) : tant que le panneau
/// est là, les flèches désignent une rue.
window.addEventListener(
  "keydown",
  (e) => {
    if ($("carte-carrefour").hidden || e.repeat) return;
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const c = balade.vue?.carrefour;
    if (!c) return;
    let i = null;
    if (["1", "2", "3"].includes(e.key)) i = Number(e.key) - 1;
    else if (e.key.startsWith("Arrow")) i = voieVers(e.key);
    else if (e.key === "Escape") {
      balade.masque = c.numero;
      entendre(null);
      majCarrefour();
    } else return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (i != null && i < c.voies.length) choisir(i);
  },
  true,
);

/// La voie dont la rue part dans la direction de la flèche, sur l'écran.
function voieVers(touche) {
  const [dx, dy] = {
    ArrowUp: [0, -1], ArrowDown: [0, 1], ArrowLeft: [-1, 0], ArrowRight: [1, 0],
  }[touche] ?? [0, 0];
  let meilleure = null;
  let score = 0.3; // en deçà, aucune rue ne va vraiment par là
  for (const { i, points } of balade.voiesEcran) {
    if (points.length < 2) continue;
    const [ax, ay] = points[0];
    const [bx, by] = points[points.length - 1];
    const n = Math.hypot(bx - ax, by - ay);
    if (n < 1) continue;
    const s = ((bx - ax) * dx + (by - ay) * dy) / n;
    if (s > score) {
      score = s;
      meilleure = i;
    }
  }
  return meilleure;
}

/// Un clic sur la carte près d'une voie tracée la choisit (`maplibre.js`,
/// avant la sélection d'un morceau), panneau masqué ou non : le choix est
/// toujours offert tant que le suivant n'est pas figé. `true` : le clic est
/// pris.
function cliquerVoie(mx, my) {
  const c = balade.vue?.carrefour;
  if (!c || balade.vue.fige) return false;
  let proche = null;
  let d = VOIE_CLIC_PX;
  for (const { i, points } of balade.voiesEcran) {
    for (let k = 1; k < points.length; k++) {
      const e = distanceSegment([mx, my], points[k - 1], points[k]);
      if (e < d && !c.voies[i].lue) {
        d = e;
        proche = i;
      }
    }
  }
  if (proche == null) return false;
  choisir(proche);
  return true;
}

function distanceSegment([px, py], [ax, ay], [bx, by]) {
  const l2 = (bx - ax) ** 2 + (by - ay) ** 2;
  const t = l2 ? Math.max(0, Math.min(1, ((px - ax) * (bx - ax) + (py - ay) * (by - ay)) / l2)) : 0;
  return Math.hypot(px - ax - t * (bx - ax), py - ay - t * (by - ay));
}

/// Les premiers `m` mètres d'une ligne `[lon, lat]`.
function premiersMetres(ligne, m) {
  const out = [ligne[0]];
  let reste = m;
  for (let k = 1; k < ligne.length && reste > 0; k++) {
    const [ax, ay] = ligne[k - 1];
    const [bx, by] = ligne[k];
    const kx = 111_320 * Math.cos((ay * Math.PI) / 180);
    const l = Math.hypot((bx - ax) * kx, (by - ay) * 110_540);
    if (l <= reste) {
      out.push(ligne[k]);
      reste -= l;
    } else {
      const f = reste / l;
      out.push([ax + (bx - ax) * f, ay + (by - ay) * f]);
      reste = 0;
    }
  }
  return out;
}

/* ------------------------------------------------- dessin */

const parId = () => new Map(carte.points.map((p) => [p.id, p]));

/// Le point à la fraction `t` d'une ligne d'écran (longueur cumulée).
function lePointA(ecran, t) {
  if (ecran.length === 1) return ecran[0];
  const longueurs = [0];
  for (let i = 1; i < ecran.length; i++) {
    const [ax, ay] = ecran[i - 1];
    const [bx, by] = ecran[i];
    longueurs.push(longueurs[i - 1] + Math.hypot(bx - ax, by - ay));
  }
  const total = longueurs[longueurs.length - 1];
  if (total === 0) return ecran[0];
  const vise = Math.min(1, Math.max(0, t)) * total;
  let i = 1;
  while (i < longueurs.length - 1 && longueurs[i] < vise) i++;
  const f = (vise - longueurs[i - 1]) / Math.max(1e-9, longueurs[i] - longueurs[i - 1]);
  const [ax, ay] = ecran[i - 1];
  const [bx, by] = ecran[i];
  return [ax + (bx - ax) * f, ay + (by - ay) * f];
}

function trait(points) {
  ctx.beginPath();
  points.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
  ctx.stroke();
}

/// Appelé par la surcouche de la carte (`surcoucheSur`), sous le halo du
/// morceau en écoute. Sur le plan de ville, la trace et le pas du promeneur
/// suivent les rues ; sur le nuage (ou dans le graphe des voisins), la droite
/// d'un morceau au suivant — les points des rues sont des lon/lat, sans
/// rapport avec le repère t-SNE.
function dessinerBalade(r, encre, accent) {
  const v = balade.vue;
  if (!v || !carte.points.length) return;
  const index = parId();
  const rues = surLesRues();
  const ecranDe = (p) => versEcran(p, r);
  const lonlat = (ll) => versEcran({ x: ll[0], y: ll[1] }, r);

  // La trace : ce qui a déjà été parcouru, en retrait.
  const trace = rues
    ? v.trace.map(lonlat)
    : v.pistes.map((id) => index.get(id)).filter(Boolean).map(ecranDe);
  ctx.save();
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  if (trace.length > 1) {
    ctx.strokeStyle = encre;
    ctx.globalAlpha = 0.4;
    ctx.lineWidth = 4;
    trait(trace);
    ctx.strokeStyle = accent;
    ctx.globalAlpha = 0.7;
    ctx.lineWidth = 1.75;
    trait(trace);
  }

  // Le pas en cours : du morceau joué au suivant.
  const courant = index.get(v.pistes[v.pistes.length - 1]);
  const suivant = v.suivant != null ? index.get(v.suivant) : null;
  let pas = [];
  if (rues && v.troncon.length) pas = v.troncon.map(lonlat);
  else if (courant && suivant) pas = [ecranDe(courant), ecranDe(suivant)];
  else if (courant) pas = [ecranDe(courant)];
  if (pas.length > 1) {
    ctx.strokeStyle = accent;
    ctx.globalAlpha = 0.45;
    ctx.lineWidth = 1.5;
    trait(pas);
  }

  // Les voies du carrefour, chacune sur ses premiers mètres dans la couleur
  // de sa famille ; la voie prise plus épaisse, une voie lue en retrait. Sur
  // le nuage, un trait court du morceau en cours vers chacune.
  balade.voiesEcran = [];
  const c = v.carrefour;
  if (c && !v.fin) {
    c.voies.forEach((voie, i) => {
      let points = [];
      if (rues && voie.rue.length > 1) points = premiersMetres(voie.rue, VOIE_TRACEE_M).map(lonlat);
      else {
        const a = courant && ecranDe(courant);
        const b = index.get(voie.morceau);
        if (a && b) {
          const [bx, by] = ecranDe(b);
          const n = Math.hypot(bx - a[0], by - a[1]) || 1;
          const l = Math.min(n, 40);
          points = [a, [a[0] + ((bx - a[0]) * l) / n, a[1] + ((by - a[1]) * l) / n]];
        }
      }
      if (points.length < 2) return;
      balade.voiesEcran.push({ i, points });
      const p = index.get(voie.morceau);
      ctx.strokeStyle = encre;
      ctx.globalAlpha = voie.lue ? 0.25 : 0.6;
      ctx.lineWidth = i === c.prise ? 7 : 5;
      trait(points);
      ctx.strokeStyle = teinte(p?.cluster ?? -1);
      ctx.globalAlpha = voie.lue ? 0.35 : 1;
      ctx.lineWidth = i === c.prise ? 4 : 2.5;
      trait(points);
    });
  }

  // Le promeneur : à la fraction jouée du morceau en cours. Le relevé de la
  // balade peut retarder d'un battement sur la lecture : si le suivant a déjà
  // commencé, il est au bout du pas, jamais revenu au début.
  const joue = carte.points.find((p) => p.path === lecture.courant);
  let t = null;
  if (joue && courant && joue.id === courant.id) {
    t = courant.duration_ms ? lecture.positionMs / courant.duration_ms : 0;
  } else if (joue && suivant && joue.id === suivant.id) {
    t = 1;
  }
  balade.promeneur = courant ? ecranDe(courant) : null;
  if (t != null && pas.length) {
    const [x, y] = lePointA(pas, t);
    balade.promeneur = [x, y];
    ctx.globalAlpha = 1;
    ctx.fillStyle = accent;
    ctx.beginPath();
    ctx.arc(x, y, 6, 0, Math.PI * 2);
    ctx.fill();
    ctx.lineWidth = 2;
    ctx.strokeStyle = encre;
    ctx.stroke();
  }
  ctx.restore();
}

/* ------------------------------------------------- gestes */

// Pas de bouton dans l'inspecteur : à côté de ▶ et ✦, une pastille de plus
// ne disait pas en quoi la balade diffère d'une playlist « dans l'esprit de »
// (retour d'usage, 5 nov.). L'entrée reste le clic droit, là où l'on voit les rues.

$("carte-balade-arreter").addEventListener("click", arreterBalade);
$("carte-balade-revenir").addEventListener("click", revenir);

/// Le clic droit sur un morceau de la carte : un menu d'une ligne.
let cibleMenu = null;
cnv.addEventListener("contextmenu", (e) => {
  const r = cnv.getBoundingClientRect();
  const p = pointSous(e.clientX - r.left, e.clientY - r.top);
  if (!p) {
    fermerMenuBalade();
    return;
  }
  e.preventDefault();
  cibleMenu = p;
  const menu = $("carte-menu");
  const zone = cnv.parentElement.getBoundingClientRect();
  menu.style.left = `${e.clientX - zone.left}px`;
  menu.style.top = `${e.clientY - zone.top}px`;
  menu.hidden = false;
  $("carte-menu-flaner").focus();
});

function fermerMenuBalade() {
  $("carte-menu").hidden = true;
  cibleMenu = null;
}

$("carte-menu-flaner").addEventListener("click", () => {
  if (cibleMenu) flanerDici(cibleMenu.id);
});
document.addEventListener("pointerdown", (e) => {
  if (!$("carte-menu").hidden && !$("carte-menu").contains(e.target)) fermerMenuBalade();
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !$("carte-menu").hidden) fermerMenuBalade();
});

// Une balade reprise au démarrage (`balade_reprendre`) : la relire une fois.
setTimeout(() => relireBalade().then(() => redessiner(true)), 1500);

export { arreterBalade, balade, cliquerVoie, dessinerBalade, flanerDici, majBalade };
