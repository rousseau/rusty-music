import "./aura.js"; // l'ordre d'exécution : voir main.js
import {
  $, invoke, navigation,
} from "./socle.js";

import {
  carte, cnv,
} from "./carte.js";
import {
  bornesGeoPoints, carteGL, carteReelle, journalCarte, majAffichageGL, pointSous, versCarte,
  versEcran, vueInitialeGL, zoomer,
} from "./maplibre.js";
import {
  basculerMode,
} from "./modes.js";

/* ------------------------------------------------- autotest de la carte
 *
 * Une webview du système ne se pilote pas de l'extérieur : sans ce banc,
 * « est-ce que le lasso marche encore ? » ne se vérifie qu'à la main, et donc
 * ne se vérifie pas. Il exerce les chemins de code que l'arrivée de MapLibre a
 * touchés — les deux transformations de coordonnées, le pointage, le lasso, la
 * bascule des modes — et rend son verdict au journal du processus.
 *
 * Déclenché par `RUSTY_MUSIC_AUTOTEST=1`.
 */
async function autotestCarte() {
  const resultats = [];
  // **Rendre compte au fil de l'eau, pas à la fin.** Grouper les résultats
  // rendait tout blocage opaque : le banc ne disait rien, et l'on ne savait
  // même pas s'il avait démarré. Chaque ligne part maintenant dès qu'elle est
  // connue, ce qui localise l'arrêt au pas près.
  const verifier = (nom, ok, detail = "") => {
    const v = { nom, ok, detail: String(detail).slice(0, 120) };
    resultats.push(v);
    journalCarte(`${v.ok ? "OK  " : "ÉCHEC"} ${v.nom}${v.detail ? " — " + v.detail : ""}`, v.ok ? "log" : "warn");
  };
  const etape = (nom) => journalCarte("… " + nom);
  const attendre = (ms) => new Promise((r) => setTimeout(r, ms));

  try {
    etape("bascule en mode explorer");
    if (navigation.mode !== "explorer") await basculerMode("explorer");
    await attendre(500);
    verifier("morceaux chargés", carte.points.length > 0, carte.points.length);

    // --- mode nuage : le repère du canevas -------------------------------
    carte.affichage = "points";
    majAffichageGL();
    await attendre(200);
    const r = cnv.getBoundingClientRect();
    verifier("canevas dimensionné", r.width > 100 && r.height > 100, `${r.width}×${r.height}`);

    const p0 = carte.points[0];
    const [ex, ey] = versEcran(p0, r);
    const [rx, ry] = versCarte(ex, ey, r);
    verifier(
      "nuage : aller-retour des coordonnées",
      Math.abs(rx - p0.x) < 1e-3 && Math.abs(ry - p0.y) < 1e-3,
      `${p0.x.toFixed(4)}→${rx.toFixed(4)}`,
    );
    verifier("nuage : pointage", pointSous(ex, ey)?.id === p0.id, pointSous(ex, ey)?.id);

    const avantZoom = carte.vue.k;
    zoomer(1.4, r.width / 2, r.height / 2);
    verifier("nuage : zoom", carte.vue.k > avantZoom, `${avantZoom.toFixed(2)}→${carte.vue.k.toFixed(2)}`);
    carte.vue = { k: 1, dx: 0, dy: 0 };

    // --- mode carte : le repère de MapLibre ------------------------------
    etape("passage en mode carte");
    carte.affichage = "carte";
    const t0 = performance.now();
    majAffichageGL();
    for (let i = 0; i < 240 && !carte.glPret; i++) await attendre(250);
    const ms = performance.now() - t0;
    verifier(
      "carte : tuiles chargées",
      carte.glPret,
      carte.glPret
        ? `${carte.gl.getStyle().layers.length} couches en ${(ms / 1000).toFixed(1)} s`
        : "délai dépassé",
    );
    // Une carte qui met plus de trois secondes à paraître passe pour cassée.
    verifier("carte : délai d'apparition", carte.glPret && ms < 3000, `${(ms / 1000).toFixed(1)} s`);

    if (carte.glPret) {
      const [gx, gy] = versEcran(p0, r);
      const [gxr, gyr] = versCarte(gx, gy, r);
      // Sur le plan de ville, `versEcran` place le morceau à son adresse, pas
      // à sa position t-SNE : c'est elle que l'aller-retour doit retrouver.
      const [attX, attY] = (carte.villeReelle && carte.positionsReelles.get(p0.id)) || [p0.x, p0.y];
      verifier(
        "carte : aller-retour des coordonnées",
        Math.abs(gxr - attX) < 1e-3 && Math.abs(gyr - attY) < 1e-3,
        `${attX.toFixed(4)}→${gxr.toFixed(4)}`,
      );
      verifier("carte : pointage", pointSous(gx, gy)?.id === p0.id, pointSous(gx, gy)?.id);

      const zAvant = carte.gl.getZoom();
      zoomer(1.4, r.width / 2, r.height / 2);
      verifier(
        "carte : zoom",
        carte.gl.getZoom() > zAvant,
        `${zAvant.toFixed(2)}→${carte.gl.getZoom().toFixed(2)}`,
      );

      // Les boutons +/− appellent `zoomer` sans coordonnées : ne doit pas
      // lever dans MapLibre (`unproject` d'un point indéfini).
      const zB = carte.gl.getZoom();
      let boutonOk = true;
      try {
        $("zoom-plus").click();
        $("zoom-moins").click();
      } catch (err) {
        boutonOk = false;
      }
      verifier(
        "carte : boutons +/−",
        boutonOk && Math.abs(carte.gl.getZoom() - zB) < 1e-6,
        carte.gl.getZoom().toFixed(2),
      );

      const cAvant = carte.gl.getCenter().lng;
      const g2 = carteGL();
      g2.panBy([-50, 0], { duration: 0 });
      verifier(
        "carte : déplacement",
        carte.gl.getCenter().lng !== cAvant,
        carte.gl.getCenter().lng.toFixed(3),
      );

      // La vue d'ensemble cadre l'emprise cible (limite communale, ou les
      // points sur le monde fictif) : elle dézoome sous le niveau d'accueil et
      // fait tenir l'emprise dans la fenêtre.
      const b = vueInitialeGL?.bounds || bornesGeoPoints();
      $("zoom-reset").click();
      const dedans = b && (() => {
        const sw = carte.gl.project(b[0]);
        const ne = carte.gl.project(b[1]);
        const { width, height } = cnv.getBoundingClientRect();
        return sw.x >= -2 && ne.x <= width + 2 && ne.y >= -2 && sw.y <= height + 2;
      })();
      verifier(
        "carte : vue d'ensemble",
        !!b && dedans && carte.gl.getZoom() < (vueInitialeGL?.zoom ?? 14),
        `zoom ${carte.gl.getZoom().toFixed(2)}`,
      );

      // Les entités des tuiles répondent-elles au pointage ?
      // Après la vue d'ensemble (animée), les tuiles du nouveau cadrage se
      // chargent : on attend qu'elles le soient avant d'interroger.
      const couche = carte.villeReelle ? "territoires-reels" : "territoires";
      for (let i = 0; i < 40 && !(carte.gl.areTilesLoaded() && !carte.gl.isMoving()); i++) {
        await attendre(250);
      }
      const rendues = carte.gl.queryRenderedFeatures({ layers: [couche] });
      const source = carte.gl.querySourceFeatures("carte", { sourceLayer: couche });
      verifier(
        "carte : entités interrogeables",
        rendues.length > 0,
        `${rendues.length} rendues, ${source.length} en source (zoom ${carte.gl.getZoom().toFixed(1)})`,
      );
    }

    // --- le lasso, dans les deux repères ---------------------------------
    for (const mode of ["points", "carte"]) {
      carte.affichage = mode;
      majAffichageGL();
      await attendre(150);
      // Le contour part de l'écran, comme un vrai geste : on prend un
      // rectangle au centre du canevas et on le convertit dans le repère du
      // mode actif. C'est justement cette conversion que MapLibre a changée.
      const c = [
        versCarte(r.width * 0.3, r.height * 0.3, r),
        versCarte(r.width * 0.7, r.height * 0.3, r),
        versCarte(r.width * 0.7, r.height * 0.7, r),
        versCarte(r.width * 0.3, r.height * 0.7, r),
      ];
      try {
        etape(`lasso (${mode}) — appel`);
        const pris = await invoke("selection", { trace: c, reel: carteReelle() });
        verifier(
          `lasso (${mode})`,
          pris.length > 0 && pris.length < carte.points.length,
          `${pris.length} morceaux`,
        );
      } catch (e) {
        verifier(`lasso (${mode})`, false, e);
      }
    }

    // --- l'itinéraire ----------------------------------------------------
    try {
      etape("itinéraire — construction du réseau, une trentaine de secondes");
      const t = await invoke("itineraire", {
        depart: carte.points[0].id,
        arrivee: null,
        profil: "sentier",
        minutes: 20,
        familles: null,
      });
      verifier(
        "itinéraire",
        t.length > 0 && t[0].pistes.length > 1,
        `${t[0]?.pistes.length} morceaux, ${(t[0]?.duree_ms / 60000).toFixed(0)} min`,
      );
    } catch (e) {
      verifier("itinéraire", false, e);
    }

    // --- l'itinéraire musical borné à une famille isolée -----------------
    try {
      etape("itinéraire borné à la famille du départ");
      const depart = carte.points[0];
      const t = await invoke("itineraire", {
        depart: depart.id,
        arrivee: null,
        profil: "sentier",
        minutes: 10,
        familles: [depart.cluster],
      });
      const hors = (t[0]?.pistes || []).filter((p) => p.id !== depart.id && p.cluster !== depart.cluster);
      verifier(
        "itinéraire : familles isolées",
        t.length > 0 && t[0].pistes.length > 1 && hors.length === 0,
        `${t[0]?.pistes.length} morceaux, ${hors.length} hors famille ${depart.cluster}`,
      );
    } catch (e) {
      // Un départ sans voisin dans sa famille est un refus légitime, pas un bogue.
      verifier("itinéraire : familles isolées", /aucun trajet/.test(String(e)), e);
    }

    // --- l'itinéraire sur voirie réelle (si une ville est importée) ------
    if (carte.villeReelle) {
      try {
        etape("itinéraire sur voirie — accrochage des morceaux aux rues");
        const rep = await invoke("itineraire_voirie", {
          depart: carte.points[0].id,
          arrivee: null,
          profil: "panoramique",
          minutes: 20,
          familles: null,
          rayonM: null,
        });
        const t = rep.trajets[0];
        verifier(
          "itinéraire voirie",
          !rep.repli && t && t.pistes.length > 1 && t.polyligne.length > 1,
          rep.repli || `${t?.pistes.length} morceaux, polyligne ${t?.polyligne.length} points`,
        );
      } catch (e) {
        verifier("itinéraire voirie", false, e);
      }
    }

    carte.affichage = "points";
    majAffichageGL();
  } catch (e) {
    verifier("autotest", false, (e && e.stack) || e);
  }

  journalCarte(
    `AUTOTEST ${resultats.filter((v) => v.ok).length}/${resultats.length}`,
  );
}

invoke("autotest_carte")
  .then((oui) => {
    if (oui) setTimeout(autotestCarte, 2500);
  })
  .catch(() => {});
