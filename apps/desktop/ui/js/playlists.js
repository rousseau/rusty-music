import "./editions.js"; // l'ordre d'exécution : voir main.js
// Les playlists enregistrées (`docs/plan-ecouter-v0.2.md`, point 4) : les
// enregistrer depuis la file, la vue Playlists d'Écouter, et, depuis P1 (9 nov.
// 2026), les gérer piste par piste : « Ajouter à une playlist » partout où un
// morceau ou un album se désigne, retirer et réordonner dans une playlist
// ouverte, en créer une vide. Sorti d'`ecouter.js`, qui ne doit plus grossir.
import {
  $, fileAttente, invoke, isoler, LIGNE, navigation, remonter, signalerErreur, txt, vue,
} from "./socle.js";
import { dessiner } from "./vues.js";
import {
  inspectionCourante, pisteSelectionnee, poser, scrollActuel, selectionner,
} from "./ecouter.js";
import { demarrerLecture } from "./lecteur.js";
import { tracerRouteSurCarte } from "./explorer/chemins.js";
import { dureeLongue } from "./bibliotheque.js";
import { pratiquerPlaylist } from "./pratiquer-playlist.js";

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
    el.appendChild(boutonAction("♩", "Pratiquer cette playlist", () =>
      pratiquerPlaylist(item.id).catch((e) => remonter(e, "playlist à travailler"))));
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
  await finirRetrait();
  // Une liste calculée se lit et s'ouvre, mais ne se retouche pas.
  ouverte = item.intelligente ? null : { id: item.id, nom: item.nom, lignes: pistes };
  poser("pistes", item.nom, pistes, {
    quoi: "playlists", titre: vue.titre, lignes: vue.lignes, scroll: scrollActuel(),
  });
  majCompte();
}


/* ------------------------------------------- P1 : gérer une playlist
 *
 * Le moteur repère une piste par son rang **dans la liste affichée** (les
 * manquantes n'y sont pas) et par son chemin, pour refuser une retouche si la
 * playlist a changé ailleurs (`retirer_de_playlist`…) : la vue est alors
 * rouverte telle qu'elle est.
 */

/// La playlist enregistrée ouverte au centre : `{ id, nom, lignes }`, où
/// `lignes` est le tableau même de `vue.lignes` (retouché en place).
let ouverte = null;

function estPlaylistOuverte() {
  return !!ouverte && vue.quoi === "pistes" && vue.lignes === ouverte.lignes;
}

/// Le retrait qu'on peut encore annuler : la ligne reste, estompée, avec
/// « annuler », le temps du message.
let retrait = null;
const RETRAIT_MS = 5000;

/// Compte et durée de la playlist ouverte, dans le fil.
function majCompte() {
  if (!estPlaylistOuverte() || navigation.mode !== "ecoute") return;
  const pistes = ouverte.lignes.filter((t) => t !== retrait?.piste);
  const ms = pistes.reduce((s, t) => s + (t.duration_ms || 0), 0);
  const n = pistes.length;
  $("fil-compte").textContent = n
    ? `${n} morceau${n > 1 ? "x" : ""} · ${dureeLongue(ms / 1000)}`
    : "vide";
}

/// La playlist a changé ailleurs, ou n'existe plus : on la rouvre telle
/// qu'elle est (ou on revient à la liste), et le fil dit pourquoi.
async function perimee(e) {
  retrait = null;
  const id = ouverte?.id;
  const liste = await invoke("playlists");
  const item = liste.find((p) => p.id === id);
  if (item) {
    const pistes = await invoke("playlist_pistes", { id });
    ouverte = { id, nom: item.nom, lignes: pistes };
    poser("pistes", item.nom, pistes, vue.retour, scrollActuel());
  } else {
    ouverte = null;
    await chargerPlaylists();
  }
  $("fil-compte").textContent = String(e);
}

/// Retire `item` (rang `index` de la liste affichée) ; la ligne reste,
/// estompée, avec « annuler », puis s'en va.
async function retirer(item, index) {
  await finirRetrait();
  const id = ouverte.id;
  try {
    await invoke("retirer_de_playlist", { id, rang: index, path: item.path });
  } catch (e) {
    await perimee(e);
    return;
  }
  retrait = { piste: item, id, minuteur: setTimeout(() => finirRetrait(), RETRAIT_MS) };
  dessiner();
  majCompte();
}

/// Le retrait en attente devient définitif : la ligne quitte la liste.
async function finirRetrait() {
  if (!retrait) return;
  const { piste, minuteur } = retrait;
  clearTimeout(minuteur);
  retrait = null;
  const i = ouverte?.lignes.indexOf(piste) ?? -1;
  if (i >= 0) ouverte.lignes.splice(i, 1);
  if (estPlaylistOuverte()) {
    dessiner();
    majCompte();
  }
}

