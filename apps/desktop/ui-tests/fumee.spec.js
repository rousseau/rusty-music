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

test("Créer : les stems du morceau en lecture prennent la main", async ({ page }) => {
  await lireUnAlbum(page);
  await page.click('button.mode[data-mode="creer"]');
  await expect(page.locator("#editer-etabli")).toBeVisible();
  await expect(page.locator("#editer-etabli .stem")).toHaveCount(4);
  await expect.poll(() => appels(page, "stems_play").then((a) => a.length)).toBeGreaterThan(0);
  const charges = (await appels(page, "stems_play")).at(-1).stems;
  expect(charges.length).toBe(4);
});

test("Pratiquer puis Créer : les mêmes stems, la partition seulement en Pratiquer", async ({ page }) => {
  await lireUnAlbum(page);
  await page.click('button.mode[data-mode="pratiquer"]');
  await expect(page.locator("#editer-etabli")).toBeVisible();
  await expect(page.locator("#bloc-usage")).toBeVisible();
  await expect(page.locator("#editer-etabli")).toHaveClass(/etabli--pratiquer/);
  const avant = (await appels(page, "stems_play")).length;
  await page.click('button.mode[data-mode="creer"]');
  await expect(page.locator("#bloc-usage")).toBeHidden();
  await expect(page.locator("#editer-etabli")).not.toHaveClass(/etabli--pratiquer/);
  await expect(page.locator("#editer-etabli .stem")).toHaveCount(4);
  // Les stems restent chargés d'un mode à l'autre : rien n'est relancé.
  expect((await appels(page, "stems_play")).length).toBe(avant);
});

test("I1 : « Choisir l'édition… » reste en Écouter, il ne suit pas dans les autres écrans", async ({ page }) => {
  await page.evaluate(() => localStorage.setItem("mb-contact", "essai@example.org"));
  await page.locator("#grille .album").first().click();
  await expect(page.locator("#choisir-edition")).toBeVisible();
  for (const m of ["explorer", "decouvrir", "valise", "bibliotheque", "pratiquer", "creer"]) {
    await page.click(`button.mode[data-mode="${m}"]`);
    await expect(page.locator("#choisir-edition"), m).toBeHidden();
  }
  await page.click('button.mode[data-mode="ecoute"]');
  await expect(page.locator("#choisir-edition")).toBeVisible();
});

test("I2 : en Explorer, le titre du fil est le nom de l'affichage", async ({ page }) => {
  await page.click('button.mode[data-mode="explorer"]');
  for (const [affichage, titre] of [["points", "Nuage"], ["carte", "Carte"], ["temps", "Temps"], ["anneau", "Anneau"], ["lama", "Lama"], ["points", "Nuage"]]) {
    await page.click(`[data-affichage="${affichage}"]`);
    await expect(page.locator("#fil-titre")).toHaveText(titre);
  }
});

test("I3 : l'aide de la carte dit « clic : choisir », le clic ne lit pas", async ({ page }) => {
  await page.click('button.mode[data-mode="explorer"]');
  for (const affichage of ["points", "carte"]) {
    await page.click(`[data-affichage="${affichage}"]`);
    await expect(page.locator("#carte-aide")).toContainText("clic : choisir");
    await expect(page.locator("#carte-aide")).not.toContainText("écouter");
  }
});

test("I4 : « Filtrer » seulement dans l'état « choisir » de Pratiquer et de Créer", async ({ page }) => {
  await lireUnAlbum(page);
  for (const m of ["pratiquer", "creer"]) {
    await page.click(`button.mode[data-mode="${m}"]`);
    await expect(page.locator("#editer-etabli")).toBeVisible();
    await expect(page.locator("#bloc-chercher"), m).toBeHidden();
    await page.click("#retour");
    await expect(page.locator("#grille")).toBeVisible();
    await expect(page.locator("#bloc-chercher"), m).toBeVisible();
  }
  await page.click('button.mode[data-mode="ecoute"]');
  await expect(page.locator("#bloc-chercher")).toBeVisible();
});

test("I5 : les stems portent des noms français, le nom du moteur reste la clé", async ({ page }) => {
  await lireUnAlbum(page);
  await page.click('button.mode[data-mode="creer"]');
  await expect(page.locator("#editer-etabli .stem__nom")).toHaveText(["Batterie", "Basse", "Autres", "Voix"]);
  expect(await page.locator("#editer-etabli .piste").evaluateAll((l) => l.map((e) => e.dataset.stem)))
    .toEqual(["drums", "bass", "other", "vocals"]);
  await page.locator("#editer-etabli .stem__nom", { hasText: "Basse" }).click();
  await expect(page.locator("#insp-stem-nom")).toHaveText("Basse");
});

test("I10 : le décompte de Pratiquer tient sur une ligne dans le rail", async ({ page }) => {
  await lireUnAlbum(page);
  await page.click('button.mode[data-mode="pratiquer"]');
  await expect(page.locator("#decomptes")).toBeVisible();
  // Une seule ligne de texte par bouton (les boîtes de ligne d'un `Range`), et
  // les trois boutons à la même hauteur.
  const lignes = await page.locator("#decomptes .segment").evaluateAll((bs) =>
    bs.map((b) => {
      const r = document.createRange();
      r.selectNodeContents(b);
      return new Set([...r.getClientRects()].map((x) => Math.round(x.top))).size;
    }),
  );
  expect(lignes).toEqual([1, 1, 1]);
});

