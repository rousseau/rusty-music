import "./decouvrir.js"; // l'ordre d'exécution : voir main.js
import {
  $, ecoute, horloge, invoke, isoler, navigation, remonter, txt, viderPochettes,
} from "./socle.js";
import {
  chargerPopulariteFraicheur, popARecalculee,
} from "./vues.js";
import {
  peindreTransport, poserLecture,
} from "./lecteur.js";
import {
  carte,
} from "./carte.js";
import {
  dessinerFamilles, majAffichageGL,
} from "./maplibre.js";
import {
  chargerFamillesParAlbum, dessinerFamillesEcoute, familleARecalculee, rafraichirGrille,
} from "./ecouter-familles.js";
import {
  chargerCarte,
} from "./fond-de-plan.js";
import {
  contactMb, depuisTexte,
} from "./decouvrir.js";
import {
  accelererAuPassage, edition,
} from "./editer.js";
import {
  arreterStems, dessinerStems, majDerive, niveaux, poserTete, positionMoteur, reconcilierStems,
  resumeStem, stemEcarte, tonaliteDe, vitesseDe,
} from "./pratiquer.js";
import {
  charger,
} from "./demarrage.js";
import { lecture, transport } from "./etat-lecture.js";

/* --------------------------------------------------- mode Bibliothèque */

/// Trois requêtes d'agrégation, rejouées à chaque entrée dans le mode — pas
/// de sondage : `library_stats` lit la base telle qu'elle est, elle ne
/// mesure rien.
async function chargerStatsBibliotheque() {
  const s = await invoke("library_stats");
  dernierTotalBiblio = s.total;
  resumeChaine();
  $("stats-total").textContent = `${s.total.toLocaleString("fr-FR")} morceaux.`;
  dessinerBarresGenres(s.genres);
  dessinerHistogramme("stats-tempo", s.tempo, (i) => `${s.tempo.min + i * s.tempo.pas}–${s.tempo.min + (i + 1) * s.tempo.pas} BPM`);
  dessinerAxe("stats-tempo-axe", s.tempo, (borne) => `${borne}`);
  $("stats-tempo-note").textContent = notesHistogramme(s.tempo, "mesurés");
  // `horloge`, pas `duree` : cette dernière rend « — » pour 0, pertinent pour
  // une piste de durée inconnue mais pas pour la borne d'une tranche.
  dessinerHistogramme("stats-durees", s.durees, (i) => horloge(s.durees.min + i * s.durees.pas));
  dessinerAxe("stats-durees-axe", s.durees, (borne) => horloge(borne));
  $("stats-durees-note").textContent = notesHistogramme(s.durees, "chronométrés");

  dessinerBarresCodecs(s.codecs);
  dessinerHistogramme("stats-bitrate", s.bitrate, (i) => `${s.bitrate.min + i * s.bitrate.pas}–${s.bitrate.min + (i + 1) * s.bitrate.pas} kb/s`);
  dessinerAxe("stats-bitrate-axe", s.bitrate, (borne) => `${borne}`);
  const bouts = [];
  // Au-delà de la dernière tranche, ce n'est pas une anomalie comme pour le
  // tempo ou la durée : plus le débit est haut, meilleure est la qualité —
  // et les formats sans perte (FLAC…) y vivent en permanence.
  if (s.bitrate.hors_gamme) {
    bouts.push(`${s.bitrate.hors_gamme.toLocaleString("fr-FR")} à ${s.bitrate.min + s.bitrate.comptes.length * s.bitrate.pas} kb/s ou plus, formats sans perte compris`);
  }
  if (s.bitrate.sans_valeur) {
    bouts.push(`${s.bitrate.sans_valeur.toLocaleString("fr-FR")} sans débit mesuré`);
  }
  $("stats-bitrate-note").textContent = bouts.join(" · ");

  dessinerBarres("stats-humeur", s.humeur, ["non mesuré"]);

  majEtat(
    "identites",
    s.sans_mbid ? `${pluriel(s.sans_mbid, "morceau", "morceaux")} sans identifiant` : "toutes connues",
    s.sans_mbid > 0,
  );
  genresEtat.sansGenre = s.genres.find(([g]) => g === "—")?.[1] ?? 0;
  majEtatGenres();

  const completude = $("stats-completude");
  completude.replaceChildren();
  for (const [libelle, n] of [
    ["Sans MusicBrainz", s.sans_mbid],
    ["Sans genre identifié", s.genres.find(([g]) => g === "—")?.[1] ?? 0],
    ["Sans tempo mesuré", s.tempo.sans_valeur],
    ["Sans biographie d'artiste", s.sans_bio],
    ["Sans critique d'album", s.sans_critique],
    ["Sans crédits Discogs", s.sans_discogs],
  ]) {
    const dt = document.createElement("dt");
    dt.textContent = libelle;
    const dd = document.createElement("dd");
    dd.textContent = `${n.toLocaleString("fr-FR")} (${((n / s.total) * 100).toFixed(1).replace(".", ",")} %)`;
    completude.append(dt, dd);
  }
}

/// Graduations de l'abscisse d'un histogramme, espacées en pourcentage de la
/// largeur plutôt qu'une par tranche — [`Histogramme`] en a jusqu'à 18, les
/// nommer toutes serait illisible. `libellerBorne(valeur)` met en forme une
/// borne (BPM brut pour le tempo, `m:ss` pour la durée).
function dessinerAxe(id, h, libellerBorne) {
  const hote = $(id);
  hote.replaceChildren();
  const tranches = h.comptes.length;
  const CIBLE = 6;
  const pas = Math.max(1, Math.round(tranches / CIBLE));
  const bornes = [];
  for (let i = 0; i <= tranches; i += pas) bornes.push(i);
  if (bornes[bornes.length - 1] !== tranches) bornes.push(tranches);

  for (const i of bornes) {
    const pct = (i / tranches) * 100;
    const el = document.createElement("span");
    el.textContent = libellerBorne(h.min + i * h.pas);
    if (i === 0) {
      el.style.left = "0";
    } else if (i === tranches) {
      el.style.left = "100%";
      el.style.transform = "translateX(-100%)";
    } else {
      el.style.left = `${pct}%`;
      el.style.transform = "translateX(-50%)";
    }
    hote.appendChild(el);
  }
}

/// Rend une liste `[nom, compte]` en barres horizontales — sert aux genres
/// comme aux formats de fichier, seul le pliage en amont diffère.
/// `nomsGris` fait ressortir en gris/italique les lignes qui ne décrivent
/// pas une vraie catégorie (« Sans genre identifié », « non mesuré »…), pour
/// qu'elles ne se confondent pas avec un genre ou un format minoritaire.
function dessinerBarres(id, lignes, nomsGris = []) {
  const hote = $(id);
  hote.replaceChildren();
  if (lignes.length === 0) return;
  const max = Math.max(...lignes.map(([, c]) => c));

  for (const [nom, compte] of lignes) {
    const el = document.createElement("div");
    el.className = "barre-genre" + (nomsGris.includes(nom) ? " barre-genre--indetermine" : "");
    el.innerHTML = `<span class="barre-genre__nom"></span>
                    <div class="barre-genre__piste"><div class="barre-genre__remplissage"></div></div>
                    <span class="barre-genre__valeur"></span>`;
    el.children[0].textContent = nom;
    el.children[0].title = nom;
    el.children[1].firstElementChild.style.width = `${(compte / max) * 100}%`;
    el.children[2].textContent = compte.toLocaleString("fr-FR");
    hote.appendChild(el);
  }
}

/// Les `n` premiers rangs affichés tels quels, le reste plié dans « Autres ».
const RANGS_AFFICHES = 12;

function plierEnAutres(lignes, unite) {
  const gardes = lignes.slice(0, RANGS_AFFICHES);
  const reste = lignes.slice(RANGS_AFFICHES).reduce((n, [, c]) => n + c, 0);
  return reste > 0
    ? [...gardes, [`Autres (${lignes.length - RANGS_AFFICHES} ${unite})`, reste]]
    : gardes;
}

function dessinerBarresGenres(genres) {
  // « — » (aucun genre résolu, ni par MusicBrainz ni par le tag du fichier)
  // n'est pas un genre parmi d'autres : le noyer dans « Autres » masquait le
  // seul chiffre qui réponde à « pourquoi autant de genres non identifiés ».
  // Il sort donc du classement et se trace à part, en dernier, quel que soit
  // son rang.
  const identifie = genres.filter(([g]) => g !== "—");
  const sansGenre = genres.find(([g]) => g === "—");
  const lignes = plierEnAutres(identifie, "genres");
  if (sansGenre) lignes.push(["Sans genre identifié", sansGenre[1]]);
  dessinerBarres("stats-genres", lignes, ["Sans genre identifié"]);
}

function dessinerBarresCodecs(codecs) {
  dessinerBarres("stats-codecs", plierEnAutres(codecs, "formats"), ["non mesuré"]);
}

/* ------------------------------------------------------ vérifications */

