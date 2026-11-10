import "./playlists.js"; // l'ordre d'exécution : voir main.js
import { $, invoke, navigation, remonter, txt } from "./socle.js";
import { demander } from "./ecrans.js";
import { selectionner } from "./ecouter.js";
import { edition } from "./etabli.js";
import {
  choisirVariante, lancerDemix, poserSourceEdition, varianteParDefaut,
} from "./etabli-stems.js";
import { basculerMode } from "./modes.js";

/* ------------------------------- P2 : Pratiquer enchaîne une playlist */

// Une playlist de morceaux à travailler, enchaînés **à la main** (décision du
// 9 nov. 2026) : ← précédent, → suivant, jamais de passage automatique. Le
// bandeau (`#pratique-playlist`) appartient au centre de Pratiquer, au-dessus
// de ce qu'il montre (choisir, séparer, établi), comme le bandeau de la
// balade sur la carte (Règle 12) ; il n'existe qu'avec une playlist en cours.
//
// Changer de morceau, c'est le sélectionner (`selectionner`) : la mémoire de
// Pratiquer du morceau quitté est gardée et celle du nouveau rendue (E1,
// `memoire-pratique.js`), exactement comme un choix dans la liste. Un morceau
// sans stems se sépare tout de suite, avec la variante de sa mémoire, sinon
// celle par défaut ; le suivant se démixe d'avance en tâche de fond
// (`pratique_preparer`, `enchainer.rs`). La playlist et le morceau où l'on en
// est sont gardés dans la base, pas dans le navigateur.
//
// Raccourcis : ⇧← et ⇧→ (← et → seuls changent de morceau dans la file de
// lecture, ⌥↑ et ⌥↓ déplacent une piste dans une playlist ouverte).

const enchainement = {
  // `PlaylistPratique` : { playlist_id, nom, pistes, rang }, ou `null`.
  liste: null,
  // Le démixage d'avance du suivant : « pret », « prepare », « attente »,
  // « poids_absents », « echec », ou `null` (pas de suivant, rien demandé).
  avance: null,
  // Le morceau dont on attend le démixage d'avance pour l'ouvrir.
  attendu: null,
  sondage: null,
  // La reprise au morceau gardé ne se fait qu'une fois par session.
  repris: false,
};

const SONDAGE_MS = 2000;

/// La variante par défaut de Pratiquer, sans toucher au sélecteur : celle
/// choisie à la main, sinon BS-RoFormer (`varianteParDefaut`).
const varianteDefaut = () => (edition.varianteChoisie ? edition.variante : "bsrofo_sw");

async function memoireDe(path) {
  try {
    return await invoke("pratique_memoire", { path });
  } catch (e) {
    remonter(e, "mémoire de Pratiquer");
    return null;
  }
}

/* ---------------------------------------------------------- le bandeau */

const TEXTE_AVANCE = {
  pret: "prêt",
  prepare: "démixage d'avance…",
  attente: "en attente",
  poids_absents: "modèle à télécharger",
  echec: "démixage d'avance impossible",
};

function majBandeau() {
  const L = enchainement.liste;
  const montre = navigation.mode === "pratiquer" && !!L && L.pistes.length > 0;
  demander("pratique-playlist", montre);
  if (!montre) return;
  const n = L.pistes.length;
  const suivant = L.pistes[L.rang + 1] ?? null;
  $("enchainement-nom").textContent = L.nom;
  $("enchainement-rang").textContent = `${L.rang + 1} / ${n}`;
  $("enchainement-suivant").textContent = suivant
    ? `suivant : ${txt(suivant.title, "?")} — ${txt(suivant.artist, "?")}`
    : "dernier morceau";
  const av = suivant ? TEXTE_AVANCE[enchainement.avance] ?? "" : "";
  $("enchainement-avance").textContent = av;
  $("enchainement-avance").dataset.etat = suivant ? enchainement.avance ?? "" : "";
  $("enchainement-precedent").disabled = L.rang <= 0;
  $("enchainement-suivant-bouton").disabled = !suivant;
  $("pratique-playlist").title = `${L.nom} — ${n} morceau${n > 1 ? "x" : ""} à travailler`;
}

