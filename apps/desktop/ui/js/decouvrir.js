import "./valise.js"; // l'ordre d'exécution : voir main.js
import {
  $, ecoute, invoke, navigation, ouvrirLien, remonter,
} from "./socle.js";
import {
  initiales, teinteNom,
} from "./vues.js";
import {
  majAutourArtiste, poser,
} from "./ecouter.js";

import {
  carte,
} from "./carte.js";
import { chargerFamilles, rendreFamilles } from "./familles.js";
import {
  basculerMode,
} from "./modes.js";
import {
  attendreFin, dessinerListeVerif, dureeLongue, SECONDES_PAR_ARTISTE,
} from "./bibliotheque.js";
import { demander } from "./ecrans.js";

/* ------------------------------------------------------ mode Découvrir */

/// Tous les artistes de la bibliothèque, chargés une fois à l'entrée dans
/// le mode et filtrés côté client à chaque frappe — un millier de lignes,
/// pas de quoi justifier une requête par lettre tapée. Seuls ceux qui ont
/// un identifiant MusicBrainz peuvent être cherchés : c'est la clé que
/// l'API demande, rien à faire sans elle.
let artistesDecouvrables = [];

async function chargerArtistesDecouvrir() {
  if (artistesDecouvrables.length === 0) {
    artistesDecouvrables = (await invoke("artists")).filter((a) => a.mbid);
  }
}

/// L'adresse de contact MusicBrainz : une seule valeur, exigée dans le
/// User-Agent pour toute requête à leur API. Les collaborations d'artiste (ce
/// mode) et la passe « Genres MusicBrainz » s'en servent ; elle se renseigne
/// dans Bibliothèque seulement (Règle 12 : pas de réglage d'application dans
/// le rail).
const CONTACT_MB_CLE = "mb-contact";
const CHAMPS_CONTACT_MB = ["analyse-contact"];

function contactMb() {
  return (localStorage.getItem(CONTACT_MB_CLE) || "").trim();
}

(function initContactMb() {
  // Reprise de l'ancienne clé, du temps où seul Découvrir gardait l'adresse
  // (et où le champ de Bibliothèque n'était pas persisté du tout).
  const ancien = localStorage.getItem("decouvrir-contact");
  if (ancien && !localStorage.getItem(CONTACT_MB_CLE)) {
    localStorage.setItem(CONTACT_MB_CLE, ancien.trim());
  }
  localStorage.removeItem("decouvrir-contact");

  const valeur = contactMb();
  for (const id of CHAMPS_CONTACT_MB) {
    const champ = $(id);
    if (!champ) continue;
    champ.value = valeur;
    champ.addEventListener("change", (e) => {
      const v = e.target.value.trim();
      localStorage.setItem(CONTACT_MB_CLE, v);
      for (const autre of CHAMPS_CONTACT_MB) {
        if (autre === id) continue;
        const c = $(autre);
        if (c) c.value = v;
      }
    });
  }
})();

$("decouvrir-recherche").addEventListener("input", (e) => {
  const q = e.target.value.trim().toLowerCase();
  if (!q) {
    dessinerListeVerif("decouvrir-suggestions", []);
    return;
  }
  const trouves = artistesDecouvrables
    .filter((a) => a.name.toLowerCase().includes(q))
    .slice(0, 20);
  dessinerListeVerif(
    "decouvrir-suggestions",
    trouves.map((a) => ({
      ligne: a.name,
      onClick: () => {
        $("decouvrir-recherche").value = "";
        dessinerListeVerif("decouvrir-suggestions", []);
        decouvrirFil = [];
        naviguerDecouvrir(a.mbid, a.name).catch((e) => remonter(e, "découvrir"));
      },
    })),
  );
});

/// La pile des artistes visités, du premier au centre courant — le fil
/// d'Ariane. Vidée à chaque nouvelle recherche : on ne mélange pas deux
/// explorations.
let decouvrirFil = [];

