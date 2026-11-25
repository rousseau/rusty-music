// SPDX-License-Identifier: GPL-3.0-or-later
// Tests de fumée : l'interface réelle, Tauri simulé (`faux-tauri.js`,
// `fixtures.js`). Ils ne vérifient pas le moteur, seulement que chaque écran
// s'ouvre, qu'un morceau se lit et qu'Éditer monte ses stems, sans la moindre
// erreur JavaScript.
const { test, expect } = require("@playwright/test");
const { ouvrir, appelsInconnus, versAlbums } = require("./outils");
const { fixtures, evenements, donnees, PISTES } = require("./fixtures");
const { pratiquerAlbum, basseTranscrite, batterieTranscrite } = require("./pratique-fictive");

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
  // L'Accueil d'abord (A2), puis la grille d'albums d'où partent les tests.
  await expect(page.locator("#accueil-vue .rangee")).toHaveCount(1);
  await versAlbums(page);
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

/* ------------------------------------------------- largeur des panneaux */

test("panneaux : les poignées règlent le rail et l'inspecteur, mémorisés, double-clic pour l'origine", async ({ page }) => {
  const largeur = (s) => page.locator(s).evaluate((e) => Math.round(e.getBoundingClientRect().width));
  expect(await largeur(".rail")).toBe(232);
  expect(await largeur(".inspecteur")).toBe(300);

  const glisser = async (sel, dx) => {
    const b = await page.locator(sel).boundingBox();
    const x = b.x + b.width / 2, y = b.y + b.height / 2;
    await page.mouse.move(x, y);
    await page.mouse.down();
    await page.mouse.move(x + dx / 2, y);
    await page.mouse.move(x + dx, y);
    await page.mouse.up();
  };
  await glisser(".poignee--rail", 80);
  await glisser(".poignee--droite", -100);
  expect(await largeur(".rail")).toBe(312);
  expect(await largeur(".inspecteur")).toBe(400);
  // La file recouvre toujours exactement l'inspecteur (Règle 10).
  await page.click("#bascule-file");
  expect(await largeur("#file")).toBe(400);
  await page.click("#bascule-file");

  // Bornes : le rail ne descend pas sous 200 px.
  await glisser(".poignee--rail", -400);
  expect(await largeur(".rail")).toBe(200);
  // Clavier : → élargit le rail d'un pas.
  await page.locator(".poignee--rail").focus();
  await page.keyboard.press("ArrowRight");
  expect(await largeur(".rail")).toBe(216);

  // Mémorisé : un rechargement garde les largeurs.
  await page.reload();
  await expect(page.locator(".poignee--rail")).toBeAttached();
  expect(await largeur(".rail")).toBe(216);
  expect(await largeur(".inspecteur")).toBe(400);

  await page.locator(".poignee--droite").dblclick();
  expect(await largeur(".inspecteur")).toBe(300);
});

/* ------------------------------------------------------------ Accueil (A2) */

async function versAccueil(page) {
  await page.locator('[data-vuelib="accueil"]').click();
  await expect(page.locator("#accueil-vue")).toBeVisible();
  await expect(page.locator("#fil-titre")).toHaveText("Accueil");
}

