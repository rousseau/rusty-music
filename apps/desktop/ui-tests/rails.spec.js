// SPDX-License-Identifier: GPL-3.0-or-later
// Le rail de chaque écran, mesuré et capturé : la Règle 13 de
// `docs/interface-guidelines.md` (le rail tient dans la fenêtre) vue de près.
// Hors des tests de fumée (`playwright.config.js`), comme `captures.spec.js` :
//
//   RAILS=avant npx playwright test rails      (ou RAILS=avant npm run rails)
//
// Imprime, par écran, la hauteur de chaque bloc visible du rail et le bas du
// dernier élément (la mesure de G8, `js/audit.js`), puis écrit une capture du
// rail de chaque écran dans `RAILS_DOSSIER` (le dossier temporaire par
// défaut) ; `rails.py` les assemble côte à côte. Bibliothèque fictive à douze
// familles, celles de la vraie ; fenêtre de 1 400 × 900 (868 px de page, comme
// la webview de l'application).
//
// Sans plan de ville réel dans les réponses figées, la Carte n'affiche pas
// ses calques : le script les montre comme le ferait un plan réel (fond de
// plan, Pentes, Relief, Brouillard et Sentiers battus), pour que la mesure
// compte ce que l'utilisateur voit.
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { test, expect } = require("@playwright/test");
const { ouvrir } = require("./outils");
const { fixtures, donnees, PISTES, ALBUMS } = require("./fixtures");

const NOMS = [
  "Rock · Grunge", "Jazz · Fusion", "Hip hop · Rap", "Électronique · Ambient", "Soul · Funk", "Reggae · Dub",
  "Classique", "Pop", "Folk · Acoustique", "Metal", "Musiques du monde", "Chanson",
];
const DOUZE = {
  ...fixtures,
  families: NOMS.map((n, i) => [i, n, 300 - i * 20]),
  families_detail: NOMS.map((n, i) => ({
    cluster: i, effectif: 300 - i * 20, fiable: true, nom_affiche: n, musicbrainz: n, clap_texte: n, lastfm: "",
  })),
  album_families: ALBUMS.map((a, i) => [a.name, a.artist, i]),
  map_view: fixtures.map_view.map((p, i) => ({ ...p, cluster: i % 12 })),
};

const ETIQUETTE = process.env.RAILS || "rails";
const DOSSIER = process.env.RAILS_DOSSIER || path.join(os.tmpdir(), "rails");

// Chaque écran, et l'état qui charge le plus son rail.
const ECRANS = [
  ["Bibliothèque", "bibliotheque"],
  ["Écouter", "ecoute"],
  ["Pratiquer · établi", "pratiquer"],
  ["Explorer · Nuage", "explorer", "points"],
  ["Explorer · Carte", "explorer", "carte"],
  ["Explorer · Temps", "explorer", "temps"],
  ["Explorer · Anneau", "explorer", "anneau"],
  ["Explorer · Lama", "explorer", "lama"],
  ["Créer · établi", "creer"],
  ["Découvrir", "decouvrir"],
  ["Valise", "valise"],
];

/// Les lignes de la Carte qu'un plan de ville réel montrerait.
function calquesDuPlanReel() {
  for (const id of ["bloc-fond", "ligne-pentes", "ligne-relief", "ligne-traces"]) {
    const el = document.getElementById(id);
    if (el) el.hidden = false;
  }
}

/// La mesure de G8 (`js/audit.js`), et la hauteur de chaque bloc visible.
function mesurer() {
  const rail = document.querySelector(".rail");
  const visible = (e) => {
    const ferme = e.closest("details:not([open])");
    if (ferme && !ferme.querySelector(":scope > summary")?.contains(e)) return false;
    const cs = getComputedStyle(e);
    if (cs.display === "none" || cs.visibility === "hidden") return false;
    const r = e.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  };
  const haut = rail.getBoundingClientRect().top - rail.scrollTop;
  let bas = 0;
  for (const e of rail.querySelectorAll("*"))
    if (visible(e) && getComputedStyle(e).position !== "fixed") bas = Math.max(bas, e.getBoundingClientRect().bottom - haut);
  const blocs = [...rail.children].filter(visible).map((e) => {
    const r = e.getBoundingClientRect();
    const nom = e.id || e.className.split(" ")[0] || e.tagName.toLowerCase();
    return [nom, Math.round(r.height)];
  });
  return { bas: Math.round(bas), hauteur: rail.clientHeight, blocs };
}

test.skip(!process.env.RAILS, "seulement sur demande : RAILS=avant|apres");

test("le rail de chaque écran, mesuré et capturé", async ({ page }) => {
  test.setTimeout(120_000);
  fs.mkdirSync(DOSSIER, { recursive: true });
  await page.setViewportSize({ width: 1400, height: 868 });
  await ouvrir(page, DOUZE, donnees);
  await expect(page.locator("#grille .album")).toHaveCount(3);
  // Des stems chargés : l'établi de Pratiquer et de Créer.
  await page.evaluate(() => localStorage.setItem("mb-contact", "essai@example.org"));
  await page.locator("#grille .album__lecture").first().click({ force: true });
  await expect(page.locator("#np-titre")).toHaveText(PISTES[0].title);

  const releve = [];
  for (const [nom, mode, affichage] of ECRANS) {
    await page.setViewportSize({ width: 1400, height: 868 });
    await page.click(`button.mode[data-mode="${mode}"]`);
    if (affichage) await page.click(`[data-affichage="${affichage}"]`);
    if (affichage === "carte") await page.evaluate(calquesDuPlanReel);
    await page.waitForTimeout(400);
    const m = await page.evaluate(mesurer);
    releve.push({ nom, ...m });
    // Assez haut pour tout le contenu du rail, sans défilement.
    await page.setViewportSize({ width: 1400, height: Math.max(868, m.bas + 120) });
    if (affichage === "carte") await page.evaluate(calquesDuPlanReel);
    await page.waitForTimeout(200);
    const fichier = path.join(DOSSIER, `${ETIQUETTE}-${String(releve.length).padStart(2, "0")}.png`);
    await page.screenshot({ path: fichier, clip: { x: 0, y: 0, width: 232, height: m.bas + 8 } });
  }
  fs.writeFileSync(path.join(DOSSIER, `${ETIQUETTE}.json`), JSON.stringify(releve, null, 1));
  // La Carte entière, calques ouverts s'ils sont posés sur elle.
  await page.setViewportSize({ width: 1400, height: 868 });
  await page.click('button.mode[data-mode="explorer"]');
  await page.click('[data-affichage="carte"]');
  await page.evaluate(calquesDuPlanReel);
  const outil = page.locator("#outil-calques");
  if (await outil.count()) {
    await page.evaluate(() => (document.getElementById("outil-calques").hidden = false));
    await outil.click();
    await page.evaluate(calquesDuPlanReel);
  }
  await page.waitForTimeout(400);
  await page.screenshot({ path: path.join(DOSSIER, `${ETIQUETTE}-carte.png`) });
  for (const r of releve) {
    console.log(`${r.nom.padEnd(22)} ${String(r.bas).padStart(5)} / ${r.hauteur}  ${r.bas > r.hauteur ? "+" + (r.bas - r.hauteur) : "tient"}`);
    console.log("    " + r.blocs.map(([n, h]) => `${n} ${h}`).join(" · "));
  }
});
