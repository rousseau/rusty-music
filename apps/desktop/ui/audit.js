/* Audit de gabarit de l'interface — `scripts/audit-interface.sh`.
 *
 * Chargé **à la demande** par `RUSTY_MUSIC_AUDIT=1` (jamais en usage normal).
 * La webview du système ne se capture pas hors de son Espace macOS : on la mesure
 * de l'intérieur. L'audit parcourt écrans × états × thèmes × tailles de fenêtre
 * et, pour chaque cellule, vérifie des invariants de gabarit (voir
 * `docs/interface-guidelines.md`, Règle 10) ; chaque cellule écrit une ligne
 * `AUDIT {…}` au journal du processus, la dernière `AUDIT FIN`.
 *
 *  G1 jetons      rail, inspecteur, transport aux dimensions du gabarit ; la file
 *                 d'attente (surimpression) recouvre exactement l'inspecteur
 *  G2 symétrie    marges gauche/droite du contenu égales, la droite mesurée
 *                 jusqu'au panneau droit **le plus à gauche** (inspecteur ou file)
 *  G3 débordement aucun défilement horizontal, aucun élément hors de sa zone
 *  G4 occultation tout contrôle visible reçoit le clic en son centre
 *  G5 repères     le repère A–Z n'existe qu'en Écoute
 *
 * Les variables lues dans la feuille de style (`--largeur-rail`…) ont une valeur
 * de repli : l'audit tourne aussi sur un état d'avant les jetons, ce qui
 * permet de vérifier qu'il attrape bien les défauts qu'il vise.
 */

/* eslint-disable no-undef -- utilise les globales de app.js (basculerMode, carte…) */

const AUDIT_ATTENTE = (ms) => new Promise((r) => setTimeout(r, ms));

/// Une dimension du gabarit : la variable CSS si elle existe, sinon la valeur
/// historique.
function jetonAudit(nom, repli) {
  const v = parseFloat(getComputedStyle(document.documentElement).getPropertyValue(nom));
  return Number.isFinite(v) ? v : repli;
}

const visibleAudit = (el) => {
  if (!el) return false;
  const cs = getComputedStyle(el);
  if (cs.display === "none" || cs.visibility === "hidden" || Number(cs.opacity) === 0) return false;
  const r = el.getBoundingClientRect();
  return r.width > 0 && r.height > 0;
};

/// Le rectangle réellement visible d'un élément : son rectangle, rogné par
/// chaque ancêtre qui coupe son contenu (défilement, `overflow: hidden`).
function rectVisibleAudit(el) {
  let r = el.getBoundingClientRect();
  let x0 = r.left, y0 = r.top, x1 = r.right, y1 = r.bottom;
  for (let a = el.parentElement; a && a !== document.body; a = a.parentElement) {
    const cs = getComputedStyle(a);
    if (cs.position === "fixed") break;
    if (cs.overflowX !== "visible" || cs.overflowY !== "visible") {
      const b = a.getBoundingClientRect();
      x0 = Math.max(x0, b.left); y0 = Math.max(y0, b.top);
      x1 = Math.min(x1, b.right); y1 = Math.min(y1, b.bottom);
    }
  }
  return { x0, y0, x1, y1 };
}

const nomAudit = (el) =>
  el ? `${el.tagName.toLowerCase()}${el.id ? "#" + el.id : ""}${el.className && typeof el.className === "string" ? "." + el.className.split(" ")[0] : ""}` : "?";

/// Ce que l'on contrôle en G2, par écran : les blocs de contenu du centre.
/// `padding` : la marge est le `padding` du bloc (il occupe tout le centre) ;
/// `gauche` : un élément posé en absolu, dont seul le bord gauche compte.
const CONTENUS_AUDIT = {
  ecoute: [
    { sel: ".fil" }, { sel: "#grille" }, { sel: "#liste .ligne" }, { sel: ".autour-artiste" },
  ],
  "explorer:points": [{ sel: ".fil" }, { sel: "#carte-aide", mode: "gauche" }],
  "explorer:carte": [{ sel: ".fil" }, { sel: "#carte-aide", mode: "gauche" }],
  "explorer:temps": [{ sel: ".fil" }, { sel: "#carte-aide", mode: "gauche" }],
  "explorer:anneau": [{ sel: ".fil" }, { sel: "#carte-aide", mode: "gauche" }],
  "explorer:lama": [{ sel: ".fil" }, { sel: ".lama__tete" }, { sel: "#lama-corps" }],
  "explorer:lama+playlist": [{ sel: ".fil" }, { sel: ".lama__tete" }, { sel: "#lama-corps" }],
  editer: [{ sel: ".fil" }, { sel: "#editer-etabli" }, { sel: "#editer-separer" }],
  decouvrir: [{ sel: ".fil" }, { sel: "#decouvrir-vue" }],
  bibliotheque: [{ sel: ".fil" }, { sel: "#bibliotheque-vue" }],
};

