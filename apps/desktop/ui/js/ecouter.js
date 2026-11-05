import "./lama.js"; // l'ordre d'exécution : voir main.js
import {
  $, duree, ecoute, estListePistes, fileAttente, invoke, isoler, LIGNE, navigation, ouvrirLien,
  pochette, remonter, signalerErreur, txt, vue, vueEnGrille,
} from "./socle.js";
import {
  ALBUM_HAUT, ALCHIMIE_PISTES, centreCorps, chargerPopularites, colonnesGrille, composerAlchimie,
  dessiner, dessinerGrille, fenetre, grille, initiales, lireAlbum, liste, teinteNom,
} from "./vues.js";

import {
  demarrerLecture, dureeFile, estAffiche, majApercuTransport,
} from "./lecteur.js";
import {
  anneau, carte, chargerAnneau,
} from "./carte.js";
import {
  chargerFamilles, tracerRouteSurCarte,
} from "./maplibre.js";
import {
  estRetenu, grilleDeTri, indexAlphaUtile, indexAnneesUtile, lignesCourantes, majBarreTri,
  majBlocFamillesEcoute, rafraichirGrille, rebrasserAlea, SENS_ORIGINE, sensDe, sensTri,
  triCourant,
} from "./ecouter-familles.js";
import {
  bruitChemin,
} from "./fond-de-plan.js";
import {
  basculerMode,
} from "./modes.js";
import {
  artistesDecouvrables, boutonLien, chargerArtistesDecouvrir, contactMb,
} from "./decouvrir.js";
import {
  dureeLongue,
} from "./bibliotheque.js";
import {
  edition,
} from "./editer.js";
import {
  majEtatEditer, poserSourceEdition,
} from "./pratiquer.js";
import {
  charger,
} from "./demarrage.js";
import { lecture } from "./etat-lecture.js";

/* --------------------------------------------------- minuteur d'arrêt
 *
 * Arrêter la lecture dans 15/30/60 min, à la fin du morceau ou de l'album. Tout
 * vit côté moteur (`minuteur.rs`) : la page n'y arme et n'y désarme rien d'autre
 * qu'en le demandant, et affiche ce que le moteur lui rapporte — avec le
 * battement, tant que ça joue, et à l'ouverture du panneau.
 */

/// « Arrêt dans 24 min », « Arrêt à la fin du morceau (1 min) »…
function texteMinuteur(m) {
  if (!m) return "";
  const reste = Number.isFinite(m.reste_ms) ? dureeFile(m.reste_ms) : null;
  if (m.mode === "duree") return `Arrêt dans ${reste ?? "…"}`;
  const cible = m.mode === "album" ? "de l'album" : "du morceau";
  return reste ? `Arrêt à la fin ${cible} (${reste})` : `Arrêt à la fin ${cible}`;
}

/// Reflète l'état du minuteur dans le panneau de file : le texte, le bouton
/// allumé, et « annuler » seulement quand il y a quelque chose à annuler.
function majMinuteur(m) {
  const texte = texteMinuteur(m);
  const el = $("file-minuteur-etat");
  if (el.textContent !== texte) el.textContent = texte;
  $("file-minuteur").setAttribute("aria-pressed", String(Boolean(m)));
  $("file-minuteur-annuler").hidden = !m;
}

function rafraichirMinuteur() {
  invoke("minuteur_etat")
    .then(majMinuteur)
    .catch((e) => remonter(e, "minuteur_etat"));
}

$("file-minuteur").addEventListener("click", () => {
  const menu = $("file-minuteur-menu");
  menu.hidden = !menu.hidden;
  $("file-minuteur").setAttribute("aria-expanded", String(!menu.hidden));
  if (!menu.hidden) rafraichirMinuteur();
});

document.querySelectorAll("[data-minuteur]").forEach((b) =>
  b.addEventListener("click", async () => {
    try {
      const minutes = b.dataset.minutes ? Number(b.dataset.minutes) : null;
      majMinuteur(await invoke("minuteur_demarrer", { mode: b.dataset.minuteur, minutes }));
    } catch (e) {
      // Le moteur dit pourquoi, en clair (« rien ne joue… »).
      signalerErreur("file-minuteur-etat", String(e), e, "minuteur_demarrer");
      return;
    }
    $("file-minuteur-menu").hidden = true;
    $("file-minuteur").setAttribute("aria-expanded", "false");
  }),
);

$("file-minuteur-annuler").addEventListener("click", async () => {
  try {
    await invoke("minuteur_annuler");
  } catch (e) {
    remonter(e, "minuteur_annuler");
  }
  majMinuteur(null);
  $("file-minuteur-menu").hidden = true;
  $("file-minuteur").setAttribute("aria-expanded", "false");
});

/* ------------------------------------------------- playlists enregistrées
 *
 * Une file qu'on garde, nommée (`docs/plan-ecouter-v0.2.md`, point 4). Trois
 * gestes : « enregistrer » dans le panneau de file, la vue « Playlists »
 * d'Écouter (jouer, ouvrir, renommer, supprimer), et l'ouverture d'une playlist
 * comme une liste de pistes ordinaire.
 */

/// La demande ou le point de départ de la file en cours — seulement si la file
/// n'a pas changé depuis qu'on l'a composée.
function origineDeLaFile() {
  const o = fileAttente.origine;
  const pistes = fileAttente.pistes;
  if (!o || pistes.length !== o.n || !pistes.some((t) => t.path === o.premier)) return null;
  return o.texte;
}

function nomParDefaut() {
  const o = origineDeLaFile();
  if (o) return o.length > 80 ? `${o.slice(0, 79)}…` : o;
  return `Playlist du ${new Date().toLocaleDateString("fr-FR", { day: "numeric", month: "long" })}`;
}

let etatEnregistrementMinuteur = null;
function etatEnregistrement(texte) {
  $("file-enregistrer-etat").textContent = texte;
  clearTimeout(etatEnregistrementMinuteur);
  if (texte) etatEnregistrementMinuteur = setTimeout(() => etatEnregistrement(""), 6000);
}

function fermerEnregistrement() {
  $("file-enregistrer-form").hidden = true;
}

$("file-enregistrer").addEventListener("click", () => {
  const form = $("file-enregistrer-form");
  if (!form.hidden) return fermerEnregistrement();
  if (!fileAttente.pistes.length) return etatEnregistrement("La file est vide : rien à enregistrer.");
  etatEnregistrement("");
  form.hidden = false;
  const champ = $("file-enregistrer-nom");
  champ.value = nomParDefaut();
  champ.focus();
  champ.select();
});
$("file-enregistrer-annuler").addEventListener("click", fermerEnregistrement);
$("file-enregistrer-nom").addEventListener("keydown", (e) => {
  // Les raccourcis globaux (espace, flèches) ne doivent pas traverser le champ.
  e.stopPropagation();
  if (e.key === "Escape") fermerEnregistrement();
});

$("file-enregistrer-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const paths = fileAttente.pistes.map((t) => t.path);
  if (!paths.length) return etatEnregistrement("La file est vide : rien à enregistrer.");
  const nom = $("file-enregistrer-nom").value.trim() || nomParDefaut();
  try {
    await invoke("creer_playlist", { nom, origine: origineDeLaFile(), paths });
  } catch (err) {
    signalerErreur("file-enregistrer-etat", "Enregistrement impossible.", err, "creer_playlist");
    return;
  }
  fermerEnregistrement();
  etatEnregistrement(`Enregistrée : ${nom} (${paths.length} pistes)`);
  if (vue.quoi === "playlists") await chargerPlaylists();
});

/// La vue Playlists : les listes calculées à partir des écoutes d'abord
/// (récents, plus écoutés, favoris — seulement celles qui ont du contenu), puis
/// les playlists enregistrées.
async function chargerPlaylists() {
  const [intelligentes, playlists] = await Promise.all([
    invoke("listes_intelligentes"),
    invoke("playlists"),
  ]);
  poser("playlists", "Playlists", [
    ...intelligentes.map((l) => ({ ...l, intelligente: true, id: l.quoi })),
    ...playlists,
  ]);
}

/// Les pistes d'une entrée de la vue Playlists, calculée ou enregistrée.
function pistesDe(item) {
  return item.intelligente
    ? invoke("liste_intelligente", { quoi: item.quoi })
    : invoke("playlist_pistes", { id: item.id });
}

/* ----------------------------------------------------------- favoris
 *
 * Un favori est un chemin (voir `memoire.rs`). L'interface en tient l'ensemble :
 * il marque les lignes de pistes (`ajouterPopEtDuree`) et colore le bouton ♥ de
 * l'inspecteur, qui suit la lecture.
 */
let favorisChemins = new Set();

invoke("favoris")
  .then((l) => {
    favorisChemins = new Set(l);
    majFavoriBouton();
    if (estListePistes()) dessiner();
  })
  .catch((e) => remonter(e, "favoris"));

/// Le bouton ♥ suit le morceau inspecté ; caché quand l'inspecteur montre un
/// album ou rien.
function majFavoriBouton() {
  const bouton = $("insp-favori");
  const path = $("insp-titre").dataset.path;
  bouton.hidden = !path;
  // « Flâner d'ici » part d'un morceau, jamais d'un album.
  $("insp-balade").hidden = !path;
  if (!path) return;
  const favori = favorisChemins.has(path);
  bouton.textContent = favori ? "♥" : "♡";
  bouton.setAttribute("aria-pressed", String(favori));
  const titre = favori ? "Retirer des favoris" : "Ajouter aux favoris";
  bouton.title = titre;
  bouton.setAttribute("aria-label", titre);
}