/// Une ligne de vérification : `ligne` en gras, `detail` en dessous en gris,
/// `mesure` alignée à droite (une distance, une date…), `action` un bouton
/// optionnel (retirer, par exemple). Même gabarit pour les cinq listes du
/// mode Bibliothèque — genres suspects, éditions, doublons, isolés, échecs.
function dessinerListeVerif(id, items) {
  const hote = $(id);
  hote.replaceChildren();
  for (const it of items) {
    const el = document.createElement("div");
    el.className = "verif" + (it.onClick ? " verif--clic" : "");
    if (it.onClick) el.addEventListener("click", it.onClick);
    const corps = document.createElement("div");
    corps.className = "verif__corps";
    const ligne = document.createElement("div");
    ligne.className = "verif__ligne";
    ligne.textContent = it.ligne;
    ligne.title = it.ligne;
    corps.appendChild(ligne);
    if (it.detail) {
      const detail = document.createElement("div");
      detail.className = "verif__detail";
      detail.textContent = it.detail;
      corps.appendChild(detail);
    }
    el.appendChild(corps);
    if (it.mesure) {
      const mesure = document.createElement("span");
      mesure.className = "verif__mesure";
      mesure.textContent = it.mesure;
      el.appendChild(mesure);
    }
    if (it.action) el.appendChild(it.action);
    hote.appendChild(el);
  }
}

/* --------------------------------------------- paramètres de la carte */

const CHAMPS_PARAMETRES_CARTE = {
  "param-perplexite": "perplexite",
  "param-epoques": "epoques",
  "param-familles": "familles",
  "param-iterations": "iterations_kmeans",
};

async function chargerParametresCarte() {
  const p = await invoke("map_parameters");
  $("param-perplexite").value = p.perplexite;
  $("param-epoques").value = p.epoques;
  $("param-familles").value = p.familles;
  $("param-iterations").value = p.iterations_kmeans;

  $("densite-resolution").value = String(p.densite_resolution);
  $("densite-noyau").value = p.densite_noyau;
  $("densite-noyau-valeur").textContent = p.densite_noyau.toFixed(3);
  $("densite-bandes").value = p.densite_bandes;
  $("densite-bandes-valeur").textContent = p.densite_bandes;
}

for (const [id, cle] of Object.entries(CHAMPS_PARAMETRES_CARTE)) {
  $(id).addEventListener("change", async (e) => {
    const valeur = Number(e.target.value);
    if (!Number.isFinite(valeur)) return;
    try {
      await invoke("set_map_parameter", { cle, valeur });
    } catch (err) {
      remonter(err, "paramètre de la carte");
    }
  });
}

/* --------------------------------- vocabulaire des familles par genre */

// Une ligne par famille : « Nom: genre1, genre2, genre3 ». Format texte
// plutôt qu'une liste éditable élément par élément — plus rapide à relire et
// à corriger d'un coup pour douze familles, et rien ici n'a besoin de
// glisser-déposer.
function serialiserVocabulaire(vocabulaire) {
  return vocabulaire.map(([nom, genres]) => `${nom}: ${genres.join(", ")}`).join("\n");
}

function analyserVocabulaire(texte) {
  const vocabulaire = [];
  for (const ligne of texte.split("\n")) {
    const deux = ligne.indexOf(":");
    if (deux < 0) continue; // ligne vide ou incomplète, ignorée sans erreur
    const nom = ligne.slice(0, deux).trim();
    const genres = ligne
      .slice(deux + 1)
      .split(",")
      .map((g) => g.trim().toLowerCase())
      .filter(Boolean);
    if (nom && genres.length) vocabulaire.push([nom, genres]);
  }
  return vocabulaire;
}

async function chargerVocabulaireFamilles() {
  const v = await invoke("vocabulaire_familles");
  $("vocabulaire-familles").value = serialiserVocabulaire(v);
}

$("enregistrer-vocabulaire").addEventListener("click", async () => {
  const bouton = $("enregistrer-vocabulaire");
  bouton.disabled = true;
  $("vocabulaire-etat").textContent = "Enregistrement…";
  try {
    const vocabulaire = analyserVocabulaire($("vocabulaire-familles").value);
    await invoke("definir_vocabulaire_familles", { vocabulaire });
    $("vocabulaire-etat").textContent =
      `${vocabulaire.length} famille(s) enregistrée(s) — « Recalculer la carte » pour l'appliquer.`;
  } catch (e) {
    remonter(e, "vocabulaire des familles");
    $("vocabulaire-etat").textContent = String(e);
  } finally {
    bouton.disabled = false;
  }
});

$("reinitialiser-vocabulaire").addEventListener("click", async () => {
  const bouton = $("reinitialiser-vocabulaire");
  bouton.disabled = true;
  try {
    // Liste vide = restaure les valeurs par défaut côté base, voir
    // `Library::definir_vocabulaire_familles`.
    await invoke("definir_vocabulaire_familles", { vocabulaire: [] });
    await chargerVocabulaireFamilles();
    $("vocabulaire-etat").textContent =
      "Valeurs par défaut restaurées — « Recalculer la carte » pour l'appliquer.";
  } catch (e) {
    remonter(e, "vocabulaire des familles");
    $("vocabulaire-etat").textContent = String(e);
  } finally {
    bouton.disabled = false;
  }
});

$("recalculer-carte").addEventListener("click", async () => {
  const bouton = $("recalculer-carte");
  bouton.disabled = true;
  $("carte-parametres-etat").textContent = "Recalcul en cours…";
  try {
    const r = await invoke("recompute_map");
    $("carte-parametres-etat").textContent =
      `${r.empreintes.toLocaleString("fr-FR")} morceaux replacés, ${r.familles.toLocaleString("fr-FR")} familles.`;
    // La carte affichée, si elle l'est, montre des positions périmées ; les
    // familles nommées et le filtre par famille de l'Écoute le sont tout autant.
    await familleARecalculee();
    if (navigation.mode === "explorer") {
      await chargerCarte();
      await dessinerFamilles();
    }
  } catch (e) {
    remonter(e, "recalcul de la carte");
    $("carte-parametres-etat").textContent = String(e);
  } finally {
    bouton.disabled = false;
  }
});

// Régénère les tuiles vectorielles (module `carto`), et rouvre MapLibre sur
// le résultat. Bascule automatiquement sur le plan de ville réel si
// `ville-paris.db` existe (`main.rs::engendrer_tuiles`) : rien à choisir ici.
//
// **Aucun autre bouton n'appelait `engendrer_tuiles`** avant celui-ci — la
// commande existait côté Rust, mais rien côté interface ne la déclenchait ;
// la carte affichée restait celle de la dernière génération, quel que soit
// le plan de ville importé depuis.
$("regenerer-tuiles").addEventListener("click", async () => {
  const bouton = $("regenerer-tuiles");
  bouton.disabled = true;
  $("tuiles-etat").textContent = "Génération en cours…";
  try {
    const r = await invoke("engendrer_tuiles");
    $("tuiles-etat").textContent = r;
    // Les tuiles ont changé sous l'instance MapLibre en place : la relancer
    // à neuf plutôt que d'espérer qu'elle redemande ce qu'elle a déjà en
    // cache.
    if (carte.gl) {
      carte.gl.remove();
      carte.gl = null;
      carte.glPret = false;
    }
    if (navigation.mode === "explorer" && carte.affichage === "carte") {
      majAffichageGL();
    }
  } catch (e) {
    remonter(e, "génération des tuiles");
    $("tuiles-etat").textContent = String(e);
  } finally {
    bouton.disabled = false;
  }
});

/* ------------------------------------------------- paramètres de densité */

// Résolution, noyau, bandes : mêmes principe que les réglages de la carte
// juste au-dessus — un changement n'écrit que la valeur, prise en compte au
// prochain « Régénérer les tuiles » (pas de recalcul à la volée : le
// pipeline de tuiles la relit à chaque génération, voir `main.rs::rassembler`).
const CHAMPS_PARAMETRES_DENSITE = {
  "densite-resolution": "densite_resolution",
  "densite-noyau": "densite_noyau",
  "densite-bandes": "densite_bandes",
};

for (const [id, cle] of Object.entries(CHAMPS_PARAMETRES_DENSITE)) {
  $(id).addEventListener("change", async (e) => {
    const valeur = Number(e.target.value);
    if (!Number.isFinite(valeur)) return;
    try {
      await invoke("set_map_parameter", { cle, valeur });
    } catch (err) {
      remonter(err, "paramètre de densité");
    }
  });
}

// Les deux curseurs affichent leur valeur au fil du geste, sans attendre le
// commit — la résolution (une liste) n'en a pas besoin.
$("densite-noyau").addEventListener("input", (e) => {
  $("densite-noyau-valeur").textContent = Number(e.target.value).toFixed(3);
});
$("densite-bandes").addEventListener("input", (e) => {
  $("densite-bandes-valeur").textContent = e.target.value;
});


/* ---------------------------------------------------------- état
 *
 * En tête de Bibliothèque, une ligne par manque (`#etat`) : un compte, et la
 * liste qui se déplie sur place. Les comptes viennent des mêmes réponses que
 * les listes (`chargerVerifications`, `chargerStatsBibliotheque`, la recherche
 * de doublons) : rien de plus n'est demandé au moteur.
 */

