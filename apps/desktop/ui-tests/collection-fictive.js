// SPDX-License-Identifier: GPL-3.0-or-later
// Une collection inventée pour les captures d'écran (`captures.spec.js`) :
// artistes, albums et titres imaginaires, pochettes engendrées (aplats et
// formes géométriques, aucune œuvre réelle), familles et nuage de points
// factices. Jamais la bibliothèque de l'auteur : ni droits sur des pochettes,
// ni vie privée en jeu. Idée reprise de Disc Player (`tests/e2e/mock-collection.mjs`,
// https://github.com/eudj1n/snowsky-disc-player, licence MIT).
//
// Tirages déterministes (graine fixe) : deux lancements donnent les mêmes images.
const { fixtures } = require("./fixtures");

const RACINE = "/Musique";
let graine = 20261101;
const hasard = () => {
  graine = (graine * 1103515245 + 12345) % 2 ** 31;
  return graine / 2 ** 31;
};

// Cinq familles : leur nom, et les artistes qui y vivent.
const FAMILLES = [
  { nom: "Jazz modal", artistes: ["Quartette Lanterne", "Inès Valmont Trio"] },
  { nom: "Électro douce", artistes: ["Brume Nord", "Les Satellites Calmes"] },
  { nom: "Folk", artistes: ["Marthe Aubrac", "Les Chemins Creux"] },
  { nom: "Rock", artistes: ["Ferraille Douce", "فرقة الريح"] },
  { nom: "Classique", artistes: ["Ensemble Clair-Obscur", "夜明けの合唱団"] },
];
const ALBUMS_PAR_ARTISTE = {
  "Quartette Lanterne": ["Nuit sur le canal", "Lanterne magique", "Sept escales"],
  "Inès Valmont Trio": ["Petites heures", "Le pas du chat"],
  "Brume Nord": ["Basse lumière", "Marée haute"],
  "Les Satellites Calmes": ["Orbite lente", "Signaux faibles"],
  "Marthe Aubrac": ["La maison aux volets", "Bois flotté"],
  "Les Chemins Creux": ["Terre battue"],
  "Ferraille Douce": ["Courant alternatif", "Pluie d'acier", "Garage"],
  "فرقة الريح": ["رمال"],
  "Ensemble Clair-Obscur": ["Suites pour un soir", "Pièces brèves"],
  "夜明けの合唱団": ["ひかりの歌"],
};
const MOTS = [
  "Ouverture", "Lent", "Retour", "Fenêtre", "Seuil", "Écume", "Lisière", "Fougère",
  "Minuit", "Passage", "Ricochet", "Halo", "Ardoise", "Clairière", "Escale", "Vertige",
];

/// Une pochette engendrée : deux couleurs tirées d'une teinte, une forme
/// (bandes, disque, carrés, diagonale). Une `data:` URI, comme la commande
/// `cover` du moteur.
function pochette(teinte) {
  const t2 = (teinte + 40 + hasard() * 140) % 360;
  // Saturation retenue : des pochettes d'encre et de papier, pas des néons.
  const fond = `hsl(${teinte} ${18 + hasard() * 22}% ${22 + hasard() * 40}%)`;
  const forme = `hsl(${t2} ${25 + hasard() * 25}% ${50 + hasard() * 30}%)`;
  const motifs = [
    `<rect y="60" width="300" height="40" fill="${forme}"/><rect y="140" width="300" height="20" fill="${forme}" opacity=".7"/><rect y="190" width="300" height="60" fill="${forme}" opacity=".45"/>`,
    `<circle cx="${110 + hasard() * 80}" cy="${110 + hasard() * 80}" r="${55 + hasard() * 50}" fill="${forme}"/>`,
    `<rect x="40" y="40" width="100" height="100" fill="${forme}"/><rect x="160" y="160" width="100" height="100" fill="${forme}" opacity=".6"/>`,
    `<polygon points="0,300 300,0 300,300" fill="${forme}"/>`,
  ];
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 300 300"><rect width="300" height="300" fill="${fond}"/>${motifs[Math.floor(hasard() * motifs.length)]}</svg>`;
  return `data:image/svg+xml;base64,${Buffer.from(svg).toString("base64")}`;
}