$("insp-favori").addEventListener("click", async () => {
  const path = $("insp-titre").dataset.path;
  if (!path) return;
  try {
    const favori = await invoke("basculer_favori", { path });
    if (favori) favorisChemins.add(path);
    else favorisChemins.delete(path);
  } catch (e) {
    remonter(e, "basculer_favori");
    return;
  }
  majFavoriBouton();
  if (estListePistes()) dessiner();
  // Les « Favoris » de la vue Playlists changent avec ce geste.
  if (vue.quoi === "playlists") await chargerPlaylists();
});

function boutonAction(texte, titre, onClick) {
  const b = document.createElement("button");
  b.type = "button";
  b.className = "ligne__action";
  b.textContent = texte;
  b.title = titre;
  b.setAttribute("aria-label", titre);
  b.addEventListener("click", (e) => {
    e.stopPropagation();
    onClick(b);
  });
  return b;
}

/// Une ligne de la vue Playlists. Un clic sur la ligne l'ouvre (ses pistes, au
/// centre) ; ▶ la lit d'un bout à l'autre ; ✎ la renomme sur place ; ✕ la
/// supprime, en deux temps — pas de boîte de dialogue.
function lignePlaylist(el, item) {
  el.classList.add("ligne--playlist");
  el.appendChild(boutonAction("▶", `Lire ${item.nom}`, () => jouerPlaylist(item).catch((e) => remonter(e, "playlist"))));

  const nom = document.createElement("span");
  nom.className = "ligne__nom";
  nom.textContent = item.nom;
  nom.title = item.origine ? `${item.nom} — ${item.origine}` : item.nom;
  el.appendChild(nom);

  const resume = document.createElement("span");
  resume.className = "ligne__sec";
  const manquantes = item.nb_manquantes > 0 ? ` · ${item.nb_manquantes} introuvable${item.nb_manquantes > 1 ? "s" : ""}` : "";
  resume.textContent = `${item.nb_pistes} piste${item.nb_pistes > 1 ? "s" : ""} · ${dureeLongue((item.duree_ms || 0) / 1000)}${manquantes}`;
  el.appendChild(resume);

  // Une liste calculée à partir des écoutes se lit et s'ouvre, mais ne se
  // renomme ni ne se supprime : elle se recalcule d'elle-même.
  if (item.intelligente) {
    nom.title = `${item.nom} — calculée à partir de vos écoutes`;
  } else {
    el.appendChild(boutonAction("⇩", "Exporter en .m3u8", () => exporterPlaylist(item).catch((e) => remonter(e, "exporter_playlist"))));
    el.appendChild(boutonAction("✎", "Renommer", () => renommerPlaylistEnLigne(el, item)));
    el.appendChild(boutonAction("✕", "Supprimer", (b) => supprimerPlaylistEnDeuxTemps(b, item)));
  }
  el.addEventListener("click", () => ouvrirPlaylist(item).catch((e) => remonter(e, "playlist")));
  return el;
}

/// Écrit la playlist en `.m3u8` dans un dossier à choisir — le même sélecteur
/// de dossier que l'export des stems (pas de permission d'écriture de plus). Le
/// résultat se lit dans l'en-tête de la liste, quelques secondes.
async function exporterPlaylist(item) {
  const dossier = await window.__TAURI__.dialog.open({
    directory: true,
    multiple: false,
    title: `Où écrire « ${item.nom} » (.m3u8)`,
  });
  if (!dossier) return;
  // Court dans l'en-tête (sans ellipse ni largeur minimale : un chemin entier
  // pousserait le titre) ; le détail va dans l'infobulle.
  let message, detail;
  try {
    const fichier = await invoke("exporter_playlist", { id: item.id, dossier });
    message = `exportée : ${fichier.split(/[\\/]/).pop()}`;
    detail = fichier;
  } catch (e) {
    remonter(e, "exporter_playlist");
    message = "export impossible";
    detail = String(e);
  }
  const compte = $("fil-compte");
  compte.textContent = message;
  compte.title = detail;
  setTimeout(() => {
    compte.title = "";
    if (vue.quoi === "playlists") compte.textContent = `${vue.lignes.length} listes`;
  }, 8000);
}

function renommerPlaylistEnLigne(el, item) {
  const nom = el.querySelector(".ligne__nom");
  const champ = document.createElement("input");
  champ.className = "ligne__champ";
  champ.maxLength = 120;
  champ.value = item.nom;
  champ.setAttribute("aria-label", "Nouveau nom");
  nom.replaceWith(champ);
  champ.focus();
  champ.select();
  let fini = false;
  const valider = async () => {
    if (fini) return;
    fini = true;
    const v = champ.value.trim();
    if (v && v !== item.nom) {
      try {
        await invoke("renommer_playlist", { id: item.id, nom: v });
      } catch (e) {
        remonter(e, "renommer_playlist");
      }
      await chargerPlaylists();
    } else {
      dessiner();
    }
  };
  champ.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter") valider();
    else if (e.key === "Escape") {
      fini = true;
      dessiner();
    }
  });
  champ.addEventListener("blur", valider);
  champ.addEventListener("click", (e) => e.stopPropagation());
}

function supprimerPlaylistEnDeuxTemps(bouton, item) {
  if (bouton.dataset.confirme === "1") {
    invoke("supprimer_playlist", { id: item.id })
      .catch((e) => remonter(e, "supprimer_playlist"))
      .then(() => chargerPlaylists());
    return;
  }
  bouton.dataset.confirme = "1";
  bouton.textContent = "supprimer ?";
  bouton.classList.add("ligne__action--danger");
  setTimeout(() => {
    if (!bouton.isConnected) return;
    delete bouton.dataset.confirme;
    bouton.textContent = "✕";
    bouton.classList.remove("ligne__action--danger");
  }, 3500);
}

/// Lit la playlist du début à la fin, comme un album.
async function jouerPlaylist(item) {
  const pistes = await pistesDe(item);
  if (pistes.length === 0) {
    $("fil-compte").textContent = "aucune piste disponible dans cette playlist";
    return;
  }
  selectionner(pistes[0]);
  fileAttente.pistes = pistes;
  tracerRouteSurCarte(pistes);
  await demarrerLecture(() => invoke("play", { paths: pistes.map((t) => t.path) }));
}

/// Montre les pistes de la playlist au centre, comme celles d'un album :
/// « ← Playlists » revient à la liste.
async function ouvrirPlaylist(item) {
  const pistes = await pistesDe(item);
  poser("pistes", item.nom, pistes, {
    quoi: "playlists", titre: vue.titre, lignes: vue.lignes, scroll: scrollActuel(),
  });
}

/* ---------------------------------------------------------- navigation */

// Le défilement d'où l'on vient : lu par `activer()` juste avant de
// descendre dans un artiste ou un album, pour que « ← » retrouve la ligne
// quittée plutôt que de remonter en haut de la liste.
function scrollActuel() {
  return (vueEnGrille() ? grille : liste).scrollTop;
}

/// Le compte du fil : « 12 albums », ou « 3 sur 12 albums » quand un filtre
/// (rail, famille) en estompe une partie — le total reste celui de ce qui est
/// affiché.
function texteCompte(quoi, lignes) {
  const unite = { artistes: "artistes", albums: "albums", playlists: "listes" }[quoi] ?? "morceaux";
  const retenus = lignes.filter(estRetenu).length;
  return retenus === lignes.length ? `${lignes.length} ${unite}` : `${retenus} sur ${lignes.length} ${unite}`;
}