async function annulerRetrait() {
  if (!retrait) return;
  const { piste, id, minuteur } = retrait;
  clearTimeout(minuteur);
  retrait = null;
  const rang = ouverte?.id === id ? ouverte.lignes.indexOf(piste) : -1;
  try {
    await invoke("remettre_dans_playlist", { id, rang: Math.max(0, rang), path: piste.path });
  } catch (e) {
    await perimee(e);
    return;
  }
  if (estPlaylistOuverte()) {
    dessiner();
    majCompte();
  }
}

/// Déplace la piste du rang `de` au rang `a` (rang dans la liste après le
/// déplacement, comme la file).
async function deplacer(de, a) {
  await finirRetrait();
  if (!estPlaylistOuverte()) return;
  const lignes = ouverte.lignes;
  const piste = lignes[de];
  if (!piste || a < 0 || a >= lignes.length || a === de) return;
  try {
    await invoke("deplacer_dans_playlist", { id: ouverte.id, de, a, path: piste.path });
  } catch (e) {
    await perimee(e);
    return;
  }
  lignes.splice(de, 1);
  lignes.splice(a, 0, piste);
  // La ligne déplacée reste à l'écran : on suit le clavier.
  const liste = $("liste");
  const haut = a * LIGNE;
  if (haut < liste.scrollTop) liste.scrollTop = haut;
  else if (haut + LIGNE > liste.scrollTop + liste.clientHeight) liste.scrollTop = haut + LIGNE - liste.clientHeight;
  dessiner();
}

/// Glisser une ligne d'une playlist ouverte : même geste et même filet que
/// la file d'attente.
let glissee = null;
function cablerGlissement(el, item) {
  el.draggable = true;
  el.addEventListener("dragstart", (ev) => {
    glissee = item;
    el.classList.add("ligne--glissee");
    ev.dataTransfer.effectAllowed = "move";
    ev.dataTransfer.setData("text/plain", item.path);
  });
  el.addEventListener("dragend", () => {
    glissee = null;
    document
      .querySelectorAll(".ligne--glissee, .ligne--depose-dessus, .ligne--depose-dessous")
      .forEach((n) => n.classList.remove("ligne--glissee", "ligne--depose-dessus", "ligne--depose-dessous"));
  });
  el.addEventListener("dragover", (ev) => {
    if (!glissee) return;
    ev.preventDefault();
    const r = el.getBoundingClientRect();
    const dessous = ev.clientY > r.top + r.height / 2;
    el.classList.toggle("ligne--depose-dessus", !dessous);
    el.classList.toggle("ligne--depose-dessous", dessous);
  });
  el.addEventListener("dragleave", () => el.classList.remove("ligne--depose-dessus", "ligne--depose-dessous"));
  el.addEventListener("drop", (ev) => {
    ev.preventDefault();
    const dessous = el.classList.contains("ligne--depose-dessous");
    el.classList.remove("ligne--depose-dessus", "ligne--depose-dessous");
    if (!glissee || glissee === item || !estPlaylistOuverte()) return;
    const lignes = ouverte.lignes;
    const de = lignes.indexOf(glissee);
    const a = lignes.filter((x) => x !== glissee).indexOf(item) + (dessous ? 1 : 0);
    glissee = null;
    deplacer(de, a).catch((e) => remonter(e, "deplacer_dans_playlist"));
  });
}

/// ⌥↑ / ⌥↓ : la piste sélectionnée monte ou descend d'un rang.
document.addEventListener("keydown", (e) => {
  if (!e.altKey || e.metaKey || e.ctrlKey || e.shiftKey) return;
  if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
  if (!estPlaylistOuverte() || navigation.mode !== "ecoute" || !pisteSelectionnee) return;
  if (e.target.closest?.("input, textarea, select, [contenteditable]")) return;
  const de = ouverte.lignes.findIndex((t) => t.path === pisteSelectionnee.path);
  if (de < 0) return;
  e.preventDefault();
  deplacer(de, de + (e.key === "ArrowUp" ? -1 : 1)).catch((err) => remonter(err, "deplacer_dans_playlist"));
});