/// Affiche les collaborateurs de `mbid` au centre, sans toucher au fil —
/// c'est [`naviguerDecouvrir`] et le fil d'Ariane qui décident quand y
/// ajouter ou y revenir.
async function afficherArtisteDecouvrir(mbid, nom) {
  $("decouvrir-vide").hidden = true;
  $("decouvrir-centre").hidden = false;
  $("decouvrir-nom").textContent = nom;
  dessinerListeVerif("decouvrir-liens", []);
  $("decouvrir-etat").textContent = "Recherche des collaborations…";

  let liens;
  try {
    liens = await invoke("artist_links", { mbid, contact: decouvrirContact() });
  } catch (e) {
    $("decouvrir-etat").textContent = String(e);
    remonter(e, "découvrir");
    return;
  }
  $("decouvrir-etat").textContent = liens.length
    ? ""
    : "Aucune collaboration connue de MusicBrainz pour cet artiste.";

  dessinerListeVerif(
    "decouvrir-liens",
    liens.map(([dstMbid, dstNom, relation]) => {
      const item = {
        ligne: dstNom,
        detail: relation,
        onClick: () =>
          naviguerDecouvrir(dstMbid, dstNom).catch((e) => remonter(e, "découvrir")),
      };
      // Un collaborateur déjà dans la bibliothèque mène à ses albums, en
      // plus de pouvoir recentrer dessus — deux gestes, deux cibles : le
      // bouton doit donc arrêter le clic avant qu'il ne remonte à la ligne.
      const local = artistesDecouvrables.find((a) => a.mbid === dstMbid);
      if (local) {
        const bouton = document.createElement("button");
        bouton.className = "verif__albums";
        bouton.textContent = "Albums";
        bouton.addEventListener("click", async (e) => {
          e.stopPropagation();
          const albums = await invoke("albums", { artist: local.name, mbid: local.mbid });
          if (navigation.mode !== "ecoute") await basculerMode("ecoute");
          poser("albums", local.name, albums, ecoute.sommet);
          majAutourArtiste(local.name, local.mbid);
        });
        item.action = bouton;
      }
      return item;
    }),
  );
}

function decouvrirContact() {
  return contactMb();
}

/// Pousse `mbid` sur le fil d'Ariane et l'affiche au centre — le geste de
/// navigation normal, en cliquant un collaborateur.
async function naviguerDecouvrir(mbid, nom) {
  decouvrirFil.push({ mbid, nom });
  dessinerFilDecouvrir();
  await afficherArtisteDecouvrir(mbid, nom);
}

function dessinerFilDecouvrir() {
  const hote = $("decouvrir-fil");
  hote.replaceChildren();
  hote.hidden = decouvrirFil.length <= 1;
  decouvrirFil.forEach((a, i) => {
    if (i > 0) hote.appendChild(document.createTextNode(" › "));
    if (i === decouvrirFil.length - 1) {
      const span = document.createElement("span");
      span.style.color = "var(--txt)";
      span.textContent = a.nom;
      hote.appendChild(span);
    } else {
      const bouton = document.createElement("button");
      bouton.textContent = a.nom;
      bouton.addEventListener("click", async () => {
        // Revenir en arrière tronque le fil plutôt que d'y repousser une
        // entrée déjà là — sinon un aller-retour répété l'allongerait sans
        // fin pour les mêmes artistes.
        decouvrirFil = decouvrirFil.slice(0, i + 1);
        dessinerFilDecouvrir();
        await afficherArtisteDecouvrir(a.mbid, a.nom);
      });
      hote.appendChild(bouton);
    }
  });
}

/* ---------------------------------------- mode Découvrir : le fil d'actualité */

/// Au bout de combien d'heures sans passe on en relance une à l'ouverture du
/// mode. Douze heures : assez pour ne pas interroger MusicBrainz à chaque
/// aller-retour, assez peu pour que « récent » le reste.
const DECOUVRIR_PEREMPTION_H = 12;