const nombre = (n) => n.toLocaleString("fr-FR");
const pluriel = (n, un, plusieurs = `${un}s`) => `${nombre(n)} ${n > 1 ? plusieurs : un}`;
const genresEtat = { suspects: null, sansGenre: null };

/// Pose le compte d'une ligne de l'état ; `aVoir` : il y a quelque chose à
/// examiner (le compte passe en `--txt`).
function majEtat(cle, texte, aVoir) {
  const el = $(`etat-${cle}-compte`);
  if (!el) return;
  el.textContent = texte;
  el.classList.toggle("etat__compte--a-voir", aVoir);
}

function majEtatGenres() {
  const { suspects, sansGenre } = genresEtat;
  const bouts = [];
  if (suspects) bouts.push(pluriel(suspects, "suspect"));
  if (sansGenre) bouts.push(`${nombre(sansGenre)} sans genre`);
  const connu = suspects !== null || sansGenre !== null;
  majEtat("genres", bouts.join(" · ") || (connu ? "rien à signaler" : "…"), bouts.length > 0);
}

/// Genres suspects et éditions multiples : peu coûteux (une requête
/// d'agrégation), rejoués à chaque entrée dans le mode comme le reste des
/// statistiques.
async function chargerVerifications() {
  const suspects = await invoke("suspect_genres");
  genresEtat.suspects = suspects.length;
  majEtatGenres();
  dessinerListeVerif(
    "genres-suspects",
    suspects.map(([, ligne, genre, dominants]) => ({
      ligne,
      detail: `Genre : ${genre} · Famille : ${dominants}`,
    })),
  );

  const editions = await invoke("multiple_editions");
  majEtat("editions", editions.length ? pluriel(editions.length, "album") : "aucune", editions.length > 0);
  dessinerListeVerif(
    "editions-multiples",
    editions.map(([artiste, titre, versions]) => ({
      ligne: `${isoler(artiste)} — ${isoler(titre)}`,
      detail: versions.map(([album, n]) => `${album} (${n})`).join(" · "),
    })),
  );

  const pochettes = await invoke("pochettes_suspectes");
  majEtat("pochettes", pochettes.length ? pluriel(pochettes.length, "à examiner", "à examiner") : "aucune", pochettes.length > 0);
  $("pochettes-suspectes-vide").textContent = pochettes.length
    ? `${pochettes.length.toLocaleString("fr-FR")} pochette${pochettes.length > 1 ? "s" : ""} à examiner.`
    : "Aucune pochette suspecte.";
  dessinerListeVerif(
    "pochettes-suspectes",
    pochettes.map((p) => {
      const actions = document.createElement("span");
      actions.className = "verif__actions";
      // Image intégrée fausse : afficher celle du dossier. Le choix est gardé
      // dans la base — corriger les tags ne tient pas si les MP3 sont
      // recopiés depuis leur source.
      if (p.anomalie === "divergente") {
        const dossier = document.createElement("button");
        dossier.className = "bouton";
        dossier.textContent = "Image du dossier";
        dossier.title = "Afficher l'image du dossier (cover.jpg…) au lieu de celle des tags";
        dossier.addEventListener("click", async () => {
          await invoke("pochette_du_dossier", { dossier: p.dossier });
          // Le cache de `socle.js`, pas la liste locale du même nom :
          // `pochettes.clear()` visait celle-ci et levait une TypeError.
          viderPochettes();
          await chargerVerifications();
        });
        actions.appendChild(dossier);
      }
      const bouton = document.createElement("button");
      bouton.className = "bouton";
      bouton.textContent = "Ignorer";
      bouton.addEventListener("click", async () => {
        await invoke("ignore_pochette_suspecte", { dossier: p.dossier });
        await chargerVerifications();
      });
      actions.appendChild(bouton);
      const nature = {
        illisible: "Image illisible",
        divergente: "Intégrée ≠ image du dossier",
        partagee: "Même image sur plusieurs albums",
      }[p.anomalie] ?? p.anomalie;
      const ecart = p.mesure == null ? "" : ` (écart ${p.mesure.toFixed(2).replace(".", ",")})`;
      return {
        ligne: `${isoler(txt(p.artiste, "?"))} — ${isoler(txt(p.album, p.dossier.split("/").pop()))}`,
        detail: `${nature}${ecart} — ${p.detail}`,
        action: actions,
      };
    }),
  );

  const echecs = await invoke("scan_failures");
  majEtat("echecs", echecs.length ? pluriel(echecs.length, "fichier") : "aucun", echecs.length > 0);
  $("echecs-scan-vide").textContent = echecs.length
    ? `${echecs.length.toLocaleString("fr-FR")} fichier${echecs.length > 1 ? "s" : ""} qu'un scan n'a pas su lire.`
    : "Aucun échec de scan connu.";
  dessinerListeVerif(
    "echecs-scan",
    echecs.map(([chemin, raison, at]) => {
      const bouton = document.createElement("button");
      bouton.className = "bouton bouton--danger";
      bouton.textContent = "Retirer";
      bouton.addEventListener("click", async () => {
        await invoke("dismiss_scan_failure", { path: chemin });
        await chargerVerifications();
      });
      return {
        ligne: chemin.split("/").pop(),
        detail: `${raison} — ${new Date(at * 1000).toLocaleDateString("fr-FR")}`,
        action: bouton,
      };
    }),
  );
}

/// Le texte d'une piste dans une liste de vérification : artiste — titre,
/// avec le nom de fichier en repli pour un morceau sans titre en tag.
function ligneTrack(t) {
  return `${isoler(txt(t.artist, "?"))} — ${isoler(txt(t.title, t.path.split("/").pop()))}`;
}

/// Doublons probables et points isolés partagent le même graphe des
/// voisins, coûteux à construire (~15 s sur toute la bibliothèque) mais
/// gardé en mémoire côté moteur — d'où le geste explicite plutôt qu'un
/// calcul silencieux à chaque entrée dans le mode.
$("chercher-doublons").addEventListener("click", async () => {
  const bouton = $("chercher-doublons");
  bouton.disabled = true;
  $("graphe-attente").textContent = "Calcul en cours — jusqu'à une trentaine de secondes la première fois…";
  try {
    const doublons = await invoke("probable_duplicates");
    dessinerListeVerif(
      "doublons-probables",
      doublons.map((d) => ({
        ligne: `${ligneTrack(d.a)}  ↔  ${ligneTrack(d.b)}`,
        mesure: `d² = ${d.distance2.toFixed(5)}`,
      })),
    );
    majEtat("doublons", doublons.length ? pluriel(doublons.length, "paire") : "aucun", doublons.length > 0);
    $("graphe-attente").textContent = doublons.length
      ? `${doublons.length.toLocaleString("fr-FR")} paire${doublons.length > 1 ? "s" : ""} probable${doublons.length > 1 ? "s" : ""}.`
      : "Aucun doublon probable trouvé.";

    const isoles = await invoke("isolated_points");
    dessinerListeVerif(
      "points-isoles",
      isoles.map((p) => ({
        ligne: ligneTrack(p.piste),
        detail: `Plus proche : ${ligneTrack(p.plus_proche)}`,
        mesure: `d² = ${p.distance2.toFixed(3)}`,
      })),
    );
  } catch (e) {
    remonter(e, "doublons et points isolés");
  } finally {
    bouton.disabled = false;
  }
});

/// `libeller(i)` nomme la tranche `i` pour l'infobulle — le texte diffère
/// entre tempo (BPM) et durée (m:ss), la forme des barres non.
function dessinerHistogramme(id, h, libeller) {
  const hote = $(id);
  hote.replaceChildren();
  const max = Math.max(1, ...h.comptes);
  h.comptes.forEach((compte, i) => {
    const col = document.createElement("div");
    col.className = "histogramme__colonne" + (compte === 0 ? " histogramme__colonne--vide" : "");
    col.style.height = `${Math.max(2, (compte / max) * 100)}%`;
    col.title = `${libeller(i)} : ${compte.toLocaleString("fr-FR")}`;
    hote.appendChild(col);
  });
}

function notesHistogramme(h, participes) {
  const bouts = [];
  if (h.hors_gamme) bouts.push(`${h.hors_gamme.toLocaleString("fr-FR")} au-delà de la dernière tranche`);
  if (h.sans_valeur) bouts.push(`${h.sans_valeur.toLocaleString("fr-FR")} jamais ${participes}`);
  return bouts.join(" · ");
}

