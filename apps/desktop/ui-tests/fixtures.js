// SPDX-License-Identifier: GPL-3.0-or-later
// Réponses figées des commandes Tauri, sur une bibliothèque fictive : deux
// artistes, trois albums. Formes recopiées des types Rust renvoyés
// (`apps/desktop/src/main.rs`, `crates/core/src/db.rs`) ; une commande
// absente d'ici rend `null` et apparaît dans le rapport du test.
//
// Les réponses calculées (fonctions) passent dans la page sous forme de texte :
// elles n'ont pas accès à ce module, seulement à `window.__DONNEES__`
// (`PISTES`, `ALBUMS`) et à l'état partagé `e`.
const RACINE = "/Musique";
const piste = (artiste, album, n, titre, annee) => ({
  id: n,
  path: `${RACINE}/${artiste}/${album}/${String(n).padStart(2, "0")} ${titre}.flac`,
  title: titre,
  artist: artiste,
  album,
  track_no: n,
  year: annee,
  duration_ms: 180_000 + n * 1000,
  artist_mbid: null,
});
const PISTES = [
  piste("Les Essais", "Premier banc", 1, "Ouverture", 1999),
  piste("Les Essais", "Premier banc", 2, "Deuxième prise", 1999),
  piste("Les Essais", "Second banc", 3, "Retour", 2004),
  piste("Orchestre Témoin", "Fumée", 4, "Largo", 2012),
  piste("Orchestre Témoin", "Fumée", 5, "Presto", 2012),
];
const album = (artiste, nom, annee) => {
  const ps = PISTES.filter((p) => p.album === nom);
  return { name: nom, artist: artiste, year: annee, tracks: ps.length, path: ps[0].path, ajoute_le: 1_700_000_000 };
};
const ALBUMS = [
  album("Les Essais", "Premier banc", 1999),
  album("Les Essais", "Second banc", 2004),
  album("Orchestre Témoin", "Fumée", 2012),
];

const fixtures = {
  // --- démarrage
  mode_initial: null,
  autotest_carte: false,
  essai_lama: { affichage: null, prompt: null, theme: null, audit: false },
  js_error: null,
  set_normalisation: null,
  favoris: [],
  roots: [{ path: RACINE, added_at: 1_700_000_000, last_scan: 1_700_000_100, tracks: PISTES.length }],
  artists: [
    { name: "Les Essais", mbid: null, tracks: 3, albums: 2 },
    { name: "Orchestre Témoin", mbid: null, tracks: 2, albums: 1 },
  ],
  albums: ALBUMS,
  // Le lecteur : `play` remplit la file, `playback_state` la rend.
  playback_state: (_, e) => ({
    current: e.file?.[0] ?? null,
    paused: !e.file,
    finished: false,
    position_ms: e.file ? 12_000 : 0,
    remaining: e.file ? e.file.length - 1 : 0,
    volume: 1,
    repetition: "aucune",
    alea: false,
    verrou: 0,
    ignorees: [],
    minuteur: null,
  }),
  tracks_of_album: (a) => window.__DONNEES__.PISTES.filter((p) => p.album === a.album),
  cover: null,
  play: (a, e) => {
    e.file = a.paths;
    return null;
  },
  set_queue: (a, e) => {
    e.file = a.paths;
    return null;
  },
};