/// Vrai tant qu'une passe tourne, pour ne pas en lancer deux.
let decouvrirEnCours = false;

/// La date (epoch s) de la dernière passe, retenue du dernier `decouvrir_feed`.
let decouvrirDernierePasse = null;

/// Un bouton stylé comme un lien, avec son action.
/// « À propos » du mode Bibliothèque : la version, et une ligne par modèle
/// de MODELES.md (nom, rôle, article, licences du code et des poids, liens).
/// Ne change pas pendant une session : dessiné une fois.
async function chargerAPropos() {
  const hote = $("a-propos-modeles");
  if (hote.childElementCount) return;
  const a = await invoke("a_propos");
  $("a-propos-version").textContent = `Rusty Music ${a.version}`;
  const div = (classe, texte) => {
    const d = document.createElement("div");
    d.className = classe;
    d.textContent = texte;
    return d;
  };
  for (const m of a.modeles) {
    const liens = div("credit__liens", null);
    for (const l of m.liens) {
      const b = boutonLien(l.texte, () => ouvrirLien(l.url));
      b.title = l.url;
      liens.append(b);
    }
    const fiche = div("credit", null);
    fiche.append(
      div("credit__nom", `${m.nom} — ${m.role}`),
      div("credit__detail", m.article),
      div("credit__licences", `code : ${m.licence_code} · poids : ${m.licence_poids}`),
      liens,
    );
    hote.append(fiche);
  }
}
// Liens écrits dans la page (code source, sources de données).
document.querySelectorAll(".a-propos__lien").forEach((b) =>
  b.addEventListener("click", () => ouvrirLien(b.dataset.url)));

function boutonLien(texte, action) {
  const b = document.createElement("button");
  b.className = "lien";
  b.textContent = texte;
  b.addEventListener("click", action);
  return b;
}

/// L'onglet ouvert dans le panneau central — mémorisé d'une visite à l'autre.
let decouvrirOnglet = "sorties";
try {
  decouvrirOnglet = localStorage.getItem("decouvrir-onglet") || "sorties";
} catch {
  /* stockage indisponible : on garde le défaut */
}

/// Pochettes du fil, par identifiant de release-group. Bornées de fait par le
/// nombre de sorties d'une passe (~60), pas d'éviction à prévoir. `null` =
/// pas de pochette connue (réponse définitive), pour ne pas la redemander à
/// chaque rendu. Une panne, elle, n'est pas mémorisée : le prochain rendu
/// redemande, comme le repli de `cover` côté Rust.
const pochettesCaa = new Map();
function pochetteDecouvrir({ rg_mbid: rgMbid, artiste_nom: artiste, titre }) {
  let p = pochettesCaa.get(rgMbid);
  if (!p) {
    p = invoke("decouvrir_pochette", { rgMbid, artiste, titre }).catch(() => {
      pochettesCaa.delete(rgMbid);
      return null;
    });
    pochettesCaa.set(rgMbid, p);
  }
  return p;
}

/// Photos des artistes voisins, par MBID : Deezer puis TheAudioDB côté Rust
/// (`decouvrir_photo_artiste`). Mêmes règles que les pochettes : `null` est une
/// réponse définitive et se mémorise, une panne non.
const photosArtistes = new Map();
function photoArtisteDecouvrir({ dst_mbid: mbid, dst_nom: nom }) {
  let p = photosArtistes.get(mbid);
  if (!p) {
    p = invoke("decouvrir_photo_artiste", { mbid, nom }).catch(() => {
      photosArtistes.delete(mbid);
      return null;
    });
    photosArtistes.set(mbid, p);
  }
  return p;
}

