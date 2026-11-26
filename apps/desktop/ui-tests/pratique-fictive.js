// SPDX-License-Identifier: GPL-3.0-or-later
// Pratiquer sur la bibliothèque fictive : ouvrir un morceau, lui donner une
// basse ou une batterie transcrite. Partagé par les tests de fumée (`fumee.spec.js`) et la
// mesure de la partition (`pratiquer-hauteur.spec.js`).
const { expect } = require("@playwright/test");

/// Ouvre en Pratiquer le morceau lu depuis l'album `rang` de la grille.
async function pratiquerAlbum(page, rang) {
  await page.click('button.mode[data-mode="ecoute"]');
  await page.locator("#grille .album__lecture").nth(rang).click({ force: true });
  await page.click('button.mode[data-mode="pratiquer"]');
  await expect(page.locator("#editer-etabli")).toBeVisible();
  await expect(page.locator("#boucle-reglage")).toBeVisible();
}

/// E2 : une basse transcrite simulée, quatre noires par mesure sur la grille
/// du test (un temps toutes les 0,5 s), et un moteur de correction réduit à
/// l'essentiel : il garde la correction et rend la transcription corrigée.
async function basseTranscrite(page) {
  await page.evaluate(() => {
    window.__FIXTURES__.pulsation = {
      temps: Array.from({ length: 32 }, (_, i) => 0.5 + i / 2),
      premiers_temps: Array.from({ length: 8 }, (_, i) => 0.5 + 2 * i),
      bpm: 120,
      metrique: 4,
    };
    window.__FIXTURES__.transcriptions_en_cache = ["bass", "bass-muscriptor"];
    // L'accordage de la tablature (V37) : `__ACCORDAGE__` le change, le
    // choix à la main (`pratique_accordage`) l'impose.
    const ACCORDS = { standard: [28, 33, 38, 43], "demi-ton": [27, 32, 37, 42], ton: [26, 31, 36, 41], "drop-d": [26, 33, 38, 43], cinq: [23, 28, 33, 38, 43] };
    const accordageDe = (e) => (e.choix ? ACCORDS[e.choix] : window.__ACCORDAGE__ ?? ACCORDS.standard);
    window.__FIXTURES__.pratique_accordage = (a, e) => {
      e.choix = a.accordage;
      return null;
    };
    let accordage = ACCORDS.standard;
    // La frette la plus basse, comme une main qui reste en bas du manche.
    const placer = (h) => {
      for (let c = accordage.length - 1; c >= 0; c--) if (h - accordage[c] >= 0 && h - accordage[c] <= 20) return [c, h - accordage[c]];
      return [null, null];
    };
    // Deux mesures jouées (la deuxième avec un silence au troisième temps).
    const debuts = [0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 4.0];
    const hauteurs = [33, 36, 38, 40, 33, 31, 28];
    const vue = (corrections, e = {}) => {
      accordage = accordageDe(e);
      let lignes = debuts.map((d, i) => ({ d, h: hauteurs[i], o: 0, s: 0, ajoutee: false }));
      for (const c of corrections) {
        const i = lignes.findIndex((l) => l.h === c.hauteur && Math.abs(l.d - c.debut_s) < 0.03);
        if (c.genre === "ajouter") lignes.push({ d: c.debut_s, h: c.hauteur, o: 0, s: 0, ajoutee: true, duree: c.duree_s });
        else if (i >= 0 && c.genre === "supprimer") lignes.splice(i, 1);
        else if (i >= 0 && c.genre === "octave") lignes[i].o = c.octaves;
        else if (i >= 0 && c.genre === "demiton") lignes[i].s = c.demitons;
      }
      lignes.sort((a, b) => a.d - b.d);
      const notes = lignes.map((l) => {
        const h = l.h + 12 * l.o + l.s;
        const [corde, frette] = placer(h);
        return { debut_s: l.d, fin_s: l.d + 0.45, hauteur: h, amplitude: 0.6, corde, frette };
      });
      const mesures = Array.from({ length: 8 }, (_, m) => ({
        debut_s: 0.5 + 2 * m,
        temps: 4,
        evenements: Array.from({ length: 4 }, (_, k) => {
          const t = 0.5 + 2 * m + k / 2;
          const i = notes.findIndex((n) => Math.abs(n.debut_s - t) < 0.1);
          const n = notes[i];
          return n
            ? { seiziemes: 4, jeu: { hauteur: n.hauteur, corde: n.corde, frette: n.frette }, lie: false, confiance: 0.6, note: i }
            : { seiziemes: 4, jeu: null, lie: false, confiance: 1, note: null };
        }),
      }));
      return {
        version: 5, instrument: "bass", modele: "muscriptor-medium", notes, coups: [], mesures, accordage,
        choix: e.choix ?? null, doigtes: 6, basses: "quatre", hauteurs: lignes.map((l) => l.h), grille: 3,
        identites: lignes.map((l) => ({
          debut_s: l.d, hauteur: l.h, ajoutee: l.ajoutee, octaves: l.o, demitons: l.s, corde_imposee: false,
        })),
        sans_effet: 0,
      };
    };
    window.__FIXTURES__.transcrire = (a, e) => vue(e.corrections ?? [], e);
    window.__FIXTURES__.corriger_transcription = (a, e) => {
      const c = a.correction;
      // Une correction par genre et par note, comme `Library::corriger_pratique`.
      const memeNote = (x) => x.hauteur === c.hauteur && Math.abs(x.debut_s - c.debut_s) < 1e-3;
      e.corrections = (e.corrections ?? []).filter((x) => !(memeNote(x) && (c.genre === "retablir" || x.genre === c.genre)));
      if (c.genre !== "retablir") e.corrections.push(c);
      return vue(e.corrections, e);
    };
  });
}

