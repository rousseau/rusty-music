// SPDX-License-Identifier: GPL-3.0-or-later
// Captures d'écran sur une collection inventée (`collection-fictive.js`), dans
// les deux thèmes, vers `docs/images/` — pour le README et la page du livre.
// Hors des tests de fumée (`playwright.config.js`) : à relancer à la main
// après un changement visible de l'interface.
//
//   CAPTURES=1 npx playwright test captures      (ou npm run captures)
const path = require("node:path");
const { test, expect } = require("@playwright/test");
const { ouvrir } = require("./outils");
const { fixturesCollection, donneesCollection, ALBUMS } = require("./collection-fictive");

const DOSSIER = path.resolve(__dirname, "../../../docs/images");
const capture = (page, nom, theme) =>
  page.screenshot({ path: path.join(DOSSIER, `${nom}-${theme}.jpg`), type: "jpeg", quality: 82 });

for (const theme of ["sombre", "clair"]) {
  test(`captures, thème ${theme}`, async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.addInitScript((t) => localStorage.setItem("rusty-music.theme", t), theme);
    await ouvrir(page, fixturesCollection, donneesCollection);
    await expect(page.locator("#grille .album")).toHaveCount(ALBUMS.length);
    await page.locator("#grille .album__pochette--pleine").first().waitFor();
    await page.waitForTimeout(600);
    await capture(page, "ecouter", theme);

    // Un album ouvert, une piste qui joue : le transport et l'inspecteur remplis.
    await page.locator("#grille .album", { hasText: "Nuit sur le canal" }).click();
    await expect(page.locator("#fil-titre")).toHaveText("Nuit sur le canal");
    await page.locator("#liste .ligne").nth(1).dblclick();
    await page.waitForTimeout(900);
    await capture(page, "album", theme);

    // La vue plein écran « En cours » : la pochette sur un disque, le spectre
    // autour, les fonds aux couleurs de la pochette.
    await page.click("#transport-art");
    await page.waitForTimeout(1200);
    await capture(page, "en-cours", theme);
    await page.keyboard.press("Escape");

    // La palette de recherche, au-dessus de l'écran.
    await page.keyboard.press("Meta+k");
    await page.keyboard.type("lan");
    await page.waitForTimeout(500);
    await capture(page, "palette", theme);
    await page.keyboard.press("Escape");

    // Explorer : le nuage, puis la frise.
    await page.click('button.mode[data-mode="explorer"]');
    await page.waitForTimeout(1200);
    await capture(page, "explorer-nuage", theme);
    await page.click('[data-affichage="temps"]');
    await page.waitForTimeout(1000);
    await capture(page, "explorer-temps", theme);
  });
}
