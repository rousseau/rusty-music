// SPDX-License-Identifier: GPL-3.0-or-later
// Tests de fumée : l'interface réelle, Tauri simulé (`faux-tauri.js`,
// `fixtures.js`). Ils ne vérifient pas le moteur, seulement que chaque écran
// s'ouvre, qu'un morceau se lit et qu'Éditer monte ses stems, sans la moindre
// erreur JavaScript.
const { test, expect } = require("@playwright/test");
const { ouvrir, appelsInconnus } = require("./outils");
const { fixtures, donnees, PISTES } = require("./fixtures");

/// Erreurs de la page, plus celles que `ui/js/` remonte lui-même au moteur.
async function erreursJs(page, erreurs) {
  const remontees = await page.evaluate(() =>
    window.__APPELS__.filter((a) => a.cmd === "js_error").map((a) => JSON.stringify(a.args)),
  );
  return [...erreurs, ...remontees];
}

async function appels(page, cmd) {
  return page.evaluate((c) => window.__APPELS__.filter((a) => a.cmd === c).map((a) => a.args), cmd);
}

async function lireUnAlbum(page) {
  await page.locator("#grille .album__lecture").first().click({ force: true });
  await expect(page.locator("#np-titre")).toHaveText(PISTES[0].title);
}

let erreurs;
test.beforeEach(async ({ page }) => {
  erreurs = await ouvrir(page, fixtures, donnees);
  await expect(page.locator("#grille .album")).toHaveCount(3);
});

test.afterEach(async ({ page }, info) => {
  const inconnus = await appelsInconnus(page);
  if (process.env.SONDE) console.log(`INCONNUS ${info.title} :`, inconnus.join(", "));
  if (inconnus.length) info.annotations.push({ type: "sans réponse figée", description: inconnus.join(", ") });
  expect(await erreursJs(page, erreurs)).toEqual([]);
});

test("démarrage : Écouter et la grille d'albums", async ({ page }) => {
  await expect(page.locator('button.mode[data-mode="ecoute"]')).toHaveClass(/mode--actif/);
  await expect(page.locator("#np-titre")).toHaveText("Rien en lecture");
});

test("lire un morceau", async ({ page }) => {
  await lireUnAlbum(page);
  const lus = await appels(page, "play");
  expect(lus.at(-1).paths).toEqual(PISTES.filter((p) => p.album === "Premier banc").map((p) => p.path));
});

for (const [mode, vue] of [
  ["bibliotheque", "#bibliotheque-vue"],
  ["explorer", "#carte-vue"],
  ["decouvrir", "#decouvrir-vue"],
  ["valise", "#valise-vue"],
]) {
  test(`ouvrir le mode ${mode}`, async ({ page }) => {
    await page.click(`button.mode[data-mode="${mode}"]`);
    await expect(page.locator(vue)).toBeVisible();
    // Laisse aux chargements asynchrones du mode le temps d'échouer.
    await page.waitForTimeout(1500);
  });
}

test("Éditer : les stems du morceau en lecture prennent la main", async ({ page }) => {
  await lireUnAlbum(page);
  await page.click('button.mode[data-mode="editer"]');
  await expect(page.locator("#editer-etabli")).toBeVisible();
  await expect(page.locator("#editer-etabli .stem")).toHaveCount(4);
  await expect.poll(() => appels(page, "stems_play").then((a) => a.length)).toBeGreaterThan(0);
  const charges = (await appels(page, "stems_play")).at(-1).stems;
  expect(charges.length).toBe(4);
});

test("le transport suit les stems que le moteur garde", async ({ page }) => {
  await lireUnAlbum(page);
  await page.click('button.mode[data-mode="editer"]');
  await expect(page.locator("#np-titre")).toHaveText("4 stems");
  // ⏭ : l'interface oublie les stems, mais ce moteur-ci ne les coupe pas
  // (une commande qui échoue en route). Le sondage doit les lui rendre.
  await page.evaluate(() => {
    window.__FIXTURES__.skip = () => null;
  });
  await page.click("#suivant");
  await expect.poll(() => appels(page, "skip").then((a) => a.length)).toBe(1);
  // Le transport se remet à sonder les stems (le titre, lui, n'aurait pas
  // bougé de toute façon : même morceau).
  const avant = (await appels(page, "stems_state")).length;
  await expect.poll(() => appels(page, "stems_state").then((a) => a.length)).toBeGreaterThan(avant + 2);
  expect(await page.evaluate(() => window.__ETAT_FAUX__.stems?.length)).toBe(4);
});

