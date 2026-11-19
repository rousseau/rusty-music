// SPDX-License-Identifier: GPL-3.0-or-later
// Pratiquer sur la bibliothèque fictive : ouvrir un morceau, lui donner une
// basse transcrite. Partagé par les tests de fumée (`fumee.spec.js`) et la
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

module.exports = { pratiquerAlbum, basseTranscrite };
