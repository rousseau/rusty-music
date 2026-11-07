import "./carte-gl.js"; // l'ordre d'exécution : voir main.js
// Explorer : le canevas commun aux quatre visualisations (nuage, carte, temps,
// anneau). Le nuage de points lui-même, le repère écran ↔ carte, l'aiguillage
// du dessin (`dessinerCarte`), la surcouche (chemins, bornes, lasso), et tous
// les gestes de la souris : survol, clic, Maj+clic, glisser, molette, zoom.
import {
  $, fileAttente, txt,
} from "../socle.js";
import { lama } from "../lama.js";
import { selectionner } from "../ecouter.js";
import {
  anneau, carte, chargerAnneau, cnv, CONTINUES, ctx, rampe, retenu, surRampe, temps,
} from "../carte.js";
import {
  aideCourante, dessinerBornes, ouvrirCheminAuPremierMaj, poserBorne,
} from "../fond-de-plan.js";
import { cliquerVoie, dessinerBalade } from "../balade.js";
import { dessinerAura } from "../aura.js";
import {
  dessinerBaladePassee, territoireSous, texteTerritoire,
} from "../traces.js";
import { dessinerRelief } from "../relief.js";
import { dessinerLieux } from "../lieux.js";
import { lecture } from "../etat-lecture.js";
import {
  bornesGeoPoints, carteDepuisGeo, carteGL, geoDepuisCarte, journalCarte, synchroniserVue,
  vueInitialeGL, zoomMax,
} from "./carte-gl.js";
import {
  dessinerTemps, survolerTemps, zoomerTemps,
} from "./temps.js";
import {
  dessinerAnneau, survolerAnneau, zoomerAnneau,
} from "./anneau.js";
import { jouerSelection, tracerDessin } from "./chemins.js";
import { couleursFamilles } from "../familles.js";

/// `true` sur le streamgraph temporel — voir l'objet `temps` et
/// `dessinerTemps`. Ni embedding ni tuiles : un troisième repère, propre à ce
/// mode, jamais mêlé aux deux autres.
const modeTemps = () => carte.affichage === "temps";

/// `true` sur l'anneau de similarité par album — voir l'objet `anneau` et
/// `dessinerAnneau`. Un quatrième repère, encore différent des trois autres :
/// un cercle centré sur un album focal, pas un axe temps ni un embedding.
const modeAnneau = () => carte.affichage === "anneau";

function dimensionner() {
  const r = cnv.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  cnv.width = Math.max(1, Math.round(r.width * dpr));
  cnv.height = Math.max(1, Math.round(r.height * dpr));
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  return r;
}

/// Demi-côté utile du canevas : le nuage vit en [-1, 1], on lui laisse une
/// marge de 28 px pour que les points de bord ne collent pas au cadre.
function echelle(r) {
  return Math.min(r.width, r.height) / 2 - 28;
}

function versEcran(p, r) {
  const g = carteGL();
  if (g) {
    // Sur le plan de ville réel, l'adresse (si ce morceau en a une) prime
    // sur la position t-SNE — c'est elle que les tuiles montrent.
    const [x, y] = (carte.villeReelle && carte.positionsReelles.get(p.id)) || [p.x, p.y];
    const q = g.project(geoDepuisCarte(x, y));
    return [q.x, q.y];
  }
  // Le nuage, lui, reste toujours en t-SNE — `p.x`/`p.y` n'ont jamais été
  // touchés, que la ville soit active ou non.
  const { k, dx, dy } = carte.vue;
  const c = echelle(r);
  return [r.width / 2 + (p.x * c) * k + dx, r.height / 2 + (p.y * c) * k + dy];
}

/// L'inverse de `versEcran` : du pixel vers le repère du nuage. Le dessin en a
/// besoin — c'est le seul endroit où l'on part de l'écran pour aller vers les
/// données, et non l'inverse.
function versCarte(mx, my, r) {
  const g = carteGL();
  if (g) {
    const p = g.unproject([mx, my]);
    return carteDepuisGeo(p.lng, p.lat);
  }
  const { k, dx, dy } = carte.vue;
  const c = echelle(r) * k;
  return [(mx - r.width / 2 - dx) / c, (my - r.height / 2 - dy) / c];
}

