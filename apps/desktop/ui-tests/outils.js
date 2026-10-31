// SPDX-License-Identifier: GPL-3.0-or-later
// Ouvre l'interface réelle (`apps/desktop/ui/`) servie depuis le disque, avec
// le Tauri simulé et des réponses figées.
const path = require("node:path");
const fs = require("node:fs");

const UI = path.resolve(__dirname, "../ui");
const ORIGINE = "http://rusty.test";

async function ouvrir(page, fixtures, donnees = {}) {
  const erreurs = [];
  page.on("pageerror", (e) => erreurs.push(String(e.stack || e)));
  page.on("console", (m) => {
    if (m.type() === "error") erreurs.push(`console : ${m.text()}`);
  });
  await page.route(`${ORIGINE}/**`, (route) => {
    const rel = decodeURIComponent(new URL(route.request().url()).pathname).replace(/^\/+/, "") || "index.html";
    const fichier = path.join(UI, rel);
    if (!fichier.startsWith(UI) || !fs.existsSync(fichier)) return route.fulfill({ status: 404, body: "" });
    return route.fulfill({ path: fichier });
  });
  // Pas de réseau : les tuiles, pochettes distantes, polices… échouent vite.
  await page.route(/^(?!http:\/\/rusty\.test)/, (route) => route.abort());
  await page.addInitScript({ path: path.join(__dirname, "faux-tauri.js") });
  await page.addInitScript((d) => {
    window.__DONNEES__ = d;
  }, donnees);
  await page.addInitScript((f) => {
    // Les fonctions ne traversent pas la sérialisation : elles arrivent en
    // texte et sont reconstruites ici.
    const t = {};
    for (const [k, v] of Object.entries(f)) t[k] = v && v.__fn ? eval(`(${v.__fn})`) : v;
    window.__FIXTURES__ = t;
  }, serialiser(fixtures));
  await page.goto(`${ORIGINE}/index.html`);
  return erreurs;
}

function serialiser(fixtures) {
  const o = {};
  for (const [k, v] of Object.entries(fixtures)) o[k] = typeof v === "function" ? { __fn: v.toString() } : v;
  return o;
}

async function appelsInconnus(page) {
  return page.evaluate(() => [...new Set(window.__APPELS__.filter((a) => !a.connu).map((a) => a.cmd))]);
}

module.exports = { ouvrir, appelsInconnus, ORIGINE };