// --- familles (numéro, nom, effectif) et carte en nuage de points
Object.assign(fixtures, {
  families: [
    [0, "Essais", 3],
    [1, "Orchestre", 2],
  ],
  families_detail: [
    { cluster: 0, effectif: 3, fiable: true, nom_affiche: "Essais", musicbrainz: "Essais", clap_texte: "Essais", lastfm: "" },
    { cluster: 1, effectif: 2, fiable: true, nom_affiche: "Orchestre", musicbrainz: "Orchestre", clap_texte: "Orchestre", lastfm: "" },
  ],
  album_families: ALBUMS.map((a, i) => [a.name, a.artist, i < 2 ? 0 : 1]),
  artist_families: [
    ["Les Essais", 0],
    ["Orchestre Témoin", 1],
  ],
  map_view: PISTES.map((p, i) => ({
    id: p.id,
    path: p.path,
    x: (i - 2) * 0.3,
    y: (i % 2) * 0.4 - 0.2,
    cluster: p.artist === "Les Essais" ? 0 : 1,
    title: p.title,
    artist: p.artist,
    album_artist: p.artist,
    album: p.album,
    track_no: p.track_no,
    year: p.year,
    duration_ms: p.duration_ms,
    bpm: 100 + i * 10,
    energy: 0.2 * i,
    popularite: 0.5,
  })),
  map_progress: [PISTES.length, PISTES.length],
  // Pas de tuiles servies : `positions_carte` échoue comme sans ville importée.
  positions_carte: new Error("positions introuvables (essai)"),
  // --- Bibliothèque
  library_stats: (() => {
    const h = { min: 0, pas: 10, comptes: [1, 2, 2], hors_gamme: 0, sans_valeur: 0 };
    return {
      total: PISTES.length,
      genres: [["Essai", 3], ["Orchestre", 2]],
      tempo: h,
      durees: h,
      codecs: [["flac", PISTES.length]],
      bitrate: h,
      sans_mbid: PISTES.length,
      humeur: [],
      sans_bio: 2,
      sans_critique: 3,
      sans_discogs: 3,
    };
  })(),
  popularite_fraicheur: [0, null, 0, PISTES.length],
  discogs_dump_info: null,
  discogs_dump_state: { en_cours: false, octets: 0, total: null, resultat: null },
  verif_pochettes_state: { en_cours: false, faits: 0, total: 0, resultat: null },
  map_parameters: {
    perplexite: 30,
    epoques: 1000,
    familles: 12,
    iterations_kmeans: 50,
    densite_noyau: 0.05,
    densite_resolution: 128,
    densite_bandes: 6,
  },
  analysis_state: { en_cours: false, faits: 0, total: 0, resultat: null },
  pochettes_suspectes: [],
  lien_ecrire_critique: null,
  decouvrir_tout_vu: null,
  vocabulaire_familles: [],
  a_propos: { version: "0.0.0-essai", modeles: [] },
  stems_cache: [0, 0],
  superres_cache: [0, 0],
  stockage_logiciel: { executable: 1, modeles: 1, base: 1, ville: 0 },
  dossier_donnees: "/Données",
  suspect_genres: [],
  scan_state: { en_cours: false, racine: "", morceaux: PISTES.length, resultat: null },
  // --- Éditer : quatre stems déjà calculés pour chaque morceau
  demix_variantes: [
    { nom: "htdemucs", presente: true, megaoctets: 84 },
    { nom: "htdemucs_6s", presente: false, megaoctets: 55 },
    { nom: "htdemucs_ft", presente: false, megaoctets: 336 },
    { nom: "bsrofo_sw", presente: false, megaoctets: 699 },
  ],
  stems_existants: (a) =>
    ["drums", "bass", "other", "vocals"].map((n) => [n, `/stems/${a.path.split("/").pop()}/${n}.wav`]),
  stems_state: (_, e) => ({
    actif: !!e.stems,
    en_pause: !e.stemsJoue,
    position_ms: e.stemsJoue ? 5_000 : 0,
    duree_ms: 181_000,
    noms: e.stems || [],
    niveaux: (e.stems || []).map(() => 1),
    vitesses: (e.stems || []).map(() => 1),
    derive_ms: 0,
    boucle: null,
    passages: 0,
    latence_ms: 20,
  }),
  stems_play: (a, e) => {
    e.stems = ["drums", "bass", "other", "vocals"];
    e.stemsJoue = true;
    return e.stems;
  },
  // L'anneau et la frise : pas d'arc, les albums seuls.
  reseau_albums: {
    albums: ALBUMS.map((a, i) => ({ id: i + 1, name: a.name, artist: a.artist, famille: i < 2 ? 0 : 1, famille_secondaire: null, annee: a.year })),
    arcs: [],
  },
  // Réglages neutres : le moteur rend les chemins d'origine sans calcul.
  start_etirer: (a) => ({ en_cours: false, faits: 0, total: 0, stems: a.stems, erreur: null }),
  stems_transport: null,
  stem_spectre: (a) => ({ largeur: a.width, hauteur: a.height, pixels: new Array(a.width * a.height).fill(40) }),
  spectre_transport: { largeur: 4, hauteur: 2, pixels: [0, 50, 100, 150, 200, 250, 30, 60], pixels_ref: null, hd: false },
  pulsation: { temps: [0.5, 1.0, 1.5, 2.0], premiers_temps: [0.5], bpm: 120, metrique: 4 },
  tempo_cible: { bpm: 120, nettete: 3 },
  transcriptions_en_cache: [],
  sections_disponibles: false,
  // --- l'inspecteur du morceau en lecture : rien d'enrichi
  descripteurs: null,
  qualite_piste: null,
  famille_piste: 0,
  neighbours: [],
  credits_piste: [],
  labels_piste: [],
  critiques_piste: [],
  superres_state: { en_cours: false, source: "", faits: 0, total: 0, telecharges: 0, a_telecharger: 0, resultat: null },
  superres_disponible: false,
  multiple_editions: [],
  loudness_state: { en_cours: false, faits: 0, total: 0, resultat: null },
  // --- Découvrir, Valise
  decouvrir_feed: { derniere_passe: null, sorties: [], collaborations: [], voisins: [] },
  valise_reglages: {
    parts: [{ regle: "recents", albums: 2, famille: null }],
    budget_octets: 8_000_000_000,
    pistes_min: 4,
    par_artiste: 2,
    graine: 1,
    epingles: [],
    exclus: [],
    modele: "",
  },
  valise_apercu: { albums: [], octets: 0, budget_octets: 8_000_000_000 },
  valise_iphone: { xcode: false, appareils: [], udid: null, app_installee: false, echeance: null, valises: [], renouvelable: false, erreur: null },
  valise_etat: {
    envoi: { en_cours: false, etape: "", faits: 0, total: 0, resultat: null },
    renouvellement: { en_cours: false, ligne: "", resultat: null },
  },
});

// Les passes de fond, toutes au repos.
for (const cmd of [
  "biographies_state",
  "critiques_state",
  "descripteurs_state",
  "discogs_import_state",
  "discogs_liaison_state",
  "enrichment_state",
  "lastfm_state",
  "popularite_state",
  "decouvrir_state",
  "demix_state",
  "etirer_state",
]) {
  fixtures[cmd] ??= { en_cours: false, faits: 0, total: 0, resultat: null };
}
Object.assign(fixtures, {
  scan_failures: [],
  stems_vitesse: null,
  stems_gain: null,
});

module.exports = { fixtures, donnees: { PISTES, ALBUMS }, PISTES, ALBUMS };