/// Appelée par `majEtatEditer` à chaque rangement du centre de Pratiquer et
/// de Créer : le bandeau, le bouton « playlist à travailler » de l'état
/// « choisir », et le rang suivi quand on choisit dans la liste un morceau de
/// la playlist.
function majEnchainement(choisir) {
  if (navigation.mode !== "pratiquer") return;
  demander("pratique-playlist-choisir", choisir);
  const L = enchainement.liste;
  const path = edition.source?.path;
  if (L && L.playlist_id >= 0 && path && edition.sourceChoisie && L.pistes[L.rang]?.path !== path) {
    const i = L.pistes.findIndex((t) => t.path === path);
    if (i >= 0) {
      L.rang = i;
      garderRang(path);
      preparerSuivant().catch((e) => remonter(e, "démixage d'avance"));
    }
  }
  majBandeau();
  // Une session qui reprend : le morceau gardé s'ouvre de lui-même, une fois,
  // si rien d'autre n'a été choisi.
  if (L && !enchainement.repris && !edition.sourceChoisie) {
    enchainement.repris = true;
    aller(L.rang).catch((e) => remonter(e, "playlist à travailler"));
  }
}

function garderRang(path) {
  const L = enchainement.liste;
  if (!L) return;
  invoke("pratique_playlist_poser", { id: L.playlist_id, path }).catch((e) =>
    remonter(e, "playlist à travailler"),
  );
}

/* ----------------------------------------------------- changer de morceau */

/// Ouvre le morceau de rang `rang` de la playlist en cours.
async function aller(rang) {
  const L = enchainement.liste;
  if (!L || rang < 0 || rang >= L.pistes.length) return;
  enchainement.repris = true;
  const t = L.pistes[rang];
  let maj = null;
  try {
    maj = await invoke("pratique_playlist_poser", { id: L.playlist_id, path: t.path });
  } catch (e) {
    remonter(e, "playlist à travailler");
    return;
  }
  // Supprimée ailleurs entre-temps.
  if (!maj) return fermer();
  enchainement.liste = maj;
  enchainement.avance = null;
  majBandeau();
  // Le même geste qu'un choix dans la liste : la séance du morceau quitté est
  // gardée, ses stems coupés ; ceux du nouveau chargés s'ils existent, et sa
  // séance rendue.
  await selectionner(t);
  if (edition.source?.path !== t.path) return;
  await ouvrirOuSeparer(t);
  await preparerSuivant();
}

/// Le morceau ouvert n'a pas de stems : on attend son démixage d'avance s'il
/// est en cours, sinon on le sépare tout de suite.
async function ouvrirOuSeparer(t) {
  let stems = [];
  try {
    stems = await invoke("stems_existants", { path: t.path });
  } catch (e) {
    remonter(e, "stems");
  }
  const prep = await invoke("pratique_preparation").catch(() => null);
  if (prep?.courant === t.path) {
    enchainement.attendu = t.path;
    $("separer-titre").textContent = `« ${txt(t.title, "?")} » — démixé d'avance, presque prêt…`;
    sonder();
    return;
  }
  if (stems.length || edition.source?.path !== t.path) return;
  const m = await memoireDe(t.path);
  if (edition.source?.path !== t.path) return;
  if (m?.variante) choisirVariante(m.variante);
  else varianteParDefaut();
  await lancerDemix();
}

/// Démixe d'avance le morceau qui suit, s'il n'a pas ses stems. Le moteur
/// attend la fin d'un démixage en cours avant de commencer.
async function preparerSuivant() {
  const L = enchainement.liste;
  const s = L?.pistes[L.rang + 1];
  if (!s) {
    enchainement.avance = null;
    majBandeau();
    return;
  }
  const m = await memoireDe(s.path);
  try {
    enchainement.avance = await invoke("pratique_preparer", {
      path: s.path,
      variant: m?.variante ?? varianteDefaut(),
    });
  } catch (e) {
    enchainement.avance = "echec";
    remonter(e, "démixage d'avance");
  }
  majBandeau();
  if (enchainement.avance === "prepare") sonder();
}

