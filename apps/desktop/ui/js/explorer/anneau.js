import "./temps.js"; // l'ordre d'exécution : voir main.js
// Explorer → Anneau : l'album focal et ses voisins (`docs/carto-anneau.md`),
// le regroupement des liens par famille, le survol et le zoom.
import {
  $, duree, fileAttente,
} from "../socle.js";
import {
  anneau, carte, cnv, ctx, dansIntervalleAnnee,
} from "../carte.js";
import { lecture } from "../etat-lecture.js";
import { dessinerCarte } from "./nuage.js";
import { hexHSL, hslRGB } from "./temps.js";
import { couleursFamilles } from "../familles.js";

/// L'anneau du mode Explorer → Anneau : un graphe circulaire à liens groupés
/// centré sur l'album focal (`anneau.focal`) — voir l'objet `anneau` et
/// `Library::album_embeddings`/`album_ring` côté cœur. Inspiré du rendu
/// d'Eigenfactor (Moritz Stefaner) : un anneau extérieur pour les familles,
/// nommées, un second anneau intérieur pour les albums.
///
/// Anneau extérieur = les 12 familles, longueur d'arc ∝ effectif
/// (`carte.familles`, déjà chargé pour la légende du rail), nom affiché en
/// bordure. Anneau intérieur = le focal et ses voisins, répartis sur le
/// sous-arc de leur famille pour ne jamais se chevaucher, chacun un arc dont
/// l'**épaisseur radiale ∝ la durée totale de l'album** (`duree_ms`) —
/// demande explicite : « proche de l'esprit Eigenfactor ». Liens = une
/// courbe quadratique par voisin, tirée vers le centre (simplification à un
/// seul foyer d'un bundling hiérarchique complet : le focal est déjà la
/// racine, pas besoin d'ancêtre commun intermédiaire) dont l'**épaisseur ∝
/// la proximité d'empreinte** (`v.distance`, normalisée parmi les voisins
/// affichés — l'échelle absolue d'une distance CLAP n'a pas de sens en soi).
/// Chemin dans la hiérarchie genre/album entre deux feuilles — voir
/// `docs/carto-anneau.md` § diagnostic. Deux niveaux seulement (racine →
/// famille → album), mais suffisants pour un vrai bundling hiérarchique :
/// deux albums d'une même famille partagent son nœud comme plus proche
/// ancêtre commun (chemin à 3 points), deux albums de familles distinctes ne
/// partagent que la racine (chemin à 5 points, en passant par les deux
/// nœuds de famille). C'est ce partage qui fait converger les liens vers une
/// même famille avant qu'ils ne divergent vers leurs feuilles respectives —
/// pas un simple rayonnement depuis le centre.
function cheminAncetres(posA, familleA, posB, familleB, hubs, racine) {
  if (familleA === familleB) return [posA, hubs.get(familleA), posB];
  return [posA, hubs.get(familleA), racine, hubs.get(familleB), posB];
}

/// Tension de Holten (*Hierarchical Edge Bundles*, 2006) : à `beta = 1`, la
/// courbe suit exactement le chemin par la hiérarchie (`chemin`) ; à `beta =
/// 0`, elle redevient la droite entre les deux feuilles. Chaque point
/// intermédiaire est le mélange des deux, comme `d3.curveBundle.beta` — les
/// extrémités, elles, restent toujours exactement les feuilles. Réglable
/// dans le rail (`anneau.beta`) : l'effet se juge à l'œil, pas par un calcul.
function courbeBundle(chemin, beta) {
  const n = chemin.length;
  if (n < 3) return chemin;
  const [dx, dy] = chemin[0];
  const [fx, fy] = chemin[n - 1];
  return chemin.map((p, i) => {
    if (i === 0 || i === n - 1) return p;
    const t = i / (n - 1);
    const tx = dx + (fx - dx) * t;
    const ty = dy + (fy - dy) * t;
    return [beta * p[0] + (1 - beta) * tx, beta * p[1] + (1 - beta) * ty];
  });
}

/// Une ligne lissée par une courbe quadratique passant par le milieu de
/// chaque paire de points consécutifs — même technique que `dessinerTemps`,
/// pour un faisceau organique plutôt qu'une polyligne à coudes.
function tracerLigneLissee(pts) {
  if (pts.length < 2) return;
  ctx.moveTo(pts[0][0], pts[0][1]);
  for (let i = 1; i < pts.length - 1; i++) {
    const mx = (pts[i][0] + pts[i + 1][0]) / 2;
    const my = (pts[i][1] + pts[i + 1][1]) / 2;
    ctx.quadraticCurveTo(pts[i][0], pts[i][1], mx, my);
  }
  const dernier = pts[pts.length - 1];
  ctx.lineTo(dernier[0], dernier[1]);
}