/// Une batterie transcrite (ADTOF simulé) : huit mesures de rock à 120,
/// grosse caisse 1 et 3, caisse claire 2 et 4 ; une crash incertaine au
/// premier temps.
async function batterieTranscrite(page) {
  await page.evaluate(() => {
    window.__FIXTURES__.pulsation = {
      temps: Array.from({ length: 32 }, (_, i) => 0.5 + i / 2),
      premiers_temps: Array.from({ length: 8 }, (_, i) => 0.5 + 2 * i),
      bpm: 120,
      metrique: 4,
    };
    window.__FIXTURES__.transcriptions_en_cache = ["drums"];
    const coups = [];
    const mesures = [];
    for (let m = 0; m < 8; m++) {
      const frappes = [];
      for (let k = 0; k < 4; k++) {
        const piece = k % 2 === 0 ? "grosse_caisse" : "caisse_claire";
        const rangs = [coups.length];
        coups.push({ instant_s: 0.5 + 2 * m + k / 2, piece, force: 0.9 });
        const pieces = [piece];
        if (m === 0 && k === 0) {
          rangs.push(coups.length);
          coups.push({ instant_s: 0.5, piece: "cymbales", force: 0.9 });
          pieces.push("cymbales");
        }
        frappes.push({ seiziemes: 4, pieces, coups: rangs });
      }
      mesures.push({ debut_s: 0.5 + 2 * m, temps: 4, frappes });
    }
    window.__FIXTURES__.transcrire = () => ({
      version: 5, instrument: "drums", modele: "adtof", notes: [], coups, mesures, accordage: [],
      doigtes: 5, basses: "quatre", hauteurs: [], grille: 4, sans_effet: 0,
    });
    // Une batterie MIDI branchée, déjà calibrée.
    window.__FIXTURES__.coach_peripheriques = ["TD-17"];
    window.__FIXTURES__.coach_ecouter = (a) =>
      a.peripherique ? { peripherique: a.peripherique, profil: { nom: "General MIDI", notes: {}, cc_charleston: 4, velocite_min: 1 }, latence_ms: 38 } : null;
    // Le moteur juge la première mesure : grosse caisse juste, caisse claire
    // en retard, grosse caisse manquée, caisse claire sur la mauvaise pièce ;
    // la crash n'est pas notée ; un coup en trop.
    const j = (reference, verdict, ecart) => ({
      reference, passage: 0, frappe: null, attendu_s: 1, ecart_ms: ecart, ecart_musical_ms: ecart, verdict,
    });
    window.__FIXTURES__.coach_juger = (a, e) => {
      e.juges = (e.juges ?? 0) + 1;
      return {
        jusqua_s: 10 + e.juges,
        jugements: e.juges === 2
          ? [j(0, "parfait", 4), j(1, "non_notee", null), j(2, "retard", 62), j(3, "manquee", null),
             j(4, "mauvaise_piece", 12), { ...j(null, "en_trop", null), attendu_s: null }]
          : [],
        frappes: [],
        places_perdues: [],
        // Puis la mesure 2 relevée, la table qui propose de boucler les
        // mesures 2 et 3 ; et, une fois la boucle posée, des passages
        // propres : l'escalier propose un cran de plus.
        releves: e.juges === 3
          ? [{ mesure: 1, attendues: 4, attrapees: 2, manquees: 1, en_trop: 1, fausses: 1, ecart_moy_ms: 30,
               ecart_et_ms: 20, pente_ms: null, erreur: 0.75 }]
          : [],
        passage_fini: window.__APPELS__.some((x) => x.cmd === "stems_boucle")
          ? { attendues: 8, attrapees: 8, ecart_et_ms: 9, propre: true, proposition: { vitesse: 0.73, monter: true } }
          : null,
        boucle_proposee: e.juges === 3
          ? { premiere: 1, derniere: 2, debut_s: 2.5, fin_s: 6.5, score: 0.75, vitesse: 0.7 }
          : null,
      };
    };
  });
}