/// Pastilles pré-dessinées, une par couleur.
///
/// 27 000 appels à `arc()` par image rendraient le survol poussif. On dessine
/// chaque pastille **une fois** dans un canevas minuscule, puis on la recopie
/// — `drawImage` d'une petite image est bien moins cher qu'un tracé de
/// chemin. Et les recopies se superposent : là où les morceaux s'entassent,
/// l'opacité s'accumule et la densité se voit, ce qu'un carré opaque cachait.
const pastilles = new Map();
function pastille(couleur, rayon, alpha) {
  const cle = `${couleur}|${rayon}|${alpha}`;
  const connue = pastilles.get(cle);
  if (connue) return connue;

  const d = Math.max(2, Math.ceil(rayon * 2) + 2);
  const c = document.createElement("canvas");
  c.width = d;
  c.height = d;
  const g = c.getContext("2d");
  g.globalAlpha = alpha;
  g.fillStyle = couleur;
  g.beginPath();
  g.arc(d / 2, d / 2, rayon, 0, Math.PI * 2);
  g.fill();
  pastilles.set(cle, c);
  return c;
}

/// Le nuage de points ordinaire — fond estompé d'abord, sélection par-dessus,
/// pour qu'elle ne soit jamais recouverte. C'était tout le corps de
/// `dessinerCarte` avant que la densité ne lui donne une alternative.
function dessinerNuage(r) {
  const style = getComputedStyle(document.documentElement);
  const encre = style.getPropertyValue("--txt").trim() || "#EDE8DC";
  const rayon = Math.max(1.1, 1.9 * Math.sqrt(carte.vue.k));
  const etapes = rampe();
  const teintes = couleursFamilles();
  const continu = CONTINUES[carte.couleur];
  const [v0, v1] = carte.bornes[carte.couleur] ?? [0, 0];
  const neutre = carte.isolees.size === 0 && !carte.filtre;

  for (const avant of [false, true]) {
    for (const p of carte.points) {
      const vise = (carte.isolees.size === 0 || carte.isolees.has(p.cluster)) && retenu(p);
      if (vise !== avant) continue;

      const [x, y] = versEcran(p, r);
      if (x < -8 || y < -8 || x > r.width + 8 || y > r.height + 8) continue;

      let couleur;
      if (continu) {
        const v = p[continu.champ];
        couleur = v != null && v1 > v0 ? surRampe(etapes, (v - v0) / (v1 - v0)) : encre;
      } else {
        couleur = teintes[p.cluster % teintes.length] ?? encre;
      }
      // L'opacité fait double emploi : elle écarte ce qui est filtré, et elle
      // révèle la densité par superposition.
      const alpha = avant ? (neutre ? 0.62 : 0.95) : 0.07;
      const rr = avant && !neutre ? rayon * 1.5 : rayon;
      const img = pastille(couleur, rr, alpha);
      ctx.drawImage(img, x - img.width / 2, y - img.height / 2);
    }
  }
}

function dessinerCarte() {
  // Le Lama n'a pas de canevas de carte : rien à redessiner (la playlist qui
  // se compose y appelle `tracerRouteSurCarte`, qui rappelle ceci).
  if (carte.affichage === "lama") return;
  // En mode carte, les tuiles portent le fond : le canevas ne garde que la
  // surcouche. En mode nuage, il dessine tout, comme avant.
  const g = carteGL();
  if (g) {
    g.resize();
    dessinerSurcouche();
    return;
  }
  const r = dimensionner();
  const style = getComputedStyle(document.documentElement);
  const encre = style.getPropertyValue("--txt").trim() || "#EDE8DC";
  const accent = style.getPropertyValue("--accent").trim() || "#C07C4A";

  // Le streamgraph est un troisième dessin sur le même canevas, pas une
  // variante du nuage : ses axes (temps × famille empilée) n'ont rien à voir
  // avec l'embedding que `dessinerNuage`/`surcoucheSur` savent tracer.
  if (modeTemps()) {
    ctx.clearRect(0, 0, r.width, r.height);
    dessinerTemps(r, encre, accent);
    return;
  }

  // L'anneau, de même : un cercle centré sur un album focal, encore un autre
  // repère que l'embedding, le lon/lat ou l'axe du temps.
  if (modeAnneau()) {
    ctx.clearRect(0, 0, r.width, r.height);
    dessinerAnneau(r, encre, accent);
    return;
  }

  ctx.clearRect(0, 0, r.width, r.height);
  if (carte.points.length === 0) {
    ctx.fillStyle = style.getPropertyValue("--mut").trim() || "#9A9284";
    ctx.font = "13px system-ui";
    ctx.textAlign = "center";
    ctx.fillText("Aucun morceau analysé pour l'instant.", r.width / 2, r.height / 2);
    return;
  }

  // Le nuage t-SNE, tel qu'il a toujours été.
  dessinerNuage(r);
  surcoucheSur(r, encre, accent);
}

