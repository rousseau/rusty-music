// SPDX-License-Identifier: GPL-3.0-or-later
// La place laissée à la partition en Pratiquer, coach de basse démarré (V43).
// Pas un test, comme `rails.spec.js` : seulement sur demande.
//
//   HAUTEUR=avant npx playwright test pratiquer-hauteur
//
// Deux fenêtres : le portable de François (écran intégré « 1 512 × 982 »,
// fenêtre agrandie : 912 px de page) et 1 400 × 900 (868 px de page, comme
// `rails.spec.js`). Trois états : coach seul, accordeur ouvert, retour ouvert.
// Imprime la hauteur de l'en-tête et de chaque bloc du centre (px) et écrit une capture par
// cas dans `HAUTEUR_DOSSIER` (le dossier temporaire par défaut).
const os = require("node:os");
const path = require("node:path");
const { test, expect } = require("@playwright/test");
const { ouvrir, versAlbums } = require("./outils");
const { fixtures, donnees } = require("./fixtures");
const { pratiquerAlbum, basseTranscrite } = require("./pratique-fictive");

const ETIQUETTE = process.env.HAUTEUR || "hauteur";
const DOSSIER = process.env.HAUTEUR_DOSSIER || os.tmpdir();
const FENETRES = [
  { nom: "portable", width: 1512, height: 912 },
  { nom: "1400", width: 1400, height: 868 },
];
const BLOCS = [".fil", "#sections-bande", ".partition-barres", "#partition-coach", "#partition-correction", "#accordeur", "#retour-basse", "#partition"];

async function mesurer(page) {
  return page.evaluate((blocs) => {
    const m = {};
    for (const s of blocs) {
      const el = document.querySelector(s);
      const r = el?.getBoundingClientRect();
      m[s] = el && el.offsetParent !== null && r.height > 0 ? Math.round(r.height) : 0;
    }
    const etabli = document.querySelector("#editer-etabli").getBoundingClientRect();
    m.etabli = Math.round(etabli.height);
    return m;
  }, BLOCS);
}

for (const f of FENETRES) {
  test(`partition en Pratiquer, ${f.nom}`, async ({ page }) => {
    await page.setViewportSize({ width: f.width, height: f.height });
    await ouvrir(page, fixtures, donnees);
    // L'Accueil d'abord (A2) : la grille d'albums d'où part `pratiquerAlbum`.
    await versAlbums(page);
    await basseTranscrite(page);
    await page.evaluate(() => {
      window.__FIXTURES__.coach_basse_entrees = ["Scarlett 2i2 USB", "Micro du MacBook"];
      window.__RETOUR__ = { ...window.__FIXTURES__.retour_basse_etat, ecoute: false, disponible: true };
      window.__FIXTURES__.coach_basse_ecouter = (a) => {
        window.__RETOUR__.ecoute = !!a.entree;
        return a.entree ? { entree: a.entree, frequence_hz: 48000, canaux: 2, canal: 0, manuel: false, latence_ms: 24 } : null;
      };
      window.__FIXTURES__.retour_basse_etat = () => ({ ...window.__RETOUR__ });
      window.__FIXTURES__.coach_basse_armer = (a) => ({ notes: a.notes.length, sures: a.notes.length, stem: true });
      window.__FIXTURES__.coach_basse_juger = () => ({
        jusqua_s: 1, jugements: [], notes: [], bilan: { justes: 0, fausses: 0, octaves: 0, inconnues: 0 }, erreur: null, canal: 0, ailleurs: null,
      });
      window.__FIXTURES__.coach_basse_accorder = () => ({ accord: null, canal: 0, erreur: null });
      window.__FIXTURES__.coach_basse_accord_disque = { ecart_cents: 0, notes: 0, stem: false };
    });
    await pratiquerAlbum(page, 0);
    await page.click('#instruments [data-instrument="bass"]');
    await expect(page.locator("#partition-rendu .at-surface")).toBeVisible();
    await page.click("#coach-basculer");
    await expect(page.locator("#coach-basculer")).toHaveAttribute("aria-pressed", "true");
    await page.waitForTimeout(400);

    const cas = {};
    const capture = (nom) => page.screenshot({ path: path.join(DOSSIER, `v43-${ETIQUETTE}-${f.nom}-${nom}.png`) });
    cas.coach = await mesurer(page);
    await capture("coach");
    // Le repli des réglages de l'entrée, ouvert par-dessus la partition : la
    // hauteur de la partition ne bouge pas.
    await page.click("#coach-reglages > summary");
    await expect(page.locator("#coach-peripherique")).toBeVisible();
    cas.reglages = await mesurer(page);
    await capture("reglages");
    await page.keyboard.press("Escape");
    await expect(page.locator("#coach-peripherique")).toBeHidden();
    await page.click("#coach-accordeur");
    await expect(page.locator("#accordeur")).toBeVisible();
    await page.waitForTimeout(200);
    cas.accordeur = await mesurer(page);
    await capture("accordeur");
    await page.click("#accordeur-fermer");
    await page.click("#coach-retour");
    await expect(page.locator("#retour-basse")).toBeVisible();
    await page.waitForTimeout(200);
    cas.retour = await mesurer(page);
    await capture("retour");
    console.log(`V43 ${ETIQUETTE} ${f.nom} ${f.width}×${f.height}`);
    for (const [k, m] of Object.entries(cas)) console.log(`  ${k.padEnd(9)} ${JSON.stringify(m)}`);
  });
}