/// Les gestes en fin de ligne de morceau (`lignePiste`, `vues.js`) : ＋ partout ;
/// dans une playlist ouverte, ✕ et le glisser en plus. Révélés au survol et
/// sur la ligne sélectionnée (`style.css`), sans rien déplacer.
function actionsPiste(el, item, index) {
  if (retrait?.piste === item && estPlaylistOuverte()) {
    el.classList.add("ligne--retiree");
    el.appendChild(boutonAction("annuler", "Remettre ce morceau dans la playlist", () =>
      annulerRetrait().catch((e) => remonter(e, "remettre_dans_playlist"))));
    return;
  }
  el.appendChild(boutonAction("＋", "Ajouter à une playlist", (b) =>
    ouvrirMenuPlaylist(b, [item.path], txt(item.title, "ce morceau"))));
  if (!estPlaylistOuverte()) return;
  el.appendChild(boutonAction("✕", "Retirer de la playlist", () =>
    retirer(item, index).catch((e) => remonter(e, "retirer_de_playlist"))));
  cablerGlissement(el, item);
}

/* ----------------------------------- « Ajouter à une playlist » : le menu
 *
 * Un seul petit menu, le même partout (ligne de piste, file, inspecteur,
 * album, vue Playlists) : les playlists modifiées récemment d'abord, puis
 * « Nouvelle playlist… », nommée sur place. Pas de fenêtre modale : il se
 * pose sous le bouton, Échap ou un clic ailleurs le ferment, et il dit en une
 * ligne où le morceau est allé avant de se refermer.
 */
const menu = document.createElement("div");
menu.id = "menu-playlist";
menu.className = "menu-playlist";
menu.hidden = true;
menu.setAttribute("role", "dialog");
menu.setAttribute("aria-label", "Ajouter à une playlist");
document.body.appendChild(menu);
let ancreMenu = null;
let fermetureMenu = null;

function fermerMenu() {
  clearTimeout(fermetureMenu);
  menu.hidden = true;
  ancreMenu?.setAttribute?.("aria-expanded", "false");
  ancreMenu = null;
}

document.addEventListener("mousedown", (e) => {
  if (!menu.hidden && !menu.contains(e.target) && e.target !== ancreMenu) fermerMenu();
}, true);
menu.addEventListener("keydown", (e) => {
  e.stopPropagation();
  if (e.key === "Escape") {
    const a = ancreMenu;
    fermerMenu();
    a?.focus?.();
  }
});

function poserMenu(ancre) {
  const r = ancre.getBoundingClientRect();
  const larg = menu.offsetWidth;
  const haut = menu.offsetHeight;
  const x = Math.max(8, Math.min(r.left, innerWidth - larg - 8));
  const y = r.bottom + 4 + haut > innerHeight - 8 ? Math.max(8, r.top - 4 - haut) : r.bottom + 4;
  menu.style.left = `${x}px`;
  menu.style.top = `${y}px`;
}

/// Le message bref qui dit où le morceau est allé, puis le menu se ferme.
function direPuisFermer(etat, texte) {
  etat.textContent = texte;
  menu.querySelectorAll("button, input").forEach((b) => { b.disabled = true; });
  fermetureMenu = setTimeout(fermerMenu, 1800);
}

function texteAjout(nom, r) {
  const ou = `« ${isoler(nom)} »`;
  if (r.ajoutes === 0) return r.deja_presents > 1 ? `Tous déjà dans ${ou}` : `Déjà dans ${ou}`;
  const quoi = r.ajoutes > 1 ? `${r.ajoutes} morceaux ajoutés` : "Ajouté";
  return r.deja_presents ? `${quoi} à ${ou} (${r.deja_presents} y étai${r.deja_presents > 1 ? "ent" : "t"} déjà)` : `${quoi} à ${ou}`;
}

/// Après un ajout ou une création : la vue Playlists et la playlist ouverte
/// suivent.
async function apresAjout(id) {
  if (vue.quoi === "playlists" && navigation.mode === "ecoute") await chargerPlaylists();
  else if (estPlaylistOuverte() && ouverte.id === id) {
    await finirRetrait();
    const pistes = await invoke("playlist_pistes", { id });
    ouverte = { ...ouverte, lignes: pistes };
    poser("pistes", ouverte.nom, pistes, vue.retour, scrollActuel());
    majCompte();
  }
}