test("B4 : zoom, Calques et Chemin sont posés sur la carte, plus dans le rail", async ({ page }) => {
  await page.click('button.mode[data-mode="explorer"]');
  await page.waitForTimeout(800);
  for (const id of ["#bloc-zoom", "#outil-chemin", "#carte-aura"])
    await expect(page.locator(`.carte__outils ${id}`)).toBeVisible();
  await expect(page.locator(".rail #bloc-zoom, .rail #bloc-chemin, .rail #bloc-fond")).toHaveCount(0);
  // Calques : le plan seulement.
  await expect(page.locator("#outil-calques")).toBeHidden();
  // Le premier Maj+clic ouvre le panneau du chemin, avec la borne posée.
  const p = fixtures.map_view[0];
  const r = await page.locator("#carte").boundingBox();
  const c = Math.min(r.width, r.height) / 2 - 28;
  const majClic = async () => {
    await page.keyboard.down("Shift");
    await page.mouse.click(r.x + r.width / 2 + p.x * c, r.y + r.height / 2 + p.y * c);
    await page.keyboard.up("Shift");
  };
  await majClic();
  await expect(page.locator("#bloc-chemin")).toBeVisible();
  await expect(page.locator("#outil-chemin")).toHaveAttribute("aria-expanded", "true");
  // Son aide est derrière « ? ».
  await expect(page.locator("#chemin-aide")).toBeHidden();
  await page.click('[data-aide-pour="chemin-aides"]');
  await expect(page.locator("#chemin-aide")).toBeVisible();
  // Refermé, il le reste au Maj+clic suivant ; le bouton le rouvre.
  await page.click("#outil-chemin");
  await expect(page.locator("#bloc-chemin")).toBeHidden();
  await majClic();
  await expect(page.locator("#bloc-chemin")).toBeHidden();
  // Calques sur la Carte, un panneau à la fois.
  await page.click('[data-affichage="carte"]');
  await page.click("#outil-chemin");
  await page.click("#outil-calques");
  await expect(page.locator("#panneau-calques")).toBeVisible();
  await expect(page.locator("#bloc-chemin")).toBeHidden();
  await expect(page.locator('#carte-theme [data-theme="encre"]')).toBeVisible();
  // L'anneau n'a pas de chemin.
  await page.click('[data-affichage="anneau"]');
  await expect(page.locator("#outil-chemin")).toBeHidden();
  await expect(page.locator("#outil-calques")).toBeHidden();
  await expect(page.locator("#panneau-calques")).toBeHidden();
});

test("B4 : le rail garde la vue, le reste a sa place (Démixage, aides, contact, compte)", async ({ page }) => {
  // Le compte de la bibliothèque, dans le bouton qui y mène.
  await expect(page.locator(".bibliotheque-entree #sommaire")).toHaveText(String(PISTES.length));
  // Découvrir : plus de champ de contact dans le rail ; il est dans Bibliothèque.
  await page.click('button.mode[data-mode="decouvrir"]');
  await expect(page.locator(".rail input[type=email]")).toHaveCount(0);
  await expect(page.locator("#decouvrir-aide")).toBeHidden();
  await page.click('[data-aide-pour="decouvrir-aide"]');
  await expect(page.locator("#decouvrir-aide")).toContainText("Bibliothèque");
  await expect(page.locator("#analyse-contact")).toHaveCount(1);
  // Pratiquer, établi ouvert : le Démixage tient sur une ligne, « changer » le
  // redéplie ; l'aide de la partie est au centre.
  await page.click('button.mode[data-mode="ecoute"]');
  await lireUnAlbum(page);
  await page.click('button.mode[data-mode="pratiquer"]');
  await expect(page.locator("#editer-etabli")).toBeVisible();
  await expect(page.locator("#demix-resume")).toBeVisible();
  await expect(page.locator("#variantes")).toBeHidden();
  await page.click("#demix-changer");
  await expect(page.locator("#variantes")).toBeVisible();
  await expect(page.locator(".rail #usage-aide")).toHaveCount(0);
  await expect(page.locator("#partition-aide")).toContainText("S et M règlent l'écoute");
});

test("chaque écran montre ce que dit sa table (ecrans.js), rien d'un autre écran", async ({ page }) => {
  // De quoi peupler les états qui fuyaient : un album ouvert (« Choisir
  // l'édition… »), des stems chargés (l'établi), le Lama.
  await page.evaluate(() => localStorage.setItem("mb-contact", "essai@example.org"));
  await lireUnAlbum(page);
  await page.locator("#grille .album").first().click();
  await expect(page.locator("#choisir-edition")).toBeVisible();
  const parcours = [
    ["explorer", "points"], ["explorer", "lama"], ["explorer", "anneau"], ["pratiquer"], ["creer"],
    ["decouvrir"], ["valise"], ["bibliotheque"], ["ecoute"],
  ];
  for (const [mode, affichage] of parcours) {
    await page.click(`button.mode[data-mode="${mode}"]`);
    if (affichage) await page.click(`[data-affichage="${affichage}"]`);
    const ecarts = () =>
      page.evaluate(async (ecran) => {
        const { ECRANS, TOUJOURS } = await import("./js/ecrans.js");
        const ecarts = [];
        for (const id of new Set(Object.values(ECRANS).flatMap(Object.keys))) {
          const place = ECRANS[ecran][id];
          const vu = document.getElementById(id).checkVisibility();
          if (!place && vu) ecarts.push(`${id} affiché hors de son écran`);
          if (place === TOUJOURS && !vu) ecarts.push(`${id} masqué`);
        }
        return ecarts;
      }, mode);
    await expect.poll(ecarts, `${mode}${affichage ? ` · ${affichage}` : ""}`).toEqual([]);
  }
});