/// Désature une teinte de famille pour le fond (anneaux), en gardant sa
/// teinte et sa clarté — même principe que la carte topographique
/// (`docs/carto-direction.md`) : la couleur reste rare et porte un sens,
/// elle ne doit pas crier. Les liens, eux, gardent la teinte pleine : c'est
/// ce qu'on est venu regarder, pas le fond.
function desature(hex, facteur = 0.4) {
  const [h, s, l] = hexHSL(hex);
  const [r, g, b] = hslRGB(h, s * facteur, l);
  return `rgb(${r}, ${g}, ${b})`;
}

/// Trait de rappel + étiquette externe pour un voisin du focal sélectionné —
/// révélation par sélection, pas un nom permanent sur chaque segment (voir
/// `docs/carto-anneau.md` § étiquettes). Comme les noms de famille, mais
/// radial depuis la position réelle du nœud plutôt que le milieu d'un arc.
function dessinerRappel(pos, nom, cx, cy, rayonExt, encre) {
  const angle = Math.atan2(pos[1] - cy, pos[0] - cx);
  const bout = [cx + Math.cos(angle) * (rayonExt + 26), cy + Math.sin(angle) * (rayonExt + 26)];
  ctx.strokeStyle = encre;
  ctx.globalAlpha = 0.45;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(pos[0], pos[1]);
  ctx.lineTo(bout[0], bout[1]);
  ctx.stroke();

  const gauche = Math.cos(angle) < 0;
  ctx.save();
  ctx.translate(bout[0], bout[1]);
  ctx.rotate(gauche ? angle + Math.PI : angle);
  ctx.fillStyle = encre;
  ctx.globalAlpha = 0.9;
  ctx.font = "12px system-ui";
  ctx.textAlign = gauche ? "right" : "left";
  ctx.fillText(nom, 0, 0);
  ctx.restore();
  ctx.globalAlpha = 1;
}