/// Ouvre le menu sous `ancre` pour `chemins` (un tableau, ou une fonction
/// qui le rend : les pistes d'un album ne se lisent qu'au choix). `quoi` nomme
/// ce qu'on ajoute dans le titre du menu. Sans chemins, le menu ne propose que
/// « Nouvelle playlist » : une playlist vide.
async function ouvrirMenuPlaylist(ancre, chemins, quoi = null) {
  if (!menu.hidden && ancreMenu === ancre) return fermerMenu();
  fermerMenu();
  const vide = Array.isArray(chemins) && chemins.length === 0;
  let liste = [];
  if (!vide) {
    try {
      liste = (await invoke("playlists")) ?? [];
    } catch (e) {
      remonter(e, "playlists");
    }
  }
  menu.replaceChildren();
  const tete = document.createElement("p");
  tete.className = "menu-playlist__tete";
  tete.textContent = vide ? "Nouvelle playlist" : quoi ? `Ajouter « ${isoler(quoi)} » à` : "Ajouter à";
  menu.appendChild(tete);

  const etat = document.createElement("p");
  etat.className = "aide menu-playlist__etat";
  etat.setAttribute("aria-live", "polite");

  const lesChemins = async () => (typeof chemins === "function" ? await chemins() : chemins);

  if (liste.length) {
    const choix = document.createElement("div");
    choix.className = "menu-playlist__choix";
    for (const p of liste) {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "menu-playlist__item";
      b.textContent = p.nom;
      b.title = `${p.nom} · ${p.nb_pistes} piste${p.nb_pistes > 1 ? "s" : ""}`;
      b.addEventListener("click", async () => {
        try {
          const r = await invoke("ajouter_a_playlist", { id: p.id, paths: await lesChemins() });
          direPuisFermer(etat, texteAjout(p.nom, r));
          await apresAjout(p.id);
        } catch (e) {
          etat.textContent = "Ajout impossible.";
          remonter(e, "ajouter_a_playlist");
        }
      });
      choix.appendChild(b);
    }
    menu.appendChild(choix);
  }

  // « Nouvelle playlist… » : le nom se tape sur place, comme « enregistrer »
  // la file.
  const form = document.createElement("form");
  form.className = "menu-playlist__nouvelle";
  const champ = document.createElement("input");
  champ.type = "text";
  champ.maxLength = 120;
  champ.autocomplete = "off";
  champ.placeholder = "Nom de la nouvelle playlist";
  champ.setAttribute("aria-label", "Nom de la nouvelle playlist");
  const ok = document.createElement("button");
  ok.type = "submit";
  ok.className = "file__bouton";
  ok.textContent = "créer";
  form.append(champ, ok);
  const nouvelle = document.createElement("button");
  nouvelle.type = "button";
  nouvelle.className = "menu-playlist__item menu-playlist__item--nouvelle";
  nouvelle.textContent = "Nouvelle playlist…";
  nouvelle.addEventListener("click", () => {
    nouvelle.replaceWith(form);
    champ.focus();
  });
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    const nom = champ.value.trim() || "Sans titre";
    try {
      const paths = await lesChemins();
      const id = await invoke("nouvelle_playlist", { nom, paths });
      direPuisFermer(etat, paths.length
        ? texteAjout(nom, { ajoutes: new Set(paths).size, deja_presents: 0 })
        : `Créée : « ${isoler(nom)} », vide`);
      if (vue.quoi === "playlists" && navigation.mode === "ecoute") await chargerPlaylists();
      else await apresAjout(id);
    } catch (err) {
      etat.textContent = "Création impossible.";
      remonter(err, "nouvelle_playlist");
    }
  });
  if (vide) menu.appendChild(form);
  else menu.appendChild(nouvelle);
  menu.appendChild(etat);

  ancreMenu = ancre;
  ancre.setAttribute?.("aria-expanded", "true");
  menu.hidden = false;
  poserMenu(ancre);
  (vide ? champ : menu.querySelector(".menu-playlist__item"))?.focus();
}

/* --------------------------------------------------- les points d'entrée */

// L'inspecteur : le morceau ou l'album qu'il montre.
$("insp-playlist").addEventListener("click", (e) => {
  const { piste, album } = inspectionCourante();
  if (piste) ouvrirMenuPlaylist(e.currentTarget, [piste.path], txt(piste.title, "ce morceau"));
  else if (album) {
    ouvrirMenuPlaylist(e.currentTarget, () =>
      invoke("tracks_of_album", { album: album.name, artist: album.artist ?? null }).then((ps) => ps.map((t) => t.path)),
    album.name);
  }
});

// Un album ouvert au centre : toutes ses pistes, dans l'ordre affiché.
$("album-playlist").addEventListener("click", (e) => {
  ouvrirMenuPlaylist(e.currentTarget, vue.lignes.map((t) => t.path), vue.titre);
});

// La vue Playlists : une playlist vide.
$("playlist-nouvelle").addEventListener("click", (e) => ouvrirMenuPlaylist(e.currentTarget, []));

export {
  actionsPiste, chargerPlaylists, estPlaylistOuverte, jouerPlaylist, lignePlaylist, ouvrirMenuPlaylist,
};