const PISTES = [];
const ALBUMS = [];
const POCHETTES = {};
const FAMILLE_DE_L_ALBUM = [];
let id = 1;
FAMILLES.forEach((famille, f) => {
  for (const artiste of famille.artistes) {
    for (const nom of ALBUMS_PAR_ARTISTE[artiste]) {
      const annee = 1962 + Math.floor(hasard() * 62);
      const n = 4 + Math.floor(hasard() * 6);
      const premiere = PISTES.length;
      for (let k = 1; k <= n; k++) {
        const titre = MOTS[Math.floor(hasard() * MOTS.length)] + (hasard() < 0.4 ? ` ${k}` : "");
        PISTES.push({
          id: id++,
          path: `${RACINE}/${artiste}/${nom}/${String(k).padStart(2, "0")} ${titre}.flac`,
          title: titre,
          artist: artiste,
          album: nom,
          track_no: k,
          year: annee,
          duration_ms: 120_000 + Math.floor(hasard() * 300_000),
          artist_mbid: null,
        });
      }
      const chemin = PISTES[premiere].path;
      POCHETTES[chemin] = pochette((f * 72 + hasard() * 50) % 360);
      for (let k = premiere; k < PISTES.length; k++) POCHETTES[PISTES[k].path] = POCHETTES[chemin];
      ALBUMS.push({
        name: nom, artist: artiste, year: annee, tracks: n, path: chemin,
        ajoute_le: 1_700_000_000 + Math.floor(hasard() * 30_000_000),
      });
      FAMILLE_DE_L_ALBUM.push(f);
    }
  }
});
ALBUMS.sort((a, b) => a.name.localeCompare(b.name, "fr"));

const famille = (artiste) => FAMILLES.findIndex((f) => f.artistes.includes(artiste));
const ARTISTES = FAMILLES.flatMap((f) => f.artistes)
  .map((nom) => ({
    name: nom, mbid: null,
    tracks: PISTES.filter((p) => p.artist === nom).length,
    albums: ALBUMS.filter((a) => a.artist === nom).length,
  }))
  .sort((a, b) => a.name.localeCompare(b.name, "fr"));

// Le nuage : une région par famille, chaque morceau un peu dispersé autour.
const CENTRES = [[-0.55, -0.4], [0.5, -0.5], [-0.6, 0.45], [0.55, 0.4], [0, 0.05]];
const MAP_VIEW = PISTES.map((p) => {
  const f = famille(p.artist);
  const [cx, cy] = CENTRES[f];
  const r = 0.32 * Math.sqrt(hasard());
  const a = hasard() * 2 * Math.PI;
  return {
    id: p.id, path: p.path, x: cx + r * Math.cos(a), y: cy + r * Math.sin(a), cluster: f,
    title: p.title, artist: p.artist, album_artist: p.artist, album: p.album,
    track_no: p.track_no, year: p.year, duration_ms: p.duration_ms,
    bpm: 70 + Math.floor(hasard() * 90), energy: hasard(), popularite: hasard(),
  };
});

const effectifs = FAMILLES.map((_, f) => PISTES.filter((p) => famille(p.artist) === f).length);

const fixturesCollection = {
  ...fixtures,
  artists: ARTISTES,
  albums: ALBUMS,
  roots: [{ path: RACINE, added_at: 1_700_000_000, last_scan: 1_700_000_100, tracks: PISTES.length }],
  cover: (a) => window.__DONNEES__.POCHETTES[a.path] ?? null,
  families: FAMILLES.map((f, i) => [i, f.nom, effectifs[i]]),
  families_detail: FAMILLES.map((f, i) => ({
    cluster: i, effectif: effectifs[i], fiable: true, nom_affiche: f.nom,
    musicbrainz: f.nom, clap_texte: f.nom, lastfm: "",
  })),
  album_families: ALBUMS.map((a) => [a.name, a.artist, famille(a.artist)]),
  artist_families: ARTISTES.map((a) => [a.name, famille(a.name)]),
  map_view: MAP_VIEW,
  map_progress: [PISTES.length, PISTES.length],
  // Un spectrogramme engendré : des graves soutenus, des médiums qui pulsent,
  // des aigus plus rares — assez pour que le transport et la vue « En cours »
  // aient quelque chose à montrer.
  spectre_transport: (a) => {
    const w = a.width, h = a.height, pixels = new Array(w * h);
    for (let x = 0; x < w; x++) {
      const temps = x / w;
      for (let y = 0; y < h; y++) {
        // Comme `crates/player/src/spectre.rs` : la rangée du haut = les aigus.
        const grave = y / (h - 1); // 1 en bas (graves), 0 en haut
        const pulse = 0.5 + 0.5 * Math.sin(temps * 90 + y * 0.6);
        const v = 255 * Math.min(1, grave ** 1.6 * 0.9 + pulse * 0.35 * (0.3 + grave));
        pixels[y * w + x] = Math.round(v);
      }
    }
    return { largeur: w, hauteur: h, pixels, pixels_ref: null, hd: false };
  },
  famille_piste: (a) => {
    const p = window.__DONNEES__.PISTES.find((x) => x.id === a.id);
    return p ? window.__DONNEES__.FAMILLE_PAR_ARTISTE[p.artist] : null;
  },
};

const donneesCollection = {
  PISTES,
  ALBUMS,
  POCHETTES,
  FAMILLE_PAR_ARTISTE: Object.fromEntries(ARTISTES.map((a) => [a.name, famille(a.name)])),
};

module.exports = { fixturesCollection, donneesCollection, PISTES, ALBUMS };