/// Conteneurs dont on contrôle le défilement horizontal (G3).
const DEFILEMENTS_AUDIT = [
  ".rail", ".centre", ".inspecteur", "#file", "#liste", "#grille", "#lama-corps",
  "#bibliotheque-vue", "#decouvrir-vue", "#editer-etabli", "#editer-separer",
];

function auditerGabarit(ecran) {
  const viol = [];
  const mes = {};
  const q = (s) => document.querySelector(s);
  const rect = (s) => q(s)?.getBoundingClientRect();
  const rail = rect(".rail");
  const insp = rect(".inspecteur");
  const trans = rect(".transport");
  const fileEl = q("#file");
  const fileOuverte = fileEl && !fileEl.hidden && visibleAudit(fileEl);
  const file = fileOuverte ? fileEl.getBoundingClientRect() : null;
  const tol = 1;

  // --- G1 jetons --------------------------------------------------------
  const attendu = {
    rail: jetonAudit("--largeur-rail", 232),
    droite: jetonAudit("--largeur-droite", 300),
    transport: jetonAudit("--hauteur-transport", 64),
    gouttiere: jetonAudit("--gouttiere", 26),
  };
  mes.fenetre = [innerWidth, innerHeight];
  const egal = (regle, quoi, mesure, voulu) => {
    if (Math.abs(mesure - voulu) > 0.6) viol.push({ regle, detail: `${quoi} : ${mesure.toFixed(1)} px, attendu ${voulu}` });
  };
  if (rail) egal("G1", "largeur du rail", rail.width, attendu.rail);
  if (insp) egal("G1", "largeur de l'inspecteur", insp.width, attendu.droite);
  if (trans) egal("G1", "hauteur du transport", trans.height, attendu.transport);
  if (file && insp) {
    egal("G1", "file : largeur (doit recouvrir exactement l'inspecteur)", file.width, insp.width);
    egal("G1", "file : bord gauche (doit être celui de l'inspecteur)", file.left, insp.left);
    if (trans) egal("G1", "file : bas (doit s'arrêter au transport)", file.bottom, trans.top);
  }

  // --- G2 symétrie des marges ------------------------------------------
  const droiteVisible = Math.min(insp ? insp.left : innerWidth, file ? file.left : Infinity);
  mes.droiteVisible = Math.round(droiteVisible);
  for (const { sel, mode } of CONTENUS_AUDIT[ecran] ?? []) {
    const el = q(sel);
    if (!visibleAudit(el) || !rail) continue;
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    const pl = mode === "gauche" ? 0 : parseFloat(cs.paddingLeft) || 0;
    const pr = parseFloat(cs.paddingRight) || 0;
    const gauche = r.left + pl - rail.right;
    if (mode === "gauche") {
      if (Math.abs(gauche - attendu.gouttiere) > tol)
        viol.push({ regle: "G2", detail: `${sel} : marge gauche ${gauche.toFixed(1)} px, attendu ${attendu.gouttiere}` });
      continue;
    }
    const droite = droiteVisible - (r.right - pr);
    mes[sel] = [Math.round(gauche), Math.round(droite)];
    if (Math.abs(gauche - droite) > tol)
      viol.push({ regle: "G2", detail: `${sel} : marges gauche ${gauche.toFixed(1)} ≠ droite ${droite.toFixed(1)} px (jusqu'au panneau droit visible)` });
    else if (gauche < attendu.gouttiere - tol)
      viol.push({ regle: "G2", detail: `${sel} : marges ${gauche.toFixed(1)} px < gouttière ${attendu.gouttiere}` });
    else if (gauche > attendu.gouttiere + tol && ecran !== "bibliotheque")
      viol.push({ regle: "G2", detail: `${sel} : marges ${gauche.toFixed(1)} px ≠ gouttière ${attendu.gouttiere}` });
  }

  // --- G3 débordements ---------------------------------------------------
  for (const sel of DEFILEMENTS_AUDIT) {
    const el = q(sel);
    if (!visibleAudit(el)) continue;
    if (el.scrollWidth > el.clientWidth + tol)
      viol.push({ regle: "G3", detail: `${sel} défile horizontalement (${el.scrollWidth} > ${el.clientWidth})` });
  }
  const zones = [".lama__haut", ".lama__section", ".inspecteur", ".rail"];
  for (const sel of zones) {
    const zone = q(sel);
    if (!visibleAudit(zone)) continue;
    const cz = getComputedStyle(zone);
    const limite = zone.getBoundingClientRect().right - (parseFloat(cz.paddingRight) || 0) + 0.5;
    const hors = [...zone.querySelectorAll("*")].filter((e) => {
      if (!visibleAudit(e) || getComputedStyle(e).position === "absolute") return false;
      for (let a = e.parentElement; a && a !== zone; a = a.parentElement)
        if (getComputedStyle(a).overflowX !== "visible") return false; // coupé par un ancêtre : pas un débordement
      return e.getBoundingClientRect().right > limite;
    });
    for (const e of hors.slice(0, 3))
      viol.push({ regle: "G3", detail: `${nomAudit(e)} dépasse la zone de ${sel} de ${(e.getBoundingClientRect().right - limite).toFixed(1)} px` });
  }

  // --- G4 occultation ------------------------------------------------------
  const interactifs = document.querySelectorAll(
    'button, input:not([type="hidden"]), select, textarea, a[href], [tabindex]:not([tabindex="-1"])',
  );
  let occultes = 0;
  for (const el of interactifs) {
    if (!visibleAudit(el)) continue;
    const v = rectVisibleAudit(el);
    if (v.x1 - v.x0 < 4 || v.y1 - v.y0 < 4) continue; // coupé, ou invisible à dessein
    const cx = (v.x0 + v.x1) / 2, cy = (v.y0 + v.y1) / 2;
    if (cx < 0 || cy < 0 || cx > innerWidth || cy > innerHeight) continue;
    const haut = document.elementFromPoint(cx, cy);
    if (!haut || haut === el || el.contains(haut) || haut.contains(el)) continue;
    // La file recouvre l'inspecteur, c'est sa raison d'être.
    if (fileOuverte && el.closest(".inspecteur") && haut.closest("#file")) continue;
    occultes++;
    if (occultes <= 4) viol.push({ regle: "G4", detail: `${nomAudit(el)} est recouvert par ${nomAudit(haut)}` });
  }

  // --- G5 repères propres à un écran ----------------------------------------
  const ia = q("#index-alpha");
  if (ia && ecran !== "ecoute" && visibleAudit(ia))
    viol.push({ regle: "G5", detail: "le repère A–Z est affiché hors de l'Écoute" });

  return { violations: viol, mesures: mes };
}