test("le transport suit les stems que le moteur garde", async ({ page }) => {
  await lireUnAlbum(page);
  await page.click('button.mode[data-mode="creer"]');
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
  for (const m of ["bibliotheque", "pratiquer", "explorer", "creer", "decouvrir", "valise", "ecoute"]) {
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
  await page.click("summary:has-text('Apparence')");
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
  await page.click("summary:has-text('Apparence')");
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

test("vue « En cours » : le morceau joué, des fonds aux couleurs de la pochette, Échap ferme", async ({ page }) => {
  // Une pochette d'une couleur franche, pour que l'extraction ait de quoi faire —
  // posée avant le rechargement : la grille a déjà mis en cache des pochettes vides.
  await page.addInitScript(() => {
    const svg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="#2a6fb0"/><circle cx="5" cy="5" r="3" fill="#e0b040"/></svg>';
    window.__FIXTURES__.cover = `data:image/svg+xml;base64,${btoa(svg)}`;
  });
  await page.reload();
  await expect(page.locator("#grille .album")).toHaveCount(3);
  await lireUnAlbum(page);
  await page.click("#transport-art");
  await expect(page.locator("#en-cours")).toBeVisible();
  await expect(page.locator("#en-cours-titre")).toHaveText(PISTES[0].title);
  await expect(page.locator("#en-cours-album")).toHaveText("Premier banc · 1999");
  // Les fonds seulement prennent la couleur ; recalée, elle reste sombre en thème sombre.
  await expect.poll(() => page.locator("#en-cours").evaluate((el) => el.style.getPropertyValue("--album-haut"))).toMatch(/^rgb\(/);
  // La file s'ouvre par-dessus ; Échap la ferme d'abord, puis la vue.
  await page.click("#bascule-file");
  await page.keyboard.press("Escape");
  await expect(page.locator("#file")).toBeHidden();
  await expect(page.locator("#en-cours")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator("#en-cours")).toBeHidden();
});

test("vue « En cours » : sans mouvement si le système le demande", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await lireUnAlbum(page);
  await page.keyboard.press("Meta+k");
  await page.keyboard.type("en cours");
  await page.keyboard.press("Enter");
  await expect(page.locator("#en-cours")).toBeVisible();
  await expect(page.locator("#en-cours-spectre")).toBeHidden();
  expect(await page.locator("#en-cours-disque").evaluate((el) => getComputedStyle(el).animationName)).toBe("none");
});

test("Bibliothèque : l'état en tête, une ligne déplie sa liste, les réglages repliés", async ({ page }) => {
  await page.evaluate(() => {
    window.__FIXTURES__.scan_failures = [["/Musique/abime.mp3", "en-tête illisible", 1_700_000_000]];
  });
  await page.click('button.mode[data-mode="bibliotheque"]');
  await expect(page.locator("#etat-echecs-compte")).toHaveText("1 fichier");
  await expect(page.locator("#etat-echecs-compte")).toHaveClass(/etat__compte--a-voir/);
  await expect(page.locator("#etat-editions-compte")).toHaveText("aucune");
  await expect(page.locator("#etat-doublons-compte")).toHaveText("à chercher");
  // Fermée, la ligne cache sa liste ; ouverte, elle la montre sur place.
  await expect(page.locator("#echecs-scan")).toBeHidden();
  await page.click("#etat-echecs summary");
  await expect(page.locator("#echecs-scan")).toContainText("abime.mp3");
  // Les réglages avancés attendent un geste (Règles 5 et 8).
  await expect(page.locator("#recalculer-carte")).toBeHidden();
  await page.click("summary:has-text('Paramètres de la carte')");
  await expect(page.locator("#recalculer-carte")).toBeVisible();
});

test("éditions face à face : ce qui part sur le réseau, les pistes alignées, retenir puis oublier", async ({ page }) => {
  await page.evaluate(() => {
    localStorage.setItem("mb-contact", "essai@example.org");
    const f = window.__FIXTURES__;
    f.editions_proposees = [
      { id: "ed-2", titre: "Premier banc", artiste: "Les Essais", date: "1999", pays: "FR", label: "Banc", catalogue: "B-1", format: "CD", pistes: 2, concorde: true },
      { id: "ed-3", titre: "Premier banc (deluxe)", artiste: "Les Essais", date: "2009", pays: null, label: null, catalogue: null, format: "2×CD", pistes: 3, concorde: false },
    ];
    f.edition_comparee = (a) => {
      const locales = window.__DONNEES__.PISTES.filter((p) => p.album === "Premier banc");
      const pistes = [
        { disque: 1, position: 1, titre: "Ouverture", duree_ms: locales[0].duration_ms, recording_id: "r1", artiste_mbid: null },
        { disque: 1, position: 2, titre: "Deuxième prise", duree_ms: locales[1].duration_ms + 8000, recording_id: "r2", artiste_mbid: null },
      ];
      if (a.id === "ed-3") pistes.push({ disque: 2, position: 1, titre: "Bonus", duree_ms: 100000, recording_id: "r3", artiste_mbid: null });
      return {
        edition: { id: a.id, titre: "Premier banc", artiste: "Les Essais", artiste_mbid: "art", release_group: null, pistes },
        locales,
        paires: pistes.map((p, i) => ({
          locale: i < locales.length ? i : null, edition: i,
          ecart_ms: i < locales.length ? p.duree_ms - locales[i].duration_ms : null, titre_differe: false,
        })),
      };
    };
    f.retenir_edition = (a, e) => { e.edition = { release_id: a.id, titre: "Premier banc", choisie_le: 1_762_000_000 }; return null; };
    f.edition_retenue = (_, e) => e.edition ?? null;
    f.oublier_edition = (_, e) => { e.edition = null; return 2; };
  });
  await page.locator("#grille .album").first().click();
  await expect(page.locator("#choisir-edition")).toBeVisible();
  await page.click("#choisir-edition");
  await expect(page.locator("#editions")).toBeVisible();
  await expect(page.locator("#editions-reseau")).toContainText("musicbrainz.org");
  // La première édition est comparée, pas retenue ; l'écart de 8 s est marqué.
  await expect(page.locator("#editions-pistes-corps tr")).toHaveCount(2);
  await expect(page.locator(".editions__ligne--marquee")).toHaveCount(1);
  await expect(page.locator(".editions__ligne--marquee .editions__marque")).toHaveText("+8 s");
  expect(await appels(page, "retenir_edition")).toEqual([]);
  // Une autre édition : une piste en trop, marquée.
  await page.locator(".editions__option").nth(1).click();
  await expect(page.locator("#editions-pistes-corps tr")).toHaveCount(3);
  await expect(page.locator("#editions-pistes-corps tr").nth(2)).toContainText("absente de l'album");
  await page.click("#editions-retenir");
  await expect(page.locator("#editions-retenue")).toBeVisible();
  expect((await appels(page, "retenir_edition")).at(-1)).toMatchObject({ id: "ed-3", album: "Premier banc" });
  await page.click("#editions-oublier");
  await expect(page.locator("#editions-retenue")).toBeHidden();
  await page.keyboard.press("Escape");
  await expect(page.locator("#editions")).toBeHidden();
});

test("passage vers l'iPhone : ⌘K emporte la lecture, le Mac dit où elle attend", async ({ page }) => {
  await page.evaluate(() => {
    const f = window.__FIXTURES__;
    f.valise_iphone = {
      xcode: true, udid: "u1", app_installee: true, echeance: null, valises: [], renouvelable: false, erreur: null,
      appareils: [{ udid: "u1", nom: "iPhone d'essai", modele: "iPhone 15", filaire: true }],
    };
    f.emporter_lecture = (a) => ({ pistes: 2, titre: "Ouverture", position_ms: 12_000, mac_en_pause: true, par_wifi: false, cable_echec: null });
  });
  await lireUnAlbum(page);
  await page.keyboard.press("Meta+k");
  await page.keyboard.type("emporter");
  await expect(page.locator(".palette__option", { hasText: "Emporter la lecture" })).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.locator("#valise-vue")).toBeVisible();
  await expect(page.locator("#iphone-emporter")).toBeVisible();
  await expect(page.locator("#iphone-emporter-etat")).toContainText("attend sur l'iPhone, en pause");
  await expect(page.locator("#iphone-emporter-etat")).toContainText("0:12");
  expect((await appels(page, "emporter_lecture")).at(-1)).toEqual({ udid: "u1" });
});

test("passage par le Wi-Fi : sans Xcode, la lecture attend l'iPhone appairé, puis on l'oublie", async ({ page }) => {
  await page.evaluate(() => {
    const f = window.__FIXTURES__;
    f.wifi_etat = (_, e) => e.oublie
      ? { appaire: false, ecoute: false, port: null, adresses: [], erreur: null, en_attente: false, dernier: null }
      : { appaire: true, ecoute: true, port: 47391, adresses: ["mac.local"], erreur: null, en_attente: Boolean(e.attente), valise: null, dernier: null };
    f.emporter_lecture = (_, e) => { e.attente = true; return { pistes: 2, titre: "Ouverture", position_ms: 12_000, mac_en_pause: false, par_wifi: true, cable_echec: null }; };
    f.oublier_iphone = (_, e) => { e.oublie = true; return null; };
  });
  await lireUnAlbum(page);
  await page.click('button.mode[data-mode="valise"]');
  await expect(page.locator("#iphone-wifi")).toContainText("appairé, l'iPhone retrouve ce Mac");
  await expect(page.locator("#iphone-emporter")).toBeVisible();
  await page.click("#iphone-emporter");
  await expect(page.locator("#iphone-emporter-etat")).toContainText("attend l'iPhone sur le Wi-Fi");
  await expect(page.locator("#iphone-wifi")).toContainText("Une lecture attend que l'iPhone la prenne");
  expect((await appels(page, "emporter_lecture")).at(-1)).toEqual({ udid: null });

  await page.click("#iphone-oublier");
  await expect(page.locator("#iphone-oublier")).toBeHidden();
  await expect(page.locator("#iphone-emporter")).toBeHidden();
});

test("valise par le Wi-Fi : sans câble, Envoyer prépare et la ligne dit qu'elle attend l'iPhone", async ({ page }) => {
  await page.evaluate(() => {
    const f = window.__FIXTURES__;
    f.valise_apercu = {
      albums: [{ id: "a1", album: "Premier disque", artiste: "Quatuor fictif", annee: 2020, regle: "recents", famille: null, nb_pistes: 8, octets: 80_000_000, duree_ms: 2_400_000, ajoute_le: null, ecoutes: 0 }],
      octets: 80_000_000, budget_octets: 8_000_000_000,
    };
    f.wifi_etat = (_, e) => ({ appaire: true, ecoute: true, port: 47391, adresses: ["mac.local"], erreur: null, en_attente: false,
      valise: e.prete ? "valise-essai" : null, dernier: null });
    f.valise_envoyer = (_, e) => { e.tours = 0; return null; };
    f.valise_etat = (_, e) => {
      const fini = e.tours !== undefined && ++e.tours > 1;
      if (fini) e.prete = true;
      return {
        envoi: e.tours === undefined || fini
          ? { en_cours: false, etape: "", faits: 0, total: 0, resultat: fini ? "« valise-essai » prête pour le Wi-Fi : 1 albums, 0.1 Go" : null }
          : { en_cours: true, etape: "Préparation pour le Wi-Fi", faits: 0, total: 1, resultat: null },
        renouvellement: { en_cours: false, ligne: "", resultat: null },
      };
    };
  });
  await page.click('button.mode[data-mode="valise"]');
  await expect(page.locator("#valise-envoyer")).toBeEnabled();
  await page.click("#valise-envoyer");
  await expect(page.locator("#valise-avancement")).toContainText("prête pour le Wi-Fi");
  await expect(page.locator("#iphone-wifi")).toContainText("«\u00a0valise-essai\u00a0» attend que l'iPhone la prenne");
  expect((await appels(page, "valise_envoyer")).at(-1).udid).toBeNull();
});

test("Compléter : la phrase dit ce qui part, la passe se lance puis s'annule en bloc", async ({ page }) => {
  await page.evaluate(() => {
    localStorage.setItem("mb-contact", "essai@example.org");
    const f = window.__FIXTURES__;
    f.completer_apercu = (_, e) => (e.complete ? { musicbrainz: 0, lastfm: 0, secondes: 0 } : { musicbrainz: 12, lastfm: 0, secondes: 24 });
    f.completer_lancer = (_, e) => { e.tours = 0; return null; };
    f.completer_etat = (_, e) => {
      if (e.tours !== undefined && e.tours < 2) {
        e.tours++;
        return { passe: { en_cours: true, etape: "Genres MusicBrainz", faits: 5, total: 12, resultat: null }, derniere: null };
      }
      if (e.tours === undefined) return { passe: { en_cours: false, etape: "", faits: 0, total: 0, resultat: null }, derniere: null };
      e.complete = !e.annulee;
      return {
        passe: { en_cours: false, etape: "", faits: 12, total: 12, resultat: "fini" },
        derniere: { id: 1, nom: "completer", debut: 1_762_000_000, fin: 1_762_000_030, annulee_le: e.annulee ? 1_762_000_100 : null, bilan: "12 artistes chez MusicBrainz, 9 avec un genre, 40 albums", ecritures: 80 },
      };
    };
    f.completer_annuler = (_, e) => { e.annulee = true; e.complete = false; return 80; };
  });
  await page.click('button.mode[data-mode="bibliotheque"]');
  await expect(page.locator("#completer-reseau")).toContainText("musicbrainz.org pour 12 artistes");
  await expect(page.locator("#completer-lancer")).toBeEnabled();
  await page.click("#completer-lancer");
  await expect(page.locator("#completer-arreter")).toBeVisible();
  await expect(page.locator("#completer-etat")).toContainText("5 / 12 artistes");
  await expect(page.locator("#completer-etat")).toContainText("Dernière passe", { timeout: 5000 });
  await expect(page.locator("#completer-lancer")).toBeDisabled();
  page.once("dialog", (d) => d.accept());
  await page.click("#completer-annuler");
  await expect(page.locator("#completer-etat")).toContainText("annulée");
  await expect(page.locator("#completer-annuler")).toBeHidden();
  expect((await appels(page, "completer_annuler")).at(-1)).toEqual({ id: 1 });
});

test("balade : clic droit sur un morceau, « Flâner d'ici », la carte dit où elle en est, on l'arrête", async ({ page }) => {
  await page.click('button.mode[data-mode="explorer"]');
  await page.waitForTimeout(800);
  // Le nuage, vue initiale (`carte.vue` = identité) : même calcul que `versEcran`.
  const p = fixtures.map_view[0];
  const r = await page.locator("#carte").boundingBox();
  const c = Math.min(r.width, r.height) / 2 - 28;
  await page.mouse.click(r.x + r.width / 2 + p.x * c, r.y + r.height / 2 + p.y * c, { button: "right" });
  await expect(page.locator("#carte-menu")).toBeVisible();
  await page.click("#carte-menu-flaner");
  await expect.poll(async () => (await appels(page, "balade_demarrer")).at(-1)?.depart).toBe(p.id);
  await expect(page.locator("#carte-menu")).toBeHidden();
  await expect(page.locator("#carte-balade")).toBeVisible();
  await expect(page.locator("#carte-balade-texte")).toHaveText("Balade · 1 morceau · par les voisins");
  await page.click("#carte-balade-arreter");
  await expect(page.locator("#carte-balade")).toBeHidden();
  expect(await appels(page, "balade_arreter")).toHaveLength(1);
});

test("carte : l'option « Pentes » montre les flèches entre quartiers et se garde", async ({ page }) => {
  // Un style minimal qui porte des pentes (le vrai sort de `style.rs`), et
  // l'instance MapLibre retenue au passage pour en lire les couches.
  await page.addInitScript(() => {
    let lib;
    Object.defineProperty(window, "maplibregl", {
      configurable: true,
      get: () => lib,
      set: (v) => {
        const Carte = v.Map;
        v.Map = class extends Carte {
          constructor(o) {
            super(o);
            window.__CARTE_GL__ = this;
          }
        };
        lib = v;
      },
    });
    const masquee = { visibility: "none" };
    const pente = {
      type: "FeatureCollection",
      features: [
        { type: "Feature", properties: { partie: "courbe", proximite: 0.8 },
          geometry: { type: "LineString", coordinates: [[2.33, 48.85], [2.35, 48.86]] } },
      ],
    };
    window.__FIXTURES__.style_carte = () => ({
      version: 8,
      center: [2.34, 48.855],
      zoom: 13,
      sources: { pentes: { type: "geojson", data: pente } },
      layers: [
        { id: "terre-reelle", type: "background", paint: { "background-color": "#F2EFE9" } },
        { id: "pentes-lisere", type: "line", source: "pentes", layout: masquee },
        { id: "pentes-courbe", type: "line", source: "pentes", layout: masquee },
        { id: "pentes-tete", type: "fill", source: "pentes", layout: masquee },
      ],
    });
  });
  await page.reload();
  await page.click('button.mode[data-mode="explorer"]');
  await page.click('[data-affichage="carte"]');
  // Les calques sont posés sur la carte (Règle 12), dans leur panneau.
  await page.click("#outil-calques");
  const option = page.locator("#carte-pentes");
  await expect(page.locator("#ligne-pentes")).toBeVisible();
  await expect(option).not.toBeChecked();
  const visibilite = () =>
    page.evaluate(() => window.__CARTE_GL__.getLayoutProperty("pentes-courbe", "visibility"));
  expect(await visibilite()).toBe("none");
  await option.check();
  await expect.poll(visibilite).toBe("visible");
  expect(await page.evaluate(() => localStorage.getItem("carte-pentes"))).toBe("1");
  await option.uncheck();
  await expect.poll(visibilite).toBe("none");
});

test("carte : les balades passées se listent, se redessinent et se rejouent", async ({ page }) => {
  await page.click('button.mode[data-mode="explorer"]');
  // Repliées dans le rail (Règle 13) : un clic sur le titre les déplie.
  await page.click("#bloc-balades-passees > summary");
  const ligne = page.locator("#balades-passees .balade-passee");
  await expect(ligne).toHaveCount(1);
  const voir = ligne.locator(".balade-passee__voir");
  await expect(voir).toContainText("3 morceaux · 9 min");
  await voir.click();
  await expect.poll(async () => (await appels(page, "balade_passee")).at(-1)?.id).toBe(7);
  await expect(voir).toHaveAttribute("aria-pressed", "true");
  await ligne.locator(".balade-passee__jouer", { hasText: "▶" }).click();
  await expect.poll(async () => (await appels(page, "balade_rejouer")).length).toBe(1);
  await expect(page.locator("#np-titre")).toHaveText(PISTES[0].title);
});

test("carte : le brouillard voile le jamais-écouté et se lève à chaque écoute", async ({ page }) => {
  // Le style réel porte les couches de `style::poser_traces` ; l'instance
  // MapLibre est retenue au passage pour en lire l'état.
  await page.addInitScript(() => {
    let lib;
    Object.defineProperty(window, "maplibregl", {
      configurable: true,
      get: () => lib,
      set: (v) => {
        const Carte = v.Map;
        v.Map = class extends Carte {
          constructor(o) {
            super(o);
            window.__CARTE_GL__ = this;
          }
        };
        lib = v;
      },
    });
    const vide = { type: "FeatureCollection", features: [] };
    const masquee = { visibility: "none" };
    window.__FIXTURES__.style_carte = () => ({
      version: 8,
      center: [2.34, 48.855],
      zoom: 15,
      sources: {
        carte: { type: "geojson", data: vide },
        sentiers: { type: "geojson", data: vide },
      },
      layers: [
        { id: "terre-reelle", type: "background", paint: { "background-color": "#F2EFE9" } },
        { id: "brouillard", type: "fill", source: "carte", layout: masquee },
        { id: "sentiers", type: "line", source: "sentiers", layout: masquee },
        { id: "brouillard-point", type: "circle", source: "carte", layout: masquee },
      ],
    });
  });
  await page.reload();
  await page.click('button.mode[data-mode="explorer"]');
  await page.click('[data-affichage="carte"]');
  await page.click("#outil-calques");
  await expect(page.locator("#ligne-traces")).toBeVisible();
  const visibilite = (c) =>
    page.evaluate((couche) => window.__CARTE_GL__.getLayoutProperty(couche, "visibility"), c);
  expect(await visibilite("brouillard")).toBe("none");
  await page.locator("#carte-brouillard").check();
  await expect.poll(() => visibilite("brouillard")).toBe("visible");
  await expect.poll(() => visibilite("brouillard-point")).toBe("visible");
  expect(await page.evaluate(() => localStorage.getItem("carte-brouillard"))).toBe("1");
  expect(await appels(page, "morceaux_ecoutes")).toHaveLength(1);
  const decouvert = (id) =>
    page.evaluate(
      (i) => !!window.__CARTE_GL__.getFeatureState({ source: "carte", id: i }).ecoute,
      id,
    );
  await expect.poll(() => decouvert(1)).toBe(true);
  expect(await decouvert(2)).toBe(false);
  await page.evaluate(() => window.__EMETTRE__("ecoute-notee", 2));
  await expect.poll(() => decouvert(2)).toBe(true);
  await page.locator("#carte-sentiers").check();
  await expect.poll(() => visibilite("sentiers")).toBe("visible");
  await expect.poll(async () => (await appels(page, "sentiers_carte")).length).toBeGreaterThan(0);
});

test("carte : le relief, sa grandeur, et une courbe qu'on suit puis qu'on ouvre", async ({ page }) => {
  await page.addInitScript(() => {
    let lib;
    Object.defineProperty(window, "maplibregl", {
      configurable: true,
      get: () => lib,
      set: (v) => {
        const Carte = v.Map;
        v.Map = class extends Carte {
          constructor(o) {
            super(o);
            window.__CARTE_GL__ = this;
          }
        };
        lib = v;
      },
    });
    const masquee = { visibility: "none" };
    // Une courbe de niveau à travers le centre de la vue.
    const courbe = {
      type: "FeatureCollection",
      features: [
        { type: "Feature", properties: { ligne: 3, niveau: 6 },
          geometry: { type: "LineString", coordinates: [[2.30, 48.855], [2.38, 48.855]] } },
      ],
    };
    window.__FIXTURES__.style_carte = () => ({
      version: 8,
      center: [2.34, 48.855],
      zoom: 14,
      sources: { courbes: { type: "geojson", data: courbe } },
      layers: [
        { id: "terre-reelle", type: "background", paint: { "background-color": "#F2EFE9" } },
        { id: "relief-ombre", type: "background", layout: masquee, paint: { "background-opacity": 0 } },
        { id: "courbes", type: "line", source: "courbes", layout: masquee,
          paint: { "line-width": 2, "line-color": "#9C7C5E" } },
        { id: "courbes-choisie", type: "line", source: "courbes", layout: masquee,
          filter: ["==", ["get", "ligne"], -1] },
        { id: "courbes-numero", type: "line", source: "courbes", layout: masquee },
      ],
    });
  });
  await page.reload();
  await page.click('button.mode[data-mode="explorer"]');
  await page.click('[data-affichage="carte"]');
  await page.click("#outil-calques");
  await expect(page.locator("#ligne-relief")).toBeVisible();
  await expect(page.locator("#relief-grandeur")).toBeHidden();
  const visibilite = (c) =>
    page.evaluate((couche) => window.__CARTE_GL__.getLayoutProperty(couche, "visibility"), c);
  await page.locator("#carte-relief").check();
  await expect.poll(() => visibilite("courbes")).toBe("visible");
  await expect(page.locator("#relief-grandeur")).toBeVisible();
  expect((await appels(page, "relief_musical")).at(-1)).toEqual({ grandeur: "energie" });
  await page.click('#relief-grandeur [data-grandeur="tempo"]');
  await expect.poll(async () => (await appels(page, "relief_musical")).at(-1)?.grandeur).toBe("tempo");
  expect(await page.evaluate(() => localStorage.getItem("relief-grandeur"))).toBe("tempo");

  // Un clic sur la courbe la désigne ; la molette la suit ; la pastille l'ouvre.
  const centre = await page.evaluate(() => {
    const p = window.__CARTE_GL__.project([2.34, 48.855]);
    return [p.x, p.y];
  });
  await expect
    .poll(() =>
      page.evaluate(([x, y]) => window.__CARTE_GL__.queryRenderedFeatures(
        [[x - 4, y - 4], [x + 4, y + 4]], { layers: ["courbes"] }).length, centre),
    )
    .toBeGreaterThan(0);
  const cnvBox = await page.locator("#carte").boundingBox();
  await page.mouse.click(cnvBox.x + centre[0], cnvBox.y + centre[1]);
  await expect.poll(async () => (await appels(page, "courbe")).at(-1)?.ligne).toBe(3);
  await expect(page.locator("#fil-compte")).toContainText("Courbe 6");
  await expect.poll(() => visibilite("courbes-choisie")).toBe("visible");
  await page.mouse.move(cnvBox.x + centre[0] + 60, cnvBox.y + centre[1] + 60);
  await page.mouse.wheel(0, 120);
  await expect(page.locator("#carte-info")).toContainText("1 / 2 sur la courbe 6");
  await page.mouse.wheel(0, 120);
  await expect(page.locator("#carte-info")).toContainText("2 / 2 sur la courbe 6");
  // La pastille du numéro est posée là où l'on a cliqué, sur la courbe.
  await page.mouse.click(cnvBox.x + centre[0], cnvBox.y + centre[1]);
  await expect.poll(async () => (await appels(page, "ouvrir_courbe")).at(-1)?.ligne).toBe(3);
  await expect(page.locator("#fil-compte")).toContainText("3 morceaux au-dessus de 6");
});

test("balade : ses réglages, dont les allures du relief, sont dans son bandeau", async ({ page }) => {
  await page.click('button.mode[data-mode="explorer"]');
  await page.waitForTimeout(800);
  // Pas dans le rail (Règle 13) : ils ne servent qu'une balade lancée.
  await expect(page.locator("#balade-intervention")).toBeHidden();
  const p = fixtures.map_view[0];
  const r = await page.locator("#carte").boundingBox();
  const c = Math.min(r.width, r.height) / 2 - 28;
  await page.mouse.click(r.x + r.width / 2 + p.x * c, r.y + r.height / 2 + p.y * c, { button: "right" });
  await page.click("#carte-menu-flaner");
  await expect(page.locator("#carte-balade")).toBeVisible();
  await page.click("#carte-balade-reglages");
  await expect(page.locator("#balade-intervention")).toBeVisible();
  await page.click('#balade-reglages [data-allure="niveau"]');
  await expect(page.locator('#balade-reglages [data-allure="niveau"]')).toHaveClass(/segment--actif/);
  await expect(page.locator('#balade-reglages [data-allure="deriver"]')).not.toHaveClass(/segment--actif/);
  expect(await page.evaluate(() => localStorage.getItem("balade-allure"))).toBe("niveau");
  await expect.poll(async () => (await appels(page, "balade_regler")).at(-1)).toEqual({ allure: "niveau" });
});

test("carte : épingler ici, flâner depuis l'épingle, la retirer", async ({ page }) => {
  // Un plan de ville : un style vide et une adresse réelle pour chaque morceau.
  await page.addInitScript(() => {
    let lib;
    Object.defineProperty(window, "maplibregl", {
      configurable: true,
      get: () => lib,
      set: (v) => {
        const Carte = v.Map;
        v.Map = class extends Carte {
          constructor(o) {
            super(o);
            window.__CARTE_GL__ = this;
          }
        };
        lib = v;
      },
    });
    window.__FIXTURES__.style_carte = () => ({
      version: 8,
      center: [2.34, 48.855],
      zoom: 15,
      sources: {},
      layers: [{ id: "terre-reelle", type: "background", paint: { "background-color": "#F2EFE9" } }],
    });
    window.__FIXTURES__.positions_carte = () =>
      Object.fromEntries(window.__DONNEES__.PISTES.map((p, i) => [p.id, [2.30 + i * 0.002, 48.80]]));
  });
  await page.reload();
  await page.click('button.mode[data-mode="explorer"]');
  await page.click('[data-affichage="carte"]');
  await expect.poll(() => page.evaluate(() => !!window.__CARTE_GL__?.loaded())).toBe(true);
  const box = await page.locator("#carte").boundingBox();
  const [x, y] = [box.x + box.width / 2, box.y + box.height / 2];
  await page.mouse.click(x, y, { button: "right" });
  await expect(page.locator("#carte-menu-epingler")).toBeVisible();
  await expect(page.locator("#carte-menu-flaner")).toBeHidden();
  await page.click("#carte-menu-epingler");
  await page.fill("#epingle-nom", "Chez moi");
  await page.fill("#epingle-emoji", "🏠");
  await page.click('#epingle-nuancier [data-couleur="#2F6FB0"]');
  await page.click("#epingle-poser");
  const pose = (await appels(page, "creer_epingle")).at(-1);
  expect(pose).toMatchObject({ nom: "Chez moi", emoji: "🏠", couleur: "#2F6FB0" });
  expect(pose.lon).toBeCloseTo(2.34, 3);
  expect(pose.lat).toBeCloseTo(48.855, 3);
  await expect(page.locator("#carte-menu")).toBeHidden();
  // Sur l'épingle : flâner d'ici (depuis le morceau le plus proche), ou la retirer.
  await page.mouse.click(x, y, { button: "right" });
  await expect(page.locator("#carte-menu-retirer")).toBeVisible();
  await expect(page.locator("#carte-menu-epingler")).toBeHidden();
  await page.click("#carte-menu-flaner");
  await expect.poll(async () => (await appels(page, "balade_demarrer")).length).toBe(1);
  await page.click("#carte-balade-arreter");
  await page.mouse.click(x, y, { button: "right" });
  await page.click("#carte-menu-retirer");
  await expect.poll(async () => (await appels(page, "supprimer_epingle")).at(-1)?.id).toBe(1);
});

test("carte : garder une balade passée en chaîne, la rejouer, retirer son tracé", async ({ page }) => {
  await page.click('button.mode[data-mode="explorer"]');
  await expect(page.locator("#bloc-chaines")).toBeHidden();
  await page.click("#bloc-balades-passees > summary");
  await page.locator("#balades-passees .balade-passee button", { hasText: "☆" }).click();
  await expect(page.locator("#carte-garder")).toBeVisible();
  await expect(page.locator("#garder-nom")).toHaveValue(/^Balade du /);
  await page.fill("#garder-nom", "Dimanche");
  await page.click("#garder-ok");
  expect((await appels(page, "garder_chaine")).at(-1)).toMatchObject({
    nom: "Dimanche", origine: "balade", morceaux: [1, 2, 3],
  });
  await expect(page.locator("#carte-garder")).toBeHidden();
  await page.click("#bloc-chaines > summary");
  const ligne = page.locator("#chaines .balade-passee");
  await expect(ligne).toHaveCount(1);
  await expect(ligne).toContainText("Dimanche · 3 morceaux");
  await ligne.locator("button", { hasText: "▶" }).click();
  await expect.poll(async () => (await appels(page, "playlist_pistes")).at(-1)?.id).toBe(100);
  await ligne.locator("button", { hasText: "✕" }).click();
  await expect(page.locator("#bloc-chaines")).toBeHidden();
});

test("carte : le haut-parleur de l'aura, éteint hors balade, fait entendre les voisins du curseur", async ({ page }) => {
  await page.click('button.mode[data-mode="explorer"]');
  await page.waitForTimeout(800);
  const hp = page.locator("#carte-aura");
  await expect(hp).toBeVisible();
  await expect(hp).toHaveAttribute("aria-pressed", "false");
  await expect(hp).toContainText("Entendre autour");
  const p = fixtures.map_view[0];
  const r = await page.locator("#carte").boundingBox();
  const c = Math.min(r.width, r.height) / 2 - 28;
  const x = r.x + r.width / 2 + p.x * c;
  const y = r.y + r.height / 2 + p.y * c;
  // Éteint : s'arrêter sur un morceau ne fait rien entendre.
  await page.mouse.move(x + 3, y + 3);
  await page.waitForTimeout(400);
  expect((await appels(page, "aura")).filter((a) => a.voix.length)).toHaveLength(0);
  // Allumé : à l'arrêt du curseur, trois voix au plus, la plus proche en tête.
  await hp.click();
  await expect(hp).toHaveAttribute("aria-pressed", "true");
  await expect(hp).toContainText("Aura allumée");
  await page.mouse.move(x + 2, y + 2);
  await expect.poll(async () => (await appels(page, "aura")).at(-1)?.voix.length ?? 0).toBeGreaterThan(0);
  const voix = (await appels(page, "aura")).at(-1).voix;
  expect(voix.length).toBeLessThanOrEqual(3);
  expect(voix[0].id).toBe(p.id);
  for (let i = 1; i < voix.length; i++) expect(voix[i].gain).toBeLessThanOrEqual(voix[i - 1].gain);
  expect(voix.every((v) => v.pan >= -1 && v.pan <= 1)).toBe(true);
  // Quitter la carte la fait taire ; l'éteindre aussi.
  await page.mouse.move(r.x + r.width / 2, r.y - 40);
  await expect.poll(async () => (await appels(page, "aura")).at(-1)?.voix.length).toBe(0);
  await hp.click();
  await expect(hp).toHaveAttribute("aria-pressed", "false");
});

test("balade : en mode « demander », le carrefour attend ; 2 prend la seconde voie, l'extrait suit le survol", async ({ page }) => {
  // Au départ, la balade reprend les derniers réglages choisis (gardés en local).
  await page.evaluate(() => {
    localStorage.setItem("balade-intervention", "demander");
    localStorage.setItem("balade-allure", "aventurer");
  });
  await page.reload();
  await page.click('button.mode[data-mode="explorer"]');
  await page.waitForTimeout(800);
  // La fin du morceau : le carrefour se présente dans les 30 dernières secondes.
  await page.evaluate(() => {
    const lire = window.__FIXTURES__.playback_state;
    window.__FIXTURES__.playback_state = (a, e) => ({ ...lire(a, e), position_ms: e.file ? 165_000 : 0 });
  });
  const p = fixtures.map_view[0];
  const r = await page.locator("#carte").boundingBox();
  const c = Math.min(r.width, r.height) / 2 - 28;
  await page.mouse.click(r.x + r.width / 2 + p.x * c, r.y + r.height / 2 + p.y * c, { button: "right" });
  await page.click("#carte-menu-flaner");
  await expect.poll(async () => (await appels(page, "balade_demarrer")).at(-1)?.intervention).toBe("demander");
  expect((await appels(page, "balade_demarrer")).at(-1).allure).toBe("aventurer");
  const panneau = page.locator("#carte-carrefour");
  await expect(panneau).toBeVisible();
  await expect(panneau.locator(".voie")).toHaveCount(2);
  await expect(page.locator("#carrefour-etat")).toHaveText("La balade attend votre choix.");
  // Le panneau ne couvre pas le promeneur (le morceau de départ).
  const b = await panneau.boundingBox();
  const px = r.x + r.width / 2 + p.x * c;
  const py = r.y + r.height / 2 + p.y * c;
  expect(px < b.x || px > b.x + b.width || py < b.y || py > b.y + b.height).toBe(true);
  // Survoler une voie fait entendre son extrait ; la quitter le coupe.
  await panneau.locator(".voie").nth(1).hover();
  await expect.poll(async () => (await appels(page, "balade_extrait")).at(-1)?.morceau).toBe(
    fixtures.map_view[3].id,
  );
  // La voie entendue le montre.
  await expect(panneau.locator(".voie").nth(1)).toHaveClass(/voie--entendue/);
  await page.mouse.move(r.x + 4, r.y + r.height - 4);
  await expect.poll(async () => (await appels(page, "balade_extrait")).at(-1)?.morceau).toBe(null);
  await expect(panneau.locator(".voie--entendue")).toHaveCount(0);
  await page.keyboard.press("2");
  await expect.poll(async () => (await appels(page, "balade_choisir")).at(-1)).toEqual({ numero: 1, voie: 1 });
  await expect(page.locator("#carrefour-etat")).toHaveText("Vous prenez la voie 2.");
  // Le suivant figé (10 dernières secondes) : le panneau se grise et le dit.
  await page.evaluate(() => {
    window.__ETAT_FAUX__.balade.fige = true;
  });
  await expect(page.locator("#carrefour-etat")).toHaveText(
    "Trop tard pour changer : le morceau suivant est déjà en route.",
  );
  await expect(panneau.locator(".voie").first()).toBeDisabled();
  // ← ne change pas de piste tant que le panneau est là.
  const avant = (await appels(page, "skip")).length + (await appels(page, "previous")).length;
  await page.keyboard.press("ArrowLeft");
  expect((await appels(page, "skip")).length + (await appels(page, "previous")).length).toBe(avant);
  // Échap masque le panneau de ce carrefour.
  await page.keyboard.press("Escape");
  await expect(panneau).toBeHidden();
  // Le réglage reste d'une session à l'autre.
  expect(await page.evaluate(() => localStorage.getItem("balade-intervention"))).toBe("demander");
  // Une autre allure en cours de balade, dans les réglages du bandeau : elle
  // part au moteur, seule.
  await page.click("#carte-balade-reglages");
  await page.click('#balade-allure [data-allure="rester"]');
  await expect.poll(async () => (await appels(page, "balade_regler")).at(-1)).toEqual({ allure: "rester" });
  await expect(page.locator('#balade-allure [data-allure="rester"]')).toHaveClass(/segment--actif/);
  expect(await page.evaluate(() => localStorage.getItem("balade-allure"))).toBe("rester");
  // En balade, l'aura s'allume d'elle-même autour du promeneur (la carte
  // dézoomée rapproche les morceaux), sans celui qu'il écoute ni le suivant.
  await expect(page.locator("#carte-aura")).toHaveAttribute("aria-pressed", "true");
  await page.click("#zoom-moins");
  await page.click("#zoom-moins");
  await expect.poll(async () => (await appels(page, "aura")).filter((a) => a.voix.length).length).toBeGreaterThan(0);
  const autour = (await appels(page, "aura")).filter((a) => a.voix.length).at(-1).voix.map((v) => v.id);
  expect(autour).not.toContain(p.id);
  expect(autour).not.toContain(await page.evaluate(() => window.__ETAT_FAUX__.balade.suivant));
});
