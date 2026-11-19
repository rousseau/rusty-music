// SPDX-License-Identifier: GPL-3.0-or-later
// Réponses figées des commandes Tauri, sur une bibliothèque fictive : deux
// artistes, trois albums. Formes recopiées des types Rust renvoyés
// (`apps/desktop/src/`, `crates/core/src/db/`) ; une commande
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
  popularites: [],
  // Comme l'index FTS du moteur : insensible à la casse et aux diacritiques.
  search: (a) => {
    const plier = (t) => String(t ?? "").normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
    const mots = plier(a.query).split(/\s+/).filter(Boolean);
    return window.__DONNEES__.PISTES.filter((p) =>
      mots.every((m) => plier(`${p.title} ${p.artist} ${p.album}`).includes(m)));
  },
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
    stems: !!e.stems,
  }),
  lama_disponibilite: { local: "absent", etat: "pret", modeles: ["gemma4:e4b"], detail: null },
  lama_telecharger: null,
  lama_telechargement_state: { en_cours: false, faits: 0, total: 0, resultat: null },
  tracks_of_album: (a) => window.__DONNEES__.PISTES.filter((p) => p.album === a.album),
  cover: null,
  // Comme le moteur, toute commande de lecture coupe les stems.
  play: (a, e) => {
    e.file = a.paths;
    e.stems = null;
    return null;
  },
  set_queue: (a, e) => {
    e.file = a.paths;
    e.stems = null;
    return null;
  },
  file_courante: (_, e) => window.__DONNEES__.PISTES.filter((p) => (e.file ?? []).includes(p.path)),
  // La balade (`explorer/balade.rs`, `BaladeVue`) : le départ joue, le
  // suivant est décidé mais pas encore dans la file. Un carrefour à deux
  // voies (P2) : la pente, proche à l'oreille, et une voie « ailleurs ».
  balade_demarrer: (a, e) => {
    const ps = window.__DONNEES__.PISTES;
    const i = ps.findIndex((p) => p.id === a.depart);
    e.file = [ps[i].path];
    e.stems = null;
    const voie = (p, distance, defaut) => ({
      morceau: p.id, distance, defaut, proche: defaut, lue: false, rue: [],
    });
    const intervention = a.intervention ?? "proposer";
    // Rendue telle quelle (pas `e.balade`, qui est `any`) : sa forme est
    // contrôlée par `npm run types`.
    const balade = {
      terrain: "voisins",
      pistes: [a.depart],
      suivant: ps[(i + 1) % ps.length].id,
      fige: false,
      troncon: [],
      trace: [],
      fin: null,
      intervention,
      allure: a.allure ?? "deriver",
      carrefour: {
        numero: 1,
        voies: [voie(ps[(i + 1) % ps.length], 0.04, true), voie(ps[(i + 3) % ps.length], 0.21, false)],
        prise: 0,
        choisie: false,
        retour: false,
      },
      attend: intervention === "demander",
      revenir: false,
      mesure: { carrefours: 0, choix: 0, ailleurs: 0, retours: 0 },
    };
    e.balade = balade;
    return balade;
  },
  balade_etat: (_, e) => e.balade ?? null,
  balade_arreter: (_, e) => {
    e.balade = null;
    return null;
  },
  balade_choisir: (a, e) => {
    const c = e.balade.carrefour;
    c.prise = a.voie;
    c.choisie = true;
    e.balade.suivant = c.voies[a.voie].morceau;
    e.balade.attend = false;
    return e.balade;
  },
  balade_revenir: (_, e) => e.balade,
  balade_regler: (a, e) => {
    if (!e.balade) return null;
    if (a.intervention) e.balade.intervention = a.intervention;
    if (a.allure) e.balade.allure = a.allure;
    e.balade.attend = e.balade.intervention === "demander" && !e.balade.carrefour?.choisie;
    return e.balade;
  },
  balade_extrait: () => null,
  // Traces et brouillard (P5, `explorer/traces.rs`) : une écoute, pas de
  // sentier (pas de ville), une balade passée de trois morceaux.
  morceaux_ecoutes: [1],
  sentiers_carte: { type: "FeatureCollection", features: [] },
  balades_passees: [
    { id: 7, debut: 1_762_500_000, terrain: "voisins", pas: 3, duree_ms: 543_000, titre: "Un", artiste: "A" },
  ],
  balade_passee: { pistes: [1, 2, 3], trace: [] },
  // Le relief musical (P6, `explorer/relief.rs`) : une courbe de niveau 6
  // bordée de deux morceaux ; l'ouvrir rend trois pistes.
  relief_musical: (a) => ({ grandeur: a.grandeur, version: 1, courbes: 1 }),
  courbe: (a) => ({
    ligne: a.ligne,
    niveau: 6,
    sommet: true,
    points: [[2.33, 48.855], [2.35, 48.855]],
    le_long: [
      { morceau: 1, a: 10, point: [2.335, 48.855] },
      { morceau: 2, a: 900, point: [2.345, 48.855] },
    ],
  }),
  ouvrir_courbe: () => window.__DONNEES__.PISTES.slice(0, 3),
  // Des lieux à soi (P8, `explorer/lieux.rs`) : épingles et chaînes en mémoire.
  epingles: (_, e) => e.epingles ?? [],
  creer_epingle: (a, e) => {
    e.epingles = e.epingles ?? [];
    const ep = { id: e.epingles.length + 1, nom: a.nom || "Sans titre", couleur: a.couleur, emoji: a.emoji, lon: a.lon, lat: a.lat };
    e.epingles.push(ep);
    return ep;
  },
  supprimer_epingle: (a, e) => {
    e.epingles = (e.epingles ?? []).filter((x) => x.id !== a.id);
    return true;
  },
  // Playlists enregistrées (P1) : en mémoire, une déjà là. Une chaîne est une
  // playlist (ids à partir de 100) : ses pistes sont les trois premières.
  __etat_playlists: (e) =>
    (e.playlists ??= [{
      id: 1, nom: "À travailler", origine: null, modifiee_le: 1_700_000_000,
      chemins: [window.__DONNEES__.PISTES[3].path],
    }]),
  listes_intelligentes: [],
  playlists: (_, e) => {
    const ps = window.__DONNEES__.PISTES;
    return [...window.__FIXTURES__.__etat_playlists(e)]
      .sort((a, b) => b.modifiee_le - a.modifiee_le || b.id - a.id)
      .map((p) => {
        const presentes = p.chemins.map((c) => ps.find((t) => t.path === c)).filter(Boolean);
        return {
          id: p.id, nom: p.nom, origine: p.origine, modifiee_le: p.modifiee_le,
          nb_pistes: p.chemins.length, nb_manquantes: p.chemins.length - presentes.length,
          duree_ms: presentes.reduce((s, t) => s + t.duration_ms, 0),
        };
      });
  },
  playlist_pistes: (a, e) => {
    const ps = window.__DONNEES__.PISTES;
    const p = window.__FIXTURES__.__etat_playlists(e).find((x) => x.id === a.id);
    if (!p) return ps.slice(0, 3);
    return p.chemins.map((c) => ps.find((t) => t.path === c)).filter(Boolean);
  },
  nouvelle_playlist: (a, e) => {
    const l = window.__FIXTURES__.__etat_playlists(e);
    const id = Math.max(0, ...l.map((p) => p.id)) + 1;
    e.horloge = (e.horloge ?? 1_800_000_000) + 1;
    l.push({ id, nom: a.nom.trim() || "Sans titre", origine: null, modifiee_le: e.horloge, chemins: [...new Set(a.paths)] });
    return id;
  },
  ajouter_a_playlist: (a, e) => {
    const p = window.__FIXTURES__.__etat_playlists(e).find((x) => x.id === a.id);
    if (!p) throw "cette playlist n'existe plus";
    let ajoutes = 0, deja_presents = 0;
    for (const c of a.paths) {
      if (p.chemins.includes(c)) deja_presents++;
      else { p.chemins.push(c); ajoutes++; }
    }
    if (ajoutes) p.modifiee_le = e.horloge = (e.horloge ?? 1_800_000_000) + 1;
    return { ajoutes, deja_presents };
  },
  // Ici toutes les pistes sont présentes : le rang affiché est le rang.
  retirer_de_playlist: (a, e) => {
    const p = window.__FIXTURES__.__etat_playlists(e).find((x) => x.id === a.id);
    if (p.chemins[a.rang] !== a.path) throw "la playlist a changé entre-temps : elle est rouverte telle qu'elle est";
    p.chemins.splice(a.rang, 1);
    return null;
  },
  remettre_dans_playlist: (a, e) => {
    const p = window.__FIXTURES__.__etat_playlists(e).find((x) => x.id === a.id);
    p.chemins.splice(a.rang, 0, a.path);
    return null;
  },
  deplacer_dans_playlist: (a, e) => {
    const p = window.__FIXTURES__.__etat_playlists(e).find((x) => x.id === a.id);
    if (p.chemins[a.de] !== a.path) throw "la playlist a changé entre-temps : elle est rouverte telle qu'elle est";
    p.chemins.splice(a.de, 1);
    p.chemins.splice(a.a, 0, a.path);
    return null;
  },
  // P2 : la playlist que Pratiquer enchaîne — une au plus, et le morceau où
  // l'on en est, par chemin (`Library::pratique_playlist`).
  __pratique_playlist: (e) => {
    const c = e.pratiquePlaylist;
    if (!c) return null;
    const p = window.__FIXTURES__.__etat_playlists(e).find((x) => x.id === c.id);
    if (!p) return null;
    const ps = window.__DONNEES__.PISTES;
    const pistes = p.chemins.map((x) => ps.find((t) => t.path === x)).filter(Boolean);
    return { playlist_id: p.id, nom: p.nom, pistes, rang: Math.max(0, pistes.findIndex((t) => t.path === c.path)) };
  },
  pratique_playlist: (_, e) => window.__FIXTURES__.__pratique_playlist(e),
  pratique_playlist_poser: (a, e) => {
    if (!window.__FIXTURES__.__etat_playlists(e).some((x) => x.id === a.id)) return null;
    e.pratiquePlaylist = { id: a.id, path: a.path };
    return window.__FIXTURES__.__pratique_playlist(e);
  },
  pratique_playlist_fermer: (_, e) => {
    e.pratiquePlaylist = null;
    return null;
  },
  // Le démixage d'avance : demandé, puis fini au premier sondage (les stems
  // de la bibliothèque fictive sont tous là).
  pratique_preparer: () => /** @type {const} */ ("prepare"),
  pratique_preparation: { en_cours: false, courant: null, en_pause: false, attente: null, echecs: [] },
  chaines: (_, e) => e.chaines ?? [],
  garder_chaine: (a, e) => {
    e.chaines = e.chaines ?? [];
    const id = 100 + e.chaines.length;
    e.chaines.push({ playlist_id: id, nom: a.nom, couleur: a.couleur, origine: a.origine, trace: a.trace, nb_pistes: a.morceaux.length });
    return id;
  },
  oublier_trace: (a, e) => {
    e.chaines = (e.chaines ?? []).filter((c) => c.playlist_id !== a.id);
    return true;
  },
  balade_rejouer: (a, e) => {
    const ids = [1, 2, 3];
    e.file = window.__DONNEES__.PISTES.filter((p) => ids.includes(p.id)).map((p) => p.path);
    e.stems = null;
    return null;
  },
  // L'aura (P4) : le moteur rend le temps passé à charger les voix, en ms.
  aura: () => 1.5,
  // « Recalculer la carte » (`explorer/voisinage.rs`, `RapportCarte`) : les
  // cinq morceaux, deux familles, le nuage gardé.
  recompute_map: { empreintes: 5, familles: 2, projection: "gardee" },
  tuiles_state: { en_cours: false, faits: 0, total: 3, resultat: null },

  // --- familles (numéro, nom, effectif) et carte en nuage de points
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
  // --- Pratiquer et Créer : quatre stems déjà calculés pour chaque morceau
  demix_variantes: [
    { nom: "htdemucs", presente: true, megaoctets: 84 },
    { nom: "htdemucs_6s", presente: false, megaoctets: 55 },
    { nom: "htdemucs_ft", presente: false, megaoctets: 336 },
    { nom: "bsrofo_sw", presente: false, megaoctets: 699 },
  ],
  // V38 : aucune piste presque vide, sauf si un test en pose.
  stems_presence: [],
  stems_existants: (a) =>
    ["drums", "bass", "other", "vocals"].map((n) => [n, `/stems/${a.path.split("/").pop()}/${n}.wav`]),
  stems_state: (_, e) => ({
    actif: !!e.stems,
    en_pause: !e.stemsJoue,
    // Un déplacement (`stems_transport` « deplacer ») est retenu ; sinon la
    // position est figée.
    position_ms: e.positionMs ?? (e.stemsJoue ? 5_000 : 0),
    duree_ms: 181_000,
    noms: e.stems || [],
    niveaux: (e.stems || []).map(() => 1),
    vitesses: (e.stems || []).map(() => e.vitesse ?? 1),
    derive_ms: 0,
    boucle: null,
    // Un test fait « finir » un passage de boucle en posant `e.passages` (V17).
    passages: e.passages ?? 0,
    latence_ms: 20,
  }),
  // La vitesse d'ensemble, relue par `appliquerVitesses` dans `stems_state`.
  stems_vitesse: (a, e) => {
    e.vitesse = a.vitesse;
    return null;
  },
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
  // L'anneau du premier album (`AnneauVue`), quel que soit l'album demandé :
  // les deux autres pour voisins, postérieurs.
  album_ring: (() => {
    const noeud = (a, i) => ({
      id: String(i + 1), name: a.name, artist: a.artist, famille: i < 2 ? 0 : 1, famille_secondaire: null,
      path: null, duree_ms: 600000, annee: a.year,
    });
    return {
      focal: noeud(ALBUMS[0], 0),
      voisins: ALBUMS.slice(1).map((a, i) => ({ ...noeud(a, i + 1), avant: false, distance: 0.1 * (i + 1) })),
    };
  })(),
  // Réglages neutres : le moteur rend les chemins d'origine sans calcul.
  start_etirer: (a) => ({ en_cours: false, faits: 0, total: 0, stems: a.stems, erreur: null }),
  stems_transport: (a, e) => {
    if (a.action === "arreter") e.stems = null;
    if (a.action === "deplacer") e.positionMs = Math.round(a.position * 1000);
    return null;
  },
  stem_spectre: (a) => ({ largeur: a.width, hauteur: a.height, pixels: new Array(a.width * a.height).fill(40) }),
  spectre_transport: { largeur: 4, hauteur: 2, pixels: [0, 50, 100, 150, 200, 250, 30, 60], pixels_ref: null, hd: false },
  pulsation: { temps: [0.5, 1.0, 1.5, 2.0], premiers_temps: [0.5], bpm: 120, metrique: 4 },
  tempo_cible: { bpm: 120, nettete: 3 },
  transcriptions_en_cache: [],
  // La mémoire de Pratiquer (E1) : une base par chemin, dans l'état partagé.
  pratique_accordage: null,
  pratique_memoire: (a, e) => (e.pratique ?? {})[a.path] ?? null,
  pratique_garder: (a, e) => {
    (e.pratique ??= {})[a.path] = structuredClone(a.memoire);
    return null;
  },
  // E4 : le coach de batterie. Aucune batterie MIDI branchée ; le test qui
  // en simule une pose les siennes.
  coach_peripheriques: [],
  coach_ecouter: null,
  coach_garder_profil: null,
  coach_profil_general_midi: {
    nom: "General MIDI",
    notes: /** @type {const} */ ({ 36: "grosse_caisse", 38: "caisse_claire", 42: "charleston", 45: "toms", 49: "cymbales" }),
    cc_charleston: 4,
    velocite_min: 1,
  },
  coach_notes_recues: [],
  coach_armer: null,
  coach_calibrer: { latence_ms: 38, dispersion_ms: 6, retenues: 14, ecartees: 2 },
  coach_juger: (a) => ({
    jusqua_s: a.depuisS ?? 0, jugements: [], frappes: [], places_perdues: [], releves: [], passage_fini: null,
    boucle_proposee: null,
  }),
  coach_table: { mesures: [], boucle_proposee: null },
  // E5 : le groove ; rien à dire tant qu'un test ne pose pas ses conseils.
  coach_groove: { pieces: [], swing: null, conseils: [] },
  // E5 : la basse en DI. Aucune entrée audio ; le test qui en simule une
  // pose les siennes.
  coach_basse_entrees: [],
  coach_basse_ecouter: null,
  coach_basse_armer: { notes: 0, sures: 0, stem: false },
  // V35 : la transposition en cours, que le coach de basse juge.
  coach_basse_transposer: null,
  // V36 : l'accordeur ; rien ne joue, pas de stem mesuré, sauf dans le test
  // qui les pose.
  coach_basse_accorder: { accord: null, canal: 0, erreur: null },
  coach_basse_accord_disque: { ecart_cents: null, notes: 0, stem: false },
  coach_basse_juger: (a) => ({
    jusqua_s: a.depuisS ?? 0, jugements: [], notes: [], bilan: { justes: 0, fausses: 0, octaves: 0, inconnues: 0 },
    erreur: null, canal: 0, ailleurs: null,
  }),
  coach_basse_calibrer: { latence_ms: 24, dispersion_ms: 4, retenues: 15, ecartees: 1, canal: 0 },
  // V34 : le canal de l'interface choisi à la main (un nombre) ou « auto » (null).
  coach_basse_canal: (a) => ({
    entree: "Interface USB", frequence_hz: 48000, canaux: 2, canal: a.canal ?? 0, manuel: a.canal != null, latence_ms: 24,
  }),
  // E7 : entendre sa basse. Sans entrée écoutée, rien n'est prêt ; le test
  // qui en simule une pose les siennes.
  retour_basse_etat: {
    ecoute: false, disponible: false, actif: false, prereglage: "brut", volume: 0.5, balance: 0, micro: false,
    latence_ms: null, stem_disque: null, trous: 0,
    prereglages: [
      ["brut", "Brut"], ["rond", "Rond"], ["funky", "Funky"], ["slap", "Slap"], ["crunch", "Crunch"],
      ["overdrive", "Overdrive"], ["vintage", "Vintage"], ["comme-le-disque", "Comme le disque"],
    ].map(([nom, libelle]) => ({ nom, libelle })),
  },
  retour_basse_regler: (a) => ({
    ecoute: false, disponible: false, actif: false, prereglage: a.prereglage, volume: a.volume, balance: a.balance,
    micro: false, latence_ms: null, stem_disque: null, trous: 0, prereglages: [],
  }),
  // E2 : la partie exportée (`exporter_partie`) ; la transcription et sa
  // correction sont simulées par le test qui s'en sert.
  exporter_partie: null,
  sections_disponibles: false,
  // F1 : chercher le greffon. Un candidat mesuré, un qui attend son démixage.
  chercher_greffon: {
    bpm: 120,
    tonalite: "A min",
    a_hauteur: false,
    texte: "funk drums, syncopated groove",
    groove: { version: 1, densite: 2, syncope: 0.2, swing: null, metrique: 4 },
    candidats: [
      {
        id: 4, path: "/Musique/Orchestre Témoin/Fumée/04 Largo.flac", title: "Largo", artist: "Orchestre Témoin",
        bpm: 122, facteur: 0.984, octaves: 0, tonalite: "C maj", separe: true,
        groove: { version: 1, densite: 2.1, syncope: 0.41, swing: 1.1, metrique: 4 },
        rang_sonore: 2, score: 1.8,
        raisons: [
          "syncope 0,41 : plus syncopé que l'original (0,20)",
          "2,1 attaques par temps : densité voisine (2,0)",
          "droit",
          "122 BPM, étiré de -1,6 %",
          "n° 2 pour « funk drums, syncopated groove » et le son du morceau",
        ],
      },
      {
        id: 5, path: "/Musique/Orchestre Témoin/Fumée/05 Presto.flac", title: "Presto", artist: "Orchestre Témoin",
        bpm: 60, facteur: 1, octaves: -1, tonalite: null, separe: false, groove: null,
        rang_sonore: 1, score: 1,
        raisons: [
          "groove à mesurer : démixage en attente",
          "60 BPM, joué à son tempo divisé par 2, au même tempo",
          "n° 1 pour « funk drums, syncopated groove » et le son du morceau",
        ],
      },
    ],
    ecartes_tempo: 31,
    ecartes_tonalite: 0,
    ecartes_metrique: 1,
    sans_tempo: 4,
  },
  greffons_preparer: null,
  greffons_etat: { en_cours: false, courant: null, prets: [], echecs: [], restants: 0 },
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
  // « Compléter » : rien en attente, aucune passe passée.
  completer_apercu: { musicbrainz: 0, lastfm: 0, secondes: 0 },
  completer_etat: { passe: { en_cours: false, etape: "", faits: 0, total: 0, resultat: null }, derniere: null },
  emporter_lecture: { pistes: 0, titre: null, position_ms: 0, mac_en_pause: true, par_wifi: false, cable_echec: null },
  wifi_etat: { appaire: false, ecoute: false, port: null, adresses: [], erreur: null, en_attente: false, valise: null, dernier: null },
  oublier_iphone: null,
  valise_iphone: { xcode: false, appareils: [], udid: null, app_installee: false, echeance: null, valises: [], renouvelable: false, erreur: null },
  valise_etat: {
    envoi: { en_cours: false, etape: "", faits: 0, total: 0, resultat: null },
    renouvellement: { en_cours: false, ligne: "", resultat: null },
  },
};

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
  stems_gain: null,
});

module.exports = { fixtures, donnees: { PISTES, ALBUMS }, PISTES, ALBUMS };
