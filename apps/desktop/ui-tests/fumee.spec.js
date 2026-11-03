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
      : { appaire: true, ecoute: true, port: 47391, adresses: ["mac.local"], erreur: null, en_attente: Boolean(e.attente), dernier: null };
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