test("Lama sans Ollama : l'accueil dit quoi faire, rien n'est interrogé", async ({ page }) => {
  await page.evaluate(() => {
    window.__FIXTURES__.lama_disponibilite = { local: "absent", etat: "absent", modeles: [], detail: "refusé" };
  });
  await page.click('button.mode[data-mode="explorer"]');
  await page.click('[data-affichage="lama"]');
  await expect(page.locator("#lama-indisponible")).toBeVisible();
  await expect(page.locator("#lama-indisponible-texte")).toContainText("ollama pull");
  await page.fill("#intention-texte", "du jazz calme");
  await page.press("#intention-texte", "Enter");
  await expect.poll(() => appels(page, "lama_disponibilite").then((a) => a.length)).toBeGreaterThan(1);
  expect(await appels(page, "path_texte_interpreter")).toEqual([]);
  // Ollama revient : « Réessayer » efface la note.
  await page.evaluate(() => {
    window.__FIXTURES__.lama_disponibilite = { local: "absent", etat: "pret", modeles: ["gemma4:e4b"], detail: null };
  });
  await page.click("#lama-reessayer");
  await expect(page.locator("#lama-indisponible")).toBeHidden();
});

test("Lama sans modèle : l'accueil propose de télécharger le modèle intégré", async ({ page }) => {
  await page.evaluate(() => {
    window.__FIXTURES__.lama_disponibilite = { local: "a_telecharger", etat: "absent", modeles: [], detail: "refusé" };
  });
  await page.click('button.mode[data-mode="explorer"]');
  await page.click('[data-affichage="lama"]');
  await expect(page.locator("#lama-indisponible-texte")).toContainText("Gemma 4 E4B");
  await expect(page.locator("#lama-telecharger")).toBeVisible();
  await page.click("#lama-telecharger");
  await expect.poll(() => appels(page, "lama_telecharger").then((a) => a.length)).toBe(1);
  // Téléchargé : le moteur répond « prêt », la note s'efface au sondage suivant.
  await page.evaluate(() => {
    window.__FIXTURES__.lama_disponibilite = { local: "pret", etat: "absent", modeles: [], detail: "refusé" };
  });
  await page.click("#lama-reessayer");
  await expect(page.locator("#lama-indisponible")).toBeHidden();
  // Le sélecteur propose alors le modèle intégré.
  await page.click("#intention-ollama");
  await expect(page.locator("#intention-modele-liste option")).toHaveText(["Gemma 4 E4B (intégré)"]);
});

test("retour en Écouter après chaque mode", async ({ page }) => {
  for (const m of ["bibliotheque", "explorer", "editer", "decouvrir", "valise", "ecoute"]) {
    await page.click(`button.mode[data-mode="${m}"]`);
    await page.waitForTimeout(500);
  }
  await expect(page.locator("#grille .album")).toHaveCount(3);
});

test("palette : ⌘K, sans accents, Entrée ouvre l'album ; l'écran reste derrière", async ({ page }) => {
  await page.keyboard.press("Meta+k");
  await expect(page.locator("#palette")).toBeVisible();
  // Règle 2 : l'écran courant n'est pas masqué, seulement atténué.
  await expect(page.locator("#grille .album").first()).toBeVisible();
  await page.keyboard.type("FUMEE");
  const meilleur = page.locator("#palette-liste [role=group]").first();
  await expect(meilleur.locator(".palette__groupe")).toHaveText("Meilleur résultat");
  await expect(meilleur.locator(".palette__nom")).toHaveText("Fumée");
  await page.keyboard.press("Enter");
  await expect(page.locator("#palette")).toBeHidden();
  await expect(page.locator("#fil-titre")).toHaveText("Fumée");
  await expect(page.locator("#liste .ligne")).toHaveCount(2);
});

test("palette : « / » ouvre, Échap ferme sans toucher la file", async ({ page }) => {
  await page.click("#bascule-file");
  await expect(page.locator("#file")).toBeVisible();
  await page.locator("body").press("/");
  await expect(page.locator("#palette")).toBeVisible();
  // Sans demande : les commandes, groupées.
  await expect(page.locator(".palette__groupe")).toHaveText(["Aller à", "Commandes"]);
  await page.keyboard.press("Escape");
  await expect(page.locator("#palette")).toBeHidden();
  await expect(page.locator("#file")).toBeVisible();
});