/// Ce que le canevas dessine par-dessus les tuiles : le lasso en cours, le
/// tracé à la souris, l'itinéraire, les bornes, le survol. Tout ce que les
/// tuiles ne portent pas parce que cela change à chaque geste.
function dessinerSurcouche() {
  if (!carteGL()) return;
  const r = dimensionner();
  const style = getComputedStyle(document.documentElement);
  ctx.clearRect(0, 0, r.width, r.height);
  surcoucheSur(
    r,
    style.getPropertyValue("--txt").trim() || "#EDE8DC",
    style.getPropertyValue("--accent").trim() || "#C07C4A",
  );
}

function surcoucheSur(r, encre, accent) {
  // Le lasso en cours : contour fermé et zone assombrie, pour qu'on voie ce
  // qu'on attrape avant de lâcher.
  if (carte.lasso && carte.lasso.length > 1) {
    ctx.beginPath();
    carte.lasso.forEach(([x, y], i) => {
      const [ex, ey] = versEcran({ x, y }, r);
      if (i === 0) ctx.moveTo(ex, ey);
      else ctx.lineTo(ex, ey);
    });
    ctx.closePath();
    ctx.fillStyle = accent;
    ctx.globalAlpha = 0.12;
    ctx.fill();
    ctx.globalAlpha = 0.9;
    ctx.strokeStyle = accent;
    ctx.lineWidth = 1.5;
    ctx.setLineDash([4, 3]);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;
  }

  // Le trait en cours de dessin : pointillé, pour le distinguer d'un chemin.
  if (carte.trace && carte.trace.length > 1) {
    ctx.strokeStyle = accent;
    ctx.lineWidth = 2;
    ctx.setLineDash([5, 4]);
    ctx.beginPath();
    carte.trace.forEach(([x, y], i) => {
      const [ex, ey] = versEcran({ x, y }, r);
      if (i === 0) ctx.moveTo(ex, ey);
      else ctx.lineTo(ex, ey);
    });
    ctx.stroke();
    ctx.setLineDash([]);
  }

  // Le chemin, par-dessus le nuage : un trait continu et ses étapes.
  //
  // Un simple trait à l'accent s'y perdait : plusieurs familles portent des
  // teintes proches de l'accent, et à cette épaisseur le trait devenait un
  // point de plus dans le nuage. Un liseré dans l'encre du texte — le ton le
  // plus contrasté sur le fond, dans les deux thèmes — le fait ressortir
  // quel que soit ce qu'il traverse, comme un tracé de route sur une carte
  // routière ; l'accent reste au-dessus, plus fin, pour l'identité du trait.
  if (carte.route && carte.route.length > 1) {
    ctx.lineJoin = "round";
    ctx.lineCap = "round";

    // Le trait suit `routeTrace` (les vraies rues, une fois habillé) — mais
    // seulement en mode Carte : ses points intermédiaires sont des lon/lat,
    // qui n'ont aucun sens passés tels quels dans le repère t-SNE du Nuage.
    // Les repères ci-dessous restent posés sur `route` (une entrée par
    // morceau) — `routeTrace` peut porter bien plus de points que d'étapes.
    const trait = (carteGL() && carte.routeTrace) || carte.route;
    const tracer = () => {
      ctx.beginPath();
      trait.forEach((p, i) => {
        const [x, y] = versEcran(p, r);
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      });
      ctx.stroke();
    };

    ctx.strokeStyle = encre;
    ctx.globalAlpha = 0.55;
    ctx.lineWidth = 4.5;
    tracer();

    // Une playlist composée par le Lama garde ses parties : le trait et les
    // repères y prennent la couleur de leur partie (la même que dans le panneau),
    // pour retrouver sur la carte ce que le récit a expliqué.
    const parties =
      lama.fileComposee && lama.fileComposee === fileAttente.pistes && lama.partieDeId.size > 1;
    const racine = getComputedStyle(document.documentElement);
    const couleurPartie = (id) =>
      parties && lama.partieDeId.has(id)
        ? racine.getPropertyValue(`--partie-${(lama.partieDeId.get(id) % 6) + 1}`).trim() || accent
        : accent;
    if (parties && trait === carte.route) {
      ctx.globalAlpha = 0.95;
      ctx.lineWidth = 2.25;
      for (let i = 1; i < trait.length; i++) {
        ctx.strokeStyle = couleurPartie(trait[i].id);
        ctx.beginPath();
        const [x0, y0] = versEcran(trait[i - 1], r);
        const [x1, y1] = versEcran(trait[i], r);
        ctx.moveTo(x0, y0);
        ctx.lineTo(x1, y1);
        ctx.stroke();
      }
    } else {
      ctx.strokeStyle = accent;
      ctx.globalAlpha = 0.95;
      ctx.lineWidth = 2.25;
      tracer();
    }

    for (const p of carte.route) {
      const [x, y] = versEcran(p, r);
      ctx.beginPath();
      ctx.arc(x, y, 4.5, 0, Math.PI * 2);
      ctx.fillStyle = couleurPartie(p.id);
      ctx.globalAlpha = 1;
      ctx.fill();
      ctx.lineWidth = 1.2;
      ctx.strokeStyle = encre;
      ctx.globalAlpha = 0.8;
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
  }

  // Les bornes du chemin : anneaux creux, l'un plein pour le départ.
  for (const [borne, plein] of [[carte.depart, true], [carte.arrivee, false]]) {
    if (!borne) continue;
    const [x, y] = versEcran(borne, r);
    ctx.strokeStyle = accent;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.arc(x, y, 5.5, 0, Math.PI * 2);
    ctx.stroke();
    if (plein) {
      ctx.fillStyle = accent;
      ctx.beginPath();
      ctx.arc(x, y, 2, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  // La balade en cours : sa trace et le promeneur (`balade.js`).
  dessinerLieux(r, encre);
  dessinerBaladePassee(r, encre, accent, ctx);
  dessinerRelief(r, encre, accent);
  dessinerBalade(r, encre, accent);

  // Les voix de l'aura : un anneau autour de chaque morceau entendu (`aura.js`).
  dessinerAura(r, accent);

  // Le morceau en écoute : un halo qui le retrouve dans 27 000 points.
  const joue = carte.points.find((p) => p.path === lecture.courant);
  if (joue) {
    const [x, y] = versEcran(joue, r);
    ctx.strokeStyle = encre;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(x, y, 9, 0, Math.PI * 2);
    ctx.stroke();
    ctx.globalAlpha = 0.35;
    ctx.beginPath();
    ctx.arc(x, y, 14, 0, Math.PI * 2);
    ctx.stroke();
    ctx.globalAlpha = 1;
  }

  // Le morceau survolé : un anneau, lisible quel que soit le fond.
  if (carte.survole) {
    const [x, y] = versEcran(carte.survole, r);
    ctx.strokeStyle = accent;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(x, y, 7, 0, Math.PI * 2);
    ctx.stroke();
  }
}

/// Point sous le curseur — sur le plan réel, vise d'abord le bâtiment que
/// MapLibre a effectivement peint sous ce pixel (`queryRenderedFeatures`) :
/// depuis que le morceau est un bâtiment entier, pas un point (voir
/// `carto-etapes.md`), un simple rayon autour du centroïde manquait tout
/// clic vers le bord d'un grand bâtiment. Retombe sur le point le plus
/// proche dans un rayon raisonnable si rien n'est peint là (bâtiment vacant,
/// zoom trop large, ou monde fictif sans bâti).
function pointSous(mx, my) {
  // Le streamgraph n'a pas de position d'écran par morceau — seulement des
  // bandes et des fils agrégés (voir `dessinerTemps`) : rien à pointer.
  // Chercher un morceau puis Entrée reste la façon de poser un départ.
  // L'anneau non plus : ses nœuds sont des albums, pas des morceaux, pointés
  // par `survolerAnneau`/`anneau.puces`, pas par `carte.points`.
  if (modeTemps() || modeAnneau()) return null;
  const g = carteGL();
  if (g && carte.villeReelle && g.getLayer("batiments-morceaux")) {
    const feats = g.queryRenderedFeatures([mx, my], { layers: ["batiments-morceaux"] });
    const id = feats[0]?.properties?.morceau;
    if (id != null && id >= 0 && (carte.isolees.size === 0 || carte.isolees.has(feats[0].properties.palier))) {
      const p = carte.points.find((pt) => pt.id === id);
      if (p) return p;
    }
  }

  const r = cnv.getBoundingClientRect();
  let meilleur = null;
  let d2min = 14 * 14;
  for (const p of carte.points) {
    if (carte.isolees.size > 0 && !carte.isolees.has(p.cluster)) continue;
    const [x, y] = versEcran(p, r);
    const d2 = (x - mx) ** 2 + (y - my) ** 2;
    if (d2 < d2min) {
      d2min = d2;
      meilleur = p;
    }
  }
  return meilleur;
}

cnv.addEventListener("mousemove", (e) => {
  const r = cnv.getBoundingClientRect();
  const mx = e.clientX - r.left;
  const my = e.clientY - r.top;

  // Lasso et tracé de chemin se suivent de la même façon : un point tous les
  // 3 px, pour ne pas accumuler des centaines de points confondus.
  const encours = carte.lasso ?? carte.trace;
  if (encours) {
    const p = versCarte(mx, my, r);
    const d = encours[encours.length - 1];
    const seuil = 3 / (echelle(r) * carte.vue.k);
    if (Math.hypot(p[0] - d[0], p[1] - d[1]) > seuil) encours.push(p);
    dessinerCarte();
    return;
  }

  if (glisse) {
    const g = carteGL();
    if (g) {
      // Le canevas relaie : MapLibre n'écoute rien lui-même.
      g.panBy([-e.movementX, -e.movementY], { duration: 0 });
    } else if (modeTemps()) {
      // Les deux axes, comme Nuage/Carte juste en dessous — le temps
      // (`dx`) et les bandes (`dy`, zoom vertical continu). Reclampés au
      // prochain dessin, voir `temps.vue`.
      temps.vue.dx += e.movementX;
      temps.vue.dy += e.movementY;
    } else if (modeAnneau()) {
      anneau.vue.dx += e.movementX;
      anneau.vue.dy += e.movementY;
    } else {
      carte.vue.dx += e.movementX;
      carte.vue.dy += e.movementY;
    }
    dessinerCarte();
    return;
  }

  if (modeTemps()) {
    survolerTemps(mx, my, r);
    return;
  }

  if (modeAnneau()) {
    survolerAnneau(mx, my, r);
    return;
  }

  const p = pointSous(mx, my);
  // Le compteur de quartier (P5 de la balade) : sous un morceau, une ligne de
  // plus ; sur un territoire sans morceau sous le curseur, à lui seul.
  const terr = territoireSous(mx, my);
  const info = $("carte-info");
  if (p !== carte.survole || terr?.famille !== carte.territoireSurvole) {
    carte.territoireSurvole = terr?.famille;
    const lignes = p
      ? [txt(p.title, "(sans titre)"), txt(p.artist, "(sans artiste)")]
      : terr ? [terr.nom || "Quartier"] : [];
    if (terr) lignes.push(p ? `${terr.nom} · ${texteTerritoire(terr)}` : texteTerritoire(terr));
    info.hidden = !lignes.length;
    info.replaceChildren(
      ...lignes.map((t, i) => {
        const el = document.createElement(i === 0 ? "b" : "span");
        el.textContent = t;
        return el;
      }),
    );
  }
  if (!info.hidden) {
    info.style.left = `${Math.min(mx + 14, r.width - 290)}px`;
    info.style.top = `${my + 14}px`;
  }
  if (p !== carte.survole) {
    carte.survole = p;
    dessinerCarte();
  }
});

let glisse = false;
// Un dessin ou un lasso se termine par un `mouseup`, donc par un `click` :
// sans ce drapeau, relâcher le trait relancerait la lecture du point survolé.
let vientDeDessiner = false;
// Un déplacement de la carte se termine lui aussi par un `click`. Sans cette
// mesure, faire glisser la vue changeait le morceau en écoute — le point sous
// le curseur à l'arrivée n'a rien demandé.
let departGlisse = null;
const SEUIL_GLISSE = 4; // px ; en deçà, c'est un clic tremblant, pas un glissé

cnv.addEventListener("mousedown", (e) => {
  // Le clic droit ouvre le menu d'un morceau (`balade.js`) : ni glisser ni
  // lasso.
  if (e.button === 2) return;
  // Un relâchement hors du canevas ne produit pas de `click` : sans cette
  // remise à zéro, le drapeau survivrait et avalerait le clic suivant.
  vientDeDessiner = false;

  // Le streamgraph n'a ni lasso ni tracé à la souris — chercher un morceau
  // puis Entrée reste le seul geste de départ du mode Temps — mais il se
  // glisse horizontalement comme les deux autres affichages (voir la
  // branche `modeTemps()` de `mousemove`, plus bas).
  if (modeTemps()) {
    glisse = true;
    departGlisse = [e.clientX, e.clientY];
    return;
  }

  // L'anneau n'a ni lasso, ni tracé — juste zoomer/glisser (comme le nuage,
  // voir `zoomerAnneau`) et le clic (voir le gestionnaire `click` plus bas) :
  // chercher un album, ou en cliquer un déjà affiché.
  if (modeAnneau()) {
    glisse = true;
    departGlisse = [e.clientX, e.clientY];
    return;
  }

  // Alt+glisser : lasso. Disponible dans tous les modes de chemin — c'est une
  // sélection, pas un chemin, et rien ne justifie de la cacher derrière un
  // mode.
  if (e.altKey) {
    const r = cnv.getBoundingClientRect();
    carte.lasso = [versCarte(e.clientX - r.left, e.clientY - r.top, r)];
    carte.route = null;
    carte.survole = null;
    $("carte-info").hidden = true;
    return;
  }

  if (carte.chemin === "dessine" && e.shiftKey) {
    const r = cnv.getBoundingClientRect();
    carte.trace = [versCarte(e.clientX - r.left, e.clientY - r.top, r)];
    carte.route = null;
    carte.survole = null;
    $("carte-info").hidden = true;
    return;
  }
  glisse = true;
  departGlisse = [e.clientX, e.clientY];
});

window.addEventListener("mouseup", (e) => {
  if (departGlisse) {
    const bouge =
      Math.hypot(e.clientX - departGlisse[0], e.clientY - departGlisse[1]) > SEUIL_GLISSE;
    departGlisse = null;
    if (bouge) vientDeDessiner = true; // le clic qui suit est à ignorer
  }
});

window.addEventListener("mouseup", async () => {
  glisse = false;
  if (carte.lasso) {
    const contour = carte.lasso;
    carte.lasso = null;
    vientDeDessiner = true;
    dessinerCarte();
    await jouerSelection(contour);
    return;
  }
  if (!carte.trace) return;
  const trace = carte.trace;
  carte.trace = null;
  vientDeDessiner = true;
  await tracerDessin(trace);
});

cnv.addEventListener("mouseleave", () => {
  carte.survole = null;
  temps.survole = null;
  anneau.survole = null;
  $("carte-info").hidden = true;
  dessinerCarte();
});

function zoomer(f, cx, cy) {
  if (modeTemps()) {
    const r = cnv.getBoundingClientRect();
    zoomerTemps(f, cx ?? r.width / 2, cy ?? r.height / 2);
    return;
  }
  if (modeAnneau()) {
    zoomerAnneau(f, cx, cy);
    return;
  }
  const g = carteGL();
  if (g) {
    // Zoomer autour du curseur : on note le point du monde qui s'y trouve,
    // on change d'échelle, puis on recentre pour l'y remettre. Les boutons
    // +/− appellent sans coordonnées : `unproject([undefined, …])` lève dans
    // MapLibre — on retombe alors sur le centre du canevas.
    const r0 = cnv.getBoundingClientRect();
    if (cx == null) cx = r0.width / 2;
    if (cy == null) cy = r0.height / 2;
    const avant = g.unproject([cx, cy]);
    const z = Math.min(zoomMax, Math.max(0, g.getZoom() + Math.log2(f)));
    g.setZoom(z);
    const apres = g.unproject([cx, cy]);
    const c = g.getCenter();
    g.setCenter([c.lng + (avant.lng - apres.lng), c.lat + (avant.lat - apres.lat)]);
    synchroniserVue();
    return;
  }
  const r = cnv.getBoundingClientRect();
  const mx = (cx ?? r.width / 2) - r.width / 2;
  const my = (cy ?? r.height / 2) - r.height / 2;
  const avant = carte.vue.k;
  carte.vue.k = Math.min(60, Math.max(0.5, carte.vue.k * f));
  // Le facteur réellement appliqué, après butée : sans cela, le décalage
  // continuerait de bouger une fois le zoom bloqué.
  const reel = carte.vue.k / avant;
  carte.vue.dx = mx - (mx - carte.vue.dx) * reel;
  carte.vue.dy = my - (my - carte.vue.dy) * reel;
  $("zoom-val").textContent = `×${carte.vue.k.toFixed(1).replace(".", ",")}`;
  dessinerCarte();
}

/// L'image hors-écran de la densité est bâtie pour un niveau de zoom donné
/// : la redessiner à chaque cran de molette la
/// referait des dizaines de fois par seconde pour rien, un délai court
/// après la fin du geste suffit — même principe que le bruit des chemins.
let attenteZoomDensite = null;

cnv.addEventListener("wheel", (e) => {
  e.preventDefault();
  const r = cnv.getBoundingClientRect();
  const mx = e.clientX - r.left;
  const my = e.clientY - r.top;

  // Proportionnel à l'ampleur du geste, et non un pas fixe par évènement :
  // un trackpad en émet des dizaines par centimètre de doigt, là où une
  // molette en émet un par cran. Le pas fixe rendait le zoom inutilisable au
  // trackpad. Le facteur est borné pour qu'une inertie brutale ne fasse pas
  // traverser toute la plage d'un coup. Un seul geste, les deux axes à la
  // fois — `zoomer` sait déjà distinguer les affichages, voir `zoomerTemps`,
  // qui zoome le temps et les bandes ensemble sur la frise, exactement comme
  // `carte.vue.k` le fait sur le nuage.
  const f = Math.exp(-Math.max(-40, Math.min(40, e.deltaY)) * 0.0035);
  zoomer(f, mx, my);
}, { passive: false });

$("zoom-plus").addEventListener("click", () => zoomer(1.4));
$("zoom-moins").addEventListener("click", () => zoomer(1 / 1.4));
$("zoom-reset").addEventListener("click", () => {
  if (modeTemps()) {
    temps.vue = { k: 1, dx: 0, dy: 0 };
    $("zoom-val").textContent = "×1,0";
    dessinerCarte();
    return;
  }
  if (modeAnneau()) {
    anneau.vue = { k: 1, dx: 0, dy: 0 };
    $("zoom-val").textContent = "×1,0";
    dessinerCarte();
    return;
  }
  const g = carteGL();
  if (g) {
    // Priorité à la limite communale (`vueInitialeGL.bounds`) : prise sur la
    // frontière réelle, elle ne se fait pas tirer au loin par un morceau mal
    // géocodé. À défaut — monde fictif — l'emprise des points. Le `zoom`
    // d'accueil du style (14, niveau du quartier) n'est qu'un dernier repli.
    const b = vueInitialeGL?.bounds || bornesGeoPoints();
    let applique = null;
    if (b) {
      // On calcule la caméra sans bouger, on la vérifie, puis on l'applique :
      // une emprise dégénérée (coordonnées parasites) donnerait un zoom
      // minuscule et une carte vide — mieux vaut alors le centre d'accueil.
      const cam = g.cameraForBounds(b, { padding: 40 });
      if (cam && cam.zoom >= 6) {
        g.jumpTo(cam);
        applique = `bounds → zoom ${cam.zoom.toFixed(2)}`;
      }
    }
    if (!applique && vueInitialeGL) {
      g.jumpTo({ center: vueInitialeGL.center, zoom: vueInitialeGL.zoom });
      applique = `centre d'accueil (bounds ${b ? "rejetées" : "absentes"})`;
    }
    journalCarte("vue d'ensemble : " + (applique || "aucune cible"));
    synchroniserVue();
    dessinerCarte();
    return;
  }
  carte.vue = { k: 1, dx: 0, dy: 0 };
  $("zoom-val").textContent = "×1,0";
  dessinerCarte();
});

cnv.addEventListener("click", async (e) => {
  if (vientDeDessiner) {
    vientDeDessiner = false;
    return;
  }

  // Anneau : cliquer n'importe quel album du fond (pas seulement un voisin
  // déjà affiché) en fait le nouvel album focal — « marcher » dans le graphe
  // de ressemblance. Cliquer le focal lui-même ne fait rien, il l'est déjà.
  if (modeAnneau()) {
    if (anneau.survole && anneau.survole.id !== anneau.focal?.id) {
      await chargerAnneau(anneau.survole.id);
    }
    return;
  }

  // Temps (frise des filiations) : même geste que l'anneau, même état focal
  // partagé (`anneau.focal`/`anneau.voisins`) — `chargerAnneau` peuple à la
  // fois la sélection dessinée (§ 2) et l'inspecteur partagé (§ 1), pas un
  // composant propre à la frise.
  if (modeTemps()) {
    if (temps.survole && temps.survole.album.id !== anneau.focal?.id) {
      await chargerAnneau(temps.survole.album.id);
    }
    return;
  }

  // Au carrefour d'une balade, un clic sur une rue tracée la prend
  // (`balade.js`) : il passe avant le morceau sous le curseur.
  {
    const r = cnv.getBoundingClientRect();
    if (!e.shiftKey && cliquerVoie(e.clientX - r.left, e.clientY - r.top)) return;
  }

  // Maj est le geste du chemin : la première fois, son panneau s'ouvre.
  if (e.shiftKey && carte.chemin !== "dessine") ouvrirCheminAuPremierMaj();

  const p = carte.survole;
  if (!p) return;

  // Maj est le modificateur « chemin » dans tous les modes ; ce qu'il déclenche
  // dépend du mode choisi dans le rail.
  if (e.shiftKey) {
    if (carte.chemin === "dessine") return; // le dessin passe par le glisser
    await poserBorne(p);
    return;
  }

  // Sans modificateur : le morceau devient le départ proposé et peuple
  // l'inspecteur — silencieux, jamais un `play` direct. Même geste que le
  // clic sur l'Anneau/la Frise (`chargerAnneau`, qui ne joue pas non plus) :
  // cliquer navigue/sélectionne, écouter est un geste à part (bouton ▶ de
  // l'inspecteur, `insp-lecture`) — cohérence voulue entre les quatre
  // visualisations plutôt qu'un clic qui coupe la lecture en cours ici et pas
  // là (`docs/interface-guidelines.md` § 1).
  carte.depart = p;
  carte.arrivee = null;
  carte.route = null;
  dessinerBornes();
  selectionner(p);
  dessinerCarte();
});

/// Signale un calcul en cours dans le pied de carte, ou l'efface.
///
/// Le premier chemin sonique ou errant construit le graphe des voisins : une
/// dizaine de secondes sur la bibliothèque entière. Sans ce mot, la carte
/// paraîtrait figée.
function patienter(texte) {
  $("carte-aide").textContent = texte ?? aideCourante();
}

window.addEventListener("resize", () => {
  if (!$("carte-vue").hidden) dessinerCarte();
});

export {
  dessinerCarte, dessinerSurcouche, echelle, modeAnneau, modeTemps, patienter, pointSous,
  versCarte, versEcran, zoomer,
};