function poser(quoi, titre, lignes, retour = null, scroll = 0) {
  vue.quoi = quoi;
  vue.titre = titre;
  vue.lignes = lignes;
  vue.retour = retour;
  if (retour === null) ecoute.sommet = { quoi, titre, lignes };

  // Bandeau « autour de cet artiste » : propre à la grille d'albums d'un
  // artiste précis (`retour` non nul — voir `ouvrirAlbumsArtiste`), jamais à
  // la liste « Albums » de toute la bibliothèque. `poser` le masque par
  // défaut à chaque changement de vue ; seul `majAutourArtiste` le repeuple.
  masquerAutourArtiste();

  // Hors mode Écoute, l'en-tête appartient à l'autre mode (« Découvrir »,
  // « Bibliothèque ») : `poser` met à jour `vue` mais n'y touche pas.
  const horsEcoute = navigation.mode !== "ecoute";
  if (!horsEcoute) {
    $("fil-titre").textContent = titre;
    $("fil-compte").textContent = texteCompte(quoi, lignes);
  }
  $("retour").hidden = retour === null;
  $("retour").textContent = `← ${retour ? retour.titre : ""}`;
  // « Choisir l'édition… » : seulement sur les pistes de l'album ouvert.
  if (quoi !== "pistes" || ecoute.albumOuvert?.name !== titre) ecoute.albumOuvert = null;
  $("choisir-edition").hidden = horsEcoute || !ecoute.albumOuvert;

  // Artistes et albums partagent le même geste de premier niveau ; le
  // commutateur du rail suit celui qui a produit la vue affichée, y compris
  // en descendant depuis un artiste vers ses albums.
  if (quoi === "artistes" || quoi === "albums" || quoi === "playlists") {
    document.querySelectorAll("[data-vuelib]").forEach((b) =>
      b.classList.toggle("segment--actif", b.dataset.vuelib === quoi),
    );
  }

  const enGrille = vueEnGrille(quoi);
  // `charger("albums")` court en fond au démarrage et se termine parfois après
  // `basculerMode` (mode imposé par `RUSTY_MUSIC_MODE`, ou clic rapide) : on
  // prépare alors les données sans remontrer grille ni liste.
  $("liste").hidden = horsEcoute || enGrille;
  $("grille").hidden = horsEcoute || !enGrille;
  // Le choix d'ordre ne concerne que les grilles (albums, artistes) de l'Écoute.
  majBarreTri(horsEcoute);
  if (enGrille) {
    // Pas de `preparerGraphe()` ici : la grille est la vue par défaut de
    // l'Écoute, donc ce qu'on construirait à *chaque* lancement de l'appli,
    // même pour qui n'ouvre jamais Explorer ni le bouton ✦. Signalé
    // directement — ce préchauffage saturait tous les cœurs dès le
    // démarrage et rendait la lecture saccadée. Le bouton ✦
    // (`genererAlchimie`) et l'entrée dans Explorer (`poserModeChemin`)
    // continuent de le construire à la demande ; le premier geste de la
    // session qui en a besoin paie simplement le prix une fois.
    grille.scrollTop = scroll;
    ecoute.grilleDernierRang = -1;
    dessinerGrille();
  } else {
    liste.scrollTop = scroll;
    dessiner();
  }

  // Repère alphabétique : seulement là où l'ordre affiché est celui des
  // noms — pas la liste des pistes d'un album (ordre du disque), ni une
  // recherche (ordre de pertinence).
  majIndexLateraux(quoi);

  // Liste de morceaux : la popularité se charge en lot, puis on repeint les
  // lignes visibles (la liste est virtualisée, `dessiner` relit le cache).
  if (estListePistes(quoi)) {
    chargerPopularites(
      lignes.map((l) => l.id),
      () => {
        if (estListePistes()) dessiner();
      },
    );
  }

  majBlocFamillesEcoute();

  // En Éditer, `poser` a masqué grille et liste (gate `horsEcoute`) : c'est
  // `majEtatEditer` qui décide de les ré-afficher, selon l'état du parcours,
  // et qui les redessine une fois visibles.
  if (navigation.mode === "editer") majEtatEditer();
}

/* -------------------------------------------------------- index alphabétique */

const LETTRES_INDEX = ["#", ..."ABCDEFGHIJKLMNOPQRSTUVWXYZ"];
const indexAlphaHote = $("index-alpha");
LETTRES_INDEX.forEach((l) => {
  const b = document.createElement("button");
  b.className = "index-alpha__lettre";
  b.dataset.lettre = l;
  b.textContent = l;
  b.addEventListener("click", () => sauterALettre(l));
  indexAlphaHote.appendChild(b);
});

/// Première lettre d'un nom, ramenée à une des 26 majuscules ou à `#` pour
/// tout le reste (chiffres, ponctuation, écritures non latines) — un repère
/// à 27 entrées reste lisible, un par caractère Unicode ne le serait pas.
function premiereLettre(nom) {
  const c = (nom || "").trim().charAt(0).toUpperCase();
  return LETTRES_INDEX.includes(c) ? c : "#";
}

// Lettre → rang de sa première entrée dans `vue.lignes`. Reconstruit à
// chaque `poser()` d'une vue indexable : Artistes et Albums sont déjà triés
// par nom côté moteur (`ORDER BY … COLLATE NOCASE`), un simple passage
// suffit donc à trouver ces rangs.
let indexAlpha = {};
function construireIndexAlpha() {
  indexAlpha = {};
  lignesCourantes().forEach((item, i) => {
    const l = premiereLettre(item.name);
    if (!(l in indexAlpha)) indexAlpha[l] = i;
  });
  indexAlphaHote.querySelectorAll(".index-alpha__lettre").forEach((b) => {
    b.classList.toggle("index-alpha__lettre--vide", !(b.dataset.lettre in indexAlpha));
  });
  majIndexActif();
}

/// Rang affiché en haut de la fenêtre visible, pour savoir quelle lettre
/// grossir dans le repère.
function rangVisible() {
  if (vueEnGrille()) return Math.floor(grille.scrollTop / ALBUM_HAUT) * colonnesGrille();
  return Math.floor(liste.scrollTop / LIGNE);
}

function majIndexActif() {
  majIndexAnneesActif();
  if ($("index-alpha").hidden) return;
  const lignes = lignesCourantes();
  const i = Math.min(lignes.length - 1, Math.max(0, rangVisible()));
  const lettre = lignes[i] ? premiereLettre(lignes[i].name) : null;
  indexAlphaHote.querySelectorAll(".index-alpha__lettre").forEach((b) =>
    b.classList.toggle("index-alpha__lettre--actif", b.dataset.lettre === lettre),
  );
}

function sauterALettre(l) {
  const rang = indexAlpha[l];
  if (rang === undefined) return;
  sauterAuRang(rang);
}

/// Amène la liste ou la grille sur la ligne de rang `rang`.
function sauterAuRang(rang) {
  if (vueEnGrille()) {
    grille.scrollTop = Math.floor(rang / colonnesGrille()) * ALBUM_HAUT;
    ecoute.grilleDernierRang = -1;
    dessinerGrille();
  } else {
    liste.scrollTop = rang * LIGNE;
    dessiner();
  }
}

/// Affiche le repère qui convient à l'ordre en vigueur — les lettres pour A–Z,
/// les années pour Année, rien pour le reste — et le reconstruit : sa liste, son
/// sens, ses entrées dépendent de l'ordre et des lignes affichées.
function majIndexLateraux(quoi = vue.quoi) {
  const alpha = indexAlphaUtile(quoi);
  const annees = indexAnneesUtile(quoi);
  $("index-alpha").hidden = !alpha;
  $("index-annees").hidden = !annees;
  if (alpha) {
    // Z–A : le repère se lit lui aussi de bas en haut, comme la liste.
    indexAlphaHote.classList.toggle("index-alpha--inverse", sensDe("alpha", quoi) === -1);
    construireIndexAlpha();
  }
  if (annees) construireIndexAnnees();
}

/* ----------------------------------------------------- repère des années */

const indexAnneesHote = $("index-annees");
/// Entrées du repère, dans l'ordre de la liste : `{ annee, rang }` (`annee`
/// `null` pour les albums sans année, toujours en dernier).
let indexAnnees = [];

/// Une bande par année présente, du haut vers le bas dans l'ordre de la liste.
/// Quatre chiffres pour 60 années ne tiennent pas dans la hauteur : on n'étiquette
/// qu'une année sur 2, 5 ou 10 — la plus fine qui laisse au moins 10 px par
/// étiquette — mais toutes restent cliquables (infobulle), et l'année en tête de
/// liste reprend son chiffre (`majIndexAnneesActif`).
function construireIndexAnnees() {
  const premier = new Map();
  lignesCourantes().forEach((a, i) => {
    const annee = a.year ?? null;
    if (!premier.has(annee)) premier.set(annee, i);
  });
  indexAnnees = [...premier].map(([annee, rang]) => ({ annee, rang }));

  const hauteur = indexAnneesHote.clientHeight || grille.clientHeight || 500;
  const bande = hauteur / Math.max(1, indexAnnees.length);
  const pas = [1, 2, 5, 10].find((k) => bande * k >= 10) ?? 10;

  // Écart minimal, en bandes, entre deux étiquettes : une année absente de la
  // bibliothèque rapproche deux années rondes (1990 et 1988 côte à côte), qui
  // se chevaucheraient.
  const ecart = Math.ceil(10 / bande);
  let derniere = -Infinity;
  indexAnneesHote.replaceChildren(
    ...indexAnnees.map(({ annee, rang }, i) => {
      const b = document.createElement("button");
      b.className = "index-alpha__lettre";
      b.dataset.annee = annee === null ? "?" : String(annee);
      b.textContent = annee === null ? "?" : String(annee);
      b.title = annee === null ? "Sans année" : String(annee);
      const etiquette = annee !== null && annee % pas === 0 && i - derniere >= ecart;
      if (etiquette) derniere = i;
      if (annee !== null && !etiquette) b.classList.add("index-alpha__lettre--muette");
      b.addEventListener("click", () => sauterAuRang(rang));
      return b;
    }),
  );
  majIndexAnneesActif();
}

/// Met en avant l'année de la ligne en tête de la fenêtre visible.
function majIndexAnneesActif() {
  if ($("index-annees").hidden) return;
  const lignes = lignesCourantes();
  const i = Math.min(lignes.length - 1, Math.max(0, rangVisible()));
  const annee = lignes[i] ? (lignes[i].year ?? "?") : null;
  indexAnneesHote.querySelectorAll(".index-alpha__lettre").forEach((b) =>
    b.classList.toggle("index-alpha__lettre--actif", b.dataset.annee === String(annee)),
  );
}