/// Charge l'image d'une carte à sa première entrée dans le viewport (avec 200 px
/// d'avance), pas à sa création : les cartes d'un onglet caché, ou tout en bas
/// d'une longue grille, ne lancent aucune requête réseau tant qu'on ne les voit
/// pas. `disconnect` à chaque rendu : les cartes remplacées ne s'y accumulent pas.
const chargeursPochettes = new WeakMap();
const observateurPochettes = new IntersectionObserver(
  (entrees) => {
    for (const e of entrees) {
      if (!e.isIntersecting) continue;
      observateurPochettes.unobserve(e.target);
      chargeursPochettes.get(e.target)?.();
    }
  },
  { rootMargin: "200px" },
);

/// URL d'une page Last.fm — les espaces en `+`, le reste encodé. Un album
/// inconnu y tombe sur une page « introuvable » avec une recherche : acceptable.
function lienLastfm(...segments) {
  const enc = (s) => encodeURIComponent(s).replace(/%20/g, "+");
  return "https://www.last.fm/music/" + segments.map(enc).join("/");
}

/// Ouvre la page Deezer d'un album (`titre` donné) ou d'un artiste, dans le
/// navigateur — c'est là qu'on l'écoute. Le lien est résolu côté Rust par la même
/// recherche que les pochettes ; si Deezer ne connaît pas ou ne répond pas, on
/// ouvre sa page de recherche plutôt que rien.
async function ouvrirDeezer(artiste, titre = null) {
  let url = null;
  try {
    url = await invoke("decouvrir_lien_deezer", { artiste, titre });
  } catch (e) {
    remonter(e, "lien Deezer");
  }
  ouvrirLien(
    url ?? `https://www.deezer.com/search/${encodeURIComponent(titre ? `${artiste} ${titre}` : artiste)}`,
  );
}

/// Entrée dans le mode : on affiche le fil tel qu'il est, puis on lance une
/// passe si la dernière est ancienne (et si une adresse de contact est là).
async function entrerDecouvrir() {
  await chargerFilDecouvrir();
  const passe = decouvrirDernierePasse;
  const vieux =
    passe === null || Date.now() / 1000 - passe > DECOUVRIR_PEREMPTION_H * 3600;
  if (vieux && contactMb().includes("@")) {
    await lancerPasseDecouvrir();
  }
}

async function chargerFilDecouvrir() {
  const fil = await invoke("decouvrir_feed");
  filDecouvrir = fil;
  decouvrirDernierePasse = fil.derniere_passe ?? null;

  $("decouvrir-fraicheur").textContent = fil.derniere_passe
    ? `Actualisé ${depuisTexte(fil.derniere_passe)}.`
    : "Jamais actualisé.";

  rendreFilDecouvrir();
}

/// Le dernier fil reçu de `decouvrir_feed`, gardé pour le re-rendre à chaque
/// changement du filtre par famille sans refaire la requête.
let filDecouvrir = null;

/// La famille sonique de l'artiste-ancre d'une sortie — celle inscrite dans le
/// fil ne bouge pas, seul le filtre change. Ensemble vide, ou familles pas
/// encore chargées : tout passe.
function sortiePasseFamille(s) {
  if (filtreFamillesDecouvrir.size === 0 || !decouverte.famillesParArtiste) return true;
  return filtreFamillesDecouvrir.has(decouverte.famillesParArtiste.get(s.artiste_mbid));
}

/// Un voisin passe si l'une de ses ancres (`src_mbids`) est dans une famille
/// cochée — il a pu être proposé par plusieurs artistes de familles différentes.
function voisinPasseFamille(v) {
  if (filtreFamillesDecouvrir.size === 0 || !decouverte.famillesParArtiste) return true;
  return (v.src_mbids ?? []).some((m) => filtreFamillesDecouvrir.has(decouverte.famillesParArtiste.get(m)));
}

