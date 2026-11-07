import "./nuage.js"; // l'ordre d'exécution : voir main.js
// Explorer → Temps : la frise des filiations (`docs/frise-filiations.md`), son
// dessin, son survol et son zoom.
import {
  $, fileAttente, isoler,
} from "../socle.js";
import {
  anneau, carte, ctx, dansIntervalleAnnee, filtreAnnee, intervalleAnneeActif, temps,
} from "../carte.js";
import { lecture } from "../etat-lecture.js";
import { dessinerCarte } from "./nuage.js";
import { couleursFamilles } from "../familles.js";

// Les tuiles vectorielles remplacent le nuage et la nappe de densité
// dessinés à la main (§ plus haut, `regenerer-tuiles`) : le pavage par
// famille et la coloration continue s'engendrent désormais côté Rust
// (`crates/core::density`, `crates/carto`) et arrivent déjà peints dans les
// tuiles que MapLibre affiche. Les helpers de couleur qui suivent
// (`hexRGB`/`hexHSL`/`hslRGB`) restent utilisés ailleurs (`desature`, mode
// Explorer → Anneau), sans rapport avec la densité.

function hexRGB(hex) {
  const n = parseInt((hex || "#9A9284").replace("#", ""), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/// Teinte → HSL, pour faire varier la clarté et la saturation avec la
/// densité plutôt que la seule opacité.
function hexHSL(hex) {
  const [r, g, b] = hexRGB(hex).map((v) => v / 255);
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  const l = (max + min) / 2;
  let h = 0, s = 0;
  const d = max - min;
  if (d > 1e-6) {
    s = d / (1 - Math.abs(2 * l - 1));
    if (max === r) h = ((g - b) / d) % 6;
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  return [h, s, l];
}

function hslRGB(h, s, l) {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  const [r, g, b] =
    h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  return [Math.round((r + m) * 255), Math.round((g + m) * 255), Math.round((b + m) * 255)];
}


/// La frise des filiations du mode Explorer → Temps : un point par album sur
/// un axe chronologique, réparti en bandes de familles dont l'ordre minimise
/// la distance verticale totale des arcs inter-familles — voir l'objet
/// `temps`, `ordonnerBandes`, et `docs/frise-filiations.md`. Remplace
/// l'ancien streamgraph (bandes empilées par bucket temporel), jugé
/// insatisfaisant — voir `docs/carto-anneau.md`.
function dessinerTemps(r, encre, accent) {
  if (!temps.charge) {
    ctx.fillStyle = encre;
    ctx.globalAlpha = 0.6;
    ctx.font = "13px system-ui";
    ctx.textAlign = "center";
    ctx.fillText("Chargement de la frise…", r.width / 2, r.height / 2);
    ctx.globalAlpha = 1;
    return;
  }
  if (temps.albums.length === 0 || temps.minAnnee == null) {
    ctx.fillStyle = encre;
    ctx.globalAlpha = 0.6;
    ctx.font = "13px system-ui";
    ctx.textAlign = "center";
    ctx.fillText("Aucune date exploitable pour l'instant.", r.width / 2, r.height / 2);
    ctx.globalAlpha = 1;
    return;
  }

  const teintes = couleursFamilles();
  const noms = new Map(carte.familles.map(([c, nom]) => [c, nom]));
  const nBandes = Math.max(1, temps.bandes.length);
  const aIncertain = temps.aIncertain;
  const largeurIncertain = aIncertain ? 34 : 0;
  const marge = { haut: 10, bas: 28, gauche: aIncertain ? 20 + largeurIncertain + 20 : 20, droite: 92 };
  const xDeb = marge.gauche;
  const xFin = r.width - marge.droite;
  const yHaut = marge.haut;
  const yBas = r.height - marge.bas;
  const hauteur = Math.max(1, yBas - yHaut);

  // Zoom vertical (même `temps.vue.k` que l'horizontal, § plus haut) : même
  // reclampage que le zoom horizontal ci-dessous, appliqué à la hauteur
  // totale des bandes plutôt qu'à la largeur du temps. `hauteurEchelle` est
  // ce que l'accordéon (juste après) répartit entre les bandes — étendre une
  // bande puis zoomer dedans composent naturellement, l'un donne un point de
  // départ, l'autre permet d'aller y regarder de près.
  const hauteurEchelle = hauteur * temps.vue.k;
  temps.vue.dy =
    hauteurEchelle <= hauteur
      ? (hauteur - hauteurEchelle) / 2
      : Math.min(0, Math.max(hauteur - hauteurEchelle, temps.vue.dy));

  // Bande étendue (§ 3, accordéon depuis la légende — `temps.bandeEtendue`,
  // basculé par `rendreFamilles`) : cette bande seule prend `FRACTION_ETENDUE`
  // de la hauteur (zoomée), les autres se partagent le reste à parts égales.
  const FRACTION_ETENDUE = 0.55;
  const iEtendue =
    temps.bandeEtendue != null ? temps.posFamille.get(temps.bandeEtendue) : undefined;
  const hauteursBandes = new Float64Array(nBandes);
  if (iEtendue !== undefined) {
    const reste = (hauteurEchelle * (1 - FRACTION_ETENDUE)) / Math.max(1, nBandes - 1);
    for (let i = 0; i < nBandes; i++) hauteursBandes[i] = i === iEtendue ? hauteurEchelle * FRACTION_ETENDUE : reste;
  } else {
    hauteursBandes.fill(hauteurEchelle / nBandes);
  }
  const y0Bandes = new Float64Array(nBandes);
  {
    let cumul = yHaut + temps.vue.dy;
    for (let i = 0; i < nBandes; i++) {
      y0Bandes[i] = cumul;
      cumul += hauteursBandes[i];
    }
  }

  const minA = temps.minAnnee;
  const maxA = temps.maxAnnee;
  const nAnnees = Math.max(1, maxA - minA);
  const xPasBase = (xFin - xDeb) / nAnnees; // pixels par année, à k = 1

  // Reclampe le décalage à chaque image, pas seulement au moment du geste :
  // un redimensionnement de la fenêtre ou un changement de l'intervalle
  // connu (nouvelle analyse) doit lui aussi retomber sur une vue sans vide.
  const xPas = xPasBase * temps.vue.k;
  const largeurDisponible = xFin - xDeb;
  const largeurTotale = nAnnees * xPas;
  temps.vue.dx =
    largeurTotale <= largeurDisponible
      ? (largeurDisponible - largeurTotale) / 2
      : Math.min(0, Math.max(largeurDisponible - largeurTotale, temps.vue.dx));
  temps.disposition = { xDeb, xFin, xPasBase, n: nAnnees + 1, yHaut, hauteur };

  const xi = 20; // colonne « incertain »
  const xDeAnnee = (annee) => xDeb + (annee - minA) * xPas + temps.vue.dx;
  const xDeAlbum = (a) => (a.annee == null ? xi + largeurIncertain / 2 : xDeAnnee(a.annee));

  // Jitter vertical déterministe (empreinte de l'id) — évite que deux albums
  // de la même année et de la même famille se superposent exactement, sans
  // jamais dépendre de qui d'autre est affiché à côté : la position d'un
  // album ne bouge jamais d'une session à l'autre (stabilité, § 1).
  const jitter = (id) => {
    let h = 2166136261;
    for (let i = 0; i < id.length; i++) {
      h ^= id.charCodeAt(i);
      h = Math.imul(h, 16777619);
    }
    return ((h >>> 0) / 4294967295) * 2 - 1; // [-1, 1]
  };
  const yDeAlbum = (a) => {
    const pos = temps.posFamille.get(a.famille) ?? 0;
    const h = hauteursBandes[pos] ?? hauteur / nBandes;
    const centre = y0Bandes[pos] + h / 2;
    return centre + jitter(a.id) * h * 0.32;
  };

  // Bandes de familles — jamais redistribuées par l'affichage lui-même (seule
  // l'accordéon de la légende change leur hauteur, § 3) : un fond ténu pour
  // situer les groupes, un séparateur entre chacune, son nom en bordure
  // droite (hors zone de zoom horizontal, l'axe des bandes ne bouge jamais).
  temps.bandes.forEach((f, i) => {
    const estompee = carte.isolees.size > 0 && !carte.isolees.has(f);
    ctx.fillStyle = teintes[f % teintes.length] ?? accent;
    ctx.globalAlpha = estompee ? 0.02 : 0.06;
    ctx.fillRect(xDeb - 4, y0Bandes[i], xFin - xDeb + 8, hauteursBandes[i]);
  });
  ctx.globalAlpha = 1;
  ctx.strokeStyle = encre;
  ctx.lineWidth = 1;
  for (let i = 0; i <= nBandes; i++) {
    const y = i < nBandes ? y0Bandes[i] : yBas;
    ctx.globalAlpha = 0.08;
    ctx.beginPath();
    ctx.moveTo(xDeb - 4, y);
    ctx.lineTo(xFin + 4, y);
    ctx.stroke();
  }
  ctx.textAlign = "left";
  ctx.font = "12px system-ui"; // Règle 11 : rien sous 12 px, canevas compris
  temps.bandes.forEach((f, i) => {
    const estompee = carte.isolees.size > 0 && !carte.isolees.has(f);
    ctx.fillStyle = teintes[f % teintes.length] ?? encre;
    ctx.globalAlpha = estompee ? 0.25 : 0.85;
    ctx.fillText(noms.get(f) || `famille ${f + 1}`, xFin + 8, y0Bandes[i] + hauteursBandes[i] / 2 + 4);
  });
  ctx.globalAlpha = 1;

  // Graduation temporelle : trait fin tous les 6 mois, trait épais tous les
  // 10 ans — l'année ne s'imprime jamais en continu, seulement au survol
  // (voir `survolerTemps`), § 5.
  const anneeDeb = Math.floor(minA) - 1;
  const anneeFin = Math.ceil(maxA) + 1;
  ctx.strokeStyle = encre;
  for (let demi = anneeDeb * 2; demi <= anneeFin * 2; demi++) {
    const annee = demi / 2;
    const x = xDeAnnee(annee);
    if (x < xDeb - 2 || x > xFin + 2) continue;
    const decennie = Number.isInteger(annee) && annee % 10 === 0;
    ctx.lineWidth = decennie ? 1.5 : 1;
    ctx.globalAlpha = decennie ? 0.35 : 0.12;
    ctx.beginPath();
    ctx.moveTo(x, yBas);
    ctx.lineTo(x, yBas + (decennie ? 8 : 4));
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
  ctx.lineWidth = 1;

  // L'intervalle d'années (`filtreAnnee`) : hors intervalle, un voile plutôt
  // qu'une disparition (même règle que l'isolement d'une famille).
  if (intervalleAnneeActif()) {
    const xGauche = xDeAnnee(filtreAnnee.debut);
    const xDroite = xDeAnnee(filtreAnnee.fin);
    ctx.fillStyle = getComputedStyle(document.documentElement).getPropertyValue("--bg").trim() || "#14120E";
    ctx.globalAlpha = 0.65;
    if (xGauche > xDeb) ctx.fillRect(xDeb, yHaut - 4, Math.min(xGauche, xFin) - xDeb, hauteur + 8);
    if (xDroite < xFin) ctx.fillRect(Math.max(xDroite, xDeb), yHaut - 4, xFin - Math.max(xDroite, xDeb), hauteur + 8);
    ctx.globalAlpha = 1;
  }

  // La colonne « dates incertaines » — ces albums n'ont pas de place légitime
  // sur l'axe du temps, mieux vaut le dire que les répartir au hasard dans
  // une année (même traitement que l'ancien streamgraph).
  if (aIncertain) {
    ctx.globalAlpha = 0.25;
    ctx.strokeStyle = encre;
    ctx.setLineDash([3, 3]);
    ctx.beginPath();
    ctx.moveTo(xi + largeurIncertain + 8, yHaut);
    ctx.lineTo(xi + largeurIncertain + 8, yBas);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.fillStyle = encre;
    ctx.globalAlpha = 0.5;
    ctx.font = "12px system-ui";
    ctx.textAlign = "center";
    ctx.fillText("incertain", xi + largeurIncertain / 2, yBas + 14);
    ctx.globalAlpha = 1;
  }

  const parId = new Map(temps.albums.map((a) => [a.id, a]));
  const filtre = (carte.filtre || "").toLowerCase();
  const correspond = (a) =>
    !filtre || a.name.toLowerCase().includes(filtre) || a.artist.toLowerCase().includes(filtre);

  // Fond permanent de liens : même mécanique que l'anneau (`dessinerAnneau`
  // § fond permanent de liens) — un trait ténu et **de force constante** pour
  // tout le réseau, coloré par la famille du premier album de l'arc. Ce n'est
  // que la sélection (plus bas) qui code la force par l'épaisseur/opacité :
  // mélanger les deux langages sur la même couche est ce qui rendait la frise
  // « moins jolie que l'anneau ». Un arc dont l'arrivée correspond à la
  // famille secondaire de l'album cible (hybridation, § 3) ressort à peine —
  // un indice, pas un second langage.
  for (const l of temps.arcs) {
    const a = parId.get(l.a);
    const b = parId.get(l.b);
    if (!a || !b) continue;
    const renforce = b.familleSecondaire != null && b.familleSecondaire === a.famille;
    ctx.strokeStyle = teintes[a.famille % teintes.length] ?? encre;
    ctx.lineWidth = renforce ? 0.9 : 0.6;
    ctx.globalAlpha = renforce ? 0.09 : 0.05;
    ctx.beginPath();
    ctx.moveTo(xDeAlbum(a), yDeAlbum(a));
    ctx.quadraticCurveTo(
      (xDeAlbum(a) + xDeAlbum(b)) / 2,
      (yDeAlbum(a) + yDeAlbum(b)) / 2 - Math.abs(xDeAlbum(b) - xDeAlbum(a)) * 0.08,
      xDeAlbum(b),
      yDeAlbum(b),
    );
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
  ctx.lineWidth = 1;

  // Sélection : les voisins du focal ressortent par-dessus le fond — même
  // mécanique que l'anneau (`dessinerAnneau` § sélection), le même état
  // partagé (`anneau.focal`/`anneau.voisins`, peuplé par `chargerAnneau` —
  // voir le gestionnaire de clic) plutôt qu'un focal propre à la frise. Les
  // vrais voisins soniques (`album_ring`), pas seulement l'arête éparse du
  // fond permanent : épaisseur et opacité codent la force, couleur = famille
  // du voisin (la provenance), jamais de tiret ni de pointillé.
  const focal = anneau.focal ? parId.get(anneau.focal.id) : null;
  if (focal && anneau.voisins.length) {
    const distances = anneau.voisins.map((v) => v.distance);
    const dMin = Math.min(...distances);
    const dMax = Math.max(...distances);
    const forceDe = (d) => (dMax <= dMin ? 0.5 : 1 - (d - dMin) / (dMax - dMin));
    const xf = xDeAlbum(focal);
    const yf = yDeAlbum(focal);
    for (const v of anneau.voisins) {
      const cible = parId.get(v.id);
      if (!cible) continue;
      const force = forceDe(v.distance);
      const xv = xDeAlbum(cible);
      const yv = yDeAlbum(cible);
      ctx.strokeStyle = teintes[v.famille % teintes.length] ?? accent;
      ctx.globalAlpha = 0.45 + force * 0.55;
      ctx.lineWidth = 1.5 + force * 4;
      ctx.beginPath();
      ctx.moveTo(xf, yf);
      ctx.quadraticCurveTo((xf + xv) / 2, (yf + yv) / 2 - Math.abs(xv - xf) * 0.08, xv, yv);
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
    ctx.lineWidth = 1;
  }

  // Albums : un point par album, teinté par sa famille maîtresse. Un anneau
  // supplémentaire marque les hubs (§ 6) — un signal explicite dans le rendu
  // du point lui-même, visible même quand ses arcs sont estompés à l'état de
  // base (règle « estomper, jamais masquer », `docs/carto-anneau.md`). Le
  // focal, lui, ignore l'estompage : une sélection reste lisible quel que
  // soit le filtre.
  temps.puces = [];
  for (const a of temps.albums) {
    const x = xDeAlbum(a);
    if (a.annee != null && (x < xDeb - 20 || x > xFin + 20)) continue;
    const y = yDeAlbum(a);
    // Culling vertical, symétrique de l'horizontal ci-dessus — utile dès que
    // le zoom ou l'accordéon poussent une bande hors du cadre visible.
    if (y < yHaut - 20 || y > yBas + 20) continue;
    const estFocal = !!focal && a.id === focal.id;
    const estompee =
      !estFocal &&
      ((carte.isolees.size > 0 && !carte.isolees.has(a.famille)) ||
        !dansIntervalleAnnee(a.annee) ||
        (!!filtre && !correspond(a)));
    const hub = a.sortants >= temps.seuilHub;
    const rayon = (hub ? 3.2 : 2.2) + (estFocal ? 1 : 0);
    ctx.fillStyle = teintes[a.famille % teintes.length] ?? accent;
    ctx.globalAlpha = estompee ? 0.15 : 0.85;
    ctx.beginPath();
    ctx.arc(x, y, rayon, 0, Math.PI * 2);
    ctx.fill();
    if (hub) {
      ctx.strokeStyle = teintes[a.famille % teintes.length] ?? accent;
      ctx.globalAlpha = estompee ? 0.15 : 0.75;
      ctx.lineWidth = 1.2;
      ctx.beginPath();
      ctx.arc(x, y, rayon + 2.5, 0, Math.PI * 2);
      ctx.stroke();
      ctx.lineWidth = 1;
    }
    if (estFocal) {
      ctx.strokeStyle = encre;
      ctx.globalAlpha = 0.9;
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.arc(x, y, rayon + (hub ? 5 : 2.5), 0, Math.PI * 2);
      ctx.stroke();
      ctx.lineWidth = 1;
    }
    temps.puces.push({ x, y, album: a });
  }
  ctx.globalAlpha = 1;

  // Pas de tracé de la playlist en cours ici : sur Temps/Anneau, l'affichage
  // ne montre **que** l'album du morceau écouté et ses voisins soniques (le
  // focal, plus haut, piloté par `suivreAlbumEnLecture`). Superposer le
  // trajet de toute la file — qui part de l'album d'origine d'une playlist
  // « alchimie » — ajoutait un second jeu de connexions étranger au disque en
  // cours. `parCle` reste : le halo « album en écoute » juste dessous en a
  // besoin.
  const parCle = new Map(temps.albums.map((a) => [`${a.name} ${a.artist}`, a]));

  // L'album en écoute : même halo que le morceau en écoute sur Nuage/Carte
  // (`surcoucheSur`) — le morceau en cours se ramène à son album par la
  // même clé que `parCle` ci-dessus, `fileAttente.pistes` n'ayant que le nom.
  const pisteEnLecture = fileAttente.pistes.find((x) => x.path === lecture.courant);
  const albumEnLecture =
    pisteEnLecture && parCle.get(`${pisteEnLecture.album || ""} ${pisteEnLecture.artist || ""}`);
  if (albumEnLecture) {
    const x = xDeAlbum(albumEnLecture);
    const y = yDeAlbum(albumEnLecture);
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
    ctx.lineWidth = 1;
  }

  // Le point survolé : un halo, comme sur l'ancien streamgraph.
  if (temps.survole) {
    ctx.strokeStyle = encre;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(temps.survole.x, temps.survole.y, 6, 0, Math.PI * 2);
    ctx.stroke();
    ctx.lineWidth = 1;
  }
}

/// Survol du streamgraph : retrouve le point de fil d'artiste le plus proche
/// du curseur (voir `temps.puces`, reconstruit à chaque `dessinerTemps`) et
/// montre son détail par album — c'est la seule façon de descendre du genre
/// à l'album sur ce mode, faute d'une position à cliquer par morceau.
/// Survol de la frise : l'album le plus proche du curseur, ou — à défaut —
/// l'année sous le curseur quand il traîne sur la bande de graduation, seule
/// façon de lire une année puisqu'elle ne s'imprime plus en continu (§ 5).
function survolerTemps(mx, my, r) {
  let meilleur = null;
  let d2min = 12 * 12;
  for (const p of temps.puces) {
    const d2 = (p.x - mx) ** 2 + (p.y - my) ** 2;
    if (d2 < d2min) {
      d2min = d2;
      meilleur = p;
    }
  }

  let anneeSurvolee = null;
  const d = temps.disposition;
  if (!meilleur && d && d.n > 1) {
    const yBas = r.height - 28;
    if (my >= yBas - 2 && my <= yBas + 14 && mx >= d.xDeb - 4 && mx <= d.xFin + 4) {
      anneeSurvolee = Math.round(
        temps.minAnnee + (mx - d.xDeb - temps.vue.dx) / (d.xPasBase * temps.vue.k),
      );
    }
  }

  const cle = meilleur ? `a:${meilleur.album.id}` : anneeSurvolee != null ? `y:${anneeSurvolee}` : null;
  if (cle === temps.survoleCle) return;
  temps.survoleCle = cle;
  // La puce entière, pas seulement ses coordonnées : le gestionnaire de clic
  // (`cnv.addEventListener("click")`) lit `temps.survole.album` pour peupler
  // l'inspecteur partagé — même schéma que `anneau.survole`.
  temps.survole = meilleur;

  const info = $("carte-info");
  if (meilleur) {
    const a = meilleur.album;
    const noms = new Map(carte.familles.map(([c, nom]) => [c, nom]));
    const fam = noms.get(a.famille) || `famille ${a.famille + 1}`;
    const secondaire = a.familleSecondaire != null ? ` + ${noms.get(a.familleSecondaire) || ""}` : "";
    const hub = a.sortants >= temps.seuilHub ? " · hub" : "";
    info.hidden = false;
    info.innerHTML = "<b></b><span></span>";
    info.children[0].textContent = `${isoler(a.name)} — ${isoler(a.artist)}`;
    info.children[1].textContent = `${a.annee ?? "date incertaine"} · ${fam}${secondaire}${hub}`;
    info.style.left = `${Math.min(mx + 14, r.width - 290)}px`;
    info.style.top = `${my + 14}px`;
  } else if (anneeSurvolee != null) {
    info.hidden = false;
    info.innerHTML = "<b></b>";
    info.children[0].textContent = String(anneeSurvolee);
    info.style.left = `${Math.min(mx + 10, r.width - 100)}px`;
    info.style.top = `${my - 24}px`;
  } else {
    info.hidden = true;
  }
  dessinerCarte();
}

/// Applique un facteur de zoom autour d'un point de l'écran.
/// Zoom horizontal du streamgraph, autour de `cx` (pixel écran, l'axe des
/// familles empilées ne zoome jamais). Même principe que le zoom du nuage :
/// on retrouve le bucket sous le curseur avant le geste, on change d'échelle,
/// puis on décale pour l'y remettre — sans quoi zoomer ferait fuir le point
/// qu'on visait plutôt que s'en rapprocher.
/// Zoom de la frise — un seul facteur (`temps.vue.k`) pour les deux axes à la
/// fois, exactement comme `zoomer` sur le nuage (`carte.vue.k` juste plus
/// bas) : un geste de molette/pincement zoome le temps et les bandes
/// ensemble, glisser déplace les deux (voir le gestionnaire `mousemove`).
/// Deux axes, donc deux pivots à préserver sous le curseur — `cx` pour
/// l'année (repère `xDeb`/`xPasBase`), `cy` pour la bande (repère `yHaut`) —
/// mais un seul `k`, recalculé une fois, jamais deux facteurs qui pourraient
/// diverger l'un de l'autre.
function zoomerTemps(f, cx, cy) {
  const d = temps.disposition;
  if (!d) return;
  const xSousCurseur = d.n >= 2 ? (cx - d.xDeb - temps.vue.dx) / (d.xPasBase * temps.vue.k) : null;
  const ySousCurseur = (cy - d.yHaut - temps.vue.dy) / temps.vue.k;
  temps.vue.k = Math.min(40, Math.max(1, temps.vue.k * f));
  if (xSousCurseur != null) {
    temps.vue.dx = cx - d.xDeb - xSousCurseur * d.xPasBase * temps.vue.k;
  }
  temps.vue.dy = cy - d.yHaut - ySousCurseur * temps.vue.k;
  $("zoom-val").textContent = `×${temps.vue.k.toFixed(1).replace(".", ",")}`;
  dessinerCarte();
}

export {
  dessinerTemps, hexHSL, hslRGB, survolerTemps, zoomerTemps,
};