document.querySelectorAll("[data-vuelib]").forEach((b) =>
  b.addEventListener("click", () =>
    (b.dataset.vuelib === "playlists" ? chargerPlaylists() : charger(b.dataset.vuelib)).catch((e) =>
      remonter(e, "parcourir"),
    ),
  ),
);

document.querySelectorAll("[data-tri]").forEach((b) =>
  b.addEventListener("click", () => choisirTriAlbums(b.dataset.tri)),
);

/// Change l'ordre de la grille d'albums. Un clic sur « Aléatoire » rebrasse à
/// chaque fois, même s'il est déjà actif ; un clic sur A–Z, Année ou Ajoutés
/// déjà actif inverse son sens. Le repère alphabétique n'a de sens que pour
/// l'ordre `alpha`, celui des années que pour `annee`.
function choisirTriAlbums(tri) {
  const etat = sensTri[grilleDeTri()];
  if (tri === "alea") rebrasserAlea(vue.lignes);
  // Un clic sur l'ordre déjà actif l'inverse ; l'atteindre depuis un autre le
  // remet au sens d'origine.
  else if (tri === triCourant()) etat[tri] = -sensDe(tri);
  else etat[tri] = SENS_ORIGINE[tri] ?? 1;
  if (vue.quoi === "artistes") ecoute.triArtistes = tri;
  else ecoute.triAlbums = tri;
  majBarreTri();
  majIndexLateraux();
  grille.scrollTop = 0;
  rafraichirGrille();
}

async function activer(item) {
  if (vue.quoi === "artistes") {
    // Les deux sont nécessaires : un artiste réunit ses pistes étiquetées
    // MusicBrainz et les autres.
    const albums = await invoke("albums", { mbid: item.mbid ?? null, artist: item.name });
    poser("albums", item.name, albums, {
      quoi: "artistes", titre: "Artistes", lignes: vue.lignes, scroll: scrollActuel(),
    });
    majAutourArtiste(item.name, item.mbid ?? null);
  } else if (vue.quoi === "albums") {
    const pistes = await invoke("tracks_of_album", { album: item.name, artist: item.artist ?? null });
    ecoute.albumOuvert = { name: item.name, artist: item.artist ?? null };
    poser("pistes", item.name, pistes, {
      quoi: vue.quoi, titre: vue.titre, lignes: vue.lignes, retour: vue.retour, scroll: scrollActuel(),
    });
  } else {
    await lancerLecture(item);
  }
}

/// Lit `item` : la suite de la liste affichée forme la file (un morceau absent
/// de la liste — voisin, carte — joue seul). Lancer la lecture d'un morceau, c'est
/// aussi le choisir : il devient la sélection (et, en Éditer, la source).
async function lancerLecture(item) {
  const depart = vue.lignes.indexOf(item);
  // Non attendue : le son ne doit pas patienter derrière la pochette de
  // l'inspecteur (`demarrerLecture` existe pour que le clic réponde vite).
  const choix = selectionner(item);
  fileAttente.pistes = depart >= 0 ? vue.lignes.slice(depart) : [item];
  tracerRouteSurCarte(fileAttente.pistes);
  await demarrerLecture(() => invoke("play", { paths: fileAttente.pistes.map((t) => t.path) }));
  await choix;
}

/// Le morceau **choisi**, distinct de celui qu'on écoute : un clic dans une
/// liste, un point de la carte. Ni lecture ni file — `morceauAEditer` le prend
/// pour source d'Éditer, `majApercuTransport` l'affiche dans le transport quand
/// rien n'est chargé. `inspecter` ne le pose pas : il suit aussi la lecture
/// (`battement`), qui n'a pas à écraser un choix explicite.
let pisteSelectionnee = null;

/// Surligne la ligne choisie **en place**, sans reconstruire la fenêtre
/// virtualisée : un `dessiner()` remplacerait les nœuds entre les deux clics
/// d'un double-clic, et le second tomberait sur une ligne toute neuve.
function majSurlignageSelection() {
  for (const el of fenetre.children) {
    const item = vue.lignes[Number(el.dataset.index)];
    el.classList.toggle("ligne--select", !!item && item.path === pisteSelectionnee?.path);
  }
}

async function selectionner(t) {
  pisteSelectionnee = t;
  // Le clic puis le double-clic d'une même ligne ne relancent pas les cinq
  // requêtes de l'inspecteur (crédits, critiques, voisins…) pour rien.
  const inspection =
    $("insp-titre").dataset.path === t.path && !inspectionAlbum ? null : inspecter(t);
  majSurlignageSelection();
  majApercuTransport();
  // En Éditer, choisir une piste, c'est choisir le morceau à travailler : on
  // passe de l'état « choisir » à « séparer / retoucher » — sans lecture.
  if (navigation.mode === "editer") {
    edition.montrerChoix = false;
    edition.sourceChoisie = true;
    await poserSourceEdition();
  }
  await inspection;
}

$("retour").addEventListener("click", () => {
  // En Éditer, depuis la carte de séparation ou l'établi, « ← Choisir un
  // morceau » ramène au sélecteur sans décharger les stems déjà séparés
  // (`docs/ui-spec-editeur.md`, état 1). En état 1, le bouton retrouve son
  // rôle de fil de navigation ordinaire.
  if (
    navigation.mode === "editer" &&
    !edition.montrerChoix &&
    (edition.stems.length || (edition.sourceChoisie && edition.source))
  ) {
    edition.montrerChoix = true;
    edition.sourceChoisie = false;
    majEtatEditer();
    return;
  }
  const r = vue.retour;
  if (r) poser(r.quoi, r.titre, r.lignes, r.retour ?? null, r.scroll ?? 0);
});

// Raccourci « Séparer « … » » de l'en-tête, en Éditer : le morceau en lecture
// devient le morceau choisi, sans avoir à le retrouver dans la liste.
$("editer-candidat").addEventListener("click", async () => {
  const cand = fileAttente.pistes.find((t) => t.path === lecture.courant);
  if (!cand) return;
  await selectionner(cand);
});

/* ---------------------------------------------------------- inspecteur */

/// L'album affiché dans l'inspecteur (`inspecterAlbum`), ou `null` si c'est
/// un morceau (`inspecter`) — l'un exclut l'autre. Sert au bouton ✦, qui n'a
/// pas la même source de chemin selon le cas (voir son gestionnaire de clic),
/// et au bouton ▶ juste à côté (`insp-lecture`), même principe.
let inspectionAlbum = null;

/// Le morceau affiché dans l'inspecteur (`inspecter`), ou `null` si c'est un
/// album de l'anneau/la frise (`inspecterAlbum`) — jumelle d'`inspectionAlbum`,
/// pour le même bouton ▶.
let inspectionPiste = null;

async function inspecter(t) {
  inspectionAlbum = null;
  inspectionPiste = t;
  $("insp-vide").hidden = true;
  $("insp").hidden = false;
  $("insp-erreur").hidden = true;
  // Sert à savoir, au retour d'un calcul, si l'inspecteur montre encore le
  // même morceau.
  $("insp-titre").dataset.path = t.path;
  $("insp-titre").dataset.id = t.id;
  majFavoriBouton();
  $("insp-titre").textContent = txt(t.title, "(sans titre)");
  $("insp-artiste").textContent = txt(t.artist, "(sans artiste)");
  // Repris par le clic sur le nom, ci-dessous : `artist_mbid` manque pour un
  // point de la carte (`MapPoint`, pas `TrackRow`), le nom seul suffit alors
  // à retrouver l'artiste, avec le même repli que `Library::albums_of_artist`.
  $("insp-artiste").dataset.artiste = t.artist ?? "";
  $("insp-artiste").dataset.mbid = t.artist_mbid ?? "";
  $("insp-album").textContent = txt(t.album, "—");
  $("insp-album").dataset.album = t.album ?? "";
  $("insp-album").dataset.artiste = t.artist ?? "";
  $("insp-annee").textContent = t.year ?? "—";
  $("insp-piste").textContent = t.track_no ?? "—";
  $("insp-duree").textContent = duree(t.duration_ms);

  const viseePochette = t.path;
  const img = await pochette(t.path);
  // L'inspecteur a pu changer de morceau pendant la résolution (cache ou
  // lecture disque) — même garde que `montrerDescripteurs` et consorts.
  if ($("insp-titre").dataset.path !== viseePochette) return;
  const el = $("pochette");
  el.style.backgroundImage = img ? `url("${img}")` : "";
  el.classList.toggle("pochette--pleine", Boolean(img));

  montrerDescripteurs(t);
  montrerVoisins(t);
  montrerNomFamille(t);
  montrerCredits(t);
  montrerLabel(t);
  montrerCritiques(t);
}