// La batterie, coach démarré, dans une playlist à travailler (24 nov.) : le
// cas de François sur son portable. Imprime chaque bloc visible au-dessus de
// la partition, du haut du centre au bas de la partition.
const { batterieTranscrite } = require("./pratique-fictive");
const { PISTES } = require("./fixtures");
for (const f of FENETRES) {
  test(`partition batterie en playlist, ${f.nom}`, async ({ page }) => {
    await page.setViewportSize({ width: f.width, height: f.height });
    await ouvrir(page, fixtures, donnees);
    await batterieTranscrite(page);
    await page.evaluate((chemins) => {
      window.__ETAT_FAUX__.playlists = [{ id: 1, nom: "À travailler", origine: null, modifiee_le: 1_700_000_000, chemins }];
    }, [PISTES[0], PISTES[2], PISTES[4]].map((t) => t.path));
    await page.click('button.mode[data-mode="pratiquer"]');
    await page.click("#pratique-playlist-choisir");
    await page.locator("#menu-pratique-playlist .menu-playlist__item", { hasText: "À travailler" }).click();
    await expect(page.locator("#editer-etabli")).toBeVisible();
    await page.click('#instruments [data-instrument="drums"]');
    await expect(page.locator("#partition-rendu .at-surface")).toBeVisible();
    await page.click("#coach-basculer");
    await expect(page.locator("#coach-basculer")).toHaveAttribute("aria-pressed", "true");
    await page.waitForTimeout(500);
    await page.screenshot({ path: path.join(DOSSIER, `batterie-${ETIQUETTE}-${f.nom}.png`) });
    const blocs = await page.evaluate(() => {
      const visible = (el) => el.offsetParent !== null && el.getBoundingClientRect().height > 0;
      const out = [];
      const centre = document.querySelector("main.centre") || document.querySelector(".centre");
      const parcourir = (el, prof) => {
        for (const c of el.children) {
          if (!visible(c)) continue;
          const r = c.getBoundingClientRect();
          out.push(`${"  ".repeat(prof)}${c.tagName.toLowerCase()}${c.id ? "#" + c.id : ""}${c.className && typeof c.className === "string" ? "." + c.className.split(" ")[0] : ""} y=${Math.round(r.top)} h=${Math.round(r.height)}`);
          if (prof < 2 && (c.id === "editer-etabli" || c.contains(document.querySelector("#partition")) )) parcourir(c, prof + 1);
        }
      };
      parcourir(centre, 0);
      const p = document.querySelector("#partition").getBoundingClientRect();
      out.push(`partition y=${Math.round(p.top)} h=${Math.round(p.height)} / fenêtre ${innerHeight}`);
      return out;
    });
    console.log(`batterie ${ETIQUETTE} ${f.nom} ${f.width}×${f.height}\n` + blocs.join("\n"));
  });
}