function dessinerAnneau(r, encre, accent) {
  // Le fond permanent n'est pas encore revenu de `chargerReseauAnneau` —
  // rien à dessiner tant qu'on n'a pas au moins la couronne des familles.
  if (!anneau.reseau.charge || !carte.familles || carte.familles.length === 0) {
    ctx.fillStyle = encre;
    ctx.globalAlpha = 0.6;
    ctx.font = "13px system-ui";
    ctx.textAlign = "center";
    ctx.fillText("Chargement du réseau…", r.width / 2, r.height / 2);
    ctx.globalAlpha = 1;
    return;
  }

  const teintes = couleursFamilles();
  const cx = r.width / 2 + anneau.vue.dx;
  const cy = r.height / 2 + anneau.vue.dy;
  // Marge plus large qu'un simple anneau : les noms de famille et les
  // étiquettes de rappel débordent au dehors de `rayonExt`. Mise à l'échelle
  // par `anneau.vue.k` : tous les rayons qui en découlent (album, hub) grossissent
  // avec elle, comme un cercle qu'on zoome — les épaisseurs de trait restent
  // fixes, en pixels écran, lisibles à tout niveau de zoom.
  const rayonExt = (Math.min(r.width, r.height) / 2 - 54) * anneau.vue.k;
  const epaisseurFamille = 10;
  const rayonAlbum = rayonExt - epaisseurFamille - 20;
  // Le nœud de famille du bundling hiérarchique — entre l'anneau des albums
  // et le centre, jamais confondu avec `rayonAlbum` (une position de calcul,
  // pas un rayon dessiné).
  const rayonHub = rayonAlbum * 0.45;
  const racine = [cx, cy];
  const EPAISSEUR_ALBUM_MIN = 1;
  const EPAISSEUR_ALBUM_MAX = 9;

  // Couronne des familles — angles stables (`anneau.reseau.arcsFamille`,
  // fixés par `calculerAnglesAnneau`), désaturée : c'est le fond, pas ce
  // qu'on est venu lire (voir `desature`).
  const arcsFamille = anneau.reseau.arcsFamille;
  const hubsFamille = new Map(); // cluster -> [x, y], nœud de la hiérarchie
  ctx.lineWidth = epaisseurFamille;
  ctx.textBaseline = "middle";
  for (const [cluster, nom, n] of carte.familles) {
    const bornes = arcsFamille.get(cluster);
    if (!bornes) continue;
    const { debut, fin } = bornes;
    const milieu = (debut + fin) / 2;
    hubsFamille.set(cluster, [cx + Math.cos(milieu) * rayonHub, cy + Math.sin(milieu) * rayonHub]);

    ctx.strokeStyle = desature(teintes[cluster % teintes.length] ?? accent);
    ctx.beginPath();
    ctx.arc(cx, cy, rayonExt - epaisseurFamille / 2, debut, fin);
    ctx.stroke();

    // Le nom, radial — retourné sur la moitié gauche du cercle pour ne
    // jamais se lire la tête en bas, comme sur un cadran ordinaire. Permanent
    // pour la famille (12 noms, lisibles) — pas pour chaque album, voir plus
    // bas.
    const gauche = Math.cos(milieu) < 0;
    ctx.save();
    ctx.translate(cx + Math.cos(milieu) * (rayonExt + 8), cy + Math.sin(milieu) * (rayonExt + 8));
    ctx.rotate(gauche ? milieu + Math.PI : milieu);
    ctx.fillStyle = encre;
    ctx.globalAlpha = 0.8;
    ctx.font = "12px system-ui";
    ctx.textAlign = gauche ? "right" : "left";
    ctx.fillText(nom || `famille ${cluster + 1}`, 0, 0);
    ctx.restore();
  }
  ctx.lineWidth = 1;
  ctx.globalAlpha = 1;

  // Position de chaque album — stable, jamais recalculée au changement de
  // focal (voir `docs/carto-anneau.md` § stabilité des positions) : c'est
  // l'angle mémorisé par `calculerAnglesAnneau` qui est fixe, seuls `cx`/`cy`/
  // les rayons (dépendants de la taille du canevas) sont recalculés ici.
  const positions = new Map();
  const parAlbum = new Map(anneau.reseau.albums.map((a) => [a.id, a]));
  for (const a of anneau.reseau.albums) {
    const angle = anneau.reseau.angleAlbum.get(a.id);
    if (angle === undefined) continue;
    positions.set(a.id, [cx + Math.cos(angle) * rayonAlbum, cy + Math.sin(angle) * rayonAlbum]);
  }

  // La largeur angulaire d'un album, par famille — même division que
  // `calculerAnglesAnneau`, recalculée ici seulement pour savoir combien de
  // place laisser entre deux arcs voisins (le « seam »).
  const comptesFamille = new Map();
  for (const a of anneau.reseau.albums) {
    if (a.famille == null || !arcsFamille.has(a.famille)) continue;
    comptesFamille.set(a.famille, (comptesFamille.get(a.famille) || 0) + 1);
  }
  const pasParFamille = new Map();
  for (const [cluster, n] of comptesFamille) {
    const { debut, fin } = arcsFamille.get(cluster);
    pasParFamille.set(cluster, (fin - debut) / n);
  }

  // Anneau intérieur : **chaque album connu**, en trait fin et désaturé —
  // c'est la texture tissée permanente qui manquait (voir
  // `docs/carto-anneau.md` § pourquoi ça manque de richesse). Le focal et ses
  // voisins ressortent par-dessus, plus loin dans cette fonction.
  const durees = anneau.reseau.albums.map((a) => a.duree_ms || 0);
  const dureeMin = Math.min(...durees);
  const dureeMax = Math.max(...durees);
  const epaisseurAlbum = (duree) => {
    if (dureeMax <= dureeMin) return (EPAISSEUR_ALBUM_MIN + EPAISSEUR_ALBUM_MAX) / 2;
    const t = (duree - dureeMin) / (dureeMax - dureeMin);
    return EPAISSEUR_ALBUM_MIN + t * (EPAISSEUR_ALBUM_MAX - EPAISSEUR_ALBUM_MIN);
  };

  anneau.puces = [];
  for (const a of anneau.reseau.albums) {
    const pos = positions.get(a.id);
    const angle = anneau.reseau.angleAlbum.get(a.id);
    if (!pos || angle === undefined) continue;
    const pas = pasParFamille.get(a.famille) || 0.02;
    const marge = pas * 0.1;

    ctx.strokeStyle = desature(teintes[a.famille % teintes.length] ?? accent);
    ctx.lineWidth = epaisseurAlbum(a.duree_ms || 0);
    // Même règle que le filtre par famille : un album hors de l'intervalle
    // d'années s'estompe, il ne disparaît jamais.
    ctx.globalAlpha = dansIntervalleAnnee(a.annee) ? 0.55 : 0.08;
    ctx.beginPath();
    ctx.arc(cx, cy, rayonAlbum, angle - pas / 2 + marge, angle + pas / 2 - marge);
    ctx.stroke();

    anneau.puces.push({ id: a.id, x: pos[0], y: pos[1], rayon: 6, noeud: a });
  }
  ctx.globalAlpha = 1;
  ctx.lineWidth = 1;

  // Fond permanent de liens : un sous-ensemble représentatif de tout le
  // réseau (le plus proche voisin de chaque album, voir `reseau_albums`),
  // très ténu — la densité tissée que la sélection vient ensuite isoler,
  // pas remplacer (même règle que le filtre par famille de la carte :
  // estomper, jamais masquer).
  for (const arc of anneau.reseau.arcs) {
    const posA = positions.get(arc.a);
    const posB = positions.get(arc.b);
    if (!posA || !posB) continue;
    const familleA = parAlbum.get(arc.a)?.famille;
    const familleB = parAlbum.get(arc.b)?.famille;
    if (familleA == null || familleB == null) continue;
    const chemin = cheminAncetres(posA, familleA, posB, familleB, hubsFamille, racine);
    ctx.strokeStyle = teintes[familleA % teintes.length] ?? accent;
    ctx.globalAlpha = 0.05;
    ctx.lineWidth = 0.6;
    ctx.beginPath();
    tracerLigneLissee(courbeBundle(chemin, anneau.beta));
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
  ctx.lineWidth = 1;

  // Sélection : les flux du focal ressortent par-dessus le fond — épaisseur
  // et opacité montent nettement, couleur du voisin (la provenance), le
  // reste reste visible mais nettement plus discret. Un seul langage pour la
  // force : jamais de tiret ni de pointillé.
  if (anneau.focal) {
    const posFocal = positions.get(anneau.focal.id);
    if (posFocal && anneau.voisins.length) {
      const distances = anneau.voisins.map((v) => v.distance);
      const dMin = Math.min(...distances);
      const dMax = Math.max(...distances);
      const forceDe = (d) => (dMax <= dMin ? 0.5 : 1 - (d - dMin) / (dMax - dMin)); // 1 : le plus proche
      for (const v of anneau.voisins) {
        const posV = positions.get(v.id);
        if (!posV) continue;
        const chemin = cheminAncetres(posFocal, anneau.focal.famille, posV, v.famille, hubsFamille, racine);
        const force = forceDe(v.distance);
        ctx.strokeStyle = teintes[v.famille % teintes.length] ?? accent;
        ctx.globalAlpha = 0.45 + force * 0.55;
        ctx.lineWidth = 1.5 + force * 4;
        ctx.beginPath();
        tracerLigneLissee(courbeBundle(chemin, anneau.beta));
        ctx.stroke();

        dessinerRappel(posV, v.name, cx, cy, rayonExt, encre);
      }
      ctx.globalAlpha = 1;
      ctx.lineWidth = 1;

      // Le focal lui-même : son arc redessiné en pleine teinte + surépaisseur,
      // pour qu'il ressorte du fond désaturé.
      const angleFocal = anneau.reseau.angleAlbum.get(anneau.focal.id);
      if (angleFocal !== undefined) {
        const pasFocal = pasParFamille.get(anneau.focal.famille) || 0.02;
        ctx.strokeStyle = teintes[anneau.focal.famille % teintes.length] ?? accent;
        ctx.lineWidth = epaisseurAlbum(anneau.focal.duree_ms || 0) + 2;
        ctx.beginPath();
        ctx.arc(cx, cy, rayonAlbum, angleFocal - pasFocal / 2, angleFocal + pasFocal / 2);
        ctx.stroke();
        ctx.lineWidth = 1;
      }
    }
  }

  // Pas de tracé de playlist ici : sur Temps/Anneau, l’affichage ne montre
  // **que** l’album du morceau écouté et ses voisins soniques (le focal, plus
  // haut, piloté par `suivreAlbumEnLecture`). Superposer le trajet de toute la
  // file — qui part de l’album d’origine d’une playlist « alchimie » —
  // ajoutait un second jeu de connexions étranger au disque en cours. `parCle`
  // reste : le halo « album en écoute » juste dessous en a besoin.
  const parCle = new Map(anneau.reseau.albums.map((a) => [`${a.name} ${a.artist}`, a]));
  // L'album en écoute : même halo que le morceau en écoute sur Nuage/Carte
  // (`surcoucheSur`) et que sur la frise (`dessinerTemps`) — même clé
  // `parCle` que ci-dessus pour ramener le morceau en cours à son album.
  const pisteEnLecture = fileAttente.pistes.find((x) => x.path === lecture.courant);
  const albumEnLecture =
    pisteEnLecture && parCle.get(`${pisteEnLecture.album || ""} ${pisteEnLecture.artist || ""}`);
  const posEnLecture = albumEnLecture && positions.get(albumEnLecture.id);
  if (posEnLecture) {
    ctx.strokeStyle = encre;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(posEnLecture[0], posEnLecture[1], 9, 0, Math.PI * 2);
    ctx.stroke();
    ctx.globalAlpha = 0.35;
    ctx.beginPath();
    ctx.arc(posEnLecture[0], posEnLecture[1], 14, 0, Math.PI * 2);
    ctx.stroke();
    ctx.globalAlpha = 1;
    ctx.lineWidth = 1;
  }

  // Le point survolé : un halo, pour le retrouver dans le fouillis des arcs.
  if (anneau.survole) {
    ctx.strokeStyle = encre;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(anneau.survole.x, anneau.survole.y, anneau.survole.rayon, 0, Math.PI * 2);
    ctx.stroke();
    ctx.lineWidth = 1;
  }
}

/// Survol de l'anneau : retrouve le nœud le plus proche du curseur (voir
/// `anneau.puces`, reconstruit à chaque `dessinerAnneau`) et montre son
/// identité — un clic dessus (voir le gestionnaire `click` du canevas) en
/// fait le nouvel album focal.
function survolerAnneau(mx, my, r) {
  let meilleur = null;
  let d2min = 14 * 14;
  for (const p of anneau.puces) {
    const d2 = (p.x - mx) ** 2 + (p.y - my) ** 2;
    if (d2 < d2min) {
      d2min = d2;
      meilleur = p;
    }
  }
  if (meilleur === anneau.survole) return;
  anneau.survole = meilleur;
  const info = $("carte-info");
  if (meilleur) {
    info.hidden = false;
    info.innerHTML = "<b></b><span></span>";
    // Le rôle (focal / antérieur / postérieur) n'a de sens que par rapport à
    // la sélection courante — un album du fond permanent, hors sélection,
    // n'en a aucun.
    const estFocal = anneau.focal && meilleur.id === anneau.focal.id;
    const voisin = anneau.voisins.find((v) => v.id === meilleur.id);
    const role = estFocal ? "album focal" : voisin ? (voisin.avant ? "antérieur" : "postérieur") : null;
    info.children[0].textContent = meilleur.noeud.name;
    info.children[1].textContent = [meilleur.noeud.artist, role, duree(meilleur.noeud.duree_ms)]
      .filter(Boolean)
      .join(" — ");
    info.style.left = `${Math.min(mx + 14, r.width - 290)}px`;
    info.style.top = `${my + 14}px`;
  } else {
    info.hidden = true;
  }
  dessinerCarte();
}

/// Zoom de l'anneau — même principe que le repli sans MapLibre du nuage
/// (`carte.vue.k` juste plus bas) : on retrouve le point sous le curseur
/// avant le geste, on change d'échelle, puis on décale pour l'y remettre.
function zoomerAnneau(f, cx, cy) {
  const r = cnv.getBoundingClientRect();
  const mx = (cx ?? r.width / 2) - r.width / 2;
  const my = (cy ?? r.height / 2) - r.height / 2;
  const avant = anneau.vue.k;
  anneau.vue.k = Math.min(6, Math.max(0.5, anneau.vue.k * f));
  const reel = anneau.vue.k / avant;
  anneau.vue.dx = mx - (mx - anneau.vue.dx) * reel;
  anneau.vue.dy = my - (my - anneau.vue.dy) * reel;
  $("zoom-val").textContent = `×${anneau.vue.k.toFixed(1).replace(".", ",")}`;
  dessinerCarte();
}

export {
  dessinerAnneau, survolerAnneau, zoomerAnneau,
};