async function dessinerRacines() {
  const racines = await invoke("roots");
  const hote = $("racines");
  hote.replaceChildren();
  // Avec une seule racine, « Tout rafraîchir » ferait la même chose que son
  // « Analyser » : le bouton n'apparaît qu'à partir de deux.
  $("tout-rafraichir").hidden = racines.length < 2;

  if (racines.length === 0) {
    hote.innerHTML = '<p class="file__vide">Aucun dossier surveillé.</p>';
    return;
  }

  for (const r of racines) {
    const el = document.createElement("div");
    el.className = "racine";
    el.innerHTML = `<span class="racine__chemin"></span>
                    <span class="racine__cpt"></span>
                    <button class="bouton racine__analyser">Analyser</button>
                    <button class="bouton bouton--danger">Oublier</button>`;
    el.children[0].textContent = r.path;
    el.children[1].textContent = `${r.tracks.toLocaleString("fr-FR")} morceaux`;
    el.children[2].addEventListener("click", () => {
      analyserRacine(r.path).catch((e) => remonter(e, "analyse de la racine"));
    });
    el.children[3].addEventListener("click", async () => {
      // Opération destructrice : elle emporte les morceaux de la racine.
      const ok = confirm(
        `Oublier ${r.path} ?\n\nLes ${r.tracks.toLocaleString("fr-FR")} morceaux ` +
          `qui en dépendent seront retirés de la bibliothèque. Les fichiers ne ` +
          `sont pas touchés.`,
      );
      if (!ok) return;
      await invoke("forget_root", { path: r.path });
      await dessinerRacines();
      await charger();
    });
    hote.appendChild(el);
  }
}

/// « Ajouter un dossier » : sélecteur natif — on ne demande pas à
/// l'utilisateur de connaître ses chemins — puis ajout et lancement
/// immédiats de toute la chaîne dessus ([`analyserRacine`] : scan, loudness,
/// empreintes, mesures, genres, popularité). Un seul geste, pas de champ à
/// remplir ni de second clic.
$("parcourir").addEventListener("click", async () => {
  const choisi = await window.__TAURI__.dialog.open({
    directory: true,
    multiple: false,
    title: "Choisir le dossier de musique",
  });
  if (!choisi) return;
  try {
    await analyserRacine(choisi);
  } catch (e) {
    remonter(e, "scan");
    $("scan-etat").textContent = String(e);
  }
});

/// « Tout rafraîchir » : rejoue la même chaîne que « Ajouter un dossier »/
/// « Analyser », mais sur toutes les racines déjà surveillées d'un coup
/// (`lancerChaineComplete`) — pour rattraper ce qui manque sans les
/// reparcourir une à une. Reprend où elle s'était arrêtée, comme les autres.
$("tout-rafraichir").addEventListener("click", async () => {
  if ((await invoke("roots")).length === 0) {
    $("scan-etat").textContent = "Ajoutez d'abord un dossier de musique.";
    return;
  }
  await lancerChaineComplete();
});

/* ---------------------------------------------------------- analyse */

/// 1,1 s/morceau, mesuré sur la carte SD — le stockage interne va bien plus
/// vite, l'estimation est donc un plafond. Sert aux durées annoncées pendant
/// la chaîne « Analyser » d'une racine ([`analyserRacine`]).
const SECONDES_PAR_MORCEAU = 1.1;

/// Jauge de progression d'une passe de fond (analyse, descripteurs, genres) —
/// le texte dit déjà le compte, la barre le donne à voir d'un coup d'œil
/// sans le lire. Cachée hors passe : une barre à 0 immobile ne dit rien
/// qu'une absence ne dise mieux.
function majJauge(id, enCours, faits, total) {
  const el = $(id);
  el.hidden = !enCours || !total;
  if (total) {
    el.max = total;
    el.value = faits;
  }
}

/// Durée en heures ou minutes, pour annoncer une passe qui dure.
function dureeLongue(s) {
  if (s < 90) return `${Math.round(s)} s`;
  if (s < 5400) return `${Math.round(s / 60)} min`;
  return `${(s / 3600).toFixed(1)} h`.replace(".", ",");
}

/// Nombre de morceaux de la bibliothèque, mis à jour par
/// `chargerStatsBibliotheque` — sert de base à `resumeChaine`, qui ne relance
/// pas de requête à chaque frappe dans le rail.
let dernierTotalBiblio = 0;

/// Ce que « Scanner »/« Analyser » va effectivement enchaîner, calculé à
/// partir des cases cochées, affiché juste au-dessus du bouton. Sans cette
/// ligne, la liste de réglages du rail (force, popularité, biographies,
/// critiques, Discogs, Last.fm…) ne dit rien de ce qui va tourner ni combien
/// de temps — on ne le découvrait qu'en cliquant. La durée reste un ordre de
/// grandeur : mêmes constantes que les « reste » affichés pendant chaque
/// passe (`SECONDES_PAR_MORCEAU`), et le coût Deezer mesuré de la popularité
/// (`docs/popularite.md`, ~7 requêtes/s, une par morceau, sans lot possible).
function resumeChaine() {
  const cible = $("resume-chaine");
  if (!cible) return;
  const contact = contactMb();
  const total = dernierTotalBiblio;
  const rafraichirPop = $("pop-rafraichir").checked;

  const etapes = ["scan", "pochettes", "loudness", "empreintes", "mesures"];
  // Les trois décodent le fichier (empreintes en fenêtres, loudness en
  // entier) ou en dérivent — même ordre de grandeur que la mesure retenue
  // pour les empreintes, faute de mieux pour la loudness à ce jour.
  let secondes = total * SECONDES_PAR_MORCEAU * 3;

  if (contact.includes("@")) etapes.push("genres");
  etapes.push(rafraichirPop ? "popularité (rafraîchie)" : "popularité");
  if (rafraichirPop) secondes += total / 7;

  const extras = [];
  if ($("bio-active").checked) extras.push("biographies");
  if ($("critiques-active").checked) extras.push("critiques");
  if ($("discogs-lier-active").checked && contact.includes("@")) extras.push("Discogs");
  if ($("lastfm-active").checked && $("lastfm-cle").value.trim()) extras.push("tags Last.fm");

  let texte = etapes.join(" → ");
  if (extras.length) texte += " + " + extras.join(" + ");
  if ($("analyse-force").checked) texte += " · refait aussi ce qui est déjà mesuré";
  if (total) {
    texte += ` — ≈ ${dureeLongue(secondes)}`;
    if (rafraichirPop) texte += " (popularité : Deezer, le plus long)";
  }
  cible.textContent = texte;
}
resumeChaine();
for (const id of [
  "analyse-force", "pop-rafraichir", "bio-active", "critiques-active",
  "discogs-lier-active", "lastfm-active",
]) {
  $(id).addEventListener("change", resumeChaine);
}
$("lastfm-cle").addEventListener("input", resumeChaine);
$("analyse-contact").addEventListener("change", resumeChaine);

/// Sonde `invoke(commande)` jusqu'à ce que `.en_cours` devienne faux, en
/// laissant `surProgres` mettre à jour l'affichage à chaque tour. Factorise
/// les passes qu'[`analyserRacine`] enchaîne — scan, empreintes,
/// descripteurs, genres —, qui partagent toutes la même forme d'état
/// (`en_cours`/`faits`/`total`/`resultat`).
function attendreFin(commande, pas, surProgres) {
  return new Promise((resolve, reject) => {
    const id = setInterval(async () => {
      let etat;
      try {
        etat = await invoke(commande);
      } catch (e) {
        clearInterval(id);
        reject(e);
        return;
      }
      surProgres(etat);
      if (!etat.en_cours) {
        clearInterval(id);
        resolve(etat);
      }
    }, pas);
  });
}