/// Peuple l'inspecteur pour un album de l'anneau — le même composant que
/// pour un morceau (`inspecter`), généralisé plutôt que dupliqué (voir
/// `docs/carto-anneau.md` § panneau d'information) : pas de piste ni de
/// descripteurs (BPM/tonalité n'ont pas de sens à l'échelle d'un album, les
/// champs retombent sur leur tiret par défaut), les « voisins soniques »
/// sont déjà ceux du dernier `album_ring` (`anneau.voisins`), pas un nouvel
/// appel au moteur.
async function inspecterAlbum(noeud) {
  inspectionAlbum = noeud;
  inspectionPiste = null;
  $("insp-vide").hidden = true;
  $("insp").hidden = false;
  $("insp-erreur").hidden = true;
  // Chaîne vide, jamais un chemin de morceau : c'est ce que les gardes anti-
  // course de `montrerDescripteurs`/`montrerVoisins` comparent pour savoir si
  // l'inspecteur montre encore la même chose — un album n'a pas de chemin,
  // mais doit tout de même invalider une réponse de morceau encore en vol.
  $("insp-titre").dataset.path = "";
  $("insp-titre").dataset.id = "";
  majFavoriBouton();
  $("insp-titre").textContent = noeud.name;
  $("insp-artiste").textContent = noeud.artist;
  $("insp-artiste").dataset.artiste = noeud.artist ?? "";
  $("insp-artiste").dataset.mbid = "";
  $("insp-album").textContent = "—";
  $("insp-album").dataset.album = "";
  $("insp-album").dataset.artiste = "";
  $("insp-annee").textContent = noeud.annee ?? "—";
  $("insp-piste").textContent = "—";
  $("insp-duree").textContent = duree(noeud.duree_ms);
  $("insp-bpm").textContent = "—";
  $("insp-tonalite").textContent = "—";
  $("insp-timbre").textContent = "—";
  // Famille/crédits/label/critiques se demandent par identifiant de piste —
  // un nœud d'album de l'anneau n'en porte pas (son `id` est celui de
  // l'album). Simplification assumée : ces blocs restent cachés à cette
  // échelle, comme `montrerDescripteurs` le fait déjà pour BPM/tonalité. La
  // bio n'est plus de ceux-là : elle vit dans le bandeau centre de l'artiste
  // ouvert, pas dans l'inspecteur (Règle 1, exception du 15 septembre 2026).
  $("bloc-nom-famille").hidden = true;
  $("bloc-credits").hidden = true;
  $("bloc-label").hidden = true;
  $("bloc-critiques").hidden = true;

  const img = noeud.path ? await pochette(noeud.path) : null;
  // L'inspecteur a pu changer d'album pendant la résolution de la pochette.
  if (inspectionAlbum !== noeud) return;
  const el = $("pochette");
  el.style.backgroundImage = img ? `url("${img}")` : "";
  el.classList.toggle("pochette--pleine", Boolean(img));

  montrerVoisinsAlbum();
}

/// Les voisins soniques d'un album, dans l'inspecteur — mêmes gabarit et
/// classe (`.voisin`) que `montrerVoisins`, mais depuis `anneau.voisins`
/// (déjà calculés par `album_ring`) : cliquer l'un d'eux en fait le nouvel
/// album focal, plutôt que de lancer sa lecture (une entrée de l'anneau n'a
/// pas de piste unique à jouer).
function montrerVoisinsAlbum() {
  const bloc = $("bloc-voisins");
  const hote = $("voisins");
  if (anneau.voisins.length === 0) {
    bloc.hidden = true;
    return;
  }
  hote.replaceChildren();
  for (const v of anneau.voisins) {
    const el = document.createElement("button");
    el.className = "voisin";
    el.innerHTML = "<b></b><span></span>";
    el.children[0].textContent = v.name;
    el.children[1].textContent = v.artist;
    el.addEventListener("click", () => chargerAnneau(v.id));
    hote.appendChild(el);
  }
  bloc.hidden = false;
}

/// Ouvre au centre la grille de tous les albums d'un artiste — le geste
/// partagé par le nom de l'artiste dans l'inspecteur et dans le transport.
async function ouvrirAlbumsArtiste(artiste, mbid, retour = ecoute.sommet) {
  if (!artiste) return;
  const albums = await invoke("albums", { artist: artiste, mbid: mbid || null });
  // « Au centre » suppose le mode Écoute : depuis Explorer ou Éditer, le
  // centre montre la carte ou le dock, pas la grille.
  if (navigation.mode !== "ecoute") await basculerMode("ecoute");
  poser("albums", artiste, albums, retour);
  majAutourArtiste(artiste, mbid);
}

/// Masque le bandeau « autour de cet artiste » — l'état par défaut de
/// `poser`, repris à chaque changement de vue.
function masquerAutourArtiste() {
  $("autour-artiste").hidden = true;
  $("autour-bio-bloc").hidden = true;
  $("autour-sonore-bloc").hidden = true;
  $("autour-collab-bloc").hidden = true;
  centreCorps.classList.remove("centre__corps--artiste");
}

/// Jeton du dernier artiste demandé — `majAutourCollab` peut retomber sur un
/// aller-retour MusicBrainz d'environ une seconde ; sans ce garde, cliquer
/// vite d'un artiste à l'autre laisserait parfois la réponse du premier
/// écraser l'affichage du second (même principe que `inspectionAlbum`).
let autourArtisteJeton = 0;

/// Dernier artiste dont le bandeau a été peuplé — `basculerMode` s'en sert
/// pour le restaurer en revenant à l'Écoute (son `else` rappelle `poser` sans
/// connaître le mbid, que `vue` ne porte pas — voir `AlbumRow`).
let dernierArtisteOuvert = { nom: null, mbid: null };

/// Peuple le bandeau sous la grille d'albums d'un artiste : proximité
/// sonique et collaborations connues, deux sources distinctes affichées côte
/// à côte plutôt qu'un classement fusionné — l'une est un score continu,
/// l'autre un fait ponctuel, et les mélanger brouillerait les deux
/// (`docs/interface-guidelines.md`, Règle 9). Les deux tolèrent l'échec en
/// silence : un bandeau secondaire ne doit jamais bloquer la grille
/// principale, déjà posée par l'appelant.
function majAutourArtiste(artiste, mbid) {
  dernierArtisteOuvert = { nom: artiste, mbid: mbid ?? null };
  const jeton = ++autourArtisteJeton;
  $("autour-artiste").hidden = false;
  centreCorps.classList.add("centre__corps--artiste");
  // `.grille` passe de « bornée par la fenêtre, défilée pour son compte » à
  // « prend toute sa hauteur de contenu » (voir la classe dans le CSS) : son
  // `clientHeight` change, donc la fenêtre de rangées déjà posée par `poser`
  // (calculée sous l'ancien mode) doit être redessinée pour couvrir tous les
  // albums, pas seulement ceux qui tenaient dans l'ancienne fenêtre bornée.
  ecoute.grilleDernierRang = -1;
  dessinerGrille();
  montrerBio(mbid, jeton);
  majAutourSonore(artiste, jeton);
  majAutourCollab(mbid, jeton);
}

/// « Sonne comme » : les artistes les plus proches par empreinte CLAP
/// (`artistes_proches`, centroïdes d'album agrégés par artiste côté moteur),
/// montrés par une pochette suggérée (`artist_covers`, comme la mosaïque de
/// `carteArtiste`) plutôt qu'un simple nom — plus agréable à l'œil, et une
/// invitation à découvrir plus explicite qu'une pastille de texte. L'opacité
/// de chaque tuile retombe avec l'éloignement, jusqu'à 45 % pour la plus
/// lointaine montrée — la force du lien, pas sa catégorie.
async function majAutourSonore(artiste, jeton) {
  const bloc = $("autour-sonore-bloc");
  let proches = [];
  try {
    proches = await invoke("artistes_proches", { artiste, k: 8 });
  } catch (e) {
    remonter(e, "autour de l'artiste");
  }
  if (jeton !== autourArtisteJeton) return;
  if (proches.length === 0) {
    bloc.hidden = true;
    return;
  }
  const pire = proches[proches.length - 1][1] || 1;
  const hote = $("autour-sonore");
  hote.replaceChildren();
  for (const [nom, distance] of proches) {
    const el = document.createElement("button");
    el.className = "proche-pochette";
    el.style.opacity = String(1 - 0.55 * (distance / pire));
    el.innerHTML = `<div class="proche-pochette__image"><span></span></div>
                     <span class="proche-pochette__nom"></span>`;
    el.querySelector(".proche-pochette__nom").textContent = nom;
    el.addEventListener("click", () => ouvrirAlbumsArtiste(nom, null));
    hote.appendChild(el);

    const image = el.querySelector(".proche-pochette__image");
    const repli = () => {
      if (!el.isConnected) return;
      image.classList.add("proche-pochette__image--vide");
      image.style.setProperty("--teinte", teinteNom(nom));
      image.querySelector("span").textContent = initiales(nom);
    };
    invoke("artist_covers", { name: nom, mbid: null, max: 1 })
      .then((chemins) => (chemins.length > 0 ? pochette(chemins[0]) : null))
      .then((img) => {
        if (!el.isConnected) return;
        if (img) image.style.backgroundImage = `url("${img}")`;
        else repli();
      })
      .catch(repli);
  }
  bloc.hidden = false;
}