test("palette : ↓ puis Entrée lance une piste", async ({ page }) => {
  await page.keyboard.press("Meta+k");
  await page.keyboard.type("orchestre");
  // Meilleur résultat : l'artiste ; puis les pistes de l'orchestre.
  await expect(page.locator(".palette__option").first().locator(".palette__nom")).toHaveText("Orchestre Témoin");
  await expect(page.locator(".palette__option", { hasText: "Presto" })).toBeVisible();
  const rang = await page.locator(".palette__option").evaluateAll((els) =>
    els.findIndex((el) => el.textContent.includes("Presto")));
  for (let i = 0; i < rang; i++) await page.keyboard.press("ArrowDown");
  await expect(page.locator(".palette__option[aria-selected=true]")).toContainText("Presto");
  await page.keyboard.press("Enter");
  await expect.poll(() => appels(page, "play").then((a) => a.at(-1)?.paths)).toEqual([PISTES[4].path]);
});

test("palette : une commande change de mode, une autre de thème", async ({ page }) => {
  await page.keyboard.press("Meta+k");
  await page.keyboard.type("explorer");
  await expect(page.locator(".palette__groupe").first()).toHaveText("Commandes");
  await page.keyboard.press("Enter");
  await expect(page.locator('button.mode[data-mode="explorer"]')).toHaveClass(/mode--actif/);
  await page.keyboard.press("Meta+k");
  await page.keyboard.type("theme clair");
  await page.keyboard.press("Enter");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "clair");
});

test("filtre du rail : estompe la grille sans rien retirer, Entrée ouvre la palette", async ({ page }) => {
  await page.fill("#q", "essais");
  await expect(page.locator("#grille .album--estompe")).toHaveCount(1);
  await expect(page.locator("#grille .album")).toHaveCount(3);
  await expect(page.locator("#fil-compte")).toHaveText("2 sur 3 albums");
  await page.press("#q", "Enter");
  await expect(page.locator("#palette")).toBeVisible();
  await expect(page.locator("#palette-q")).toHaveValue("essais");
});

test("thème : Bibliothèque choisit Clair, gardé au rechargement, posé avant le premier affichage", async ({ page }) => {
  await page.click('button.mode[data-mode="bibliotheque"]');
  await page.click('[data-choix-theme="clair"]');
  await expect(page.locator("html")).toHaveAttribute("data-theme", "clair");
  await expect(page.locator('[data-choix-theme="clair"]')).toHaveClass(/segment--actif/);
  // Au rechargement, le thème est là quand le <body> apparaît : pas de flash sombre.
  await page.addInitScript(() => {
    new MutationObserver((_, obs) => {
      if (!document.body) return;
      window.__THEME_AU_BODY__ = document.documentElement.dataset.theme ?? null;
      obs.disconnect();
    }).observe(document, { childList: true, subtree: true });
  });
  await page.reload();
  await expect(page.locator("#grille .album")).toHaveCount(3);
  expect(await page.evaluate(() => window.__THEME_AU_BODY__)).toBe("clair");
});

test("thème : « Système » suit le réglage clair ou sombre", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await page.click('button.mode[data-mode="bibliotheque"]');
  await page.click('[data-choix-theme="systeme"]');
  await expect(page.locator("html")).toHaveAttribute("data-theme", "clair");
  await page.emulateMedia({ colorScheme: "dark" });
  await expect(page.locator("html")).not.toHaveAttribute("data-theme", /.+/);
  // La palette bascule explicitement : le choix quitte « Système ».
  await page.keyboard.press("Meta+k");
  await page.keyboard.type("theme clair");
  await page.keyboard.press("Enter");
  await expect(page.locator('[data-choix-theme="clair"]')).toHaveClass(/segment--actif/);
});

test("familles d'Écouter : une famille cochée estompe les autres sans rien retirer", async ({ page }) => {
  await expect(page.locator("#bloc-familles-ecoute")).toBeVisible();
  await page.locator("#familles-ecoute button.famille").first().click();
  await expect(page.locator("#grille .album")).toHaveCount(3);
  await expect(page.locator("#grille .album--estompe")).toHaveCount(1);
  await expect(page.locator("#fil-compte")).toHaveText("2 sur 3 albums");
});