/// Le bouton « Analyser » d'une racine, tout ce que la bibliothèque sait
/// faire sur ses fichiers, enchaîné : scan (relit tags/format/débit),
/// empreintes CLAP, tempo/tonalité/énergie, genres MusicBrainz si une adresse
/// de contact est renseignée, puis popularité générale (ListenBrainz +
/// Deezer, sans clé). Un seul bouton par racine plutôt que cinq sections
/// séparées à faire tourner soi-même dans l'ordre — c'est toujours le même
/// ordre, autant l'écrire une fois.
///
/// Les passes restent globales côté moteur (pas de filtre par racine) : dans
/// l'usage courant — une racine qu'on vient d'ajouter ou de changer — ce qui
/// est « en attente » est justement ce qu'on vient de scanner, donc le
/// résultat correspond à l'intention même sans filtrage explicite.
async function analyserRacine(chemin) {
  verrouillerActualisation(true);
  const force = $("analyse-force").checked;
  const contact = contactMb();
  const etat = $("scan-etat");
  const jauge = $("scan-jauge");
  try {
    etat.textContent = `${chemin} — scan…`;
    jauge.hidden = true;
    await invoke("start_scan", { path: chemin, force });
    await attendreFin("scan_state", 1000, (s) => {
      etat.textContent = s.en_cours
        ? `${chemin} — scan : ${s.morceaux.toLocaleString("fr-FR")} morceaux en base`
        : (etat.textContent = s.resultat ?? "");
    });

    etat.textContent = `${chemin} — pochettes…`;
    await invoke("start_verif_pochettes", { force });
    await attendreFin("verif_pochettes_state", 1000, (v) => {
      majJauge("scan-jauge", v.en_cours, v.faits, v.total);
      if (v.en_cours && v.total) {
        etat.textContent = `${chemin} — pochettes : ${v.faits.toLocaleString("fr-FR")} / ${v.total.toLocaleString("fr-FR")}`;
      }
    });

    etat.textContent = `${chemin} — loudness…`;
    await invoke("start_loudness", { force });
    await attendreFin("loudness_state", 2000, (l) => {
      majJauge("scan-jauge", l.en_cours, l.faits, l.total);
      if (l.en_cours) {
        etat.textContent = l.total
          ? `${chemin} — loudness : ${l.faits.toLocaleString("fr-FR")} / ${l.total.toLocaleString("fr-FR")}`
          : `${chemin} — loudness…`;
      }
    });

    etat.textContent = `${chemin} — empreintes…`;
    await invoke("start_analysis");
    await attendreFin("analysis_state", 2000, (a) => {
      majJauge("scan-jauge", a.en_cours, a.faits, a.total);
      if (a.en_cours) {
        const reste = Math.max(0, a.total - a.faits);
        etat.textContent = a.total
          ? `${chemin} — empreintes : ${a.faits.toLocaleString("fr-FR")} / ${a.total.toLocaleString("fr-FR")} — reste ${dureeLongue(reste * SECONDES_PAR_MORCEAU)}`
          : `${chemin} — empreintes…`;
      }
    });
    // La projection a replacé tous les points : la carte affichée est
    // périmée, y compris ses familles et le filtre par famille de l'Écoute.
    if (navigation.mode === "explorer") await chargerCarte();
    await familleARecalculee();

    etat.textContent = `${chemin} — tempo, tonalité, énergie…`;
    await invoke("start_descripteurs", { force });
    await attendreFin("descripteurs_state", 2000, (d) => {
      majJauge("scan-jauge", d.en_cours, d.faits, d.total);
      if (d.en_cours) {
        etat.textContent = d.total
          ? `${chemin} — mesures : ${d.faits.toLocaleString("fr-FR")} / ${d.total.toLocaleString("fr-FR")}`
          : `${chemin} — mesures…`;
      }
    });

    if (contact.includes("@")) {
      etat.textContent = `${chemin} — genres…`;
      await invoke("start_enrichment", { contact });
      await attendreFin("enrichment_state", 3000, (e) => {
        majJauge("scan-jauge", e.en_cours, e.artistes, e.total);
        if (e.en_cours) {
          const reste = Math.max(0, e.total - e.artistes);
          etat.textContent = e.total
            ? `${chemin} — genres : ${e.artistes.toLocaleString("fr-FR")} / ${e.total.toLocaleString("fr-FR")} artistes — reste ${dureeLongue(reste * SECONDES_PAR_ARTISTE)}`
            : `${chemin} — genres…`;
        }
      });
      // Nommées à la volée : il suffit de les redemander. Les numéros de
      // famille ne bougent pas (le clustering est intact), le filtre par
      // famille de l'Écoute reste donc valide — on ne rafraîchit que les noms.
      carte.familles = null;
      carte.familleVotes = null;
      ecoute.famillesParAlbum = null;
      await chargerFamillesParAlbum();
      if (navigation.mode === "explorer") await dessinerFamilles();
      else if (navigation.mode === "ecoute") {
        await dessinerFamillesEcoute();
        rafraichirGrille();
      }
    }

    // Popularité générale (ListenBrainz + Deezer). Sans condition d'adresse :
    // les deux API sont publiques ; le contact ne sert qu'au User-Agent.
    etat.textContent = `${chemin} — popularité…`;
    await invoke("start_popularite", { contact, rafraichir: $("pop-rafraichir").checked });
    await attendreFin("popularite_state", 3000, (p) => {
      majJauge("scan-jauge", p.en_cours, p.faits, p.total);
      if (p.en_cours) {
        etat.textContent = p.total
          ? `${chemin} — popularité : ${p.faits.toLocaleString("fr-FR")} / ${p.total.toLocaleString("fr-FR")}`
          : `${chemin} — popularité…`;
      }
    });
    popARecalculee();
    await chargerPopulariteFraicheur().catch((e) => remonter(e, "popularité"));

    if ($("bio-active").checked) {
      etat.textContent = `${chemin} — biographies…`;
      await invoke("start_biographies", { cle: null });
      await attendreFin("biographies_state", 2000, (b) => majJauge("scan-jauge", b.en_cours, b.faits, b.total));
    }
    if ($("critiques-active").checked) {
      etat.textContent = `${chemin} — critiques…`;
      await invoke("start_critiques", { rafraichir: false });
      await attendreFin("critiques_state", 2000, (c) => majJauge("scan-jauge", c.en_cours, c.faits, c.total));
    }
    if ($("discogs-lier-active").checked && contact.includes("@")) {
      etat.textContent = `${chemin} — liaison Discogs…`;
      await invoke("start_discogs_liaison", { contact });
      await attendreFin("discogs_liaison_state", 2000, (d) => majJauge("scan-jauge", d.en_cours, d.faits, d.total));
      await importerDiscogsSiUtile(`${chemin} — `);
    }
    const cleLastfm = $("lastfm-cle").value.trim();
    if ($("lastfm-active").checked && cleLastfm) {
      etat.textContent = `${chemin} — tags Last.fm…`;
      await invoke("start_lastfm", { cle: cleLastfm, rafraichir: false });
      await attendreFin("lastfm_state", 2000, (l) => majJauge("scan-jauge", l.en_cours, l.faits, l.total));
      carte.familleVotes = null; // le vote peut avoir changé
    }

    jauge.hidden = true;
    etat.textContent = `${chemin} — terminé.`;
    await dessinerRacines();
    await chargerStatsBibliotheque();
    await chargerVerifications();
  } catch (e) {
    remonter(e, "analyse de la racine");
    etat.textContent = String(e);
  } finally {
    verrouillerActualisation(false);
  }
}

/* ----------------------------------------- chaîne de passes de fond */

/// Les passes de fond, enchaînées par « Scanner » quand il n'y a pas de
/// dossier neuf à ajouter (`lancerChaineComplete`). Même moteur que la chaîne
/// « Analyser » d'une racine ([`analyserRacine`]), mais sur toutes les racines
/// à la fois. Les commandes sont globales côté moteur ; le scan, seul à
/// prendre une racine, est rejoué sur chacune à la suite.

/// Une ligne d'avancement dans le panneau : la barre (visible et graduée dès
/// qu'un total est connu, cachée sinon) et le texte, préfixé de l'étape quand
/// c'est la chaîne complète qui tourne (« Étape 2/4 — … »).
function avancementActu(phase, texte, faits, total) {
  const jauge = $("scan-jauge");
  jauge.hidden = !total;
  if (total) {
    jauge.max = total;
    jauge.value = faits;
  }
  $("scan-etat").textContent = phase + texte;
}

const pourcent = (a, b) => (b ? ` (${Math.round((a / b) * 100)} %)` : "");

async function passeScan(force, phase) {
  const racines = await invoke("roots");
  if (racines.length === 0) {
    $("scan-etat").textContent = phase + "aucun dossier surveillé.";
    return;
  }
  for (let i = 0; i < racines.length; i++) {
    const r = racines[i];
    const ou = racines.length > 1 ? ` (dossier ${i + 1}/${racines.length})` : "";
    avancementActu(phase, `scan${ou} : démarrage…`, 0, 0);
    await invoke("start_scan", { path: r.path, force });
    await attendreFin("scan_state", 1000, (s) => {
      // Le scan n'annonce pas de total : le compte qui monte est la seule
      // mesure d'avancement possible, sans barre.
      if (s.en_cours) {
        avancementActu(phase, `scan${ou} : ${s.morceaux.toLocaleString("fr-FR")} morceaux vus`, 0, 0);
      }
    });
  }
  await charger();
}

async function passeEmpreintes(phase) {
  avancementActu(phase, "empreintes : démarrage…", 0, 0);
  await invoke("start_analysis");
  // Même cadence que `analyserRacine` pour ce même sondage — les deux
  // chemins ne doivent pas diverger sans raison.
  await attendreFin("analysis_state", 2000, (a) => {
    if (!a.en_cours) return;
    const reste = Math.max(0, a.total - a.faits);
    avancementActu(
      phase,
      a.total
        ? `empreintes : ${a.faits.toLocaleString("fr-FR")} / ${a.total.toLocaleString("fr-FR")}${pourcent(a.faits, a.total)} — reste ${dureeLongue(reste * SECONDES_PAR_MORCEAU)}`
        : "empreintes : démarrage…",
      a.faits,
      a.total,
    );
  });
  // La projection a replacé tous les points — mêmes suites qu'`analyserRacine`.
  if (navigation.mode === "explorer") await chargerCarte();
  await familleARecalculee();
}

async function passeDescripteurs(force, phase) {
  avancementActu(phase, "tempo, tonalité, énergie : démarrage…", 0, 0);
  await invoke("start_descripteurs", { force });
  // Même cadence que `analyserRacine` pour ce même sondage.
  await attendreFin("descripteurs_state", 2000, (d) => {
    if (!d.en_cours) return;
    const reste = Math.max(0, d.total - d.faits);
    avancementActu(
      phase,
      d.total
        ? `tempo, tonalité, énergie : ${d.faits.toLocaleString("fr-FR")} / ${d.total.toLocaleString("fr-FR")}${pourcent(d.faits, d.total)} — reste ${dureeLongue(reste * SECONDES_PAR_MORCEAU)}`
        : "tempo, tonalité, énergie : démarrage…",
      d.faits,
      d.total,
    );
  });
}