/// « A aussi joué avec » : les relations MusicBrainz de cet artiste
/// (`artist_links`, le même cache que le mode Découvrir) restreintes à celles
/// qui mènent vers un artiste déjà dans la bibliothèque — un clic y conduit
/// toujours ses propres albums. Le reste (artistes hors bibliothèque) reste
/// le terrain de Découvrir, pas un doublon ici.
async function majAutourCollab(mbid, jeton) {
  const bloc = $("autour-collab-bloc");
  if (!mbid) {
    bloc.hidden = true;
    return;
  }
  let liens = [];
  try {
    liens = await invoke("artist_links", { mbid, contact: contactMb() });
  } catch {
    // Cache vide et pas de contact MusicBrainz renseigné : silence, ce
    // bandeau est secondaire (voir `majAutourArtiste`).
    if (jeton === autourArtisteJeton) bloc.hidden = true;
    return;
  }
  await chargerArtistesDecouvrir();
  if (jeton !== autourArtisteJeton) return;
  const connus = new Map();
  for (const [dstMbid, dstNom] of liens) {
    if (dstMbid !== mbid && artistesDecouvrables.some((a) => a.mbid === dstMbid)) {
      connus.set(dstMbid, dstNom);
    }
  }
  if (connus.size === 0) {
    bloc.hidden = true;
    return;
  }
  const hote = $("autour-collab");
  hote.replaceChildren();
  for (const [dstMbid, dstNom] of connus) {
    const el = document.createElement("button");
    el.className = "proche";
    el.textContent = dstNom;
    el.addEventListener("click", () => ouvrirAlbumsArtiste(dstNom, dstMbid));
    hote.appendChild(el);
  }
  bloc.hidden = false;
}

/// Radio « dans l'esprit de cet artiste » — même mécanisme que le ✦ d'une
/// case d'album ou de l'inspecteur (`genererAlchimie`/`insp-alchimie`), mais
/// le pivot (`path_artist`, côté moteur) est cherché sur l'ensemble des
/// morceaux de l'artiste plutôt qu'un seul album : l'équivalent de
/// « Sonne comme » mais pour écouter tout de suite plutôt que naviguer.
$("autour-radio").addEventListener("click", () => {
  const { nom, mbid } = dernierArtisteOuvert;
  if (!nom) return;
  composerAlchimie({
    bouton: $("autour-radio"),
    origine: `Radio à partir de ${nom}`,
    chemin: () =>
      invoke("path_artist", {
        artist: nom,
        mbid: mbid || null,
        steps: ALCHIMIE_PISTES,
        seed: Math.floor(Math.random() * 2 ** 31),
        bruit: bruitChemin,
      }),
    demarrer: (pistes) => invoke("play", { paths: pistes.map((t) => t.path) }),
  });
});

/// Playlist de cet artiste — uniquement ses propres morceaux, mélangés côté
/// moteur (`shuffle_artist`) à chaque clic : pas une dérive sonique comme
/// « Radio à partir de cet artiste », donc pas de graphe à préparer ni de
/// route à tracer sur la carte (`composerAlchimie({graphe: false})`).
$("autour-playlist").addEventListener("click", () => {
  const { nom, mbid } = dernierArtisteOuvert;
  if (!nom) return;
  composerAlchimie({
    bouton: $("autour-playlist"),
    origine: `Playlist de ${nom}`,
    graphe: false,
    chemin: () => invoke("shuffle_artist", { artist: nom, mbid: mbid || null }),
    demarrer: (pistes) => invoke("play", { paths: pistes.map((t) => t.path) }),
  });
});

/// Le nom de l'artiste, dans l'inspecteur, ouvre ses albums au centre — le
/// même geste que cliquer l'artiste depuis la liste « Artistes », mais depuis
/// n'importe quel morceau inspecté (piste, voisin sonique, point de la carte).
$("insp-artiste").addEventListener("click", () =>
  ouvrirAlbumsArtiste($("insp-artiste").dataset.artiste, $("insp-artiste").dataset.mbid),
);

/// Le nom de l'album, dans l'inspecteur, ouvre ses pistes au centre — même
/// geste et même principe que le nom de l'artiste juste au-dessus.
$("insp-album").addEventListener("click", async () => {
  const album = $("insp-album").dataset.album;
  if (!album) return;
  const artiste = $("insp-album").dataset.artiste || null;
  const pistes = await invoke("tracks_of_album", { album, artist: artiste });
  if (navigation.mode !== "ecoute") await basculerMode("ecoute");
  poser("pistes", album, pistes, ecoute.sommet);
});

/// Le titre du morceau en cours, dans le transport, peuple l'inspecteur —
/// qui donne alors, gratuitement, la re-navigation vers son album ou les
/// albums de l'artiste (les deux gestionnaires juste au-dessus). Rien ne
/// joue : `dataset.path` est vide, `find` ne trouve rien, no-op.
$("np-titre").addEventListener("click", () => {
  const t = fileAttente.pistes.find((x) => x.path === $("np-titre").dataset.path);
  if (t) inspecter(t);
});

/// L'artiste du morceau en cours, dans le transport, ouvre ses albums au
/// centre — même fonction que le nom d'artiste de l'inspecteur, prévue pour
/// ce second usage (voir son commentaire ci-dessus).
$("np-artiste").addEventListener("click", () =>
  ouvrirAlbumsArtiste($("np-artiste").dataset.artiste, $("np-artiste").dataset.mbid),
);

/// Bouton ▶ de l'inspecteur : joue le morceau ou l'album affiché — même
/// principe que le bouton ✦ juste en dessous (`inspectionAlbum` tranche entre
/// les deux). C'est le seul geste qui lance l'écoute depuis les quatre
/// visualisations d'Explorer (Nuage/Carte inclus) : cliquer un point ou un
/// album n'y fait plus que sélectionner/naviguer, jamais jouer — cohérence
/// voulue entre les quatre modes plutôt qu'un clic qui coupe la lecture en
/// cours ici et pas là (voir le clic du canevas, `cnv` → `click`). Toujours
/// visible sur la pochette, pas seulement au survol (`style.css`), pour que
/// ce geste reste devinable.
$("insp-lecture").addEventListener("click", async () => {
  if (inspectionAlbum) {
    await lireAlbum(inspectionAlbum).catch((err) => remonter(err, "lireAlbum (inspecteur)"));
    return;
  }
  if (!inspectionPiste) return;
  selectionner(inspectionPiste);
  fileAttente.pistes = [inspectionPiste];
  tracerRouteSurCarte(fileAttente.pistes);
  await demarrerLecture(() => invoke("play", { paths: [inspectionPiste.path] }));
});

/// Playlist « dans l'esprit de ce morceau » — même mécanisme que le bouton ✦
/// d'une case d'album (`genererAlchimie`), mais partie d'un seul morceau déjà
/// connu de la carte : une errance sonique depuis son point, pas besoin de
/// lui chercher un centre au préalable.
$("insp-alchimie").addEventListener("click", () => {
  // L'inspecteur montre soit un morceau (`inspecter`), soit un album de
  // l'anneau (`inspecterAlbum`) — l'un exclut l'autre (`inspectionAlbum`).
  // Un album n'a pas un seul morceau d'où partir : `path_album` (déjà
  // spécifié pour la case ✦ de la grille) part du morceau le plus central.
  if (inspectionAlbum) {
    const { name, artist } = inspectionAlbum;
    composerAlchimie({
      bouton: $("insp-alchimie"),
      origine: `Dans l'esprit de « ${name} »`,
      chemin: () =>
        invoke("path_album", {
          album: name,
          artist: artist || null,
          steps: ALCHIMIE_PISTES,
          seed: Math.floor(Math.random() * 2 ** 31),
          bruit: bruitChemin,
        }),
      demarrer: (pistes) => invoke("remplacer_file", { paths: pistes.map((t) => t.path) }),
    });
    return;
  }
  const id = Number($("insp-titre").dataset.id);
  if (!Number.isFinite(id) || $("insp-titre").dataset.id === "") return;
  composerAlchimie({
    bouton: $("insp-alchimie"),
    origine: `Dans l'esprit de « ${$("insp-titre").textContent} »`,
    chemin: () =>
      invoke("path", {
        from: id,
        mode: "errance",
        steps: ALCHIMIE_PISTES,
        seed: Math.floor(Math.random() * 2 ** 31),
        bruit: bruitChemin,
      }),
    // `remplacer_file`, pas `set_queue` : la playlist doit devenir la file
    // dès le morceau suivant. `set_queue` gardait le préchargement de la
    // file précédente (les résultats de recherche), qui s'intercalait avant
    // la playlist. Le morceau en cours, lui, continue sans coupure.
    demarrer: (pistes) => invoke("remplacer_file", { paths: pistes.map((t) => t.path) }),
  });
});

/// Tempo et tonalité sous le titre du transport.
///
/// Séparé de l'inspecteur, qui suit la **sélection** : le transport suit ce
/// qu'on **écoute**, et les deux divergent dès qu'on explore la carte sans
/// changer de morceau.
/// Avertissement transitoire du lecteur — une piste sautée —, dans la ligne
/// `#np-mesures` du transport : pas de bandeau ni de toast
/// (`docs/interface-guidelines.md`), le transport a déjà cette ligne. Les
/// trois endroits qui l'écrivent (`mesuresDuTransport` et les deux
/// remises à zéro au changement de morceau) la laissent tranquille tant que
/// l'avertissement dure — la piste suivante démarre juste après le saut, et
/// effacerait sinon le message avant qu'on ait pu le lire.
const ALERTE_TRANSPORT_MS = 8000;
let alerteTransportJusqua = 0;
let alerteTransportMinuteur = null;

function alerteTransportActive() {
  return Date.now() < alerteTransportJusqua;
}