/// L'état « Lama après une playlist », fabriqué à partir de vrais points de la
/// carte (aucune dépendance à Ollama) : le plan et la composition que
/// `path_texte_interpreter`/`path_texte` rendraient, passés aux mêmes fonctions
/// d'affichage que le flux réel.
function fabriquerLamaAudit() {
  const pts = carte.points.slice(0, 12);
  const piste = (p) => ({
    id: p.id, path: p.path, title: p.title, artist: p.artist, album: p.album,
    track_no: p.track_no, year: p.year, duration_ms: p.duration_ms ?? 200000, artist_mbid: null,
  });
  const pistes = pts.map(piste);
  const duree = (a, b) => pistes.slice(a, b).reduce((t, x) => t + x.duration_ms, 0);
  planTexte = {
    depart: null, arrivee: null, arrivee_demandee: null, etapes: ["rock music", "hip hop music"], n: 12,
    duree_minutes: null, plafond_par_artiste: null,
    filtres: { genres: [], exclure_genres: ["rock"], exclure_artistes: ["Prince"], annee_min: 1970, annee_max: 1979, bpm_min: null, bpm_max: null, energie: "calme", popularite: null },
    reformulation: "Douze minutes de rock, puis vingt de hip hop.",
    parties: [
      { description: "rock music", genres: ["rock"], energie: null, duree_minutes: 12, n: null },
      { description: "hip hop music", genres: ["hip hop"], energie: null, duree_minutes: 20, n: null },
    ],
    brut: "{}",
  };
  const ids = pts.map((p) => p.id);
  const compo = {
    pistes, relaches: [], duree_ms: duree(0, pistes.length),
    parties: [
      { libelle: "Rock", debut: 0, fin: 5, duree_ms: duree(0, 5), cible_ms: 720000, n_admissibles: ids.length, entonnoir: [["Bibliothèque", 27385], ["genre : rock", 7922]], depart_pourquoi: "le plus proche de la description", relaches: [], ids_admissibles: ids },
      { libelle: "Hip hop", debut: 5, fin: pistes.length, duree_ms: duree(5, pistes.length), cible_ms: 1200000, n_admissibles: ids.length, entonnoir: [["Bibliothèque", 27385], ["genre : hip hop", 3462]], depart_pourquoi: "le plus proche de la fin de la partie précédente", relaches: ["durée visée 20 min, mais la marche n'enchaîne que 14 min de morceaux admissibles"], ids_admissibles: ids.slice(0, 6) },
    ],
  };
  $("lama-accueil").hidden = true;
  $("intention-texte").value = "Rock, pendant 12 minutes, puis hip hop pendant 20 minutes";
  montrerPlanTexte(planTexte);
  montrerComposition(compo);
  fileCourante = pistes;
}