/// Redessine les trois onglets. Le filtre par famille n'ôte rien : les cartes
/// hors filtre sont estompées (`carteDecouvrir`), la grille ne bouge pas. Les
/// compteurs, eux, comptent les cartes qui passent le filtre.
function rendreFilDecouvrir() {
  const fil = filDecouvrir;
  if (!fil) return;

  observateurPochettes.disconnect();
  rendreListeSorties("decouvrir-sorties", "decouvrir-vide-sorties", fil.sorties);
  rendreListeSorties("decouvrir-collabs", "decouvrir-vide-collabs", fil.collaborations);
  rendreListeVoisins(fil.voisins);

  const n = (id, v) => ($(id).textContent = v ? ` ${v}` : "");
  n("decouvrir-n-sorties", fil.sorties.filter(sortiePasseFamille).length);
  n("decouvrir-n-collabs", fil.collaborations.filter(sortiePasseFamille).length);
  n("decouvrir-n-voisins", fil.voisins.filter(voisinPasseFamille).length);

  const vide =
    fil.sorties.length === 0 && fil.collaborations.length === 0 && fil.voisins.length === 0;
  $("decouvrir-actus-vide").hidden = !vide;
  $("decouvrir-onglets").hidden = vide;
  poserOngletDecouvrir(decouvrirOnglet);

  const nouveaux =
    [...fil.sorties, ...fil.collaborations, ...fil.voisins].some((x) => !x.vu);
  $("decouvrir-vu-tout").hidden = !nouveaux;

  majBlocFamillesDecouvrir();
}

/* ------------------------------------------ filtre par famille — mode Découvrir
 *
 * Même légende que le mode Explorer (`rendreFamilles`), même multi-sélection
 * que le filtre de l'Écoute. Une sortie, une collaboration ou un voisin est
 * rangé dans la famille sonique de son artiste-ancre (celui de la
 * bibliothèque). Ensemble vide = tout est montré. N'a de sens qu'une fois la
 * carte calculée.
 */

const filtreFamillesDecouvrir = new Set();
// Ce que Découvrir partage avec les autres modes.
const decouverte = {
  famillesParArtiste: null, // Map mb_album_artist_id → cluster dominant
};

async function chargerFamillesParArtiste() {
  try {
    const paires = await invoke("artist_families");
    decouverte.famillesParArtiste = new Map(paires);
  } catch (e) {
    remonter(e, "familles des artistes");
    decouverte.famillesParArtiste = new Map();
  }
}

async function dessinerFamillesDecouvrir() {
  await chargerFamilles();
  rendreFamilles(
    $("familles-decouvrir"),
    (c) => filtreFamillesDecouvrir.has(c),
    (c) => {
      if (filtreFamillesDecouvrir.has(c)) filtreFamillesDecouvrir.delete(c);
      else filtreFamillesDecouvrir.add(c);
      dessinerFamillesDecouvrir();
      rendreFilDecouvrir();
    },
  );
  $("familles-decouvrir-tout").hidden = filtreFamillesDecouvrir.size === 0;
}

/// Affiche le bloc « Familles » de Découvrir quand il a un sens : familles
/// des artistes chargées, et carte déjà calculée.
function majBlocFamillesDecouvrir() {
  const utile =
    decouverte.famillesParArtiste &&
    decouverte.famillesParArtiste.size > 0 &&
    (carte.familles?.length ?? 0) > 0;
  demander("bloc-familles-decouvrir", utile);
}

$("familles-decouvrir-tout").addEventListener("click", () => {
  filtreFamillesDecouvrir.clear();
  dessinerFamillesDecouvrir();
  rendreFilDecouvrir();
});

function poserOngletDecouvrir(onglet) {
  decouvrirOnglet = onglet;
  try {
    localStorage.setItem("decouvrir-onglet", onglet);
  } catch {
    /* rien : la préférence ne survivra pas à la session, sans plus */
  }
  for (const b of $("decouvrir-onglets").children) {
    b.classList.toggle("segment--actif", b.dataset.onglet === onglet);
  }
  $("decouvrir-panneau-sorties").hidden = onglet !== "sorties";
  $("decouvrir-panneau-collabs").hidden = onglet !== "collabs";
  $("decouvrir-panneau-voisins").hidden = onglet !== "voisins";
}