/// Suit le démixage d'avance tant qu'il tourne : l'état du suivant sur le
/// bandeau, et le morceau ouvert qu'on attendait, chargé dès qu'il est prêt.
function sonder() {
  if (enchainement.sondage) return;
  enchainement.sondage = setInterval(async () => {
    let p;
    try {
      p = await invoke("pratique_preparation");
    } catch (e) {
      remonter(e, "démixage d'avance");
      return;
    }
    const L = enchainement.liste;
    const s = L?.pistes[L.rang + 1];
    if (s && enchainement.avance !== "pret" && enchainement.avance !== "poids_absents") {
      // En pause, il cède la place à un démixage lancé à la main (V31).
      if (p.courant === s.path) enchainement.avance = p.en_pause ? "attente" : "prepare";
      else if (p.attente === s.path) enchainement.avance = "attente";
      else if (p.echecs.includes(s.path)) enchainement.avance = "echec";
      else {
        const stems = await invoke("stems_existants", { path: s.path }).catch(() => []);
        if (stems.length) enchainement.avance = "pret";
      }
    }
    const a = enchainement.attendu;
    if (a && p.courant !== a) {
      enchainement.attendu = null;
      // Toujours le morceau ouvert : ses stems arrivent.
      if (edition.source?.path === a && !edition.stems.length) await poserSourceEdition();
    }
    majBandeau();
    if (!p.en_cours && !enchainement.attendu) {
      clearInterval(enchainement.sondage);
      enchainement.sondage = null;
    }
  }, SONDAGE_MS);
}

async function fermer() {
  enchainement.liste = null;
  enchainement.avance = null;
  enchainement.attendu = null;
  majBandeau();
  try {
    await invoke("pratique_playlist_fermer");
  } catch (e) {
    remonter(e, "playlist à travailler");
  }
}

/// Travailler la playlist `id` en Pratiquer : depuis la vue Playlists
/// d'Écouter ou le sélecteur de l'état « choisir ». La playlist déjà en
/// cours reprend où elle en était ; une autre commence au premier morceau.
async function pratiquerPlaylist(id) {
  const meme = enchainement.liste?.playlist_id === id;
  const path = meme ? enchainement.liste.pistes[enchainement.liste.rang]?.path ?? null : null;
  const L = await invoke("pratique_playlist_poser", { id, path });
  if (!L) return;
  enchainement.liste = L;
  enchainement.avance = null;
  enchainement.repris = true;
  if (!L.pistes.length) {
    majBandeau();
    return;
  }
  if (navigation.mode !== "pratiquer") await basculerMode("pratiquer");
  await aller(L.rang);
}

/* ------------------------------------------------------------- gestes */

$("enchainement-precedent").addEventListener("click", () => {
  if (enchainement.liste) aller(enchainement.liste.rang - 1).catch((e) => remonter(e, "playlist à travailler"));
});
$("enchainement-suivant-bouton").addEventListener("click", () => {
  if (enchainement.liste) aller(enchainement.liste.rang + 1).catch((e) => remonter(e, "playlist à travailler"));
});
$("enchainement-fermer").addEventListener("click", () => {
  fermer().catch((e) => remonter(e, "playlist à travailler"));
});

// En capture : ⇧← et ⇧→ ne doivent pas aussi changer de morceau dans la file
// de lecture (`lecteur.js` écoute ← et → sans regarder ⇧).
document.addEventListener("keydown", (e) => {
  if (!e.shiftKey || e.altKey || e.metaKey || e.ctrlKey || e.repeat) return;
  if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
  if (navigation.mode !== "pratiquer" || !enchainement.liste) return;
  if (e.target.closest?.("input, textarea, select, [contenteditable]")) return;
  e.preventDefault();
  e.stopPropagation();
  const d = e.key === "ArrowLeft" ? -1 : 1;
  aller(enchainement.liste.rang + d).catch((err) => remonter(err, "playlist à travailler"));
}, true);