test("A2 : l'Accueil, l'album à la une et ses rangées, demandés pour le jour local", async ({ page }) => {
  await versAccueil(page);
  await expect(page.locator("#accueil-une-raison")).toHaveText("Nouvel album de Les Essais : « Troisième banc »");
  await expect(page.locator("#accueil-une-titre")).toHaveText("Premier banc");
  await expect(page.locator("#accueil-rangees .rangee__titre")).toHaveText(["Jamais écoutés"]);
  await expect(page.locator("#accueil-rangees .album")).toHaveCount(2);
  // Ni tri, ni repère, ni filtre par famille sur l'Accueil.
  for (const id of ["grille", "liste", "tri-albums", "index-alpha", "bloc-familles-ecoute"]) {
    await expect(page.locator(`#${id}`)).toBeHidden();
  }
  const demande = (await appels(page, "accueil")).at(-1);
  expect(demande.date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  expect(demande.decalages).toEqual({});
});

test("filtre du rail sur l'Accueil : les albums trouvés dans toute la bibliothèque, à la place de la page", async ({ page }) => {
  await versAccueil(page);
  await page.fill("#q", "essais");
  await expect(page.locator("#accueil-resultats .album")).toHaveCount(2);
  await expect(page.locator("#accueil-rangees")).toBeHidden();
  await expect(page.locator("#accueil-une")).toBeHidden();
  await expect(page.locator("#fil-compte")).toHaveText("2 albums trouvés");
  await page.fill("#q", "");
  await expect(page.locator("#accueil-resultats")).toBeHidden();
  await expect(page.locator("#accueil-rangees .album")).toHaveCount(2);
  await expect(page.locator("#accueil-une")).toBeVisible();
});

test("A2 : ↻ redemande la page avec le décalage de sa seule rangée", async ({ page }) => {
  await versAccueil(page);
  await page.locator(".rangee__autre").click();
  await expect.poll(async () => (await appels(page, "accueil")).at(-1).decalages).toEqual({ jamais: 1 });
});

test("A2 : une pochette ouvre l'album, ← revient à l'Accueil", async ({ page }) => {
  await versAccueil(page);
  await page.locator("#accueil-rangees .album", { hasText: "Second banc" }).click();
  await expect(page.locator("#fil-titre")).toHaveText("Second banc");
  await expect(page.locator("#accueil-vue")).toBeHidden();
  await page.locator("#retour").click();
  await expect(page.locator("#fil-titre")).toHaveText("Accueil");
  await expect(page.locator("#accueil-rangees .album")).toHaveCount(2);
});

test("A3 : l'image large de l'artiste derrière l'album à la une, demandée par MBID", async ({ page }) => {
  // Sans image (réponse par défaut) : le dégradé seul, pas de mention.
  await versAccueil(page);
  await expect(page.locator("#accueil-une")).not.toHaveClass(/une--photo/);
  await expect(page.locator("#accueil-une-credit")).toBeHidden();
  expect((await appels(page, "accueil_fond")).at(-1)).toEqual({ mbid: "0383dadf-2a4e-4d10-a46a-e9e041da8eb3" });

  // Avec une image, au rechargement : la photo et sa source.
  await page.addInitScript(() => {
    const svg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 9"><rect width="16" height="9" fill="#406080"/></svg>';
    window.__FIXTURES__.accueil_fond = `data:image/svg+xml;base64,${btoa(svg)}`;
  });
  await page.reload();
  await expect(page.locator("#accueil-une")).toHaveClass(/une--photo/);
  await expect(page.locator("#accueil-une-credit")).toHaveText("photo : TheAudioDB");
  await expect(page.locator("#accueil-une .une__fond")).toHaveCSS("background-image", /^url\(/);
});

test("A2 : ▶ Lecture lit l'album à la une", async ({ page }) => {
  await versAccueil(page);
  await page.locator("#accueil-une-lire").click();
  await expect(page.locator("#np-titre")).toHaveText(PISTES[0].title);
  const lus = await appels(page, "play");
  expect(lus.at(-1).paths).toEqual(PISTES.filter((p) => p.album === "Premier banc").map((p) => p.path));
});

test("lire un morceau", async ({ page }) => {
  await lireUnAlbum(page);
  const lus = await appels(page, "play");
  expect(lus.at(-1).paths).toEqual(PISTES.filter((p) => p.album === "Premier banc").map((p) => p.path));
});

/// Ce que la surveillance a ajouté en base, simulé : un album de plus dans
/// les réponses, puis l'évènement que Rust émet (`demarrer_surveillance`).
async function surveillanceAjouteUnAlbum(page) {
  await page.evaluate(() => {
    const neuf = {
      name: "Nouveau banc", artist: "Les Essais", year: 2026, tracks: 1,
      path: "/Musique/Les Essais/Nouveau banc/01.flac", ajoute_le: 1_800_000_000,
    };
    window.__FIXTURES__.albums = [...window.__FIXTURES__.albums, neuf];
    window.__FIXTURES__.roots = window.__FIXTURES__.roots.map((r) => ({ ...r, tracks: r.tracks + 1 }));
    // Une rafale : un seul rechargement attendu.
    for (let i = 0; i < 5; i++) window.__EMETTRE__("bibliotheque-modifiee", { ajoutes: 1, mis_a_jour: 0, retires: 0 });
  });
}

test("surveillance : un album copié apparaît dans Écouter, la lecture continue", async ({ page }) => {
  await lireUnAlbum(page);
  const avant = (await appels(page, "albums")).length;
  await surveillanceAjouteUnAlbum(page);
  // La relecture attend 1,5 s de calme (`CALME_BIBLIOTHEQUE_MS`) : 10 s, pour
  // une machine de CI chargée (5 s par défaut n'y suffisaient pas toujours).
  await expect(page.locator("#grille .album")).toHaveCount(4, { timeout: 10_000 });
  await expect(page.locator("#grille .album__nom", { hasText: "Nouveau banc" })).toHaveCount(1);
  // La rafale regroupée : une seule relecture des albums.
  expect((await appels(page, "albums")).length - avant).toBe(1);
  await expect(page.locator("#sommaire")).toHaveText("6");
  // Ni la vue ni la lecture n'ont bougé.
  await expect(page.locator('button.mode[data-mode="ecoute"]')).toHaveClass(/mode--actif/);
  await expect(page.locator("#np-titre")).toHaveText(PISTES[0].title);
});

test("surveillance : hors d'Écouter, la grille se met à jour au retour", async ({ page }) => {
  await page.click('button.mode[data-mode="bibliotheque"]');
  await expect(page.locator("#bibliotheque-vue")).toBeVisible();
  const avant = (await appels(page, "albums")).length;
  await surveillanceAjouteUnAlbum(page);
  await page.waitForTimeout(2000);
  expect((await appels(page, "albums")).length).toBe(avant);
  await page.click('button.mode[data-mode="ecoute"]');
  await expect(page.locator("#grille .album")).toHaveCount(4);
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
  await expect(page.locator("#dock-pistes .stem")).toHaveCount(4);
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
  await expect(page.locator("#dock-pistes .stem")).toHaveCount(4);
  // Les stems restent chargés d'un mode à l'autre : rien n'est relancé.
  expect((await appels(page, "stems_play")).length).toBe(avant);
});

/// V39 : chaque spectrogramme de la pile a la taille d'écran de son canevas
/// (largeur affichée × densité), à toute densité, après Pratiquer (où ils sont
/// cachés) et après un redimensionnement.
async function spectresNets(page) {
  await expect
    .poll(() =>
      page.locator("#dock-pistes .stem__spectre").evaluateAll((l) => {
        const d = Math.min(2, window.devicePixelRatio || 1);
        return l.length > 0 && l.every((c) => {
          const r = c.getBoundingClientRect();
          return c.width === Math.round(r.width * d) && c.height === Math.round(64 * d) && r.width > 300;
        });
      }),
    )
    .toBe(true);
}
for (const densite of [1, 2]) {
  test.describe(`à ${densite}×`, () => {
    test.use({ deviceScaleFactor: densite });
    test(`V39 : la pile des stems reste nette à ${densite}× — Pratiquer puis Créer, puis fenêtre agrandie`, async ({ page }) => {
      expect(await page.evaluate(() => window.devicePixelRatio)).toBe(densite);
      await lireUnAlbum(page);
      await page.click('button.mode[data-mode="pratiquer"]');
      await expect(page.locator("#editer-etabli")).toHaveClass(/etabli--pratiquer/);
      await expect(page.locator("#dock-pistes .stem")).toHaveCount(4);
      await page.click('button.mode[data-mode="creer"]');
      await expect(page.locator("#dock-pistes .stem__spectre").first()).toBeVisible();
      await spectresNets(page);
      // Aucun spectrogramme demandé au plancher d'un canevas caché.
      expect((await appels(page, "stem_spectre")).every((a) => a.width !== 240)).toBe(true);
      const taille = page.viewportSize();
      await page.setViewportSize({ width: taille.width + 300, height: taille.height });
      await spectresNets(page);
    });
  });
}

test("V40 : le temps restant du démixage part de l'estimation et suit la progression", async ({ page }) => {
  const restes = await page.evaluate(async () => {
    const { texteRestant } = await import("./js/etabli-stems.js");
    const t0 = 1_000_000;
    const d = (faits, total) => ({ estimation_s: 360, debut_ms: t0, segments_faits: faits, segments_total: total });
    return [
      texteRestant(d(0, 0), t0), // au lancement : l'estimation, 6 min
      texteRestant(d(0, 0), t0 + 60_000), // décodage et chauffe : 5 min
      texteRestant(d(50, 100), t0 + 120_000), // plus vite que prévu : 50 % en 2 min
      texteRestant(d(90, 100), t0 + 540_000), // plus lent : 90 % en 9 min
      texteRestant(d(0, 0), t0 + 400_000), // estimation dépassée, rien ne tombe
      texteRestant({ estimation_s: null, debut_ms: t0 }, t0), // durée du morceau inconnue
    ];
  });
  expect(restes).toEqual([
    "environ 6 min restantes",
    "environ 5 min restantes",
    "environ 3 min restantes",
    "environ 1 min restante",
    "moins d'une minute",
    "",
  ]);
});


test("V38 : une piste presque vide est masquée dans la pile, et se montre d'un geste", async ({ page }) => {
  await page.evaluate(() => {
    window.__FIXTURES__.stems_presence = [
      { nom: "drums", part_db: -4.1, presque_vide: false },
      { nom: "bass", part_db: -5.2, presque_vide: false },
      { nom: "other", part_db: -98.2, presque_vide: true },
      { nom: "vocals", part_db: -6.6, presque_vide: false },
    ];
    window.__FIXTURES__.pulsation = {
      temps: Array.from({ length: 48 }, (_, i) => 0.5 + i / 2),
      premiers_temps: Array.from({ length: 12 }, (_, i) => 0.5 + 2 * i),
      bpm: 120,
      metrique: 4,
    };
  });
  await pratiquerAlbum(page, 0);
  const noms = page.locator("#dock-pistes .stem__nom");
  await expect(noms).toHaveText(["Batterie", "Basse", "Voix"]);
  expect((await appels(page, "stems_presence")).at(-1).path).toBe(PISTES[0].path);
  const geste = page.locator("#dock-pistes .pile__vides");
  await expect(geste).toHaveText("1 piste presque vide masquée : Autres · montrer");
  await geste.click();
  await expect(noms).toHaveText(["Batterie", "Basse", "Autres", "Voix"]);
  await expect(geste).toHaveText("masquer la piste presque vide : Autres");
  await geste.click();
  await expect(noms).toHaveCount(3);
  // Masquée, elle n'est ni coupée ni en solo : la somme reste le morceau.
  expect(await page.evaluate(async () => (await import("./js/etabli.js")).edition.muets.has("other"))).toBe(false);
});

test("E1 : rouvrir un morceau en Pratiquer rend la séance comme on l'a laissée", async ({ page }) => {
  // Une grille de mesures assez longue pour poser une boucle de 4 mesures.
  await page.evaluate(() => {
    window.__FIXTURES__.pulsation = {
      temps: Array.from({ length: 48 }, (_, i) => 0.5 + i / 2),
      premiers_temps: Array.from({ length: 12 }, (_, i) => 0.5 + 2 * i),
      bpm: 120,
      metrique: 4,
    };
  });
  const premier = PISTES[0].path;
  await pratiquerAlbum(page, 0);
  await expect(page.locator("#demix-source")).toContainText(PISTES[0].title);
  // La séance : batterie, deux mesures de décompte, 90 %, une boucle.
  await page.click('#instruments [data-instrument="drums"]');
  await page.click('#decomptes [data-decompte="2"]');
  await page.click("#vitesse-unite"); // en pour cent : la valeur se lit sans BPM
  for (let i = 0; i < 2; i++) await page.click('.dock__tete [data-r="vitesse"][data-d="-1"]');
  const vitesse = await page.locator("#val-vitesse").inputValue();
  expect(vitesse).not.toBe("100");
  await page.click("#boucle");
  await expect(page.locator("#boucle")).toHaveAttribute("aria-pressed", "true");
  const gardee = async (path) =>
    (await appels(page, "pratique_garder")).filter((a) => a.path === path).at(-1)?.memoire ?? null;
  await expect.poll(() => gardee(premier).then((m) => m?.boucles?.length ?? 0), { timeout: 6000 }).toBe(1);
  const m = await gardee(premier);
  expect(m).toMatchObject({ instrument: "drums", decompte: 2 });
  expect(m.vitesse).toBeLessThan(1);
  const boucle = m.boucles[0];

  // Un autre morceau, réglé autrement.
  await pratiquerAlbum(page, 1);
  await expect(page.locator("#demix-source")).not.toContainText(PISTES[0].title);
  await page.click('#instruments [data-instrument="bass"]');
  await page.click('#decomptes [data-decompte="0"]');
  // Pas de boucle : celle du premier ne le suit pas.
  await expect(page.locator("#boucle")).toHaveAttribute("aria-pressed", "false");

  // Retour au premier : la séance revient, boucle posée au moteur comprise.
  const avant = (await appels(page, "stems_boucle")).length;
  await pratiquerAlbum(page, 0);
  await expect(page.locator("#demix-source")).toContainText(PISTES[0].title);
  await expect(page.locator('#instruments [data-instrument="drums"]')).toHaveClass(/segment--actif/);
  await expect(page.locator('#decomptes [data-decompte="2"]')).toHaveClass(/segment--actif/);
  await expect(page.locator("#val-vitesse")).toHaveValue(vitesse);
  await expect(page.locator("#boucle")).toHaveAttribute("aria-pressed", "true");
  await expect.poll(() => appels(page, "stems_boucle").then((a) => a.slice(avant).at(-1))).toEqual({
    debut: boucle.debut_s,
    fin: boucle.fin_s,
  });
});

test("V42 : la tonalité d'ensemble se garde par morceau, et revient à ±0 sur un morceau sans réglage", async ({ page }) => {
  await page.evaluate(() => {
    window.__FIXTURES__.pulsation = {
      temps: Array.from({ length: 48 }, (_, i) => 0.5 + i / 2),
      premiers_temps: Array.from({ length: 12 }, (_, i) => 0.5 + 2 * i),
      bpm: 120,
      metrique: 4,
    };
  });
  const premier = PISTES[0].path;
  const champ = page.locator("#val-tonalite");
  const hauteurs = async () => (await appels(page, "start_etirer")).at(-1)?.demiTons ?? null;
  await pratiquerAlbum(page, 0);
  await expect(page.locator("#demix-source")).toContainText(PISTES[0].title);
  await expect(champ).toHaveValue("±0");
  // Un demi-ton plus bas (un morceau enregistré sous le diapason), et plus lent.
  await page.click('.dock__tete [data-r="tonalite"][data-d="-1"]');
  await expect(champ).toHaveValue("−½ ton");
  await page.click("#vitesse-unite"); // en pour cent
  await page.click('.dock__tete [data-r="vitesse"][data-d="-1"]');
  const gardee = async (path) =>
    (await appels(page, "pratique_garder")).filter((a) => a.path === path).at(-1)?.memoire ?? null;
  await expect.poll(() => gardee(premier).then((m) => m?.tonalite), { timeout: 6000 }).toBe(-1);
  expect((await gardee(premier)).vitesse).toBeLessThan(1);

  // Un autre morceau, sans mémoire : ni la hauteur ni la vitesse ne le suivent ;
  // ses stems se chargent à la hauteur d'origine.
  await pratiquerAlbum(page, 1);
  await expect(page.locator("#demix-source")).not.toContainText(PISTES[0].title);
  await expect(champ).toHaveValue("±0");
  await expect(page.locator("#val-vitesse")).toHaveValue("100");
  await expect.poll(hauteurs).toEqual([0, 0, 0, 0]);

  // Retour au premier : −½ ton, et ses stems chargés transposés d'emblée.
  await pratiquerAlbum(page, 0);
  await expect(page.locator("#demix-source")).toContainText(PISTES[0].title);
  await expect(champ).toHaveValue("−½ ton");
  await expect(page.locator("#val-vitesse")).not.toHaveValue("100");
  await expect.poll(hauteurs).toEqual([-1, -1, -1, -1]);
});

test("V17 : une boucle se nomme, se rappelle par son nom et revient avec sa vitesse atteinte", async ({ page }) => {
  await page.evaluate(() => {
    window.__FIXTURES__.pulsation = {
      temps: Array.from({ length: 96 }, (_, i) => 0.5 + i / 2),
      premiers_temps: Array.from({ length: 24 }, (_, i) => 0.5 + 2 * i),
      bpm: 120,
      metrique: 4,
    };
  });
  const premier = PISTES[0].path;
  await pratiquerAlbum(page, 0);
  await expect(page.locator("#demix-source")).toContainText(PISTES[0].title);
  await page.click("#vitesse-unite"); // en pour cent
  // Ni liste ni vitesse atteinte tant qu'aucune boucle n'est nommée ni jouée.
  await expect(page.locator("#boucles-liste")).toBeHidden();
  await expect(page.locator("#vitesse-atteinte")).toBeHidden();

  // Une boucle à 90 %, jouée une fois jusqu'au bout, nommée « couplet ».
  for (let i = 0; i < 2; i++) await page.click('.dock__tete [data-r="vitesse"][data-d="-1"]');
  await page.click("#boucle");
  await expect(page.locator("#boucle-nom")).toBeVisible();
  await page.evaluate(() => (window.__ETAT_FAUX__.passages = 1));
  await expect(page.locator("#vitesse-atteinte")).toHaveText("atteinte : 90 %");
  await page.fill("#boucle-nom", "couplet");
  await page.press("#boucle-nom", "Enter");
  await expect(page.locator("#boucles-liste")).toBeVisible();
  const couplet = (await appels(page, "stems_boucle")).at(-1);

  // Une seconde boucle, plus loin, nommée « pont » ; puis on ôte la boucle.
  await page.click("#boucle");
  await page.click('.dock__tete [data-r="vitesse"][data-d="1"]');
  await page.evaluate(() => (window.__ETAT_FAUX__.passages = 0));
  await page.evaluate(async () => {
    const { deplacerLecture } = await import("./js/etabli-stems.js");
    await deplacerLecture(0.15);
  });
  await page.click("#boucle");
  await page.fill("#boucle-nom", "pont");
  await page.press("#boucle-nom", "Enter");
  const pont = (await appels(page, "stems_boucle")).at(-1);
  expect(pont.debut).not.toBe(couplet.debut);
  await page.click("#boucle");
  await expect(page.locator("#boucle")).toHaveAttribute("aria-pressed", "false");
  // Sans boucle : la vitesse atteinte du morceau.
  await expect(page.locator("#vitesse-atteinte")).toHaveText("atteinte : 90 %");

  // Rappeler « couplet » au clavier : la liste, flèche, Entrée.
  await page.click("#boucles-liste");
  await expect(page.locator("#boucles-menu")).toBeVisible();
  await expect(page.locator("#boucles-menu .boucles__rappel")).toHaveCount(2);
  await page.keyboard.press("Home");
  await expect(page.locator("#boucles-menu .boucles__rappel").first()).toBeFocused();
  await expect(page.locator("#boucles-menu .boucles__rappel").first()).toContainText("couplet");
  await page.keyboard.press("Enter");
  await expect(page.locator("#boucles-menu")).toBeHidden();
  await expect(page.locator("#boucle")).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#boucle-nom")).toHaveValue("couplet");
  expect((await appels(page, "stems_boucle")).at(-1)).toEqual(couplet);
  await expect(page.locator("#vitesse-atteinte")).toHaveText("atteinte : 90 %");

  // Gardées par la mémoire : la boucle posée, et les deux nommées.
  const gardee = async () =>
    (await appels(page, "pratique_garder")).filter((a) => a.path === premier).at(-1)?.memoire ?? null;
  await expect
    .poll(() => gardee().then((m) => (m?.boucles ?? []).map((b) => b.nom)), { timeout: 6000 })
    .toEqual([null, "couplet", "pont"]);

  // Un autre morceau : pas de boucles nommées ; retour : elles reviennent.
  await pratiquerAlbum(page, 1);
  await expect(page.locator("#demix-source")).not.toContainText(PISTES[0].title);
  await expect(page.locator("#boucles-liste")).toBeHidden();
  await pratiquerAlbum(page, 0);
  await expect(page.locator("#demix-source")).toContainText(PISTES[0].title);
  await expect(page.locator("#boucles-liste")).toBeVisible();
  await expect(page.locator("#boucle-nom")).toHaveValue("couplet");
  await expect(page.locator("#vitesse-atteinte")).toHaveText("atteinte : 90 %");

  // Oublier « pont » au clavier (Suppr) : il quitte la liste et la mémoire.
  await page.click("#boucles-liste");
  await page.keyboard.press("End");
  await expect(page.locator("#boucles-menu .boucles__rappel").last()).toContainText("pont");
  await page.keyboard.press("Delete");
  await expect(page.locator("#boucles-menu .boucles__rappel")).toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(page.locator("#boucles-menu")).toBeHidden();
  await expect
    .poll(() => gardee().then((m) => (m?.boucles ?? []).map((b) => b.nom)), { timeout: 6000 })
    .toEqual([null, "couplet"]);

  // Créer ne nomme ni ne rappelle : c'est la mémoire de Pratiquer.
  await page.click('button.mode[data-mode="creer"]');
  await expect(page.locator("#boucles-liste")).toBeHidden();
  await expect(page.locator("#boucle-nom")).toBeHidden();
  await expect(page.locator("#vitesse-atteinte")).toBeHidden();
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
  await expect(page.locator("#dock-pistes .stem__nom")).toHaveText(["Batterie", "Basse", "Autres", "Voix"]);
  expect(await page.locator("#dock-pistes .piste").evaluateAll((l) => l.map((e) => e.dataset.stem)))
    .toEqual(["drums", "bass", "other", "vocals"]);
  await page.locator("#dock-pistes .stem__nom", { hasText: "Basse" }).click();
  await expect(page.locator("#insp-stem-nom")).toHaveText("Basse");
});

test("F1 : chercher le greffon dit pourquoi chaque candidat est classé là", async ({ page }) => {
  await lireUnAlbum(page);
  await page.click('button.mode[data-mode="creer"]');
  await page.locator("#dock-pistes .stem__nom", { hasText: "Batterie" }).click();
  await page.locator(".greffe__champ").fill("une batterie plus funk");
  await page.locator(".stem__b", { hasText: "remplacer par…" }).click();
  await expect(page.locator(".candidat")).toHaveCount(2);
  const cherche = (await appels(page, "chercher_greffon")).at(-1);
  expect(cherche).toMatchObject({ stem: "drums", guide: "une batterie plus funk" });
  // Syncope, tempo, et la tonalité ignorée pour une batterie : dits.
  await expect(page.locator(".candidat__pourquoi").first()).toContainText("plus syncopé");
  await expect(page.locator(".candidat__pourquoi").first()).toContainText("122 BPM");
  await expect(page.locator(".stem__note").filter({ hasText: "Tonalité ignorée" })).toHaveCount(1);
  // Le candidat sans groove part en préparation de fond, le prêt non.
  await expect.poll(() => appels(page, "greffons_preparer").then((a) => a.length)).toBeGreaterThan(0);
  const prepare = (await appels(page, "greffons_preparer")).at(-1);
  expect(prepare.ids).toEqual([5]);
  await expect(page.locator(".candidat").nth(0)).toContainText("prêt");
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

test("V6 : en Pratiquer, chaque ligne des Partitions tient sur une ligne à 1 400 × 900", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 868 });
  await lireUnAlbum(page);
  await page.click('button.mode[data-mode="pratiquer"]');
  await expect(page.locator("#bloc-partitions")).toBeVisible();
  const lignes = page.locator("#bloc-partitions .partition-ligne:visible");
  await expect(lignes).toHaveCount(2);
  await expect(lignes.first().locator(".partition-ligne__action")).toHaveText("Calculer");
  // Une ligne : nom, état et bouton sur la même rangée, aucun ne passe à la ligne.
  const hauteurs = await lignes.evaluateAll((els) => els.map((l) => {
    const r = l.getBoundingClientRect().height;
    const enfants = [...l.children].filter((e) => !e.hidden).map((e) => e.getBoundingClientRect());
    const texte = [...l.querySelectorAll(".partition-ligne__nom, .partition-ligne__etat")]
      .map((e) => e.getClientRects().length);
    return { r, ecart: Math.max(...enfants.map((e) => e.top)) - Math.min(...enfants.map((e) => e.top)), texte };
  }));
  for (const h of hauteurs) {
    expect(h.r).toBeLessThan(32);
    expect(h.texte.every((n) => n <= 1)).toBe(true);
  }
});

test("V6 : légende des familles, le nom entier au survol et au clavier", async ({ page }) => {
  await page.click('button.mode[data-mode="explorer"]');
  const famille = page.locator('#familles .famille[data-famille="1"]');
  await expect(famille).toBeVisible();
  await expect(famille).toHaveAttribute("title", "Orchestre · 2 morceaux");
  // Au clavier, le `title` ne s'affiche pas : le nom entier s'ouvre au focus.
  await page.keyboard.press("Shift");
  await famille.focus();
  const contenu = await famille.evaluate((el) => getComputedStyle(el, "::after").content);
  expect(contenu).toBe('"Orchestre · 2 morceaux"');
  await famille.blur();
  expect(await famille.evaluate((el) => getComputedStyle(el, "::after").content)).toBe("none");
});

test("V6 : la palette trouve une famille, et le geste est celui de la légende", async ({ page }) => {
  // En Écouter : la famille est cochée dans le filtre de l'écran.
  await page.keyboard.press("Meta+k");
  await page.keyboard.type("orchest");
  const groupe = page.locator("#palette-liste [role=group]", { has: page.locator(".palette__groupe", { hasText: "Familles" }) });
  await expect(groupe.locator(".palette__nom")).toHaveText(["Orchestre"]);
  await expect(groupe.locator(".palette__sec")).toHaveText(["famille · 2 morceaux"]);
  await groupe.locator(".palette__option").click();
  await expect(page.locator("#palette")).toBeHidden();
  await expect(page.locator('#familles-ecoute .famille[data-famille="1"]')).toHaveClass(/famille--isolee/);
  // Sans légende à l'écran (Bibliothèque) : Explorer, la famille isolée.
  await page.click('button.mode[data-mode="bibliotheque"]');
  await page.keyboard.press("Meta+k");
  await page.keyboard.type("famille essais");
  await expect(page.locator(".palette__option", { hasText: "famille · 3 morceaux" })).toBeVisible();
  await page.locator(".palette__option", { hasText: "famille · 3 morceaux" }).click();
  await expect(page.locator('button.mode[data-mode="explorer"]')).toHaveClass(/mode--actif/);
  await expect(page.locator('#familles .famille[data-famille="0"]')).toHaveClass(/famille--isolee/);
  // Sans demande, la palette ne liste pas les familles.
  await page.keyboard.press("Meta+k");
  await expect(page.locator(".palette__groupe")).toHaveText(["Aller à", "Commandes"]);
  await page.keyboard.press("Escape");
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

test("filtre du rail : masque dans la grille ce qui n'y répond pas, Entrée ouvre la palette", async ({ page }) => {
  await page.fill("#q", "essais");
  await expect(page.locator("#grille .album")).toHaveCount(2);
  await expect(page.locator("#grille .album--estompe")).toHaveCount(0);
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
  await versAlbums(page);
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
  await versAlbums(page);
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

test("vue « En cours » : le bouton du transport et la touche F l'ouvrent et la ferment", async ({ page }) => {
  await lireUnAlbum(page);
  const bouton = page.locator("#bascule-en-cours");
  await bouton.click();
  await expect(page.locator("#en-cours")).toBeVisible();
  await expect(bouton).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#en-cours-temps")).toHaveText(/^\d\d:\d\d \/ \d\d:\d\d$/);
  await page.keyboard.press("f");
  await expect(page.locator("#en-cours")).toBeHidden();
  await expect(bouton).toHaveAttribute("aria-pressed", "false");
  await page.locator("body").press("f");
  await expect(page.locator("#en-cours")).toBeVisible();
  // Dans un champ, F s'écrit : la vue ne bouge pas.
  await page.keyboard.press("Escape");
  await page.fill("#q", "");
  await page.locator("#q").press("f");
  await expect(page.locator("#en-cours")).toBeHidden();
  await expect(page.locator("#q")).toHaveValue("f");
});

test("inspecteur : la pochette du morceau qui joue devient un vinyle, et redevient carrée", async ({ page }) => {
  await lireUnAlbum(page);
  const pochette = page.locator("#pochette");
  // Le morceau qui joue, ouvert dans l'inspecteur : un disque.
  await page.locator("#grille .album").first().click();
  await page.locator("#liste .ligne").first().click();
  await expect(page.locator("#insp-titre")).toHaveText(PISTES[0].title);
  await expect(pochette).toHaveClass(/pochette--disque/);
  // Un autre morceau inspecté : la pochette redevient un carré.
  await page.locator("#liste .ligne").nth(1).click();
  await expect(page.locator("#insp-titre")).toHaveText(PISTES[1].title);
  await expect(pochette).not.toHaveClass(/pochette--disque/, { timeout: 3000 });
});

test("vinyle : le bouton scratch arme le disque, la molette le pilote, Échap et un second clic en sortent", async ({ page }) => {
  await lireUnAlbum(page);
  await page.locator("#grille .album").first().click();
  await page.locator("#liste .ligne").first().click();
  const pochette = page.locator("#pochette");
  await expect(pochette).toHaveClass(/pochette--disque/);
  const bouton = page.locator("#insp-scratch");
  // Discret : invisible tant que le disque n'est pas survolé.
  await expect(bouton).toHaveCSS("opacity", "0");
  await pochette.hover();
  await expect(bouton).toHaveCSS("opacity", "1");
  await bouton.click();
  await expect(bouton).toHaveAttribute("aria-pressed", "true");
  await expect(pochette).toHaveClass(/scratch--arme/);
  expect(await appels(page, "scratch_entrer")).toHaveLength(1);
  // La molette sur le disque : une main qui fait tourner, en arrière ici.
  const b = await pochette.boundingBox();
  await page.mouse.move(b.x + b.width / 2, b.y + b.height * 0.15);
  await page.mouse.wheel(60, 0);
  await expect.poll(async () => (await appels(page, "scratch_main")).some((a) => a.vitesse !== null && a.vitesse < 0)).toBe(true);
  // Échap sort du mode, sans rien fermer d'autre.
  await page.keyboard.press("Escape");
  await expect(bouton).toHaveAttribute("aria-pressed", "false");
  await expect(pochette).not.toHaveClass(/scratch--arme/);
  expect(await appels(page, "scratch_sortir")).toHaveLength(1);
  // Un second clic sur le bouton sort aussi.
  await pochette.hover();
  await bouton.click();
  await expect(bouton).toHaveAttribute("aria-pressed", "true");
  await bouton.click();
  await expect(bouton).toHaveAttribute("aria-pressed", "false");
  expect(await appels(page, "scratch_sortir")).toHaveLength(2);
});

test("vinyle : les phases du trackpad tiennent le disque, le lâchent et écartent l'inertie", async ({ page }) => {
  await lireUnAlbum(page);
  await page.locator("#grille .album").first().click();
  await page.locator("#liste .ligne").first().click();
  const pochette = page.locator("#pochette");
  await expect(pochette).toHaveClass(/pochette--disque/);
  await pochette.hover();
  await page.locator("#insp-scratch").click();
  await expect(pochette).toHaveClass(/scratch--arme/);
  const b = await pochette.boundingBox();
  await page.mouse.move(b.x + b.width / 2, b.y + b.height * 0.15);
  // Comme le moteur sous macOS (`trackpad.rs`), à l'instant présent.
  const emettre = (phase) => page.evaluate((p) => window.__EMETTRE__("scratch-phase",
    { ...p, t_ms: performance.timeOrigin + performance.now() }), phase);
  const depuis = async () => (await appels(page, "scratch_main")).length;
  const apres = async (n) => (await appels(page, "scratch_main")).slice(n).map((a) => a.vitesse);
  // Deux doigts posés sans bouger : le disque est tenu, arrêté, et le reste
  // bien après le délai où l'heuristique aurait levé la main.
  await emettre(evenements.scratch_phase.poses);
  await expect(pochette).toHaveClass(/scratch--main/);
  let n = await depuis();
  await page.waitForTimeout(300);
  let v = await apres(n);
  expect(v.length).toBeGreaterThan(3);
  expect(v.every((x) => x !== null && Math.abs(x) < 0.05)).toBe(true);
  // Les doigts bougent : la main fait tourner le disque, en arrière.
  await emettre(evenements.scratch_phase.debut);
  await page.mouse.wheel(60, 0);
  await expect.poll(async () => (await appels(page, "scratch_main")).some((a) => a.vitesse !== null && a.vitesse < 0)).toBe(true);
  // Levés : la main lâche le disque, le moteur reprend.
  await emettre(evenements.scratch_phase.fin);
  await expect(pochette).not.toHaveClass(/scratch--main/);
  n = await depuis();
  await expect.poll(async () => (await apres(n)).includes(null)).toBe(true);
  // L'inertie de macOS : ses deltas ne reprennent pas le disque.
  await emettre(evenements.scratch_phase.inertie);
  n = await depuis();
  for (let i = 0; i < 5; i++) await page.mouse.wheel(40 - i * 6, 0);
  await page.waitForTimeout(150);
  v = await apres(n);
  expect(v.length).toBeGreaterThan(2);
  expect(v.every((x) => x === null)).toBe(true);
  await emettre(evenements.scratch_phase.inertieFin);
  // Au désarmement, la latence du lâcher part au journal.
  await page.keyboard.press("Escape");
  await expect.poll(async () => (await appels(page, "journal_ui")).some((a) => /latence du lâcher \(phases\)/.test(a.message))).toBe(true);
});

test("vue « En cours » : scratch du disque, Échap sort du mode puis ferme la vue", async ({ page }) => {
  await lireUnAlbum(page);
  await page.click("#bascule-en-cours");
  await expect(page.locator("#en-cours")).toBeVisible();
  const bouton = page.locator("#en-cours-scratch");
  await page.locator(".en-cours__platine").hover();
  await bouton.click();
  await expect(bouton).toHaveAttribute("aria-pressed", "true");
  // Glisser au pointeur sur le disque : vers l'avant (sens horaire, par le haut).
  const b = await page.locator("#en-cours-disque").boundingBox();
  const cx = b.x + b.width / 2, cy = b.y + b.height / 2;
  await page.mouse.move(cx - b.width * 0.3, cy - b.height * 0.3);
  await page.mouse.down();
  for (let i = 1; i <= 6; i++) await page.mouse.move(cx - b.width * 0.3 + i * 20, cy - b.height * 0.35, { steps: 2 });
  await expect.poll(async () => (await appels(page, "scratch_main")).some((a) => a.vitesse > 0)).toBe(true);
  await page.mouse.up();
  // Armé, une animation pilotée par le moteur remplace l'animation CSS.
  await expect.poll(() => page.locator("#en-cours-disque").evaluate((el) => el.style.animationName)).toBe("none");
  await page.keyboard.press("Escape");
  await expect(bouton).toHaveAttribute("aria-pressed", "false");
  await expect(page.locator("#en-cours")).toBeVisible();
  // Rendu : l'animation CSS reprend, la pilotée a disparu.
  expect(await page.locator("#en-cours-disque").evaluate((el) =>
    [el.style.animationName, el.getAnimations().filter((x) => !(x instanceof CSSAnimation)).length])).toEqual(["", 0]);
  await page.keyboard.press("Escape");
  await expect(page.locator("#en-cours")).toBeHidden();
});

test("inspecteur : pas de vinyle si le système demande moins de mouvement", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await lireUnAlbum(page);
  await page.locator("#grille .album").first().click();
  await page.locator("#liste .ligne").first().click();
  await expect(page.locator("#insp-titre")).toHaveText(PISTES[0].title);
  await page.waitForTimeout(400);
  await expect(page.locator("#pochette")).not.toHaveClass(/pochette--disque/);
});

test("vue « En cours » : sans mouvement si le système le demande", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await lireUnAlbum(page);
  await page.keyboard.press("Meta+k");
  await page.keyboard.type("en cours");
  await page.keyboard.press("Enter");
  await expect(page.locator("#en-cours")).toBeVisible();
  // La lueur du disque ne suit pas le morceau : `--intensite` n'est jamais posée.
  await page.waitForTimeout(300);
  expect(await page.locator(".en-cours__platine").evaluate((el) => el.style.getPropertyValue("--intensite"))).toBe("");
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

test("V45 : popularité et pochettes en ligne se coupent dans les options de la chaîne, gardées au rechargement", async ({ page }) => {
  // Au lancement, le moteur reçoit le réglage (permis par défaut).
  await expect.poll(() => appels(page, "set_pochettes_en_ligne").then((a) => a.at(-1)?.actif)).toBe(true);
  await page.click('button.mode[data-mode="bibliotheque"]');
  await page.click("summary:has-text('Options de la chaîne')");
  const resume = page.locator("#resume-chaine");
  await expect(resume).toContainText("popularité");
  await page.uncheck("#pop-active");
  await expect(resume).not.toContainText("popularité");
  await expect(page.locator("#pop-rafraichir")).toBeDisabled();
  await page.uncheck("#pochettes-en-ligne");
  await expect.poll(() => appels(page, "set_pochettes_en_ligne").then((a) => a.at(-1)?.actif)).toBe(false);
  // Gardés : au rechargement, coupés d'emblée, et le moteur le sait.
  await page.reload();
  await expect.poll(() => appels(page, "set_pochettes_en_ligne").then((a) => a.at(-1)?.actif)).toBe(false);
  await page.click('button.mode[data-mode="bibliotheque"]');
  await page.click("summary:has-text('Options de la chaîne')");
  await expect(page.locator("#pop-active")).not.toBeChecked();
  await expect(page.locator("#pochettes-en-ligne")).not.toBeChecked();
  await expect(page.locator("#resume-chaine")).not.toContainText("popularité");
  await page.check("#pop-active");
  await expect(page.locator("#pop-rafraichir")).toBeEnabled();
  await expect(page.locator("#resume-chaine")).toContainText("popularité");
});

test("Recalculer la carte : nuage refait, le plan suit en tâche de fond (V16)", async ({ page }) => {
  await page.evaluate(() => {
    const f = window.__FIXTURES__;
    // Le moteur lance la régénération en rendant « refaite » ; elle dure
    // quatre sondes, puis rend son bilan.
    f.recompute_map = (_, etat) => {
      etat.plan = 0;
      return { empreintes: 5, familles: 2, projection: "refaite" };
    };
    f.tuiles_state = (_, etat) => {
      if (etat.plan === undefined) return { en_cours: false, faits: 0, total: 3, resultat: null };
      etat.plan += 1;
      return etat.plan < 5
        ? { en_cours: true, faits: Math.min(etat.plan - 1, 2), total: 3, resultat: null }
        : { en_cours: false, faits: 3, total: 3, resultat: "12 tuiles (1,0 Mo) en 0,1 s" };
    };
  });
  await page.click('button.mode[data-mode="bibliotheque"]');
  await page.click("summary:has-text('Paramètres de la carte')");
  await page.click("#recalculer-carte");
  await expect(page.locator("#carte-parametres-etat")).toContainText("nuage redessiné");
  await expect(page.locator("#tuiles-etat")).toContainText("plan de Paris : régénération");
  await expect(page.locator("#regenerer-tuiles")).toBeDisabled();
  await expect(page.locator("#tuiles-etat")).toHaveText("12 tuiles (1,0 Mo) en 0,1 s");
  await expect(page.locator("#regenerer-tuiles")).toBeEnabled();
  // Le moteur régénère seul : l'interface n'appelle pas `engendrer_tuiles`.
  expect(await appels(page, "engendrer_tuiles")).toEqual([]);
});

test("Recalculer la carte : nuage gardé, le plan ne bouge pas (V16)", async ({ page }) => {
  await page.click('button.mode[data-mode="bibliotheque"]');
  await page.click("summary:has-text('Paramètres de la carte')");
  // Le retour dans Bibliothèque sonde les passes en cours, celle-ci comprise.
  const avant = (await appels(page, "tuiles_state")).length;
  await page.click("#recalculer-carte");
  await expect(page.locator("#carte-parametres-etat")).toContainText("nuage inchangé");
  expect((await appels(page, "tuiles_state")).length).toBe(avant);
  await expect(page.locator("#tuiles-etat")).toHaveText("");
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

test("V41 : Valise dit quand la recherche de l'iPhone est finie (« recherche », puis « lu »)", async ({ page }) => {
  await page.evaluate(() => {
    window.__IPHONE__ = null;
    // Une lecture qui ne finit que quand le test la libère.
    window.__FIXTURES__.valise_iphone = () => new Promise((r) => (window.__IPHONE__ = r));
  });
  await page.click('button.mode[data-mode="valise"]');
  const vue = page.locator("#valise-vue");
  await expect(vue).toHaveAttribute("data-iphone", "recherche");
  await expect(page.locator("#iphone-nom")).toHaveText("Recherche de l'iPhone…");
  await page.evaluate(() => window.__IPHONE__({
    xcode: false, appareils: [], udid: null, app_installee: false, echeance: null, valises: [], renouvelable: false, erreur: null,
  }));
  await expect(vue).toHaveAttribute("data-iphone", "lu");
  await expect(page.locator("#iphone-nom")).toHaveText("Xcode absent");
  // Actualiser repasse par « recherche ».
  await page.evaluate(() => {
    window.__FIXTURES__.valise_iphone = () => new Promise((r) => (window.__IPHONE__ = r));
  });
  await page.click("#iphone-actualiser");
  await expect(vue).toHaveAttribute("data-iphone", "recherche");
  await page.evaluate(() => window.__IPHONE__({
    xcode: false, appareils: [], udid: null, app_installee: false, echeance: null, valises: [], renouvelable: false, erreur: null,
  }));
  await expect(vue).toHaveAttribute("data-iphone", "lu");
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


/// Clique le temps `indice` de la mesure `mesure` dans la partition rendue.
async function cliquerTemps(page, mesure, indice) {
  const point = await page.evaluate(
    async ([m, i]) => {
      const { partition } = await import("./js/pratiquer.js");
      const api = partition.api;
      const beat = api.score.tracks[0].staves[0].bars[m].voices[0].beats[i];
      const b = api.boundsLookup.findBeat(beat);
      const s = document.querySelector("#partition-rendu .at-surface").getBoundingClientRect();
      const v = b.visualBounds;
      return { x: s.left + v.x + v.w / 2, y: s.top + v.y + v.h / 2 };
    },
    [mesure, indice],
  );
  await page.mouse.click(point.x, point.y);
}

test("E2 : corriger une note de la partition, en ajouter une, exporter la partie", async ({ page }) => {
  await basseTranscrite(page);
  await pratiquerAlbum(page, 0);
  await page.click('#instruments [data-instrument="bass"]');
  const barre = page.locator("#partition-correction");
  await expect(barre).toBeVisible();
  await expect(page.locator("#correction-note")).toContainText("Cliquer une note");
  await expect(page.locator("#partition-rendu .at-surface")).toBeVisible();
  // Rien dans le rail : la barre est au centre, sous la rangée des stems.
  expect(await barre.evaluate((el) => !!el.closest("#editer-etabli") && !el.closest(".rail"))).toBe(true);

  // La deuxième note : do2, posée par le faux moteur sur la frette la plus
  // basse.
  await cliquerTemps(page, 0, 1);
  await expect(page.locator("#correction-note")).toHaveText("do2 · corde de la, frette 3");
  await expect(barre.locator('[data-corriger="octave-haut"]')).toBeVisible();
  await expect(barre.locator('[data-corriger="ajouter"]')).toBeHidden();
  await expect(barre.locator('[data-corriger="retablir"]')).toBeHidden();
  await barre.locator('[data-corriger="octave-haut"]').click();
  await expect(page.locator("#correction-note")).toHaveText("do3 · corde de sol, frette 5 · corrigée");
  const octave = (await appels(page, "corriger_transcription")).at(-1);
  expect(octave.correction).toEqual({ genre: "octave", debut_s: 1, hauteur: 36, octaves: 1 });
  expect(octave.qualite).toBe(true);
  // La note corrigée est à l'encre d'accent dans la partition redessinée.
  const marquee = () =>
    page.evaluate(async () => {
      const { partition } = await import("./js/pratiquer.js");
      return partition.api.score.tracks[0].staves[0].bars[0].voices[0].beats[1].notes[0].style?.colors.size ?? 0;
    });
  await expect.poll(marquee).toBe(2);
  if (process.env.CAPTURES) await page.screenshot({ path: "test-results/e2-correction.png" });
  // Rétablir rend la note transcrite.
  await barre.locator('[data-corriger="retablir"]').click();
  await expect(page.locator("#correction-note")).toHaveText("do2 · corde de la, frette 3");
  expect((await appels(page, "corriger_transcription")).at(-1).correction.genre).toBe("retablir");

  // Supprimer la quatrième note : un silence à sa place.
  await cliquerTemps(page, 0, 3);
  await expect(page.locator("#correction-note")).toContainText("mi2");
  await barre.locator('[data-corriger="supprimer"]').click();
  await expect(page.locator("#correction-note")).toHaveText("Silence");

  // Sur le silence du troisième temps de la deuxième mesure, une note : la
  // hauteur de celle d'avant (sol1), au temps de la grille, sur sa durée.
  await cliquerTemps(page, 1, 2);
  await expect(page.locator("#correction-note")).toHaveText("Silence");
  await barre.locator('[data-corriger="ajouter"]').click();
  await expect(page.locator("#correction-note")).toHaveText("sol1 · corde de mi, frette 3 · ajoutée");
  const ajout = (await appels(page, "corriger_transcription")).at(-1).correction;
  expect(ajout).toMatchObject({ genre: "ajouter", hauteur: 31 });
  expect(ajout.debut_s).toBeCloseTo(3.5, 6);
  expect(ajout.duree_s).toBeCloseTo(0.5, 6);
  // Une note ajoutée change de hauteur au demi-ton : reposée, pas déplacée.
  await barre.locator('[data-corriger="demi-haut"]').click();
  await expect(page.locator("#correction-note")).toHaveText("sol♯1 · corde de mi, frette 4 · ajoutée");

  // Exporter la partie corrigée : un vrai MIDI, un vrai Guitar Pro (zip),
  // depuis la barre de l'établi (24 nov.).
  const exporterPartie = page.locator("#partie-export");
  await expect(exporterPartie).toBeVisible();
  expect(await exporterPartie.evaluate((el) => !!el.closest("#dock"))).toBe(true);
  await page.evaluate(() => (window.__CHEMIN_EXPORT__ = "/tmp/partie.mid"));
  await exporterPartie.locator('[data-exporter="midi"]').click();
  await expect.poll(() => appels(page, "exporter_partie").then((a) => a.length)).toBe(1);
  const midi = (await appels(page, "exporter_partie"))[0];
  expect(midi.chemin).toBe("/tmp/partie.mid");
  expect(String.fromCharCode(...midi.octets.slice(0, 4))).toBe("MThd");
  await expect(page.locator("#correction-note")).toContainText("Partie exportée (MIDI)");
  await page.evaluate(() => (window.__CHEMIN_EXPORT__ = "/tmp/partie.gp"));
  await exporterPartie.locator('[data-exporter="gp"]').click();
  await expect.poll(() => appels(page, "exporter_partie").then((a) => a.length)).toBe(2);
  expect(String.fromCharCode(...(await appels(page, "exporter_partie"))[1].octets.slice(0, 2))).toBe("PK");

  // Créer n'a ni partition ni correction.
  await page.click('button.mode[data-mode="creer"]');
  await expect(barre).toBeHidden();
});

test("E4 : jouer la batterie transcrite sur sa batterie MIDI, chaque note jugée sur la partition", async ({ page }) => {
  await batterieTranscrite(page);
  await pratiquerAlbum(page, 0);
  await page.click('#instruments [data-instrument="drums"]');
  const barre = page.locator("#partition-coach");
  await expect(barre).toBeVisible();
  await expect(page.locator("#partition-rendu .at-surface")).toBeVisible();
  // Au centre, pas dans le rail ; ni la barre de correction de la basse.
  expect(await barre.evaluate((el) => !!el.closest("#editer-etabli") && !el.closest(".rail"))).toBe(true);
  await expect(page.locator("#partition-correction")).toBeHidden();
  // Rien n'écoute avant le geste.
  expect(await appels(page, "coach_ecouter")).toEqual([]);
  await expect(page.locator("#coach-calibrer")).toBeHidden();
  await expect(page.locator("#coach-accordeur")).toBeHidden();
  // V43 : rien à régler avant l'écoute, une seule batterie : pas de repli.
  await expect(page.locator("#coach-reglages")).toBeHidden();

  await page.click("#coach-basculer");
  await expect(page.locator("#coach-basculer")).toHaveAttribute("aria-pressed", "true");
  await expect.poll(() => appels(page, "coach_ecouter").then((a) => a.at(-1)?.peripherique)).toBe("TD-17");
  // Un seul périphérique : pas de liste à choisir. « calibrer » est dans le
  // repli des réglages de l'entrée (V43), fermé : la barre tient sur une ligne.
  const reglages = page.locator("#coach-reglages");
  await expect(reglages).toBeVisible();
  await expect(page.locator("#coach-calibrer")).toBeHidden();
  await reglages.locator("summary").click();
  await expect(page.locator("#coach-peripherique")).toBeHidden();
  await expect(page.locator("#coach-calibrer")).toBeVisible();
  // Échap le referme, le focus revient à son bouton.
  await page.keyboard.press("Escape");
  await expect(page.locator("#coach-calibrer")).toBeHidden();
  await expect(reglages.locator("summary")).toBeFocused();
  // Les pads (E4) : derrière « pads », la note frappée et la pièce qu'elle
  // compte ; un pad non affecté s'affecte d'un choix, gardé aussitôt.
  await reglages.locator("summary").click();
  const profil = page.locator("#coach-profil");
  await expect(profil).toBeHidden();
  await page.click("#coach-pads");
  await expect(profil).toBeVisible();
  await expect(page.locator("#coach-profil-dernier")).toContainText("Frapper un pad");
  await expect(page.locator("#coach-profil-choix")).toBeHidden();
  // Les cinq pièces, une rangée chacune.
  await expect(page.locator("#coach-profil-table .coach__profil-classe")).toHaveCount(5);
  await page.evaluate(() => {
    window.__FIXTURES__.coach_notes_recues = [{ t_s: 50, note: 81, velocite: 70, classe: null, sous_le_seuil: false }];
  });
  await expect(page.locator("#coach-profil-dernier")).toHaveText("Note 81 · vélocité 70 → ignorée");
  await page.selectOption("#coach-profil-classe", "toms");
  await expect.poll(() => appels(page, "coach_garder_profil").then((a) => a.at(-1)?.profil?.notes?.[81])).toBe("toms");
  expect((await appels(page, "coach_garder_profil")).at(-1).profil.nom).toBe("TD-17");
  await expect(page.locator("#coach-profil-dernier")).toHaveText("Note 81 · vélocité 70 → Toms");
  await expect(page.locator('.coach__profil-note--frappee[data-note="81"]')).toBeVisible();
  // Retirer une note d'un clic ; la vélocité minimale ; General MIDI.
  await page.click('.coach__profil-note[data-note="81"]');
  await expect.poll(() => appels(page, "coach_garder_profil").then((a) => 81 in (a.at(-1)?.profil?.notes ?? {}))).toBe(false);
  await page.fill("#coach-profil-seuil", "12");
  await page.locator("#coach-profil-seuil").dispatchEvent("change");
  await expect.poll(() => appels(page, "coach_garder_profil").then((a) => a.at(-1)?.profil?.velocite_min)).toBe(12);
  await page.click("#coach-profil-gm");
  await expect.poll(() => appels(page, "coach_garder_profil").then((a) => a.at(-1)?.profil?.nom)).toBe("General MIDI");
  await expect(page.locator('.coach__profil-note[data-note="38"]')).toBeVisible();
  if (process.env.CAPTURES) await page.screenshot({ path: "test-results/e4-pads.png" });
  // Le repli refermé arrête le sondage.
  await page.keyboard.press("Escape");
  await expect(profil).toBeHidden();
  const sondes = (await appels(page, "coach_notes_recues")).length;
  await page.waitForTimeout(400);
  expect((await appels(page, "coach_notes_recues")).length).toBe(sondes);
  await page.evaluate(() => (window.__FIXTURES__.coach_notes_recues = []));

  // Armé sur les coups de la transcription et la grille des mesures.
  const armer = (await appels(page, "coach_armer")).at(-1);
  expect(armer.coups).toHaveLength(33);
  expect(armer.premiersTemps).toHaveLength(8);

  // Les jugements reçus : un cercle par note jugée et notée, la crash
  // incertaine sans cercle.
  const marques = page.locator("#partition-rendu .coach__marque:visible");
  await expect(marques).toHaveCount(4);
  const classes = await marques.evaluateAll((els) => els.map((e) => e.className.replace("coach__marque coach__marque--", "")));
  expect(classes.sort()).toEqual(["juste", "manquee", "piece", "retard"]);
  // V35 : un signe double chaque couleur — → en retard, × sur la mauvaise
  // pièce, + après la note qui précède le coup en trop.
  const signes = await marques.evaluateAll((els) =>
    els.map((e) => [e.className.replace("coach__marque coach__marque--", ""), e.querySelector(".coach__signe--verdict")?.textContent ?? ""]));
  expect(Object.fromEntries(signes)).toEqual({ juste: "", retard: "→", manquee: "", piece: "×" });
  await expect(page.locator("#partition-rendu .coach__signe--trop:visible")).toHaveCount(1);
  // La légende derrière « ? » (24 nov.) : le bandeau tient sur une ligne.
  await expect(page.locator("#coach-cle")).toBeVisible();
  await expect(page.locator("#coach-legende")).toBeHidden();
  await page.click("#coach-cle > summary");
  await expect(page.locator("#coach-legende")).toBeVisible();
  await expect(page.locator("#coach-legende .coach__cle")).toHaveText(
    ["juste", "←en avance", "→en retard", "×mauvaise pièce", "manquée", "+en trop"]);
  await expect(page.locator("#coach-etat")).toContainText("2 / 4 attrapées");
  await expect(page.locator("#coach-etat")).toContainText("1 en trop");
  await expect(page.locator("#coach-etat")).toContainText("latence 38 ms");
  // Chaque cercle entoure une tête de note de la première mesure.
  const dedans = await page.evaluate(async () => {
    const { partition } = await import("./js/pratiquer.js");
    const table = partition.api.boundsLookup;
    const m = table.findMasterBarByIndex(0).realBounds;
    const s = document.querySelector("#partition-rendu .at-surface").getBoundingClientRect();
    return [...document.querySelectorAll("#partition-rendu .coach__marque")]
      .filter((c) => !c.hidden)
      .every((c) => {
        const r = c.getBoundingClientRect();
        const x = r.left + r.width / 2 - s.left;
        return x >= m.x && x <= m.x + m.w;
      });
  });
  expect(dedans).toBe(true);
  if (process.env.CAPTURES) await page.screenshot({ path: "test-results/e4-coach.png" });

  // Armé avec le morceau : sa table par mesure est lue.
  expect(armer.chemin).toBeTruthy();
  expect((await appels(page, "coach_table")).at(-1).chemin).toBe(armer.chemin);
  // La table propose de boucler les pires mesures : rien ne bouge sans le
  // geste ; le geste pose la boucle et repart à 70 %.
  const boucler = page.locator("#coach-boucler");
  await expect(boucler).toHaveText("boucler mes. 2–3 à 70 %");
  expect(await appels(page, "stems_boucle")).toEqual([]);
  await boucler.click();
  await expect.poll(() => appels(page, "stems_boucle").then((a) => a.at(-1))).toEqual({ debut: 2.5, fin: 6.5 });
  await expect.poll(() => appels(page, "stems_vitesse").then((a) => a.at(-1)?.vitesse)).toBe(0.7);
  await expect(boucler).toBeHidden();
  // En boucle, deux passages propres : l'escalier propose 73 %.
  const vitesse = page.locator("#coach-vitesse");
  await expect(vitesse).toHaveText("↑ 73 %");
  // Rien sous les seuils du groove : le passage seul, sans conseil (E5).
  await expect(page.locator("#coach-etat")).toHaveText("Passage propre : 8 / 8 · ± 9 ms — passer à 73 % ?");
  await vitesse.click();
  await expect.poll(() => appels(page, "stems_vitesse").then((a) => a.at(-1)?.vitesse)).toBe(0.73);
  await expect(vitesse).toBeHidden();

  // Calibrer : le geste attend ▶ ; arrêter ferme l'écoute et efface tout.
  await page.click("#coach-reglages > summary");
  await page.click("#coach-calibrer");
  await expect(page.locator("#coach-etat")).toContainText("seize clics");
  await page.click("#coach-basculer");
  await expect.poll(() => appels(page, "coach_ecouter").then((a) => a.at(-1)?.peripherique)).toBe(null);
  await expect(marques).toHaveCount(0);
  // Sans basse transcrite, la barre reste et offre de la calculer (V34),
  // par le même chemin que le bloc Partitions (ici : les conditions de
  // MuScriptor, pas encore acceptées).
  await page.click('#instruments [data-instrument="bass"]');
  await expect(barre).toBeVisible();
  await expect(barre).toHaveAttribute("aria-label", "Coach de basse");
  await expect(page.locator("#coach-basculer")).toBeHidden();
  // V36 : le diapason est là dès la basse choisie, partition ou non ; pas
  // pour la batterie.
  await expect(page.locator("#coach-accordeur")).toBeVisible();
  const transcrire = page.locator("#coach-transcrire");
  await expect(transcrire).toHaveText("transcrire la basse pour jouer avec");
  await transcrire.click();
  await expect(page.locator("#muscriptor-conditions")).toBeVisible();
});

test("V34 : la batterie sans partition, la barre du coach offre de la transcrire", async ({ page }) => {
  await batterieTranscrite(page);
  // Les mesures sont là, la partition pas encore calculée.
  await page.evaluate(() => {
    window.__FIXTURES__.transcriptions_en_cache = [];
  });
  await pratiquerAlbum(page, 0);
  await page.click('#instruments [data-instrument="drums"]');
  const barre = page.locator("#partition-coach");
  await expect(barre).toBeVisible();
  await expect(barre).toHaveAttribute("aria-label", "Coach de batterie");
  await expect(page.locator("#coach-basculer")).toBeHidden();
  await expect(page.locator("#coach-calibrer")).toBeHidden();
  const transcrire = page.locator("#coach-transcrire");
  await expect(transcrire).toHaveText("transcrire la batterie pour jouer avec");
  await transcrire.click();
  await expect.poll(() => appels(page, "transcrire").then((a) => a.at(-1)?.instrument)).toBe("drums");
  // Rien n'écoute avant que la partition soit là.
  expect(await appels(page, "coach_ecouter")).toEqual([]);
});

test("E5 : jouer la basse transcrite en DI, chaque note jugée en temps et en hauteur", async ({ page }) => {
  await basseTranscrite(page);
  await page.evaluate(() => {
    // Une interface audio branchée, déjà calibrée.
    window.__FIXTURES__.coach_basse_entrees = ["Interface USB", "Micro intégré"];
    window.__FIXTURES__.coach_basse_ecouter = (a) =>
      a.entree ? { entree: a.entree, frequence_hz: 48000, canaux: 2, canal: 0, manuel: false, latence_ms: 24 } : null;
    // Le stem confirme six notes sur sept.
    window.__FIXTURES__.coach_basse_armer = (a) => ({ notes: a.notes.length, sures: a.notes.length - 1, stem: true });
    // Le moteur juge la première mesure : la 1ʳᵉ juste, la 2ᵉ fausse d'un
    // demi-ton, la 3ᵉ à l'octave, la 4ᵉ non notée (le stem ne la confirme
    // pas) ; puis la 1ʳᵉ de la 2ᵉ mesure manquée.
    const j = (reference, verdict, ecart, hauteur) => ({
      reference, passage: 0, frappe: null, attendu_s: 1, ecart_ms: ecart, ecart_musical_ms: ecart, verdict, hauteur,
    });
    window.__FIXTURES__.coach_basse_juger = (a, e) => {
      e.juges = (e.juges ?? 0) + 1;
      return {
        jusqua_s: 10 + e.juges,
        jugements: e.juges === 2
          ? [j(0, "parfait", 3, { type: "juste" }), j(1, "bon", 12, { type: "fausse", demi_tons: 1 }),
             j(2, "retard", 48, { type: "octave", octaves: 1 }), j(3, "non_notee", null, null), j(4, "manquee", null, null)]
          : [],
        notes: [],
        bilan: { justes: 1, fausses: 1, octaves: 1, inconnues: 0 },
        erreur: null,
        canal: 0,
        // V34 : une fois l'entrée 1 choisie à la main, la basse joue sur la 2.
        ailleurs: window.__APPELS__.some((x) => x.cmd === "coach_basse_canal" && x.args?.canal === 0) ? 1 : null,
      };
    };
  });
  await pratiquerAlbum(page, 0);
  await page.click('#instruments [data-instrument="bass"]');
  const barre = page.locator("#partition-coach");
  await expect(barre).toBeVisible();
  await expect(barre).toHaveAttribute("aria-label", "Coach de basse");
  await expect(page.locator("#partition-rendu .at-surface")).toBeVisible();
  // La barre de correction reste là : la basse se corrige et se joue.
  await expect(page.locator("#partition-correction")).toBeVisible();
  expect(await appels(page, "coach_basse_ecouter")).toEqual([]);

  await page.click("#coach-basculer");
  await expect.poll(() => appels(page, "coach_basse_ecouter").then((a) => a.at(-1)?.entree)).toBe("Interface USB");
  // Deux entrées : la liste se montre dans le repli des réglages de l'entrée
  // (V43), avec le canal ; rien de la batterie n'est ouvert.
  await expect(page.locator("#coach-peripherique")).toBeHidden();
  await page.click("#coach-reglages > summary");
  await expect(page.locator("#coach-peripherique")).toBeVisible();
  expect(await appels(page, "coach_ecouter")).toEqual([]);
  const armer = (await appels(page, "coach_basse_armer")).at(-1);
  expect(armer.notes).toHaveLength(7);
  expect(armer.notes[1]).toEqual({ debut_s: 1, hauteur: 36 });

  const marques = page.locator("#partition-rendu .coach__marque:visible");
  await expect(marques).toHaveCount(4);
  const classes = await marques.evaluateAll((els) => els.map((e) => e.className.replace("coach__marque coach__marque--", "")));
  expect(classes.sort()).toEqual(["fausse", "juste", "manquee", "octave"]);
  const titres = await marques.evaluateAll((els) => els.map((e) => e.title));
  // V35 : la note jouée (do attendu, un demi-ton trop haut) écrite à côté.
  expect(titres).toContain("fausse note : do♯ joué (+1 demi-ton)");
  const signes = await marques.evaluateAll((els) => els.map((e) => e.querySelector(".coach__signe--verdict")?.textContent ?? ""));
  expect(signes.sort()).toEqual(["", "", "8", "do♯"]);
  expect(titres).toContain("mauvaise octave (trop haut)");
  // Après l'annonce de l'armement (six notes confirmées sur sept).
  await expect(page.locator("#coach-etat")).toContainText("3 / 4 attrapées", { timeout: 10_000 });
  await expect(page.locator("#coach-etat")).toContainText("1 fausse, 1 à l'octave");
  await expect(page.locator("#coach-etat")).toContainText("entrée 1 · latence 24 ms");
  // V34 : l'interface a deux canaux ; « auto » par défaut.
  const canal = page.locator("#coach-canal");
  await expect(canal).toBeVisible();
  await expect(canal).toHaveValue("auto");
  expect(await canal.locator("option").allTextContents()).toEqual(["auto", "entrée 1", "entrée 2"]);
  // Ni boucle proposée ni escalier pour la basse.
  await expect(page.locator("#coach-boucler")).toBeHidden();
  await expect(page.locator("#coach-vitesse")).toBeHidden();
  if (process.env.CAPTURES) await page.screenshot({ path: "test-results/e5-coach-basse.png" });

  // V34 : l'entrée 1 choisie à la main ; le moteur entend la basse sur la 2
  // et la ligne le dit.
  await canal.selectOption("0");
  await expect.poll(() => appels(page, "coach_basse_canal").then((a) => a.at(-1)?.canal)).toBe(0);
  await expect(page.locator("#coach-etat")).toContainText("la basse semble sur l'entrée 2", { timeout: 10_000 });
  await canal.selectOption("auto");
  await expect.poll(() => appels(page, "coach_basse_canal").then((a) => a.at(-1)?.canal)).toBe(null);

  // Calibrer : la consigne de la basse ; arrêter ferme l'entrée.
  await page.click("#coach-calibrer");
  await expect(page.locator("#coach-etat")).toContainText("corde à vide, étouffée");
  await page.click("#coach-basculer");
  await expect.poll(() => appels(page, "coach_basse_ecouter").then((a) => a.at(-1)?.entree)).toBe(null);
  await expect(marques).toHaveCount(0);
});

test("V35, V37 : la basse transposée — le coach et le code des erreurs suivent la hauteur, la tablature non", async ({ page }) => {
  await basseTranscrite(page);
  await page.evaluate(() => {
    window.__FIXTURES__.coach_basse_entrees = ["Interface USB"];
    window.__FIXTURES__.coach_basse_ecouter = (a) =>
      a.entree ? { entree: a.entree, frequence_hz: 48000, canaux: 1, canal: 0, manuel: false, latence_ms: 24 } : null;
    window.__FIXTURES__.coach_basse_armer = (a) => ({ notes: a.notes.length, sures: a.notes.length, stem: true });
    const j = (reference, verdict, ecart, hauteur) => ({
      reference, passage: 0, frappe: null, attendu_s: 1, ecart_ms: ecart, ecart_musical_ms: ecart, verdict, hauteur,
    });
    // Une fois la transposition reçue : la 1ʳᵉ en avance, la 2ᵉ fausse d'un
    // demi-ton, la 3ᵉ en retard, puis une note en trop.
    window.__FIXTURES__.coach_basse_juger = (a, e) => {
      const transpose = window.__APPELS__.some((x) => x.cmd === "coach_basse_transposer");
      const jugements = transpose && !e.rendu
        ? [j(0, "avance", -60, { type: "juste" }), j(1, "bon", 5, { type: "fausse", demi_tons: 1 }),
           j(2, "retard", 70, { type: "juste" }), { ...j(null, "en_trop", null, null), attendu_s: null }]
        : [];
      if (jugements.length) e.rendu = true;
      return {
        jusqua_s: 10, jugements, notes: [], bilan: { justes: 2, fausses: 1, octaves: 0, inconnues: 0 },
        erreur: null, canal: 0, ailleurs: null,
      };
    };
  });
  await pratiquerAlbum(page, 0);
  await page.click('#instruments [data-instrument="bass"]');
  await expect(page.locator("#partition-rendu .at-surface")).toBeVisible();
  const barre = page.locator("#partition-coach");
  // Au repos : le bandeau d'accent, sans légende ni transposition.
  await expect(barre).not.toHaveClass(/coach--ecoute/);
  await expect(page.locator("#coach-cle")).toBeHidden();
  await expect(page.locator("#coach-transpose")).toBeHidden();
  const fond = await barre.evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(fond).not.toBe("rgba(0, 0, 0, 0)");

  await page.click("#coach-basculer");
  await expect(barre).toHaveClass(/coach--ecoute/);
  await expect(page.locator("#coach-basculer")).toHaveText("■ arrêter l'écoute");
  await expect.poll(() => appels(page, "coach_basse_armer").then((a) => a.at(-1)?.demiTons)).toBe(0);
  await expect(page.locator("#coach-legende .coach__cle")).toHaveText(
    ["juste", "←en avance", "→en retard", "fafausse (note jouée)", "8octave", "manquée", "+en trop"]);

  /// Les frettes de la première mesure, et les textes posés sur les temps.
  const tablature = () =>
    page.evaluate(async () => {
      const { partition } = await import("./js/pratiquer.js");
      const bars = partition.api.score.tracks[0].staves[0].bars;
      return {
        frettes: bars[0].voices[0].beats.map((b) => b.notes[0]?.fret ?? null),
        textes: bars.flatMap((b) => b.voices[0].beats.map((x) => x.text).filter(Boolean)),
      };
    });
  expect((await tablature()).frettes).toEqual([0, 3, 0, 2]);

  // Un demi-ton plus haut, par le champ du dock : le coach juge contre la
  // référence transposée ; la tablature garde ses frettes (V37) et le
  // bandeau dit l'accordage à utiliser.
  const champ = page.locator("#val-tonalite");
  await champ.fill("½");
  await champ.press("Enter");
  await expect.poll(() => appels(page, "coach_basse_transposer").then((a) => a.at(-1)?.demiTons)).toBe(1);
  await expect(page.locator("#coach-transpose")).toHaveText(
    "disque : standard (E A D G) · avec +½ ton : ½ ton au-dessus (F B♭ E♭ A♭)");
  expect((await tablature()).frettes).toEqual([0, 3, 0, 2]);

  // Le code des erreurs : ← en avance, la note jouée (ré : do transposé en
  // do♯, joué un demi-ton plus haut), → en retard, + après la 3ᵉ.
  const marques = page.locator("#partition-rendu .coach__marque:visible");
  await expect(marques).toHaveCount(3, { timeout: 10_000 });
  const vus = await marques.evaluateAll((els) =>
    els.map((e) => [e.className.replace("coach__marque coach__marque--", ""), e.querySelector(".coach__signe--verdict")?.textContent ?? "", e.title]));
  expect(vus.map((v) => v.slice(0, 2)).sort()).toEqual([["avance", "←"], ["fausse", "ré"], ["retard", "→"]]);
  expect(vus.find((v) => v[0] === "fausse")[2]).toBe("fausse note : ré joué (+1 demi-ton)");
  expect(vus.find((v) => v[0] === "retard")[2]).toContain("puis 1 note en trop");
  await expect(page.locator("#partition-rendu .coach__signe--trop:visible")).toHaveText("+");
  if (process.env.CAPTURES) await page.screenshot({ path: "test-results/v35-coach-transpose.png" });

  // Un demi-ton plus bas : toujours les mêmes frettes, jamais « hors
  // manche » ; l'accordage à utiliser descend d'autant.
  await champ.fill("-½");
  await champ.press("Enter");
  await expect.poll(() => appels(page, "coach_basse_transposer").then((a) => a.at(-1)?.demiTons)).toBe(-1);
  await expect(page.locator("#coach-transpose")).toHaveText(
    "disque : standard (E A D G) · avec −½ ton : ½ ton dessous (E♭ A♭ D♭ G♭)");
  expect((await tablature()).frettes).toEqual([0, 3, 0, 2]);
  expect((await tablature()).textes).toEqual([]);
  // Les marques posées sur l'ancienne hauteur sont effacées.
  await expect(marques).toHaveCount(0);
});

test("V37 : la tablature dans l'accordage du disque — mêmes frettes à toute hauteur, accordage dit et choisi à la main", async ({ page }) => {
  await basseTranscrite(page);
  // Un disque en ½ ton dessous (About Damn Time).
  await page.evaluate(() => (window.__ACCORDAGE__ = [27, 32, 37, 42]));
  await pratiquerAlbum(page, 0);
  await page.click('#instruments [data-instrument="bass"]');
  await expect(page.locator("#partition-rendu .at-surface")).toBeVisible();
  const tablature = () =>
    page.evaluate(async () => {
      const { partition } = await import("./js/pratiquer.js");
      const bars = partition.api.score.tracks[0].staves[0].bars;
      return {
        frettes: bars[0].voices[0].beats.map((b) => b.notes[0]?.fret ?? null),
        accord: partition.api.score.tracks[0].staves[0].tuning,
      };
    });
  const avant = await tablature();
  expect(avant.frettes).toEqual([1, 4, 1, 3]);
  // Le rail dit l'accordage déduit ; le bandeau, celui du disque.
  await expect(page.locator("#accordage-ligne")).toBeVisible();
  await expect(page.locator("#accordage-basse option:checked")).toHaveText("déduit : ½ ton dessous");
  await expect(page.locator("#coach-transpose")).toHaveText("accordage : ½ ton dessous (E♭ A♭ D♭ G♭)");

  // +½ ton : mêmes frettes, à jouer en standard.
  const champ = page.locator("#val-tonalite");
  await champ.fill("½");
  await champ.press("Enter");
  await expect(page.locator("#coach-transpose")).toHaveText(
    "disque : ½ ton dessous (E♭ A♭ D♭ G♭) · avec +½ ton : standard (E A D G)");
  expect(await tablature()).toEqual(avant);

  // Le choix à la main : gardé par le moteur, la tablature reposée.
  await page.selectOption("#accordage-basse", "drop-d");
  await expect.poll(() => appels(page, "pratique_accordage").then((a) => a.at(-1)?.accordage)).toBe("drop-d");
  await expect(page.locator("#accordage-basse")).toHaveValue("drop-d");
  await expect(page.locator("#coach-transpose")).toHaveText(
    "disque : drop D (D A D G) · avec +½ ton : drop E♭ (E♭ B♭ E♭ A♭)");
  await expect.poll(async () => (await tablature()).frettes).toEqual([0, 3, 0, 2]);
  // Retour aux notes.
  await page.selectOption("#accordage-basse", "");
  await expect.poll(() => appels(page, "pratique_accordage").then((a) => a.at(-1)?.accordage)).toBe(null);
  await expect.poll(async () => (await tablature()).frettes).toEqual([1, 4, 1, 3]);
});

test("E7 : entendre sa basse — haut-parleur du bandeau, son, volume, balance, micro signalé", async ({ page }) => {
  await basseTranscrite(page);
  await page.evaluate(() => {
    window.__FIXTURES__.coach_basse_entrees = ["Scarlett Solo USB"];
    const base = window.__FIXTURES__.retour_basse_etat;
    // Le moteur simulé : l'écoute s'ouvre, l'entrée retenue est un micro
    // (entrée 1 d'une Scarlett Solo) tant que le test ne dit pas le contraire.
    window.__RETOUR__ = { ...base, ecoute: false, disponible: true, micro: false };
    window.__FIXTURES__.coach_basse_ecouter = (a) => {
      window.__RETOUR__.ecoute = !!a.entree;
      window.__RETOUR__.micro = !!a.entree;
      return a.entree ? { entree: a.entree, frequence_hz: 48000, canaux: 2, canal: 0, manuel: false, latence_ms: 24 } : null;
    };
    window.__FIXTURES__.retour_basse_etat = () => ({ ...window.__RETOUR__ });
    window.__FIXTURES__.retour_basse_regler = (a) => {
      Object.assign(window.__RETOUR__, {
        actif: a.actif && window.__RETOUR__.ecoute, prereglage: a.prereglage, volume: a.volume, balance: a.balance,
        latence_ms: a.actif ? 9.6 : null, stem_disque: a.prereglage === "comme-le-disque" ? false : null,
      });
      return { ...window.__RETOUR__ };
    };
  });
  await pratiquerAlbum(page, 0);
  await page.click('#instruments [data-instrument="bass"]');
  await expect(page.locator("#partition-rendu .at-surface")).toBeVisible();
  const hautParleur = page.locator("#coach-retour");
  await expect(hautParleur).toBeVisible();
  await expect(hautParleur).toHaveAttribute("aria-label", "Entendre ma basse");
  const panneau = page.locator("#retour-basse");
  await expect(panneau).toBeHidden();

  // Ouvert : huit sons, coupé, mi-volume, balance au milieu.
  await hautParleur.click();
  await expect(panneau).toBeVisible();
  expect(await panneau.evaluate((el) => !!el.closest("#editer-etabli") && !el.closest(".rail"))).toBe(true);
  await expect(page.locator("#retour-prereglage option")).toHaveCount(8);
  await expect(page.locator("#retour-prereglage")).toHaveValue("brut");
  await expect(page.locator("#retour-volume")).toHaveValue("50");
  await expect(page.locator("#retour-balance")).toHaveValue("0");
  await expect(page.locator("#retour-actif")).not.toBeChecked();

  // Cochée sur un micro : l'entrée s'ouvre, rien ne sort, l'avertissement
  // (la case se décoche d'elle-même).
  await page.click("#retour-actif");
  await expect(page.locator("#retour-avertissement")).toBeVisible();
  await expect(page.locator("#retour-avertissement")).toContainText("larsen");
  await expect(page.locator("#retour-actif")).not.toBeChecked();
  expect((await appels(page, "coach_basse_ecouter")).at(-1).entree).toBe("Scarlett Solo USB");
  expect((await appels(page, "retour_basse_regler")).filter((a) => a.actif)).toHaveLength(0);
  if (process.env.CAPTURES) await page.screenshot({ path: "test-results/e7-retour-micro.png" });

  // Cochée à nouveau : entendue quand même.
  await page.check("#retour-actif");
  await expect.poll(() => appels(page, "retour_basse_regler").then((a) => a.at(-1)?.actif)).toBe(true);
  await expect(page.locator("#retour-actif")).toBeChecked();
  await expect(hautParleur).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#retour-etat")).toHaveText("latence ≈ 10 ms");

  // Le son, le volume, la balance partent au moteur.
  await page.selectOption("#retour-prereglage", "funky");
  await expect.poll(() => appels(page, "retour_basse_regler").then((a) => a.at(-1)?.prereglage)).toBe("funky");
  await page.locator("#retour-volume").fill("80");
  await expect.poll(() => appels(page, "retour_basse_regler").then((a) => a.at(-1)?.volume)).toBe(0.8);
  await page.locator("#retour-balance").fill("-50");
  await expect.poll(() => appels(page, "retour_basse_regler").then((a) => a.at(-1)?.balance)).toBe(-0.5);
  // « Comme le disque » sans stem : le dire.
  await page.selectOption("#retour-prereglage", "comme-le-disque");
  await expect(page.locator("#retour-etat")).toContainText("pas de stem de basse");
  expect((await appels(page, "retour_basse_regler")).at(-1).id).toBeTruthy();

  // L'écoute fermée par ailleurs (le coach, l'accordeur) : rouverte, le son rallumé.
  await page.evaluate(() => Object.assign(window.__RETOUR__, { ecoute: false, actif: false }));
  await expect.poll(() => page.evaluate(() => window.__RETOUR__.actif), { timeout: 5000 }).toBe(true);

  // Fermer le panneau garde le son ; décocher l'éteint.
  await page.click("#retour-fermer");
  await expect(panneau).toBeHidden();
  await hautParleur.click();
  await page.uncheck("#retour-actif");
  await expect.poll(() => appels(page, "retour_basse_regler").then((a) => a.at(-1)?.actif)).toBe(false);
  await expect(hautParleur).toHaveAttribute("aria-pressed", "false");
});

test("V42 : l'entrée de la basse suit le défaut de macOS ; elle se choisit dans l'accordeur et le retour", async ({ page }) => {
  // La règle, seule : le choix gardé tient tant que le défaut n'a pas bougé.
  const regle = await page.evaluate(async () => {
    const { entreeAOuvrir } = await import("./js/entree-basse.js");
    const l = ["Scarlett 2i2 USB", "Micro du MacBook"];
    return [
      entreeAOuvrir(l, "Micro du MacBook", "Scarlett 2i2 USB"), // choisi à la main, défaut inchangé
      entreeAOuvrir(l, "Micro du MacBook", "Micro du MacBook"), // le défaut a changé : on le suit
      entreeAOuvrir(l, "Micro du MacBook", null), // gardé avant V42 : le défaut
      entreeAOuvrir(l, "Interface débranchée", "Scarlett 2i2 USB"),
      entreeAOuvrir([], "Micro du MacBook", "Scarlett 2i2 USB"),
    ];
  });
  expect(regle).toEqual(["Micro du MacBook", "Scarlett 2i2 USB", "Scarlett 2i2 USB", "Scarlett 2i2 USB", null]);

  await basseTranscrite(page);
  await page.evaluate(() => {
    // Le cas de François : le micro du Mac gardé d'avant, la Scarlett par
    // défaut dans macOS (en tête de la liste du moteur).
    localStorage.setItem("coach-entree", "Micro du MacBook");
    localStorage.removeItem("coach-entree-defaut");
    window.__FIXTURES__.coach_basse_entrees = ["Scarlett 2i2 USB", "Micro du MacBook"];
    window.__RETOUR__ = { ...window.__FIXTURES__.retour_basse_etat, ecoute: false, disponible: true };
    window.__FIXTURES__.coach_basse_ecouter = (a) => {
      window.__RETOUR__.ecoute = !!a.entree;
      return a.entree ? { entree: a.entree, frequence_hz: 48000, canaux: 2, canal: 0, manuel: false, latence_ms: 24 } : null;
    };
    window.__FIXTURES__.retour_basse_etat = () => ({ ...window.__RETOUR__ });
    window.__FIXTURES__.coach_basse_accorder = () => ({ accord: null, canal: 0, erreur: null });
    window.__FIXTURES__.coach_basse_accord_disque = { ecart_cents: 0, notes: 0, stem: false };
  });
  await pratiquerAlbum(page, 0);
  await page.click('#instruments [data-instrument="bass"]');
  await expect(page.locator("#partition-rendu .at-surface")).toBeVisible();
  const ouvertes = async () => (await appels(page, "coach_basse_ecouter")).map((a) => a.entree);

  // L'accordeur ouvre la Scarlett, et montre de quoi choisir.
  await page.click("#coach-accordeur");
  const choixAccordeur = page.locator("#accordeur-entree");
  await expect(choixAccordeur).toBeVisible();
  await expect(choixAccordeur.locator("option")).toHaveCount(2);
  await expect(choixAccordeur).toHaveValue("Scarlett 2i2 USB");
  expect((await ouvertes()).at(-1)).toBe("Scarlett 2i2 USB");

  // Le micro, choisi à la main : rouvert, gardé avec le défaut du moment.
  await choixAccordeur.selectOption("Micro du MacBook");
  await expect.poll(async () => (await ouvertes()).at(-1)).toBe("Micro du MacBook");
  expect(await page.evaluate(() => [localStorage.getItem("coach-entree"), localStorage.getItem("coach-entree-defaut")]))
    .toEqual(["Micro du MacBook", "Scarlett 2i2 USB"]);
  await page.click("#accordeur-fermer");

  // Le retour montre le même choix, et l'ouvre tant que macOS n'a pas bougé.
  await page.click("#coach-retour");
  const choixRetour = page.locator("#retour-entree");
  await expect(choixRetour).toBeVisible();
  await expect(choixRetour).toHaveValue("Micro du MacBook");
  await page.click("#retour-fermer");

  // macOS passe au micro, puis revient à la Scarlett : son nouveau défaut l'emporte.
  await page.evaluate(() => {
    window.__FIXTURES__.coach_basse_entrees = ["Micro du MacBook", "Scarlett 2i2 USB"];
  });
  await page.click("#coach-accordeur");
  await expect(choixAccordeur).toHaveValue("Micro du MacBook");
  await page.click("#accordeur-fermer");
  await page.evaluate(() => {
    window.__FIXTURES__.coach_basse_entrees = ["Scarlett 2i2 USB", "Micro du MacBook"];
  });
  await page.click("#coach-accordeur");
  await expect(choixAccordeur).toHaveValue("Scarlett 2i2 USB");
  expect((await ouvertes()).at(-1)).toBe("Scarlett 2i2 USB");
});

test("V36 : l'accordeur de basse — diapason du bandeau, note, cents, corde, référence du disque", async ({ page }) => {
  await basseTranscrite(page);
  await page.evaluate(() => {
    window.__FIXTURES__.coach_basse_entrees = ["Interface USB"];
    window.__FIXTURES__.coach_basse_ecouter = (a) =>
      a.entree ? { entree: a.entree, frequence_hz: 48000, canaux: 1, canal: 0, manuel: false, latence_ms: 24 } : null;
    // La corde de la, 6 cents trop haut ; le test change ce qu'entend le moteur.
    window.__ACCORD__ = { f0_hz: 55.2, note: 33, cents: 6.3, corde: 33, cents_corde: 6.3, confiance: 0.9 };
    window.__FIXTURES__.coach_basse_accorder = () => ({ accord: window.__ACCORD__, canal: 0, erreur: null });
    window.__FIXTURES__.coach_basse_accord_disque = { ecart_cents: -12.4, notes: 41, stem: true };
    try {
      localStorage.removeItem("accordeur-morceau");
    } catch {}
  });
  await pratiquerAlbum(page, 0);
  await page.click('#instruments [data-instrument="bass"]');
  await expect(page.locator("#partition-rendu .at-surface")).toBeVisible();
  const diapason = page.locator("#coach-accordeur");
  await expect(diapason).toBeVisible();
  await expect(diapason).toHaveAttribute("aria-label", "Accordeur");
  await expect(diapason.locator("svg")).toHaveCount(1);
  const panneau = page.locator("#accordeur");
  await expect(panneau).toBeHidden();

  // Ouvert : l'entrée du coach écoutée, la note et ses cents, la corde visée.
  await diapason.click();
  await expect(panneau).toBeVisible();
  await expect(diapason).toHaveAttribute("aria-pressed", "true");
  expect(await panneau.evaluate((el) => !!el.closest("#editer-etabli") && !el.closest(".rail"))).toBe(true);
  await expect.poll(() => appels(page, "coach_basse_ecouter").then((a) => a.at(-1)?.entree)).toBe("Interface USB");
  await expect(page.locator("#accordeur-note")).toHaveText("la1");
  await expect(page.locator("#accordeur-cents")).toHaveText("+6 cents");
  await expect(page.locator("#accordeur-corde")).toHaveText("corde de la (la1) : +6 cents, trop haut : détendre");
  await expect(page.locator("#accordeur-juste")).toBeHidden();
  await expect(page.locator("#accordeur-disque")).toHaveText("La 440");
  expect((await appels(page, "coach_basse_accorder")).at(-1).laHz).toBe(440);
  // V37 : les cordes visées sont celles de la tablature, ici standard.
  expect((await appels(page, "coach_basse_accorder")).at(-1).cordes).toEqual([28, 33, 38, 43]);
  // L'aiguille à droite du milieu.
  const gauche = await page.locator("#accordeur-aiguille").evaluate((el) => parseFloat(el.style.left));
  expect(gauche).toBeGreaterThan(55);
  if (process.env.CAPTURES) await page.screenshot({ path: "test-results/v36-accordeur.png" });

  // Accordée : l'aiguille lissée rejoint le milieu, « ✓ juste » s'affiche.
  await page.evaluate(() => (window.__ACCORD__ = { ...window.__ACCORD__, f0_hz: 55.01, cents: 0.4, cents_corde: 0.4 }));
  await expect(page.locator("#accordeur-juste")).toBeVisible();
  await expect(page.locator("#accordeur-juste")).toHaveText("✓ juste");
  await expect(page.locator("#accordeur-corde")).toContainText(", juste");

  // Sur le morceau : l'accord du disque devient la référence, gardé.
  await page.check("#accordeur-morceau");
  await expect(page.locator("#accordeur-disque")).toHaveText("le disque est à −12 cents (La 436,9)");
  expect((await appels(page, "coach_basse_accord_disque")).at(-1).id).toBeTruthy();
  await expect.poll(() => appels(page, "coach_basse_accorder").then((a) => Math.round(a.at(-1).laHz * 10))).toBe(4369);
  expect(await page.evaluate(() => localStorage.getItem("accordeur-morceau"))).toBe("1");

  // Rien ne joue : le dire.
  await page.evaluate(() => (window.__ACCORD__ = null));
  await expect(page.locator("#accordeur-corde")).toHaveText("Jouez une corde.");
  await expect(page.locator("#accordeur-note")).toHaveText("–");

  // Le coach prend l'entrée : l'accordeur se ferme et le dit.
  await page.click("#coach-basculer");
  await expect(panneau).toBeHidden();
  await expect(page.locator("#coach-basculer")).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#coach-etat")).toContainText("L'accordeur se ferme");
  // Et l'inverse : rouvrir l'accordeur arrête le coach.
  await diapason.click();
  await expect(panneau).toBeVisible();
  await expect(page.locator("#coach-basculer")).toHaveAttribute("aria-pressed", "false");
  await expect(page.locator("#accordeur-morceau")).toBeChecked();
  // Fermer : l'entrée n'est plus écoutée.
  await page.click("#accordeur-fermer");
  await expect(panneau).toBeHidden();
  await expect(diapason).toHaveAttribute("aria-pressed", "false");
  await expect.poll(() => appels(page, "coach_basse_ecouter").then((a) => a.at(-1)?.entree)).toBe(null);
});

/// Les lignes qu'occupent les enfants visibles de la barre de correction.
const lignesBarre = (barre) =>
  barre.evaluate((el) => {
    const hauts = new Set();
    // La largeur que demande le contenu sur une ligne : enfants, marges et
    // écarts (l'export, poussé à droite, compte sans sa marge automatique).
    const gap = parseFloat(getComputedStyle(el).columnGap) || 0;
    let contenu = 0;
    let n = 0;
    for (const x of el.querySelectorAll(":scope > *")) {
      const r = x.getBoundingClientRect();
      if (r.width === 0) continue;
      hauts.add(Math.round(r.top + r.height / 2));
      const st = getComputedStyle(x);
      contenu += r.width + (x.matches(".correction__export") ? 0 : parseFloat(st.marginLeft) + parseFloat(st.marginRight));
      n++;
    }
    contenu += gap * Math.max(0, n - 1);
    const r = el.getBoundingClientRect();
    return { lignes: hauts.size, largeur: Math.round(r.width), hauteur: Math.round(r.height), contenu: Math.round(contenu) };
  });

test("V18 : la barre de correction tient sur une ligne à 1 400 px ; une note transcrite change au demi-ton", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  await basseTranscrite(page);
  await pratiquerAlbum(page, 0);
  await page.click('#instruments [data-instrument="bass"]');
  const barre = page.locator("#partition-correction");
  await expect(page.locator("#partition-rendu .at-surface")).toBeVisible();
  // Une note transcrite corrigée : tous les gestes à la fois (corde, octave,
  // demi-ton, supprimer, rétablir) et l'export.
  await cliquerTemps(page, 0, 1);
  await barre.locator('[data-corriger="octave-haut"]').click();
  await expect(page.locator("#correction-note")).toHaveText("do3 · corde de sol, frette 5 · corrigée");
  for (const g of ["grave", "aigue", "octave-bas", "octave-haut", "demi-bas", "demi-haut", "supprimer", "retablir"]) {
    await expect(barre.locator(`[data-corriger="${g}"]`)).toBeVisible();
  }
  const mesure = await lignesBarre(barre);
  console.log(`V18 barre de correction à 1400×900 : ${JSON.stringify(mesure)}`);
  if (process.env.CAPTURES) await page.screenshot({ path: "test-results/v18-correction.png" });
  expect(mesure.lignes).toBe(1);

  // Le demi-ton sur la note transcrite : gardé à part de l'octave.
  await barre.locator('[data-corriger="demi-haut"]').click();
  await expect(page.locator("#correction-note")).toHaveText("do♯3 · corde de sol, frette 6 · corrigée");
  expect((await appels(page, "corriger_transcription")).at(-1).correction).toEqual({
    genre: "demiton", debut_s: 1, hauteur: 36, demitons: 1,
  });
  await barre.locator('[data-corriger="demi-bas"]').click();
  await barre.locator('[data-corriger="demi-bas"]').click();
  await expect(page.locator("#correction-note")).toHaveText("si2 · corde de sol, frette 4 · corrigée");
  expect((await appels(page, "corriger_transcription")).at(-1).correction.demitons).toBe(-1);
  // Rétablir oublie octave et demi-ton.
  await barre.locator('[data-corriger="retablir"]').click();
  await expect(page.locator("#correction-note")).toHaveText("do2 · corde de la, frette 3");
});

/* ------------------------------------------------ P1 : gérer les playlists */

/// Le menu « Ajouter à une playlist » ouvert sous `bouton`, puis le choix.
async function ajouterVia(page, bouton, nom) {
  await bouton.click();
  const menu = page.locator("#menu-playlist");
  await expect(menu).toBeVisible();
  await menu.locator(".menu-playlist__item", { hasText: nom }).click();
  await expect(menu.locator(".menu-playlist__etat")).toContainText(nom);
  await expect(menu).toBeHidden();
}

test("P1 : « Ajouter à une playlist » depuis une ligne, l'inspecteur, un album et la file", async ({ page }) => {
  const premierBanc = PISTES.filter((p) => p.album === "Premier banc");
  await page.locator("#grille .album").first().click();
  const lignes = page.locator("#liste .ligne--pistes");
  await expect(lignes).toHaveCount(2);

  // Une ligne : le ＋ n'apparaît qu'au survol, et ne sélectionne pas la ligne.
  const plus = lignes.nth(0).locator(".ligne__action", { hasText: "＋" });
  await expect(plus).toBeHidden();
  await lignes.nth(0).hover();
  await ajouterVia(page, plus, "À travailler");
  expect((await appels(page, "ajouter_a_playlist")).at(-1)).toEqual({ id: 1, paths: [PISTES[0].path] });
  await expect(lignes.nth(0)).not.toHaveClass(/ligne--select/);

  // « Nouvelle playlist… », nommée sur place, sans fenêtre modale.
  await lignes.nth(1).hover();
  await lignes.nth(1).locator(".ligne__action", { hasText: "＋" }).click();
  await page.locator("#menu-playlist .menu-playlist__item--nouvelle").click();
  await page.locator("#menu-playlist input").fill("Soirée");
  await page.locator("#menu-playlist input").press("Enter");
  await expect(page.locator("#menu-playlist .menu-playlist__etat")).toContainText("Soirée");
  expect((await appels(page, "nouvelle_playlist")).at(-1)).toEqual({ nom: "Soirée", paths: [PISTES[1].path] });

  // L'inspecteur : le morceau sélectionné ; les récentes d'abord.
  await lignes.nth(1).click();
  await expect(page.locator("#insp-playlist")).toBeVisible();
  await page.locator("#insp-playlist").click();
  await expect(page.locator("#menu-playlist .menu-playlist__item").first()).toHaveText("Soirée");
  await page.keyboard.press("Escape");
  await expect(page.locator("#menu-playlist")).toBeHidden();
  await ajouterVia(page, page.locator("#insp-playlist"), "À travailler");
  expect((await appels(page, "ajouter_a_playlist")).at(-1)).toEqual({ id: 1, paths: [PISTES[1].path] });

  // L'album ouvert : toutes ses pistes ; celle déjà là est dite.
  await ajouterVia(page, page.locator("#album-playlist"), "À travailler");
  expect((await appels(page, "ajouter_a_playlist")).at(-1)).toEqual({ id: 1, paths: premierBanc.map((p) => p.path) });

  // La file : le ＋ d'une ligne ne saute pas au morceau.
  await page.click("#retour");
  await lireUnAlbum(page);
  await page.click("#bascule-file");
  const ligneFile = page.locator("#file-liste .file__ligne").nth(1);
  await ligneFile.hover();
  const sauts = (await appels(page, "jump_to")).length;
  await ajouterVia(page, ligneFile.locator(".file__ajout"), "Soirée");
  expect((await appels(page, "ajouter_a_playlist")).at(-1)).toEqual({ id: 2, paths: [premierBanc[1].path] });
  expect((await appels(page, "jump_to")).length).toBe(sauts);
});

test("P1 : une playlist vide se crée ; dans une playlist ouverte, retirer, annuler, déplacer", async ({ page }) => {
  await page.evaluate((ps) => {
    window.__ETAT_FAUX__.playlists = [{
      id: 1, nom: "À travailler", origine: null, modifiee_le: 1_700_000_000, chemins: ps.slice(0, 3).map((p) => p.path),
    }];
  }, PISTES);
  await page.click('[data-vuelib="playlists"]');
  await expect(page.locator("#liste .ligne--playlist")).toHaveCount(1);

  // Une playlist vide, depuis l'en-tête de la vue.
  await page.click("#playlist-nouvelle");
  await page.locator("#menu-playlist input").fill("Vide");
  await page.locator("#menu-playlist input").press("Enter");
  expect((await appels(page, "nouvelle_playlist")).at(-1)).toEqual({ nom: "Vide", paths: [] });
  await expect(page.locator("#liste .ligne--playlist")).toHaveCount(2);
  await expect(page.locator("#liste .ligne--playlist").first()).toContainText("0 piste");

  // Ouvrir « À travailler » : compte et durée dans le fil.
  await page.locator("#liste .ligne--playlist", { hasText: "À travailler" }).locator(".ligne__nom").click();
  await expect(page.locator("#playlist-nouvelle")).toBeHidden();
  await expect(page.locator("#fil-compte")).toHaveText(/^3 morceaux · /);
  const noms = () => page.locator("#liste .ligne--pistes .ligne__nom").allTextContents();
  expect(await noms()).toEqual(PISTES.slice(0, 3).map((p) => p.title));

  // ✕ : la ligne reste, estompée, « annuler » la remet.
  const lignes = page.locator("#liste .ligne--pistes");
  await lignes.nth(0).hover();
  await lignes.nth(0).locator(".ligne__action", { hasText: "✕" }).click();
  expect((await appels(page, "retirer_de_playlist")).at(-1)).toEqual({ id: 1, rang: 0, path: PISTES[0].path });
  await expect(lignes.nth(0)).toHaveClass(/ligne--retiree/);
  await expect(page.locator("#fil-compte")).toHaveText(/^2 morceaux · /);
  await lignes.nth(0).locator(".ligne__action", { hasText: "annuler" }).click();
  expect((await appels(page, "remettre_dans_playlist")).at(-1)).toEqual({ id: 1, rang: 0, path: PISTES[0].path });
  await expect(lignes.nth(0)).not.toHaveClass(/ligne--retiree/);
  await expect(page.locator("#fil-compte")).toHaveText(/^3 morceaux · /);

  // ⌥↓ : la piste sélectionnée descend d'un rang.
  await lignes.nth(0).click();
  await page.keyboard.press("Alt+ArrowDown");
  expect((await appels(page, "deplacer_dans_playlist")).at(-1)).toEqual({ id: 1, de: 0, a: 1, path: PISTES[0].path });
  await expect.poll(noms).toEqual([PISTES[1], PISTES[0], PISTES[2]].map((p) => p.title));

  // Glisser la dernière au-dessus de la première.
  await lignes.nth(2).dragTo(lignes.nth(0), { targetPosition: { x: 40, y: 6 } });
  expect((await appels(page, "deplacer_dans_playlist")).at(-1)).toEqual({ id: 1, de: 2, a: 0, path: PISTES[2].path });
  await expect.poll(noms).toEqual([PISTES[2], PISTES[1], PISTES[0]].map((p) => p.title));

  // ✕ sans annuler : la ligne s'en va après le message.
  await lignes.nth(1).hover();
  await lignes.nth(1).locator(".ligne__action", { hasText: "✕" }).click();
  await expect(lignes).toHaveCount(2, { timeout: 8000 });
  expect(await noms()).toEqual([PISTES[2], PISTES[0]].map((p) => p.title));
  expect(await page.evaluate(() => window.__ETAT_FAUX__.playlists.find((p) => p.id === 1).chemins))
    .toEqual([PISTES[2].path, PISTES[0].path]);
});

/* ------------------------------- P2 : Pratiquer enchaîne une playlist */

/// Une playlist de trois morceaux à travailler (id 1), posée dans l'état du
/// faux moteur.
const A_TRAVAILLER = [PISTES[0], PISTES[2], PISTES[4]];
async function poserPlaylistATravailler(page) {
  await page.evaluate((chemins) => {
    window.__ETAT_FAUX__.playlists = [{ id: 1, nom: "À travailler", origine: null, modifiee_le: 1_700_000_000, chemins }];
  }, A_TRAVAILLER.map((t) => t.path));
}

test("P2 : choisir une playlist en Pratiquer, suivant et précédent rendent la séance de chaque morceau", async ({ page }) => {
  await poserPlaylistATravailler(page);
  // La séance déjà gardée du deuxième morceau : batterie, deux mesures.
  await page.evaluate((path) => {
    window.__ETAT_FAUX__.pratique = { [path]: { instrument: "drums", decompte: 2, boucles: [], accel_boucle: false } };
  }, A_TRAVAILLER[1].path);
  await page.click('button.mode[data-mode="pratiquer"]');
  // Sans playlist en cours : pas de bandeau ; le sélecteur dans l'état « choisir ».
  await expect(page.locator("#pratique-playlist")).toBeHidden();
  await expect(page.locator("#pratique-playlist-choisir")).toBeVisible();

  await page.click("#pratique-playlist-choisir");
  const menu = page.locator("#menu-pratique-playlist");
  await expect(menu).toBeVisible();
  await menu.locator(".menu-playlist__item", { hasText: "À travailler" }).click();
  await expect(menu).toBeHidden();
  const bandeau = page.locator("#pratique-playlist");
  await expect(bandeau).toBeVisible();
  await expect(page.locator("#enchainement-rang")).toHaveText("1 / 3");
  await expect(page.locator("#enchainement-suivant")).toHaveText(`suivant : ${A_TRAVAILLER[1].title} — ${A_TRAVAILLER[1].artist}`);
  await expect(page.locator("#demix-source")).toContainText(A_TRAVAILLER[0].title);
  await expect(page.locator("#editer-etabli")).toBeVisible();
  await expect(page.locator("#enchainement-precedent")).toBeDisabled();
  // Le suivant se démixe d'avance, avec la variante par défaut de Pratiquer
  // (il n'a pas de variante gardée), puis se dit prêt.
  expect((await appels(page, "pratique_preparer")).at(-1)).toEqual({ path: A_TRAVAILLER[1].path, variant: "bsrofo_sw" });
  await expect(page.locator("#enchainement-avance")).toHaveText("prêt", { timeout: 6000 });
  // Le bouton du sélecteur ne vit que dans l'état « choisir ».
  await expect(page.locator("#pratique-playlist-choisir")).toBeHidden();

  // Une séance sur le premier : basse, sans décompte.
  await page.click('#instruments [data-instrument="bass"]');
  await page.click('#decomptes [data-decompte="0"]');
  // … et un demi-ton plus bas (V42) : le suivant ne le reste pas.
  await page.click('[data-r="tonalite"][data-d="-1"]');
  await expect(page.locator("#val-tonalite")).toHaveValue("−½ ton");
  const gardee = async (path) =>
    (await appels(page, "pratique_garder")).filter((a) => a.path === path).at(-1)?.memoire ?? null;
  await expect.poll(() => gardee(A_TRAVAILLER[0].path).then((m) => m?.decompte), { timeout: 6000 }).toBe(0);

  // → : le deuxième, et sa séance revient.
  await page.click("#enchainement-suivant-bouton");
  await expect(page.locator("#enchainement-rang")).toHaveText("2 / 3");
  await expect(page.locator("#demix-source")).toContainText(A_TRAVAILLER[1].title);
  await expect(page.locator('#instruments [data-instrument="drums"]')).toHaveClass(/segment--actif/);
  await expect(page.locator('#decomptes [data-decompte="2"]')).toHaveClass(/segment--actif/);
  expect((await appels(page, "pratique_playlist_poser")).at(-1)).toEqual({ id: 1, path: A_TRAVAILLER[1].path });
  await expect(page.locator("#val-tonalite")).toHaveValue("±0");

  // ⇧← : retour au premier, sa séance rendue ; la file de lecture n'a pas bougé.
  const sauts = (await appels(page, "next")).length + (await appels(page, "previous")).length;
  await page.locator("body").press("Shift+ArrowLeft");
  await expect(page.locator("#enchainement-rang")).toHaveText("1 / 3");
  await expect(page.locator("#demix-source")).toContainText(A_TRAVAILLER[0].title);
  await expect(page.locator('#instruments [data-instrument="bass"]')).toHaveClass(/segment--actif/);
  await expect(page.locator('#decomptes [data-decompte="0"]')).toHaveClass(/segment--actif/);
  expect((await appels(page, "next")).length + (await appels(page, "previous")).length).toBe(sauts);
  await expect(page.locator("#val-tonalite")).toHaveValue("−½ ton");

  // Le dernier n'a pas de stems : il se sépare dès qu'on y arrive, avec la
  // variante de sa séance.
  await page.evaluate(({ path, stems }) => {
    const f = window.__FIXTURES__;
    const avant = f.stems_existants;
    f.stems_existants = (a, e) => (a.path === path && !e.separe ? [] : avant(a, e));
    f.start_demix = (_, e) => { e.separe = true; return null; };
    f.demix_state = { en_cours: false, faits: 0, total: 0, resultat: "4 stems écrits", stems };
    window.__ETAT_FAUX__.pratique[path] = { variante: "htdemucs", boucles: [], accel_boucle: false };
  }, { path: A_TRAVAILLER[2].path, stems: ["drums", "bass", "other", "vocals"].map((n) => [n, `/stems/x/${n}.wav`]) });
  await page.locator("body").press("Shift+ArrowRight");
  await page.locator("body").press("Shift+ArrowRight");
  await expect(page.locator("#enchainement-rang")).toHaveText("3 / 3");
  await expect(page.locator("#enchainement-suivant")).toHaveText("dernier morceau");
  await expect(page.locator("#enchainement-suivant-bouton")).toBeDisabled();
  await expect.poll(() => appels(page, "start_demix").then((a) => a.at(-1))).toEqual({ path: A_TRAVAILLER[2].path, variant: "htdemucs" });
  await expect(page.locator("#editer-etabli")).toBeVisible();

  // × : plus de playlist en cours, plus de bandeau.
  await page.click("#enchainement-fermer");
  await expect(bandeau).toBeHidden();
  expect(await appels(page, "pratique_playlist_fermer")).toHaveLength(1);
});

test("P2 : « Pratiquer cette playlist » depuis Écouter, et la reprise d'une session à l'autre", async ({ page }) => {
  await poserPlaylistATravailler(page);
  await page.click('[data-vuelib="playlists"]');
  const ligne = page.locator("#liste .ligne--playlist", { hasText: "À travailler" });
  await ligne.hover();
  await ligne.locator(".ligne__action", { hasText: "♩" }).click();
  await expect(page.locator('button.mode[data-mode="pratiquer"]')).toHaveClass(/mode--actif/);
  await expect(page.locator("#enchainement-rang")).toHaveText("1 / 3");
  await page.click("#enchainement-suivant-bouton");
  await expect(page.locator("#enchainement-rang")).toHaveText("2 / 3");
  // Le bandeau est à Pratiquer : Créer et Écouter ne le montrent pas.
  await page.click('button.mode[data-mode="creer"]');
  await expect(page.locator("#pratique-playlist")).toBeHidden();
  await page.click('button.mode[data-mode="ecoute"]');
  await expect(page.locator("#pratique-playlist")).toBeHidden();

  // Une nouvelle session : la base a gardé la playlist et le morceau.
  const garde = await page.evaluate(() => window.__ETAT_FAUX__.pratiquePlaylist);
  expect(garde).toEqual({ id: 1, path: A_TRAVAILLER[1].path });
  await page.addInitScript(({ garde, chemins }) => {
    window.__ETAT_FAUX__.pratiquePlaylist = garde;
    window.__ETAT_FAUX__.playlists = [{ id: 1, nom: "À travailler", origine: null, modifiee_le: 1_700_000_000, chemins }];
  }, { garde, chemins: A_TRAVAILLER.map((t) => t.path) });
  await page.reload();
  await versAlbums(page);
  await expect(page.locator("#grille .album")).toHaveCount(3);
  await page.click('button.mode[data-mode="pratiquer"]');
  await expect(page.locator("#enchainement-rang")).toHaveText("2 / 3");
  await expect(page.locator("#demix-source")).toContainText(A_TRAVAILLER[1].title);
});

/* ---------- V31 : le démixage d'avance cède la place à un démixage manuel */

test("V31 : le bandeau dit « en attente » quand le démixage d'avance cède la place", async ({ page }) => {
  await poserPlaylistATravailler(page);
  // Le faux moteur : le suivant se démixe d'avance, l'état se règle de la page.
  await page.evaluate((path) => {
    window.__ETAT_FAUX__.avance = { en_cours: true, courant: path, en_pause: false, attente: null, echecs: [] };
    window.__FIXTURES__.pratique_preparation = (_, e) => e.avance;
  }, A_TRAVAILLER[1].path);
  await page.click('button.mode[data-mode="pratiquer"]');
  await page.click("#pratique-playlist-choisir");
  await page.locator("#menu-pratique-playlist .menu-playlist__item", { hasText: "À travailler" }).click();
  const avance = page.locator("#enchainement-avance");
  await expect(avance).toHaveText("démixage d'avance…", { timeout: 6000 });
  // Un démixage lancé à la main passe d'abord : l'avance se met en pause.
  await page.evaluate(() => { window.__ETAT_FAUX__.avance.en_pause = true; });
  await expect(avance).toHaveText("en attente", { timeout: 6000 });
  // Il a fini : l'avance reprend, puis va au bout.
  await page.evaluate(() => { window.__ETAT_FAUX__.avance.en_pause = false; });
  await expect(avance).toHaveText("démixage d'avance…", { timeout: 6000 });
  await page.evaluate(() => { Object.assign(window.__ETAT_FAUX__.avance, { en_cours: false, courant: null }); });
  await expect(avance).toHaveText("prêt", { timeout: 6000 });
});

/* ---------------- V33 : les liens Markdown des critiques, rendus en liens */

test("V33 : une critique montre ses liens Markdown comme des liens, sans HTML ni syntaxe brute", async ({ page }) => {
  await page.evaluate(() => {
    window.__FIXTURES__.critiques_piste = [{
      texte: "Avec [Rage Against the Machine](http://www.bbc.co.uk/music/artists/3798b104 \"titre\") et <b>rien de gras</b>, [un faux lien](javascript:alert(1)) et [ceci] (pas un lien).",
      auteur: "BBC", url_originale: "https://critiquebrainz.org/review/1", licence_id: "CC BY-NC-SA 3.0", licence_nom: "CC BY-NC-SA 3.0",
    }];
    window.__LIENS__ = [];
    window.__TAURI__.opener.openUrl = async (url) => { window.__LIENS__.push(url); };
  });
  await lireUnAlbum(page);
  const critique = page.locator("#insp-critiques .critique").first();
  const texte = critique.locator("p").first();
  await expect(texte).toHaveText("Avec Rage Against the Machine et <b>rien de gras</b>, un faux lien et [ceci] (pas un lien).");
  // Un seul lien, le http(s) ; le HTML reste du texte.
  await expect(texte.locator("a")).toHaveCount(1);
  await expect(texte.locator("b")).toHaveCount(0);
  await texte.locator("a").click();
  expect(await page.evaluate(() => window.__LIENS__)).toEqual(["http://www.bbc.co.uk/music/artists/3798b104"]);
  // L'attribution CritiqueBrainz reste là, sous la critique.
  await expect(critique.locator(".critique-attribution")).toContainText("BBC · CritiqueBrainz · CC BY-NC-SA 3.0");
});

/* ------------------------- E5 : le groove de la batterie MIDI, en conseils */

test("E5 : un conseil du groove suit la fin d'un passage de boucle", async ({ page }) => {
  await batterieTranscrite(page);
  await page.evaluate(() => {
    window.__FIXTURES__.coach_groove = (_, e) => ({
      pieces: [], swing: null,
      conseils: [{ motif: "devant", classe: "caisse_claire", texte: e.conseil ?? "Tu joues la caisse claire 22 ms devant le disque." }],
    });
  });
  await pratiquerAlbum(page, 0);
  await page.click('#instruments [data-instrument="drums"]');
  await page.click("#coach-basculer");
  // Armé avec la grille des battements (swing, décalage du disque au temps).
  await expect.poll(() => appels(page, "coach_armer").then((a) => a.at(-1)?.temps?.length)).toBe(32);
  // Hors boucle : une mesure relevée ne suffit pas à demander le groove.
  await expect(page.locator("#coach-boucler")).toHaveText("boucler mes. 2–3 à 70 %");
  expect(await appels(page, "coach_groove")).toEqual([]);
  // En boucle, la fin d'un passage dit son compte et le conseil.
  await page.locator("#coach-boucler").click();
  const etat = page.locator("#coach-etat");
  await expect(etat).toHaveText(
    "Passage propre : 8 / 8 · ± 9 ms — passer à 73 % ? · Tu joues la caisse claire 22 ms devant le disque.",
  );
  // Le texte entier au survol, si la ligne le coupe.
  await expect(etat).toHaveAttribute("title", /devant le disque/);
});