$("decouvrir-onglets").addEventListener("click", (e) => {
  const b = e.target.closest("button[data-onglet]");
  if (b) poserOngletDecouvrir(b.dataset.onglet);
});

/// « il y a 3 jours », « il y a 5 h », « à l'instant » — à partir d'un epoch s.
function depuisTexte(epochS) {
  const s = Math.max(0, Math.round(Date.now() / 1000 - epochS));
  if (s < 3600) return "il y a moins d'une heure";
  if (s < 86400) return `il y a ${Math.round(s / 3600)} h`;
  const j = Math.round(s / 86400);
  return j <= 1 ? "hier" : `il y a ${j} jours`;
}

/// Nombre de jours écoulés depuis une date 'YYYY-MM-DD' (ou 'YYYY-MM', 'YYYY',
/// complétée au premier jour) ; `null` si elle manque ou est illisible.
function joursDepuisSortie(date) {
  if (!date) return null;
  const complet = date.length === 4 ? `${date}-01-01` : date.length === 7 ? `${date}-01` : date;
  const j = Math.round((Date.now() - new Date(complet).getTime()) / 86400000);
  return Number.isFinite(j) ? j : null;
}

/// « il y a N jours » à partir d'une date 'YYYY-MM-DD' partielle.
function ageSortie(date) {
  if (!date) return "";
  const j = joursDepuisSortie(date);
  if (j === null) return date;
  if (j <= 0) return "aujourd'hui";
  if (j === 1) return "hier";
  if (j < 31) return `il y a ${j} jours`;
  return date;
}

/// Ouvre l'explorateur de collaborations sur un artiste, depuis le fil, et
/// l'amène à l'écran : il est sous la grille, un clic sans effet visible
/// passerait pour un clic raté.
function explorerDepuisFil(mbid, nom) {
  decouvrirFil = [];
  naviguerDecouvrir(mbid, nom).catch((e) => remonter(e, "découvrir"));
  const reduit = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  $("decouvrir-centre").scrollIntoView({ behavior: reduit ? "auto" : "smooth", block: "start" });
}

/// Une carte du fil, sur le gabarit des cases d'album d'Écouter : pochette
/// carrée (image chargée à la demande, ou repli initiales teintées de
/// `initialesDe`), titre, une
/// ligne d'artiste, une ligne de méta, et au survol un bandeau de liens
/// étiquetés. Le clic sur la carte (ou Entrée) ouvre l'explorateur.
///
/// Hors filtre par famille (`estompe`), la carte reste là, à 25 % — un filtre
/// ne doit pas faire recouler la page (R2/R4 de `interface-guidelines.md`).
function carteDecouvrir({
  nouveau, estompe, pochette, photo, initialesDe, type, nom, sec, secTitre, meta, collab, liens, ouvrir,
}) {
  const el = document.createElement("div");
  el.className =
    "album album--decouvrir" +
    (nouveau ? " album--nouveau" : "") +
    (estompe ? " album--estompe" : "");
  el.tabIndex = 0;

  const image = document.createElement("div");
  image.className = "album__pochette";
  // Repli sans pochette : initiales sur aplat teinté, comme la tuile d'artiste
  // d'Écouter. Posé seulement quand on sait qu'il n'y a pas d'image — pendant
  // le chargement, on garde le placeholder rayé de `.album__pochette`.
  const initialesTeintees = () => {
    const mos = document.createElement("div");
    mos.className = "mosaique mosaique--vide";
    mos.style.setProperty("--teinte", teinteNom(initialesDe));
    mos.textContent = initiales(initialesDe);
    image.prepend(mos);
  };
  // `pochette` est un thunk, appelé à la première entrée dans le viewport.
  // `photo` : un portrait d'artiste (cadré vers le haut), pas une pochette.
  chargeursPochettes.set(image, () =>
    pochette().then((uri) => {
      if (!uri) return initialesTeintees();
      image.style.backgroundImage = `url("${uri}")`;
      if (photo) image.classList.add("album__pochette--photo");
    }),
  );
  observateurPochettes.observe(image);
  if (type) {
    const pastille = document.createElement("span");
    pastille.className = "album__type";
    pastille.textContent = type;
    image.appendChild(pastille);
  }
  const actions = document.createElement("div");
  actions.className = "album__actions";
  for (const [texte, action] of liens) {
    const b = document.createElement("button");
    b.className = "album__action";
    b.textContent = texte;
    // Une action peut être asynchrone (résolution d'un lien) : le bouton reste
    // désactivé le temps qu'elle aboutisse — pas de double clic.
    b.addEventListener("click", async (e) => {
      e.stopPropagation();
      b.disabled = true;
      try {
        await action();
      } finally {
        b.disabled = false;
      }
    });
    actions.appendChild(b);
  }
  image.appendChild(actions);
  el.appendChild(image);

  const ligne = (classe, texte, titre = texte) => {
    const d = document.createElement("div");
    d.className = classe;
    d.textContent = texte;
    d.title = titre;
    el.appendChild(d);
  };
  ligne("album__nom", nom);
  ligne("album__sec", sec, secTitre ?? sec);
  if (meta) ligne("album__meta", meta);
  if (collab) ligne("album__collab", `avec ${collab}`);

  el.addEventListener("click", ouvrir);
  el.addEventListener("keydown", (e) => {
    if (e.target === el && (e.key === "Enter" || e.key === " ")) {
      e.preventDefault();
      ouvrir();
    }
  });
  return el;
}