/* ----------------------------------- le sélecteur de l'état « choisir » */

// Le même petit menu que « Ajouter à une playlist » (`.menu-playlist`) : les
// playlists modifiées récemment d'abord ; Échap ou un clic ailleurs le
// ferment.
const menu = document.createElement("div");
menu.id = "menu-pratique-playlist";
menu.className = "menu-playlist";
menu.hidden = true;
menu.setAttribute("role", "dialog");
menu.setAttribute("aria-label", "Playlist à travailler");
document.body.appendChild(menu);

function fermerMenu() {
  menu.hidden = true;
  $("pratique-playlist-choisir").setAttribute("aria-expanded", "false");
}

async function ouvrirMenu() {
  const ancre = $("pratique-playlist-choisir");
  if (!menu.hidden) return fermerMenu();
  let liste = [];
  try {
    liste = (await invoke("playlists")) ?? [];
  } catch (e) {
    remonter(e, "playlists");
  }
  menu.replaceChildren();
  const tete = document.createElement("p");
  tete.className = "menu-playlist__tete";
  tete.textContent = liste.length ? "Travailler la playlist" : "Aucune playlist : composez-en une dans Écouter.";
  menu.appendChild(tete);
  const choix = document.createElement("div");
  choix.className = "menu-playlist__choix";
  for (const p of liste) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "menu-playlist__item";
    b.textContent = p.nom;
    b.title = `${p.nom} · ${p.nb_pistes} piste${p.nb_pistes > 1 ? "s" : ""}`;
    b.disabled = p.nb_pistes - p.nb_manquantes <= 0;
    b.addEventListener("click", () => {
      fermerMenu();
      pratiquerPlaylist(p.id).catch((e) => remonter(e, "playlist à travailler"));
    });
    choix.appendChild(b);
  }
  menu.appendChild(choix);
  menu.hidden = false;
  ancre.setAttribute("aria-expanded", "true");
  const r = ancre.getBoundingClientRect();
  menu.style.left = `${Math.max(8, Math.min(r.left, innerWidth - menu.offsetWidth - 8))}px`;
  menu.style.top = `${r.bottom + 4}px`;
  choix.querySelector("button:not(:disabled)")?.focus();
}

$("pratique-playlist-choisir").addEventListener("click", () => {
  ouvrirMenu().catch((e) => remonter(e, "playlists"));
});
document.addEventListener("mousedown", (e) => {
  if (!menu.hidden && !menu.contains(e.target) && e.target !== $("pratique-playlist-choisir")) fermerMenu();
}, true);
menu.addEventListener("keydown", (e) => {
  e.stopPropagation();
  if (e.key === "Escape") {
    fermerMenu();
    $("pratique-playlist-choisir").focus();
  }
});

/* ------------------------------------------------------------ reprise */

// La playlist gardée par la session précédente : le bandeau la montre dès
// l'entrée en Pratiquer, et son morceau s'ouvre si rien d'autre n'est choisi.
invoke("pratique_playlist")
  .then((L) => {
    enchainement.liste = L ?? null;
    if (L && navigation.mode === "pratiquer") majEnchainement(!edition.sourceChoisie);
  })
  .catch((e) => remonter(e, "playlist à travailler"));

/// L'audit d'interface (`audit.js`) : le bandeau sur une playlist fictive
/// (`pistes`), sans rien demander au moteur ; `null` le retire.
function bandeauAudit(pistes) {
  enchainement.liste = pistes ? { playlist_id: -1, nom: "Playlist à travailler", pistes, rang: 1 } : null;
  enchainement.avance = pistes ? "prepare" : null;
  enchainement.repris = true;
  majBandeau();
}

export { bandeauAudit, majEnchainement, pratiquerPlaylist };