/// Loudness EBU R128 (piste + album), pour la normalisation de volume à la
/// lecture. Ne dépend d'aucune autre passe — pas besoin d'empreinte ni de
/// descripteur — donc placée tôt : si l'utilisateur écoute pendant qu'une
/// chaîne plus longue (genres, popularité) tourne encore, la normalisation
/// est déjà disponible.
async function passeLoudness(force, phase) {
  avancementActu(phase, "loudness : démarrage…", 0, 0);
  await invoke("start_loudness", { force });
  await attendreFin("loudness_state", 2000, (l) => {
    if (!l.en_cours) return;
    const reste = Math.max(0, l.total - l.faits);
    avancementActu(
      phase,
      l.total
        ? `loudness : ${l.faits.toLocaleString("fr-FR")} / ${l.total.toLocaleString("fr-FR")}${pourcent(l.faits, l.total)}`
        : "loudness : démarrage…",
      l.faits,
      l.total,
    );
  });
}

/// Vérification des pochettes (image illisible, intégrée ≠ `cover.jpg`, même
/// image sur des albums différents). Placée juste après le scan : c'est lui qui
/// amène les fichiers neufs, et la passe est incrémentale (quelques secondes
/// quand rien n'a changé). Les résultats s'affichent dans « Vérifications ».
async function passePochettes(force, phase) {
  avancementActu(phase, "pochettes : démarrage…", 0, 0);
  await invoke("start_verif_pochettes", { force });
  await attendreFin("verif_pochettes_state", 1000, (v) => {
    if (!v.en_cours) return;
    avancementActu(
      phase,
      v.total
        ? `pochettes : ${v.faits.toLocaleString("fr-FR")} / ${v.total.toLocaleString("fr-FR")}${pourcent(v.faits, v.total)}`
        : "pochettes : démarrage…",
      v.faits,
      v.total,
    );
  });
}

async function passeGenres(contact, phase) {
  avancementActu(phase, "genres : démarrage…", 0, 0);
  await invoke("start_enrichment", { contact });
  await attendreFin("enrichment_state", 3000, (e) => {
    if (!e.en_cours) return;
    const reste = Math.max(0, e.total - e.artistes);
    avancementActu(
      phase,
      e.total
        ? `genres : ${e.artistes.toLocaleString("fr-FR")} / ${e.total.toLocaleString("fr-FR")} artistes${pourcent(e.artistes, e.total)} — reste ${dureeLongue(reste * SECONDES_PAR_ARTISTE)}`
        : "genres : démarrage…",
      e.artistes,
      e.total,
    );
  });
  carte.familles = null;
  carte.familleVotes = null;
  ecoute.famillesParAlbum = null;
  await chargerFamillesParAlbum();
  if (navigation.mode === "explorer") await dessinerFamilles();
  else if (navigation.mode === "ecoute") {
    await dessinerFamillesEcoute();
    rafraichirGrille();
  }
}

/// La popularité générale (ListenBrainz + Deezer). Ne dépend d'aucune clé ni
/// adresse : `contact` ne sert qu'au User-Agent de ListenBrainz. `rafraichir`
/// réinterroge aussi ce qui date de plus de 90 jours (case du rail).
async function passePopularite(contact, phase, rafraichir = false) {
  avancementActu(phase, "popularité : démarrage…", 0, 0);
  await invoke("start_popularite", { contact, rafraichir });
  await attendreFin("popularite_state", 3000, (p) => {
    if (!p.en_cours) return;
    avancementActu(
      phase,
      p.total
        ? `popularité : ${p.faits.toLocaleString("fr-FR")} / ${p.total.toLocaleString("fr-FR")}${pourcent(p.faits, p.total)}`
        : "popularité : démarrage…",
      p.faits,
      p.total,
    );
  });
  popARecalculee();
  chargerPopulariteFraicheur().catch((e) => remonter(e, "popularité"));
}

/// Biographies (TheAudioDB) — aucune clé requise, décochable indépendamment
/// (case « Biographies (TheAudioDB) » du rail). Résolution exclusivement par
/// MBID : un artiste sans MBID n'est jamais interrogé, voir
/// `docs/enrichissement-lecteur.md`.
async function passeBiographies(phase) {
  avancementActu(phase, "biographies : démarrage…", 0, 0);
  await invoke("start_biographies", { cle: null });
  await attendreFin("biographies_state", 2000, (b) => {
    if (!b.en_cours) return;
    avancementActu(
      phase,
      b.total
        ? `biographies : ${b.faits.toLocaleString("fr-FR")} / ${b.total.toLocaleString("fr-FR")}${pourcent(b.faits, b.total)}`
        : "biographies : démarrage…",
      b.faits,
      b.total,
    );
  });
}

/// Critiques (CritiqueBrainz) — aucune clé requise, décochable indépendamment
/// (case « Critiques (CritiqueBrainz) » du rail).
async function passeCritiques(phase) {
  avancementActu(phase, "critiques : démarrage…", 0, 0);
  await invoke("start_critiques", { rafraichir: false });
  await attendreFin("critiques_state", 2000, (c) => {
    if (!c.en_cours) return;
    avancementActu(
      phase,
      c.total
        ? `critiques : ${c.faits.toLocaleString("fr-FR")} / ${c.total.toLocaleString("fr-FR")}${pourcent(c.faits, c.total)}`
        : "critiques : démarrage…",
      c.faits,
      c.total,
    );
  });
}

/// Liaison Discogs (léger : retrouve l'édition Discogs de chaque édition
/// MusicBrainz connue) — décochable indépendamment (case « Liaison Discogs »
/// du rail). Demande un contact MusicBrainz, comme les genres ; sautée sans
/// adresse plutôt que de faire échouer la chaîne.
async function passeDiscogsLiaison(contact, phase) {
  avancementActu(phase, "liaison Discogs : démarrage…", 0, 0);
  await invoke("start_discogs_liaison", { contact });
  await attendreFin("discogs_liaison_state", 2000, (d) => {
    if (!d.en_cours) return;
    avancementActu(
      phase,
      d.total
        ? `liaison Discogs : ${d.faits.toLocaleString("fr-FR")} / ${d.total.toLocaleString("fr-FR")}${pourcent(d.faits, d.total)}`
        : "liaison Discogs : démarrage…",
      d.faits,
      d.total,
    );
  });
}

/// Importe aussitôt crédits et labels si le dump est déjà sur le disque et
/// qu'une édition reliée n'a encore jamais été importée
/// (`discogs_import_utile`) — pour ne plus exiger un second clic sur
/// « Importer crédits et labels » après une liaison.
///
/// **Le critère n'est jamais « cette liaison-ci a-t-elle relié du neuf »** :
/// une édition a pu être reliée par un passage précédent (avant ce
/// correctif, ou un import interrompu en cours de route) sans jamais avoir
/// été importée — se fier au seul bilan de la liaison qui vient de tourner
/// manquerait ce cas-là indéfiniment. `discogs_import_utile` interroge
/// directement ce qui reste à faire, peu importe quand ça a été relié.
///
/// Relit tout le dump (~10 min sur 11 Go, le format n'a pas d'index — pas de
/// raccourci possible) : ne se déclenche donc que s'il y a vraiment du neuf
/// à en tirer, jamais à chaque scan pour rien si tout est déjà importé.
/// Reste silencieuse (ni erreur ni changement d'état) si le dump n'a jamais
/// été téléchargé — l'import restera alors disponible à la main.
async function importerDiscogsSiUtile(phase) {
  let utile;
  let info;
  try {
    [utile, info] = await Promise.all([invoke("discogs_import_utile"), invoke("discogs_dump_info")]);
  } catch {
    return;
  }
  if (!utile || !info) return;

  avancementActu(phase, "import Discogs (crédits + labels) : démarrage…", 0, 0);
  await invoke("start_discogs_import");
  await attendreFin("discogs_import_state", 1000, (i) => {
    if (!i.en_cours) return;
    avancementActu(
      phase,
      `import Discogs : ${i.editions_vues.toLocaleString("fr-FR")} éditions parcourues · ` +
        `${i.editions_retenues.toLocaleString("fr-FR")} retenues…`,
      0,
      0,
    );
  });
}

/// Tags de genre Last.fm — vote de nommage des familles
/// (`docs/nommage-familles.md`), pas la popularité. Décochable indépendamment
/// (case « Tags de genre (Last.fm) » du rail) ; contrairement aux trois
/// autres sources, exige une clé personnelle — appelant garant qu'elle est
/// présente (voir `lancerChaineComplete`).
async function passeLastfm(cle, phase) {
  avancementActu(phase, "tags Last.fm : démarrage…", 0, 0);
  await invoke("start_lastfm", { cle, rafraichir: false });
  await attendreFin("lastfm_state", 2000, (l) => {
    if (!l.en_cours) return;
    avancementActu(
      phase,
      l.total
        ? `tags Last.fm : ${l.faits.toLocaleString("fr-FR")} / ${l.total.toLocaleString("fr-FR")}${pourcent(l.faits, l.total)}`
        : "tags Last.fm : démarrage…",
      l.faits,
      l.total,
    );
  });
  carte.familleVotes = null; // le vote peut avoir changé
}