const PERIODES = ["Cette semaine", "Ce mois-ci", "Plus ancien", "Sans date"];

/// Range les sorties par période sans changer leur ordre (le fil arrive trié
/// par date décroissante). Les périodes vides sont omises.
function grouperParPeriode(sorties) {
  const groupes = PERIODES.map(() => []);
  for (const s of sorties) {
    const j = joursDepuisSortie(s.date_sortie);
    groupes[j === null ? 3 : j <= 7 ? 0 : j <= 31 ? 1 : 2].push(s);
  }
  return PERIODES.map((titre, i) => ({ titre, items: groupes[i] })).filter((g) => g.items.length);
}

/// Une section de période : son intertitre, puis sa grille de cartes.
function periodeDecouvrir(titre, cartes) {
  const section = document.createElement("section");
  section.className = "decouvrir-periode";
  if (titre) {
    const h = document.createElement("h3");
    h.className = "decouvrir-periode__titre";
    h.textContent = titre;
    section.appendChild(h);
  }
  const grille = document.createElement("div");
  grille.className = "decouvrir-grille";
  grille.append(...cartes);
  section.appendChild(grille);
  return section;
}

function rendreListeSorties(idHote, idVide, sorties) {
  $(idVide).hidden = sorties.length > 0;
  $(idHote).replaceChildren(
    ...grouperParPeriode(sorties).map(({ titre, items }) =>
      periodeDecouvrir(
        titre,
        items.map((s) =>
          carteDecouvrir({
            nouveau: !s.vu,
            estompe: !sortiePasseFamille(s),
            pochette: () => pochetteDecouvrir(s),
            initialesDe: s.artiste_nom,
            type: s.type_primaire && s.type_primaire !== "Album" ? s.type_primaire : "",
            nom: s.titre,
            sec: s.artiste_nom,
            meta: ageSortie(s.date_sortie),
            collab: s.collaborateurs,
            liens: [
              ["Deezer", () => ouvrirDeezer(s.artiste_nom, s.titre)],
              ["Last.fm", () => ouvrirLien(lienLastfm(s.artiste_nom, s.titre))],
            ],
            ouvrir: () => explorerDepuisFil(s.artiste_mbid, s.artiste_nom),
          }),
        ),
      ),
    ),
  );
}