async function auditerInterface() {
  const attendre = AUDIT_ATTENTE;
  const clic = (aff) => document.querySelector(`[data-affichage="${aff}"]`)?.click();
  // Explorer d'abord : il charge les points de la carte, dont la sélection et
  // l'état « Lama après playlist » ont besoin.
  await basculerMode("explorer");
  for (let i = 0; i < 60 && !carte.points.length; i++) await attendre(250);
  if (!carte.points.length) throw new Error("la carte n'a pas chargé ses points");

  const ECRANS = [
    ["explorer:points", async () => { await basculerMode("explorer"); clic("points"); }],
    ["explorer:carte", async () => { await basculerMode("explorer"); clic("carte"); await attendre(1500); }],
    ["explorer:temps", async () => { await basculerMode("explorer"); clic("temps"); await attendre(600); }],
    ["explorer:anneau", async () => { await basculerMode("explorer"); clic("anneau"); await attendre(600); }],
    ["explorer:lama", async () => { await basculerMode("explorer"); clic("lama"); $("lama-haut").hidden = true; $("lama-accueil").hidden = false; ["lama-compris", "lama-cherche", "lama-resultat"].forEach((i) => ($(i).hidden = true)); }],
    ["explorer:lama+playlist", async () => { await basculerMode("explorer"); clic("lama"); fabriquerLamaAudit(); await attendre(300); dessinerMiniNuage(); }],
    ["ecoute", async () => { await basculerMode("ecoute"); await attendre(700); }],
    ["editer", async () => { await basculerMode("editer"); await attendre(500); }],
    ["decouvrir", async () => { await basculerMode("decouvrir"); await attendre(900); }],
    ["bibliotheque", async () => { await basculerMode("bibliotheque"); await attendre(900); }],
  ];
  const FENETRES = [[960, 620], [1400, 900], [1920, 1080]];
  const THEMES = ["sombre", "clair"];
  const piste = carte.points[0];

  let cellules = 0, total = 0;
  for (const [w, h] of FENETRES) {
    await invoke("essai_fenetre", { largeur: w, hauteur: h });
    await attendre(600);
    for (const theme of THEMES) {
      if (theme === "clair") document.documentElement.dataset.theme = "clair";
      else delete document.documentElement.dataset.theme;
      for (const [ecran, aller] of ECRANS) {
        await basculerFile(false);
        await aller();
        await attendre(350);
        for (const etat of ["base", "file ouverte", "morceau sélectionné"]) {
          if (etat === "file ouverte") {
            if (ecran !== "explorer:lama+playlist") fileCourante = fileCourante.length ? fileCourante : [piste];
            basculerFile(true);
          } else {
            basculerFile(false);
            if (etat === "morceau sélectionné") await selectionner(piste);
          }
          await attendre(250);
          const { violations, mesures } = auditerGabarit(ecran);
          cellules++;
          total += violations.length;
          journalCarte("AUDIT " + JSON.stringify({ ecran, etat, theme, fenetre: [innerWidth, innerHeight], violations, mesures }), violations.length ? "warn" : "log");
        }
      }
    }
  }
  basculerFile(false);
  journalCarte(`AUDIT FIN cellules=${cellules} violations=${total}`, total ? "warn" : "log");
}
