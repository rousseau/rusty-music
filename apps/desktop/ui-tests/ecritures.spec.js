// SPDX-License-Identifier: GPL-3.0-or-later
// Règle 11 : les noms en écriture de droite à gauche et en japonais. Une
// bibliothèque à part (un album arabe, un album japonais) : `fumee.spec.js`
// compte trois albums.
const { test, expect } = require("@playwright/test");
const { ouvrir } = require("./outils");
const { fixtures, donnees } = require("./fixtures");

const ALBUMS = [
  { name: "معرفة", artist: "فيروز", year: 1991, tracks: 1, path: "/Musique/f/01.flac", ajoute_le: 1_700_000_000 },
  { name: "風街ろまん", artist: "はっぴいえんど", year: 1971, tracks: 1, path: "/Musique/h/01.flac", ajoute_le: 1_700_000_000 },
];

test("écritures : kana balisés lang=ja, nom arabe isolé dans une chaîne jointe", async ({ page }) => {
  await ouvrir(page, { ...fixtures, albums: ALBUMS, album_families: [] }, donnees);
  await expect(page.locator("#grille .album")).toHaveCount(2);
  // Le nom japonais reçoit sa langue ; l'artiste aussi (kana).
  const japonais = page.locator("#grille .album", { hasText: "風街ろまん" });
  await expect(japonais.locator(".album__nom")).toHaveAttribute("lang", "ja");
  await expect(japonais.locator(".album__artiste")).toHaveAttribute("lang", "ja");
  // Des idéogrammes ou de l'arabe seuls : aucune langue devinée.
  await expect(page.locator("#grille .album", { hasText: "معرفة" }).locator(".album__nom")).not.toHaveAttribute("lang", /.+/);
  // Chaque élément en ligne est un isolat bidi.
  expect(await page.locator(".album__artiste").first().evaluate((el) => getComputedStyle(el).unicodeBidi)).toBe("isolate");
  // Une chaîne jointe (« album · artiste · année ») isole le nom arabe.
  await page.keyboard.press("Meta+k");
  await page.keyboard.type("معرفة");
  const sec = page.locator(".palette__option .palette__sec").first();
  await expect(sec).toContainText("⁨فيروز⁩ · 1991");
});