function alerteTransport(texte) {
  const el = $("np-mesures");
  el.textContent = texte;
  el.title = texte; // la ligne est tronquée par une ellipse
  el.classList.add("np__mesures--alerte");
  alerteTransportJusqua = Date.now() + ALERTE_TRANSPORT_MS;
  clearTimeout(alerteTransportMinuteur);
  alerteTransportMinuteur = setTimeout(() => {
    el.classList.remove("np__mesures--alerte");
    el.title = "";
    el.textContent = "";
    // Rend la ligne à ce qu'elle montre d'ordinaire (tempo, tonalité).
    const t = fileAttente.pistes.find((x) => x.path === lecture.courant);
    if (t) mesuresDuTransport(t);
  }, ALERTE_TRANSPORT_MS);
}

/// Dit à l'utilisateur quelles pistes le lecteur vient de sauter
/// (`playback_state.ignorees`, relevées une seule fois côté moteur).
function signalerPistesIgnorees(ignorees) {
  if (!ignorees || !ignorees.length) return;
  if (ignorees.length > 1) {
    alerteTransport(`${ignorees.length} pistes illisibles ignorées`);
    return;
  }
  const { path } = ignorees[0];
  const t = fileAttente.pistes.find((x) => x.path === path);
  const nom = t ? txt(t.title, "(sans titre)") : path.split("/").pop();
  alerteTransport(`Illisible : ${nom} — suivante`);
}

async function mesuresDuTransport(t) {
  const vise = t.path;
  let d;
  try {
    d = await invoke("descripteurs", { id: t.id });
  } catch {
    return;
  }
  if (!d || !estAffiche(vise) || alerteTransportActive()) return;
  const bouts = [];
  if (d.bpm) bouts.push(`${Math.round(d.bpm)} BPM`);
  const ton = tonaliteFr(d.tonalite);
  if (ton) bouts.push(ton);
  $("np-mesures").textContent = bouts.join(" · ");
}

/// Conteneurs sans perte : on y affiche la profondeur de bits plutôt que le
/// débit (qui n'y est pas constant et ne dit rien de la qualité).
const CODECS_SANS_PERTE = new Set(["FLAC", "WAV", "AIFF", "APE", "WavPack", "ALAC"]);

/// 44100 → « 44,1 kHz » ; 48000 → « 48 kHz ».
function khz(hz) {
  const v = hz / 1000;
  return `${(Number.isInteger(v) ? v : v.toFixed(1)).toString().replace(".", ",")} kHz`;
}

/// Ligne de qualité du transport : « FLAC · 16 bit · 44,1 kHz »,
/// « MP3 · 320 kb/s · 44,1 kHz ». Segments omis quand la donnée manque.
function formatQualite(q) {
  const bouts = [];
  if (q.codec) bouts.push(q.codec);
  const sansPerte = q.codec && CODECS_SANS_PERTE.has(q.codec);
  if (sansPerte && q.bit_depth) bouts.push(`${q.bit_depth} bit`);
  else if (!sansPerte && q.bitrate) bouts.push(`${q.bitrate} kb/s`);
  if (q.sample_rate) bouts.push(khz(q.sample_rate));
  if (q.channels === 1) bouts.push("mono");
  return bouts.join(" · ");
}

/// Qualité du fichier sous le compteur de temps. Comme `mesuresDuTransport` :
/// suit ce qu'on **écoute**, chargé à la volée, vide si le morceau n'a pas été
/// scanné pour son format.
async function qualiteDuTransport(t) {
  const vise = t.path;
  let q;
  try {
    q = await invoke("qualite_piste", { id: t.id });
  } catch {
    return;
  }
  if (!q || !estAffiche(vise)) return;
  $("np-qualite").textContent = formatQualite(q);
}

/// Noms français des douze classes de hauteur.
///
/// **La base note à l'anglaise** — `C`, `F#`, `A` — parce que c'est ce
/// qu'écrivent les profils de Krumhansl-Schmuckler et tout le domaine. Ici on
/// affiche, donc on traduit. Les altérations restent des dièses : la mesure ne
/// distingue pas un fa dièse d'un sol bémol, et choisir l'un des deux
/// prétendrait le contraire.
const NOTES_FR = {
  C: "Do", "C#": "Do♯", D: "Ré", "D#": "Ré♯", E: "Mi", F: "Fa",
  "F#": "Fa♯", G: "Sol", "G#": "Sol♯", A: "La", "A#": "La♯", B: "Si",
};

/// « F min » devient « Fa mineur ». Rend la chaîne telle quelle si elle ne se
/// lit pas — mieux vaut afficher ce qu'on a que de le perdre.
function tonaliteFr(t) {
  if (!t) return null;
  const [note, mode] = String(t).split(/\s+/);
  const fr = NOTES_FR[note];
  if (!fr) return t;
  return `${fr} ${mode === "min" ? "mineur" : mode === "maj" ? "majeur" : (mode ?? "")}`.trim();
}

/// Tempo et tonalité du morceau inspecté.
///
/// **Un tiret quand ce n'est pas mesuré, jamais une valeur par défaut.** La
/// Passerelle vers `signalerErreur` pour les chargements secondaires de
/// l'inspecteur (descripteurs, voisins, bio, crédits, critiques) : n'affiche
/// rien si l'inspecteur montre déjà un autre morceau (changé pendant
/// l'appel), et démasque `#insp-erreur` — contrairement aux slots que
/// `signalerErreur` réutilise ailleurs (`#chemin-aide`, `#dock-aide`…),
/// celui-ci reste caché tant que rien n'a échoué.
function signalerErreurInspecteur(vise, texte, e, source) {
  if ($("insp-titre").dataset.path !== vise) return;
  signalerErreur("insp-erreur", texte, e, source);
  $("insp-erreur").hidden = false;
}

/// passe couvre 15 847 morceaux sur 27 044 ; afficher « 120 BPM » sur le reste
/// donnerait une mesure qu'on n'a pas.
async function montrerDescripteurs(t) {
  const vise = t.path;
  $("insp-bpm").textContent = "—";
  $("insp-tonalite").textContent = "—";
  $("insp-timbre").textContent = "—";
  let d;
  try {
    d = await invoke("descripteurs", { id: t.id });
  } catch (e) {
    signalerErreurInspecteur(vise, "échec du chargement des descripteurs", e, "descripteurs");
    return;
  }
  // L'inspecteur a pu changer de morceau pendant l'appel.
  if (!d || $("insp-titre").dataset.path !== vise) return;
  if (d.bpm) $("insp-bpm").textContent = `${Math.round(d.bpm)} BPM`;
  const ton = tonaliteFr(d.tonalite);
  if (ton) $("insp-tonalite").textContent = ton;
  // Descripteurs timbraux — mêmes réserves que tempo/tonalité : un tiret
  // tant qu'un morceau n'est pas passé dans la passe, jamais une valeur
  // inventée.
  const timbre = [];
  if (d.centroide_moy) timbre.push(`centroïde ${Math.round(d.centroide_moy)} Hz`);
  if (d.flatness_moy != null) timbre.push(`aplatissement ${d.flatness_moy.toFixed(2)}`);
  if (d.zcr != null) timbre.push(`ZCR ${d.zcr.toFixed(2)}`);
  if (timbre.length) $("insp-timbre").textContent = timbre.join(" · ");
}

/// Les morceaux les plus proches à l'oreille du moteur.
///
/// Seuls les morceaux analysés en ont : tant que la passe tourne, la plupart
/// n'y figurent pas encore, et le bloc reste caché plutôt que vide.
async function montrerVoisins(t) {
  const bloc = $("bloc-voisins");
  const hote = $("voisins");
  const vise = t.path;
  bloc.hidden = true;

  let proches = [];
  try {
    proches = await invoke("neighbours", { id: t.id, count: 6 });
  } catch (e) {
    signalerErreurInspecteur(vise, "échec du chargement des voisins soniques", e, "voisins");
    return;
  }
  // L'inspecteur a pu changer de morceau pendant le calcul.
  if (proches.length === 0 || $("insp-titre").dataset.path !== vise) return;

  hote.replaceChildren();
  for (const v of proches) {
    const el = document.createElement("button");
    el.className = "voisin";
    el.innerHTML = "<b></b><span></span>";
    el.children[0].textContent = txt(v.title, "(sans titre)");
    el.children[1].textContent = txt(v.artist, "(sans artiste)");
    el.addEventListener("click", async () => {
      selectionner(v);
      fileAttente.pistes = [v];
      tracerRouteSurCarte(fileAttente.pistes);
      await demarrerLecture(() => invoke("play", { paths: [v.path] }));
    });
    hote.appendChild(el);
  }
  bloc.hidden = false;
}