/// La chaîne complète — scan, pochettes, loudness, empreintes, tempo/tonalité/énergie, genres,
/// popularité, puis biographies/critiques/liaison Discogs si cochées — sur
/// toutes les racines surveillées, l'étape en cours affichée en clair. C'est
/// ce que « Scanner » lance quand aucun dossier neuf n'est saisi ; chaque
/// passe reprend où elle s'était arrêtée.
async function lancerChaineComplete() {
  const force = $("analyse-force").checked;
  const contact = contactMb();
  const etat = $("scan-etat");
  verrouillerActualisation(true);
  try {
    await passeScan(force, "Étape 1/7 — ");
    await passePochettes(force, "Étape 2/7 — ");
    await passeLoudness(force, "Étape 3/7 — ");
    await passeEmpreintes("Étape 4/7 — ");
    await passeDescripteurs(force, "Étape 5/7 — ");
    if (contact.includes("@")) await passeGenres(contact, "Étape 6/7 — ");
    else etat.textContent = "Étape 6/7 — genres sautés (pas d'adresse de contact MusicBrainz)";
    await passePopularite(contact, "Étape 7/7 — ", $("pop-rafraichir").checked);
    if ($("bio-active").checked) await passeBiographies("Biographies — ");
    if ($("critiques-active").checked) await passeCritiques("Critiques — ");
    if ($("discogs-lier-active").checked && contact.includes("@")) {
      await passeDiscogsLiaison(contact, "Liaison Discogs — ");
      await importerDiscogsSiUtile("Liaison Discogs — ");
    }
    const cleLastfm = $("lastfm-cle").value.trim();
    if ($("lastfm-active").checked && cleLastfm) {
      await passeLastfm(cleLastfm, "Tags Last.fm — ");
    }
    etat.textContent = `${etat.textContent} — terminé.`;
  } catch (e) {
    remonter(e, "actualisation");
    etat.textContent = String(e);
  } finally {
    $("scan-jauge").hidden = true;
    $("scan-jauge").value = 0;
    verrouillerActualisation(false);
    await dessinerRacines();
    await chargerStatsBibliotheque();
    await chargerVerifications();
  }
}

/// Une passe ne tourne qu'à un exemplaire côté moteur : tant qu'elle est là,
/// on grise tout ce qui pourrait en relancer une — « Scanner » comme les
/// « Analyser » des racines.
function verrouillerActualisation(occupe) {
  document
    .querySelectorAll("#parcourir, #tout-rafraichir, .racine__analyser")
    .forEach((b) => (b.disabled = occupe));
}

/// Une passe lancée avant de quitter le mode continue de tourner : en
/// revenant dans Bibliothèque, on raccroche l'affichage à son état plutôt que
/// de laisser la ligne muette pendant qu'elle avance.
async function reprendreActualisationEnCours() {
  const sondes = [
    ["scan_state", "scan"],
    ["verif_pochettes_state", "pochettes"],
    ["loudness_state", "loudness"],
    ["analysis_state", "empreintes"],
    ["descripteurs_state", "tempo, tonalité, énergie"],
    ["enrichment_state", "genres"],
    ["popularite_state", "popularité"],
    ["biographies_state", "biographies"],
    ["critiques_state", "critiques"],
    ["discogs_liaison_state", "liaison Discogs"],
    ["discogs_import_state", "import Discogs"],
  ];
  for (const [cmd, nom] of sondes) {
    let s;
    try {
      s = await invoke(cmd);
    } catch {
      continue;
    }
    if (!s.en_cours) continue;
    verrouillerActualisation(true);
    attendreFin(cmd, 2000, (e) => {
      if (!e.en_cours) {
        $("scan-etat").textContent = `${nom} — terminé.`;
        return;
      }
      // `discogs_import_state` n'a ni `faits` ni `total` connu à l'avance
      // (le nombre d'éditions à parcourir dans le dump ne se sait qu'en le
      // faisant) — `editions_vues` sert alors de seul repère, comme pour
      // `EtatScan::morceaux`.
      const faits = e.faits ?? e.artistes ?? e.editions_vues ?? 0;
      avancementActu(
        "",
        e.total
          ? `${nom} : ${faits.toLocaleString("fr-FR")} / ${e.total.toLocaleString("fr-FR")}${pourcent(faits, e.total)}`
          : `${nom} : ${faits.toLocaleString("fr-FR")}…`,
        faits,
        e.total,
      );
    })
      .catch((err) => remonter(err, "actualisation"))
      .finally(async () => {
        $("scan-jauge").hidden = true;
        verrouillerActualisation(false);
        if (cmd === "popularite_state") {
          popARecalculee();
          chargerPopulariteFraicheur().catch((e) => remonter(e, "popularité"));
        }
        await dessinerRacines();
        await chargerStatsBibliotheque();
      });
    return;
  }
}

/* --------------------------------------------- stockage & cache */

/// Le dossier où l'application écrit tout — montré tel quel, l'utilisateur doit
/// pouvoir le retrouver dans son gestionnaire de fichiers, le sauvegarder ou le
/// purger.
async function chargerDossierDonnees() {
  $("dossier-donnees").textContent = await invoke("dossier_donnees");
}

/// Ce que le logiciel lui-même pèse — pas ce qu'il produit (stems, HD, plus
/// haut dans le rail), mais ce qu'il faut garder sur le disque pour qu'il
/// tourne : l'exécutable, les modèles ML, les bases nécessaires (la
/// bibliothèque, le plan de ville s'il est importé), et le dump Discogs s'il
/// en reste un — ≈ 11 Go à lui seul, sans quoi le total sous-estimerait
/// grossièrement la trace réelle du logiciel.
async function majStockageLogiciel() {
  const [s, dump] = await Promise.all([
    invoke("stockage_logiciel"),
    invoke("discogs_dump_info"),
  ]);
  const go = (o) => (o / 1e9).toFixed(2).replace(".", ",");
  const bases = s.ville
    ? `${go(s.base + s.ville)} Go (bibliothèque + plan de ville)`
    : `${go(s.base)} Go (bibliothèque)`;
  const bouts = [
    `Logiciel : ${go(s.executable + s.modeles)} Go (exécutable + modèles)`,
    `Bases nécessaires : ${bases}`,
  ];
  if (dump) bouts.push(`Dump Discogs conservé : ${go(dump.octets)} Go`);
  $("stockage-logiciel").textContent = `${bouts.join(". ")}.`;
}

/// Dump Discogs (`releases.xml.gz`) — affiche la taille et la date du
/// fichier déjà sur le disque, ou reprend le suivi d'un téléchargement
/// encore en cours (rechargement de page, changement de mode pendant qu'il
/// tournait). L'import qui en tire crédits et labels n'a pas de bouton ici :
/// il fait partie de la chaîne « Scanner » (case « Discogs » du rail,
/// `importerDiscogsSiUtile`) — un seul bouton pour tout, pas deux à
/// retenir.
async function chargerDumpDiscogs() {
  let dl;
  try {
    dl = await invoke("discogs_dump_state");
  } catch (e) {
    remonter(e, "dump Discogs");
    return;
  }
  if (dl.en_cours) {
    suivreTelechargementDump();
    return;
  }
  await afficherInfoDumpDiscogs();
}

/// La ligne d'état statique : taille et date du dernier téléchargement
/// réussi, ou l'absence de dump — jamais de valeur inventée.
async function afficherInfoDumpDiscogs() {
  let info;
  try {
    info = await invoke("discogs_dump_info");
  } catch (e) {
    remonter(e, "dump Discogs");
    return;
  }
  const el = $("discogs-dump-etat");
  if (!info) {
    el.textContent = "Aucun dump téléchargé pour l'instant.";
    return;
  }
  const go = (info.octets / 1e9).toFixed(1).replace(".", ",");
  el.textContent =
    `${go} Go, téléchargé ${depuisTexte(info.telecharge_le)} ` +
    `(${new Date(info.telecharge_le * 1000).toLocaleDateString("fr-FR")}).`;
}

/// Sonde `discogs_dump_state` jusqu'à la fin, jauge et texte d'avancement en
/// octets — même patron que `passeDescripteurs`, avec ses propres éléments
/// plutôt que la jauge partagée de la chaîne « Scanner ».
async function suivreTelechargementDump() {
  $("discogs-dump-telecharger").disabled = true;
  $("discogs-dump-etat").textContent = "Téléchargement en cours…";

  let fin;
  try {
    fin = await attendreFin("discogs_dump_state", 1000, (d) => {
      majJauge("discogs-dump-jauge", d.en_cours, d.octets, d.total ?? 0);
      if (!d.en_cours) {
        $("discogs-dump-progres").textContent = "";
        return;
      }
      const vu = (d.octets / 1e9).toFixed(1).replace(".", ",");
      $("discogs-dump-progres").textContent = d.total
        ? `${vu} / ${(d.total / 1e9).toFixed(1).replace(".", ",")} Go${pourcent(d.octets, d.total)}`
        : `${vu} Go…`;
    });
  } catch (e) {
    remonter(e, "téléchargement du dump Discogs");
    fin = null;
  } finally {
    $("discogs-dump-jauge").hidden = true;
    $("discogs-dump-telecharger").disabled = false;
  }

  // Un échec est rapporté dans `resultat`, pas par un rejet de promesse (le
  // fil de fond attrape ses propres erreurs) — direct plutôt que masqué par
  // la ligne d'état habituelle, sinon l'échec disparaît derrière « aucun
  // dump téléchargé ».
  if (fin?.resultat?.startsWith("échec")) {
    $("discogs-dump-etat").textContent = fin.resultat;
    return;
  }
  await afficherInfoDumpDiscogs();
}