/// Une guitare et une voix transcrites (démixage à six stems) : la guitare
/// plaque un accord de mi majeur ouvert sur chaque premier temps, puis une
/// note seule ; la voix chante mi4 sur deux temps, liée, puis sol4, avec ses
/// paroles (« Back in »).
async function guitareEtVoixTranscrites(page) {
  await page.evaluate(() => {
    window.__FIXTURES__.pulsation = {
      temps: Array.from({ length: 32 }, (_, i) => 0.5 + i / 2),
      premiers_temps: Array.from({ length: 8 }, (_, i) => 0.5 + 2 * i),
      bpm: 120,
      metrique: 4,
    };
    window.__FIXTURES__.stems_existants = (a) =>
      ["drums", "bass", "other", "vocals", "guitar", "piano"].map((n) => [n, `/stems/${a.path.split("/").pop()}/${n}.wav`]);
    window.__FIXTURES__.transcriptions_en_cache = ["guitar-muscriptor", "vocals"];
    const jeu = (hauteur, corde, frette) => ({ hauteur, corde, frette });
    const silence = (seiziemes) => ({ seiziemes, jeu: null, lie: false, confiance: 1, note: null });
    window.__FIXTURES__.transcrire = (a) => {
      const guitare = a.instrument === "guitar";
      const mesures = Array.from({ length: 8 }, (_, m) => ({
        debut_s: 0.5 + 2 * m,
        temps: 4,
        evenements: guitare
          ? [
              { seiziemes: 8, jeu: jeu(40, 0, 0), accord: [jeu(47, 1, 2), jeu(52, 2, 2), jeu(56, 3, 1)], lie: false, confiance: 1, note: 5 * m },
              { seiziemes: 4, jeu: jeu(48, 1, 3), lie: false, confiance: 1, note: 5 * m + 4 },
              silence(4),
            ]
          : [
              { seiziemes: 4, jeu: jeu(64, null, null), lie: false, confiance: 1, note: 0 },
              { seiziemes: 4, jeu: jeu(64, null, null), lie: true, confiance: 1, note: 0 },
              { seiziemes: 4, jeu: jeu(67, null, null), lie: false, confiance: 1, note: 1 },
              silence(4),
            ],
      }));
      // La guitare : par mesure, l'accord (quatre notes) puis la note seule.
      const notes = guitare
        ? Array.from({ length: 8 }, (_, m) => [40, 47, 52, 56, 48].map((h, k) => ({
            debut_s: 0.5 + 2 * m + (k === 4 ? 1 : 0), fin_s: 0.5 + 2 * m + (k === 4 ? 1.5 : 1), hauteur: h, amplitude: 1, corde: null, frette: null,
          }))).flat()
        : [];
      return {
        version: 6, instrument: a.instrument, modele: guitare ? "muscriptor-medium" : "pyin", notes, coups: [], mesures,
        accordage: guitare ? [40, 45, 50, 55, 59, 64] : [], choix: null, doigtes: 6, basses: "quatre",
        hauteurs: [], grille: 4, sans_effet: 0,
        ...(guitare ? {} : { paroles: [{ note: 0, texte: "Back" }, { note: 1, texte: "in" }], source_paroles: "lrclib" }),
      };
    };
  });
}

module.exports = { pratiquerAlbum, basseTranscrite, batterieTranscrite, guitareEtVoixTranscrites };