function rendreListeVoisins(voisins) {
  $("decouvrir-vide-voisins").hidden = voisins.length > 0;
  const cartes = voisins.map((v) =>
    carteDecouvrir({
      nouveau: !v.vu,
      estompe: !voisinPasseFamille(v),
      pochette: () => photoArtisteDecouvrir(v),
      photo: true,
      initialesDe: v.dst_nom,
      nom: v.dst_nom,
      sec: v.portes.length ? `proche de ${v.portes.slice(0, 2).join(", ")}` : "artiste voisin",
      secTitre: v.portes.length ? `proche de ${v.portes.join(", ")}` : "artiste voisin",
      liens: [
        ["Deezer", () => ouvrirDeezer(v.dst_nom)],
        ["Explorer ▸", () => explorerDepuisFil(v.dst_mbid, v.dst_nom)],
      ],
      ouvrir: () => explorerDepuisFil(v.dst_mbid, v.dst_nom),
    }),
  );
  $("decouvrir-voisins").replaceChildren(...(cartes.length ? [periodeDecouvrir("", cartes)] : []));
}

/// Lance la passe et suit son avancement (barre + texte), puis rafraîchit le fil.
async function lancerPasseDecouvrir() {
  if (decouvrirEnCours) return;
  const contact = contactMb();
  if (!contact.includes("@")) {
    $("decouvrir-fraicheur").textContent =
      "Renseignez une adresse de contact MusicBrainz dans le rail.";
    return;
  }
  decouvrirEnCours = true;
  $("decouvrir-actualiser").disabled = true;
  const ligne = $("decouvrir-avancement");
  const jauge = $("decouvrir-jauge");
  ligne.hidden = false;
  jauge.hidden = false;
  jauge.removeAttribute("value"); // barre indéterminée tant qu'on n'a pas de total
  ligne.textContent = "Actualisation… (recherche des sorties récentes)";
  try {
    await invoke("start_decouvrir", { contact });
    await attendreFin("decouvrir_state", 1500, (d) => {
      if (!d.en_cours) return;
      if (d.total) {
        jauge.max = d.total;
        jauge.value = d.artistes;
        const reste = Math.max(0, d.total - d.artistes);
        ligne.textContent =
          `${d.artistes.toLocaleString("fr-FR")} / ${d.total.toLocaleString("fr-FR")} étapes` +
          ` — ${d.sorties_neuves} sorties, ${d.voisins_neufs} voisins` +
          (reste ? ` — reste ~${dureeLongue(reste * SECONDES_PAR_ARTISTE)}` : "");
      }
    });
  } catch (e) {
    ligne.textContent = String(e);
    remonter(e, "découvrir");
  } finally {
    decouvrirEnCours = false;
    $("decouvrir-actualiser").disabled = false;
    ligne.hidden = true;
    jauge.hidden = true;
    jauge.value = 0;
    await chargerFilDecouvrir().catch((e) => remonter(e, "découvrir"));
    const r = (await invoke("decouvrir_state").catch(() => null))?.resultat;
    if (r && r.startsWith("échec")) $("decouvrir-fraicheur").textContent = r;
  }
}

$("decouvrir-actualiser").addEventListener("click", () =>
  lancerPasseDecouvrir().catch((e) => remonter(e, "découvrir")),
);

$("decouvrir-vu-tout").addEventListener("click", async () => {
  await invoke("decouvrir_tout_vu").catch((e) => remonter(e, "découvrir"));
  await chargerFilDecouvrir().catch((e) => remonter(e, "découvrir"));
});


export {
  artistesDecouvrables, boutonLien, chargerAPropos, chargerArtistesDecouvrir,
  chargerFamillesParArtiste, contactMb, decouverte, depuisTexte, dessinerFamillesDecouvrir,
  entrerDecouvrir, filtreFamillesDecouvrir, rendreFilDecouvrir,
};