/// Biographie d'artiste (TheAudioDB) — résolue par MBID seulement, jamais par
/// nom (`docs/enrichissement-lecteur.md`). FR si connue, sinon EN ; le bloc
/// reste caché si aucune des deux n'est disponible — jamais de texte inventé.
/// Vit dans le bandeau « autour de l'artiste » au centre, pas dans
/// l'inspecteur (`docs/interface-guidelines.md`, Règle 1, exception du 15
/// septembre 2026) : elle dépend de l'artiste ouvert au centre, pas du
/// morceau en cours d'écoute — même garde de jeton que `majAutourSonore`.
async function montrerBio(mbid, jeton) {
  const bloc = $("autour-bio-bloc");
  bloc.hidden = true;
  if (!mbid) return;

  let bio = null;
  try {
    bio = await invoke("bio_artiste", { mbid });
  } catch (e) {
    remonter(e, "biographie de l'artiste");
    return;
  }
  if (jeton !== autourArtisteJeton || !bio) return;
  const texte = bio.biographie_fr || bio.biographie_en;
  if (!texte) return;
  $("autour-bio").textContent = texte;
  bloc.hidden = false;
}

/// Le nom de la famille du morceau — vote entre MusicBrainz, le vocabulaire
/// CLAP-texte et Last.fm (`docs/nommage-familles.md`). Le vote lui-même est
/// par famille, pas par morceau : servi une fois par `chargerFamilles()` et
/// mis en cache dans `carte.familleVotes`, consulté ici plutôt que redemandé
/// — seul le numéro de famille du morceau manque parfois (un point de la
/// carte le porte déjà, `t.cluster` ; les autres vues de piste non), d'où le
/// seul appel réseau de cette fonction.
async function montrerNomFamille(t) {
  const bloc = $("bloc-nom-famille");
  const vise = t.path;
  bloc.hidden = true;
  $("insp-famille-incertain").hidden = true;
  $("insp-famille-affinage").hidden = true;

  let cluster = t.cluster;
  if (cluster === undefined || cluster === null) {
    try {
      cluster = await invoke("famille_piste", { id: t.id });
    } catch (e) {
      signalerErreurInspecteur(vise, "échec du chargement de la famille", e, "famille");
      return;
    }
    if ($("insp-titre").dataset.path !== vise) return;
  }
  if (cluster === null || cluster === undefined || cluster < 0) return;

  await chargerFamilles();
  if ($("insp-titre").dataset.path !== vise) return;
  const vote = (carte.familleVotes ?? []).find((v) => v.cluster === cluster);
  if (!vote) return;

  $("insp-famille-nom").textContent = vote.nom_affiche;
  if (!vote.fiable) {
    const hote = $("insp-famille-propositions");
    hote.replaceChildren();
    for (const [source, nom] of [
      ["MusicBrainz", vote.musicbrainz],
      ["CLAP-texte", vote.clap_texte],
      ["Last.fm", vote.lastfm],
    ]) {
      if (!nom) continue;
      const li = document.createElement("li");
      li.textContent = `${source} : ${nom}`;
      hote.appendChild(li);
    }
    $("insp-famille-incertain").hidden = false;
    $("insp-famille-affiner").dataset.cluster = String(cluster);
  }
  bloc.hidden = false;
}

/// Affinage local d'une famille « incertaine », suggéré jamais automatique
/// (`docs/nommage-familles.md`) : un k-means restreint aux seuls morceaux de
/// cette famille, chaque sous-groupe nommé par le même vote. N'écrit rien en
/// base — ni territoire de carte, ni bande d'anneau ou de frise.
$("insp-famille-affiner").addEventListener("click", async () => {
  const bouton = $("insp-famille-affiner");
  const cluster = Number(bouton.dataset.cluster);
  if (Number.isNaN(cluster)) return;
  const hote = $("insp-famille-affinage");
  bouton.disabled = true;
  bouton.textContent = "Affinage…";
  try {
    const sous = await invoke("subdiviser_famille", { cluster });
    hote.replaceChildren();
    for (const s of sous) {
      const p = document.createElement("p");
      p.textContent = `${s.nom_affiche} (${s.effectif.toLocaleString("fr-FR")})`;
      hote.appendChild(p);
    }
    hote.hidden = sous.length === 0;
  } catch (e) {
    remonter(e, "affinage de famille");
  } finally {
    bouton.disabled = false;
    bouton.textContent = "Affiner cette famille ?";
  }
});

/// Le champ de clé Last.fm n'a de sens que la case cochée — décochée par
/// défaut (contrairement aux trois autres sources, Last.fm exige une clé
/// personnelle, voir `docs/nommage-familles.md`).
$("lastfm-active").addEventListener("change", () => {
  $("lastfm-cle-ligne").hidden = !$("lastfm-active").checked;
});

/// Crédits par édition (Discogs, dumps CC0) — musicien de session,
/// producteur, ingénieur du son… Vide tant que la liaison MusicBrainz →
/// Discogs et l'import mensuel n'ont pas couvert cette édition précise.
async function montrerCredits(t) {
  const bloc = $("bloc-credits");
  const hote = $("insp-credits");
  const vise = t.path;
  bloc.hidden = true;

  let credits = [];
  try {
    credits = await invoke("credits_piste", { id: t.id });
  } catch (e) {
    signalerErreurInspecteur(vise, "échec du chargement des crédits", e, "crédits");
    return;
  }
  if (credits.length === 0 || $("insp-titre").dataset.path !== vise) return;

  hote.replaceChildren();
  for (const c of credits) {
    const li = document.createElement("li");
    li.textContent = c.pistes ? `${isoler(c.personne)} — ${c.role} (${c.pistes})` : `${isoler(c.personne)} — ${c.role}`;
    hote.appendChild(li);
  }
  bloc.hidden = false;
}

/// Label et numéro de catalogue par édition (Discogs, dumps CC0) — pure
/// métadonnée, pas d'entrée vers un mode de navigation par label (décision
/// utilisateur explicite). Vide tant que la liaison et l'import mensuel
/// n'ont pas couvert cette édition.
async function montrerLabel(t) {
  const bloc = $("bloc-label");
  const hote = $("insp-label");
  const vise = t.path;
  bloc.hidden = true;

  let labels = [];
  try {
    labels = await invoke("labels_piste", { id: t.id });
  } catch (e) {
    signalerErreurInspecteur(vise, "échec du chargement du label", e, "label");
    return;
  }
  if (labels.length === 0 || $("insp-titre").dataset.path !== vise) return;

  hote.replaceChildren();
  for (const l of labels) {
    const li = document.createElement("li");
    li.textContent = l.catno ? `${l.nom} (${l.catno})` : l.nom;
    hote.appendChild(li);
  }
  bloc.hidden = false;
}

/// Critiques d'album (CritiqueBrainz) — licence CC (BY-SA ou BY-NC-SA selon
/// la critique, jamais supposée uniforme) : attribution obligatoire à chaque
/// critique affichée, jamais mutualisée en une seule mention. La plupart des
/// albums n'en ont aucune — état normal, pas une erreur : on invite alors à en
/// écrire une plutôt que de laisser un vide silencieux.
async function montrerCritiques(t) {
  const bloc = $("bloc-critiques");
  const hote = $("insp-critiques");
  const lienBloc = $("insp-ecrire-critique");
  const vise = t.path;
  bloc.hidden = true;
  lienBloc.hidden = true;
  // Vidé ici, pas seulement dans la branche « critiques trouvées » plus bas :
  // sinon une critique de l'ancien morceau reste dans le DOM et réapparaît
  // dès que le bloc redevient visible pour l'invite « écrire une critique ».
  hote.replaceChildren();

  let critiques = [];
  try {
    critiques = await invoke("critiques_piste", { id: t.id });
  } catch (e) {
    signalerErreurInspecteur(vise, "échec du chargement des critiques", e, "critiques");
    return;
  }
  if ($("insp-titre").dataset.path !== vise) return;

  if (critiques.length === 0) {
    let lien = null;
    try {
      lien = await invoke("lien_ecrire_critique", { id: t.id });
    } catch {
      /* pas de release-group connu : ni critique ni invitation à en écrire */
    }
    if (lien && $("insp-titre").dataset.path === vise) {
      $("insp-lien-critique").dataset.url = lien;
      lienBloc.hidden = false;
      bloc.hidden = false;
    }
    return;
  }

  for (const c of critiques) {
    const el = document.createElement("article");
    el.className = "critique";
    const texte = document.createElement("p");
    texte.textContent = c.texte;
    const attribution = document.createElement("p");
    attribution.className = "critique-attribution";
    attribution.append(`${txt(c.auteur, "auteur inconnu")} · `);
    if (c.url_originale) {
      const lien = boutonLien("CritiqueBrainz", () => ouvrirLien(c.url_originale));
      lien.classList.add("lien--inline");
      attribution.append(lien);
    } else {
      attribution.append("CritiqueBrainz");
    }
    attribution.append(` · ${txt(c.licence_nom, c.licence_id)}`);
    el.append(texte, attribution);
    hote.appendChild(el);
  }
  bloc.hidden = false;
}

$("insp-lien-critique").addEventListener("click", (e) => {
  e.preventDefault();
  const url = e.currentTarget.dataset.url;
  if (url) ouvrirLien(url);
});

export {
  activer, alerteTransportActive, construireIndexAlpha, construireIndexAnnees,
  dernierArtisteOuvert, favorisChemins, inspecter, inspecterAlbum, lancerLecture, lignePlaylist,
  majAutourArtiste, majIndexActif, majMinuteur, masquerAutourArtiste, mesuresDuTransport,
  ouvrirAlbumsArtiste, pisteSelectionnee, poser, qualiteDuTransport, rafraichirMinuteur,
  scrollActuel, selectionner, signalerPistesIgnorees, texteCompte,
};