$("discogs-dump-telecharger").addEventListener("click", async () => {
  try {
    await invoke("start_discogs_dump");
  } catch (e) {
    remonter(e, "téléchargement du dump Discogs");
    return;
  }
  suivreTelechargementDump();
});

/// Ce que l'audio dérivé occupe sur le disque — stems démixés et rendus HD.
///
/// Un jeu de quatre stems pèse 124 Mo, un cache HD grossit d'un morceau à
/// l'autre : sans ce compte, c'est une fuite qu'on ne découvre qu'en cherchant
/// pourquoi le disque se remplit.
async function majCache() {
  const [[oStems, nStems], [oHd, nHd]] = await Promise.all([
    invoke("stems_cache"),
    invoke("superres_cache"),
  ]);
  const octets = oStems + oHd;
  $("vider-cache").disabled = octets === 0;
  if (octets === 0) {
    $("cache-taille").textContent = "Aucun audio dérivé en cache pour l'instant.";
    return;
  }
  const bouts = [];
  if (nStems) bouts.push(`${nStems} morceau${nStems > 1 ? "x" : ""} démixé${nStems > 1 ? "s" : ""}`);
  if (nHd) bouts.push(`${nHd} rendu${nHd > 1 ? "s" : ""} HD`);
  $("cache-taille").textContent =
    `${bouts.join(" · ")} — ${(octets / 1e9).toFixed(2).replace(".", ",")} Go. ` +
    `Les vider force à refaire ces rendus, rien d'autre n'est perdu.`;
}

$("vider-cache").addEventListener("click", async () => {
  // Même garde-fou que « Oublier » une racine : l'action force à refaire des
  // rendus coûteux (démixage, super-résolution), même si aucun morceau n'est
  // perdu.
  const ok = confirm(
    "Vider le cache d'audio dérivé ?\n\nLes stems démixés et les rendus HD " +
      "seront supprimés ; ils devront être refaits à la demande (une " +
      "trentaine de secondes par morceau démixé, davantage en HD). Aucun " +
      "fichier de la bibliothèque n'est touché.",
  );
  if (!ok) return;
  $("vider-cache").disabled = true;
  try {
    await invoke("stems_cache_vider");
    await invoke("vider_cache_hd");
  } catch (e) {
    remonter(e, "vidage du cache");
  }
  await majCache();
});

/// Écrit ce qu'on entend dans un dossier choisi.
///
/// **Une seule sortie, et c'est un choix.** La spec en prévoyait trois — un
/// stem, la sélection, le mélange — mais mettre un stem en solo *est* la
/// sélection : un menu de plus n'aurait dit que ce que le dock montre déjà.
///
/// Le moteur refuse d'écrire sous une racine surveillée ; le message le dit et
/// nomme le dossier fautif, plutôt que de laisser un rendu être ingéré comme un
/// morceau à la surveillance suivante.
$("exporter").addEventListener("click", async () => {
  if (!edition.stems.length) return;
  const dossier = await window.__TAURI__.dialog.open({
    directory: true,
    multiple: false,
    title: "Où écrire le rendu",
  });
  if (!dossier) return;

  // Le nom dit ce qu'on entend : le morceau, ce qui est isolé, et les réglages
  // qui ne sont pas neutres. Sans cela, deux rendus du même morceau seraient
  // indiscernables.
  const parts = [txt(edition.source?.title, "rendu")];
  if (edition.solo) parts.push(edition.solo);
  if (Math.abs(edition.vitesse - 1) > 1e-3) parts.push(`${Math.round(edition.vitesse * 100)}%`);
  if (edition.tonalite) parts.push(`${edition.tonalite > 0 ? "+" : ""}${edition.tonalite}`);
  // Un stem greffé ou réglé à part change le rendu autant que la vitesse
  // d'ensemble : le nom doit le porter, sinon deux exports diffèrent sans
  // qu'on sache en quoi.
  for (const st of edition.stems) {
    if (st.greffe) parts.push(`${st.nom} greffé`);
    else if (stemEcarte(st)) parts.push(`${st.nom} ${resumeStem(st)}`);
  }
  const nom = parts.join(" — ").replace(/[/\\:]/g, "-");

  $("exporter").disabled = true;
  $("dock-aide").textContent = "écriture…";
  try {
    const ecrit = await invoke("stems_exporter", {
      stems: edition.stems.map((s) => [s.nom, s.chemin]),
      niveaux: niveaux(),
      // Une valeur par stem : c'est le décalage entre eux qu'il faut rendre,
      // et il n'existe pas dans leur somme.
      vitesses: edition.stems.map(vitesseDe),
      demiTons: edition.stems.map(tonaliteDe),
      destination: dossier,
      nom,
    });
    $("dock-aide").textContent = `écrit : ${ecrit.split("/").pop()}`;
  } catch (e) {
    $("dock-aide").textContent = "";
    remonter(e, "export");
  }
  $("exporter").disabled = false;
});

/// Deux requêtes par artiste et une par seconde : c'est MusicBrainz qui
/// impose le rythme. Sert aux durées annoncées pendant la chaîne
/// « Analyser » d'une racine ([`analyserRacine`]).
///
/// L'adresse de contact n'est pas une formalité : MusicBrainz exige un agent
/// qui identifie l'application et donne un moyen de la joindre. Sans elle,
/// la passe de genres ne récolterait que des refus — `analyserRacine` la
/// saute plutôt que d'essayer.
const SECONDES_PAR_ARTISTE = 5;

/// Le transport du bas, quand ce sont les stems qui jouent.
///
/// Même barre, même minutage, mêmes commandes : seule la source change.
/// L'onde laisse la place au minutage — chaque stem a son spectrogramme dans
/// le dock, une onde du mélange n'apprendrait rien de plus.
let battementStemsEnVol = false;
async function battementStems() {
  // Même garde que `battement`, et pour la même raison : sans elle, cinq
  // sondages par seconde s'empilent sur le verrou des stems et tout ce qui
  // arrive après — un clic, par exemple — attend son tour.
  if (battementStemsEnVol) return;
  // Les stems ont pris le transport : l'aperçu d'un morceau sélectionné s'efface.
  if (transport.apercu) {
    transport.apercu = false;
    document.querySelector(".transport").classList.remove("transport--apercu");
  }
  battementStemsEnVol = true;
  let e;
  try {
    e = await invoke("stems_state");
  } finally {
    battementStemsEnVol = false;
  }
  // Plus rien de chargé côté moteur (une commande du lecteur ordinaire les a
  // coupés) : arrivés au bout, les stems attendent en pause, ils ne passent
  // pas par ici.
  if (!e.actif && !e.en_pause) {
    await arreterStems();
    dessinerStems();
    return;
  }
  if (Date.now() >= transport.ignorerEtatJusqua) {
    poserLecture(!e.en_pause);
  }
  reconcilierStems(e);
  lecture.stems.dureeMs = e.duree_ms;
  // Un saut de la position (déplacement, retour de boucle) ou l'arrêt : la
  // nouvelle écoute part de là.
  if (e.en_pause || Math.abs(e.position_ms - positionMoteur()) > 250) {
    lecture.stems.ancreMs = e.position_ms;
  }
  lecture.stems.latenceMs = e.latence_ms ?? 0;
  lecture.stems.positionMs = e.position_ms;
  lecture.stems.lueA = performance.now();
  lecture.stems.enPause = e.en_pause;
  accelererAuPassage(e.passages ?? 0);
  $("tc").textContent = `${horloge(e.position_ms)} / ${horloge(e.duree_ms)}`;
  lecture.stems.deriveMs = e.derive_ms;
  majDerive();
  $("np-titre").textContent = `${edition.stems.length} stems`;
  $("np-artiste").textContent = edition.source
    ? `${isoler(txt(edition.source.artist, "?"))} — ${isoler(txt(edition.source.title, "?"))}`
    : "";

  const frac = e.duree_ms ? Math.min(1, e.position_ms / e.duree_ms) : 0;
  peindreTransport(frac);
  // La même fraction pour tout le monde : barre du bas et spectrogrammes
  // montrent le même instant parce qu'ils lisent la même valeur.
  poserTete(frac);
}

export {
  attendreFin, battementStems, chargerDossierDonnees, chargerDumpDiscogs, chargerParametresCarte,
  chargerStatsBibliotheque, chargerVerifications, chargerVocabulaireFamilles, dessinerListeVerif,
  dessinerRacines, dureeLongue, majCache, majStockageLogiciel, passePopularite,
  reprendreActualisationEnCours, SECONDES_PAR_ARTISTE, verrouillerActualisation,
};
