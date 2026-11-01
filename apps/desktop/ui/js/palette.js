import "./autotest-carte.js"; // l'ordre d'exécution : voir main.js
import {
  $, invoke, isoler, navigation, plier, pochette, remonter, txt,
} from "./socle.js";
import {
  initiales, ouvrirAlbumDePiste, teinteNom,
} from "./vues.js";
import {
  lancerLecture, ouvrirAlbumsArtiste,
} from "./ecouter.js";
import {
  basculerMode,
} from "./modes.js";
import {
  basculerTheme, clairActif,
} from "./theme.js";
import {
  ouvrirEnCours,
} from "./en-cours.js";
import { lecture } from "./etat-lecture.js";

/* ---------------------------------------------------- palette de recherche
 *
 * ⌘K (Ctrl+K) ou « / » ouvrent, au-dessus de l'écran courant, une boîte qui
 * cherche dans toute la bibliothèque — meilleur résultat, artistes, albums,
 * pistes — et dans les commandes de l'application. L'écran reste visible
 * derrière, atténué (Règle 2 : estomper, jamais masquer). ↑/↓ se déplacent,
 * Entrée ouvre l'artiste ou l'album, ou lance la piste ; Échap ferme.
 *
 * L'idée et le comportement au clavier viennent de la palette de Disc Player
 * (https://github.com/eudj1n/snowsky-disc-player, `src/layout/SearchPalette.vue`,
 * licence MIT, © eudj1n) ; aucun code n'en est repris. Les commandes ne font
 * qu'appeler des gestes qui existent déjà ailleurs dans l'interface.
 */

/// Combien de réponses par groupe : la palette mène quelque part, elle ne
/// remplace pas la grille.
const LIMITES = { artistes: 3, albums: 4, pistes: 5 };
const PISTES_DEMANDEES = 30;

/* -------------------------------------------------------------- commandes */

const bouton = (id) => $(id);
const utilisable = (el) => !!el && !el.disabled && !el.hidden;
const ouvrirFile = () => {
  if ($("file").hidden) $("bascule-file").click();
};

async function versMode(mode) {
  if (navigation.mode !== mode) await basculerMode(mode);
}

const MODES = [
  ["bibliotheque", "Bibliothèque", "réglages statistiques vérifications dossiers"],
  ["ecoute", "Écouter", "albums artistes playlists"],
  ["explorer", "Explorer", "carte nuage temps anneau lama"],
  ["editer", "Éditer", "stems séparer partition pratiquer"],
  ["decouvrir", "Découvrir", "sorties collaborations nouveautés"],
  ["valise", "Valise", "iphone compagnon"],
];
const AFFICHAGES = [
  ["points", "Nuage"], ["carte", "Carte"], ["temps", "Temps"], ["anneau", "Anneau"],
  ["lama", "Lama", "playlist en mots intention"],
];
const PARCOURIR = [["artistes", "Artistes"], ["albums", "Albums"], ["playlists", "Playlists"]];

/// Les commandes : `groupe` (« Aller à » ou « Commandes »), un libellé, des
/// mots de plus auxquels elles répondent, `pret()` (masquée sinon) et
/// `lancer()`, appelé une fois la palette fermée. Recalculées à chaque
/// ouverture : leurs libellés suivent l'état (thème, file ouverte…).
function commandes() {
  const mode = (m) => document.querySelector(`.mode[data-mode="${m}"]`);
  const clic = (id) => () => bouton(id).click();
  return [
    ...MODES.map(([m, nom, mots]) => ({
      groupe: "Aller à", libelle: nom, mots: `mode ${mots}`,
      pret: () => utilisable(mode(m)) && navigation.mode !== m,
      lancer: () => versMode(m),
    })),
    ...AFFICHAGES.map(([a, nom, mots = ""]) => ({
      groupe: "Aller à", libelle: `Explorer · ${nom}`, mots: `affichage ${mots}`,
      pret: () => true,
      lancer: async () => {
        await versMode("explorer");
        document.querySelector(`[data-affichage="${a}"]`).click();
      },
    })),
    ...PARCOURIR.map(([v, nom]) => ({
      groupe: "Aller à", libelle: `Écouter · ${nom}`, mots: "parcourir",
      pret: () => true,
      lancer: async () => {
        await versMode("ecoute");
        document.querySelector(`[data-vuelib="${v}"]`).click();
      },
    })),
    {
      groupe: "Commandes", libelle: clairActif() ? "Thème sombre" : "Thème clair",
      mots: "thème apparence couleurs clair sombre",
      pret: () => true, lancer: basculerTheme,
    },
    {
      groupe: "Commandes", libelle: "Vue « En cours »", mots: "plein écran disque spectre pochette",
      pret: () => !!lecture.courant, lancer: ouvrirEnCours,
    },
    { groupe: "Commandes", libelle: "Lecture / pause", mots: "jouer lire arrêter", pret: () => true, lancer: clic("lecture") },
    { groupe: "Commandes", libelle: "Piste suivante", mots: "suivant", pret: () => true, lancer: clic("suivant") },
    { groupe: "Commandes", libelle: "Piste précédente", mots: "précédent", pret: () => true, lancer: clic("precedent") },
    {
      groupe: "Commandes",
      libelle: $("file").hidden ? "Ouvrir la file d'attente" : "Fermer la file d'attente",
      mots: "queue", pret: () => true, lancer: clic("bascule-file"),
    },
    {
      groupe: "Commandes", libelle: "Lecture aléatoire", mots: "mélanger file",
      pret: () => utilisable(bouton("file-alea")),
      lancer: () => { ouvrirFile(); bouton("file-alea").click(); },
    },
    {
      groupe: "Commandes", libelle: "Répéter", mots: "répétition boucle file",
      pret: () => utilisable(bouton("file-repeter")),
      lancer: () => { ouvrirFile(); bouton("file-repeter").click(); },
    },
    {
      groupe: "Commandes", libelle: "Enregistrer la file comme playlist", mots: "sauver",
      pret: () => utilisable(bouton("file-enregistrer")),
      lancer: () => { ouvrirFile(); bouton("file-enregistrer").click(); },
    },
    {
      groupe: "Commandes", libelle: "Minuteur d'arrêt", mots: "sommeil dormir",
      pret: () => utilisable(bouton("file-minuteur")),
      lancer: () => { ouvrirFile(); bouton("file-minuteur").click(); },
    },
    {
      groupe: "Commandes",
      libelle: bouton("insp-favori").getAttribute("aria-pressed") === "true"
        ? "Retirer des favoris" : "Ajouter aux favoris",
      mots: "favori cœur aimer",
      pret: () => utilisable(bouton("insp-favori")), lancer: clic("insp-favori"),
    },
    { groupe: "Commandes", libelle: "Amélioration du son « E »", mots: "améliorer spectre", pret: () => utilisable(bouton("ameliorer")), lancer: clic("ameliorer") },
    { groupe: "Commandes", libelle: "Écouter en HD", mots: "super-résolution aero", pret: () => utilisable(bouton("hd")), lancer: clic("hd") },
    { groupe: "Commandes", libelle: "Normaliser le volume", mots: "loudness r128", pret: () => utilisable(bouton("normaliser")), lancer: clic("normaliser") },
    {
      groupe: "Commandes", libelle: "Ajouter un dossier de musique…", mots: "bibliothèque source racine",
      pret: () => utilisable(bouton("parcourir")),
      lancer: async () => { await versMode("bibliotheque"); bouton("parcourir").click(); },
    },
    {
      groupe: "Commandes", libelle: "Tout rafraîchir", mots: "rescanner bibliothèque actualiser",
      pret: () => utilisable(bouton("tout-rafraichir")),
      lancer: async () => { await versMode("bibliotheque"); bouton("tout-rafraichir").click(); },
    },
  ].filter((c) => c.pret());
}

/* -------------------------------------------------------------- recherche */

const motsDe = (texte) => texte.split(/[^\p{L}\p{N}]+/u).filter(Boolean);

/// La force d'une réponse à la demande `q` (déjà pliée) : 100 nom exact, 80 nom
/// qui commence par la demande, 60 chaque mot de la demande commence un mot du
/// nom, 30 tous les mots sont dans le nom, 15 certains seulement dans le texte
/// secondaire (l'artiste d'un album, l'album d'une piste). 0 : un mot manque.
function force(q, nom, secondaire = "") {
  if (nom === q) return 100;
  if (nom.startsWith(q)) return 80;
  const mots = q.split(/\s+/);
  if (!mots.every((m) => `${nom} ${secondaire}`.includes(m))) return 0;
  if (!mots.every((m) => nom.includes(m))) return 15;
  const motsNom = motsDe(nom);
  return mots.every((m) => motsNom.some((w) => w.startsWith(m))) ? 60 : 30;
}

/// Une commande ne répond qu'au début de ses mots (libellé ou mots de plus),
/// pas à des lettres prises au milieu.
function forceCommande(q, c) {
  const libelle = plier(c.libelle);
  if (libelle.startsWith(q)) return 85;
  const mots = motsDe(`${libelle} ${plier(c.mots)}`);
  return q.split(/\s+/).every((m) => mots.some((w) => w.startsWith(m))) ? 50 : 0;
}

// Artistes et albums, pliés une fois à l'ouverture : la palette les filtre à la
// frappe sans aller-retour. Les pistes, trop nombreuses, passent par la
// recherche du moteur (`search`, index FTS qui replie déjà les diacritiques).
let index = { artistes: [], albums: [] };

async function chargerIndex() {
  const [artistes, albums] = await Promise.all([
    invoke("artists"),
    invoke("albums", { artist: null, mbid: null }),
  ]);
  index = {
    artistes: artistes.map((a) => ({ item: a, nom: plier(a.name) })),
    albums: albums.map((a) => ({ item: a, nom: plier(a.name), sec: plier(a.artist) })),
  };
}

const RANG_GENRE = { artistes: 0, albums: 1, pistes: 2 };

/// Les réponses d'un genre, triées par force puis par nom.
function classer(entrees, q, genre, nomDe, secDe) {
  return entrees
    .map((e) => ({ genre, item: e.item ?? e, force: force(q, nomDe(e), secDe(e)) }))
    .filter((r) => r.force > 0)
    .sort((a, b) => b.force - a.force);
}

/// Les sections affichées pour la demande `texte` : sans demande, les
/// commandes ; sinon meilleur résultat, artistes, albums, pistes et les
/// commandes qui répondent — avant la bibliothèque si elles répondent mieux.
function sections(texte, pistes) {
  const q = plier(texte.trim());
  const cmds = commandes();
  if (!q) {
    return ["Aller à", "Commandes"].map((g) => ({
      titre: g,
      options: cmds.filter((c) => c.groupe === g).map((c) => ({ genre: "commande", item: c })),
    }));
  }
  const groupes = {
    artistes: classer(index.artistes, q, "artistes", (e) => e.nom, () => ""),
    albums: classer(index.albums, q, "albums", (e) => e.nom, (e) => e.sec),
    pistes: classer(pistes, q, "pistes", (p) => plier(p.title), (p) => plier(`${p.artist} ${p.album}`))
      // Le moteur a trouvé la piste par son artiste ou son album : elle répond.
      .concat(pistes.filter((p) => !force(q, plier(p.title), plier(`${p.artist} ${p.album}`)))
        .map((p) => ({ genre: "pistes", item: p, force: 10 }))),
  };
  const meilleur = Object.values(groupes).flat()
    .sort((a, b) => b.force - a.force || RANG_GENRE[a.genre] - RANG_GENRE[b.genre])[0];
  const trouvees = cmds
    .map((c) => ({ genre: "commande", item: c, force: forceCommande(q, c) }))
    .filter((r) => r.force > 0)
    .sort((a, b) => b.force - a.force)
    .slice(0, 3);
  const bibliotheque = [
    ...(meilleur ? [{ titre: "Meilleur résultat", options: [meilleur] }] : []),
    ...[["artistes", "Artistes"], ["albums", "Albums"], ["pistes", "Pistes"]].map(([g, titre]) => ({
      titre,
      options: groupes[g].filter((r) => r !== meilleur).slice(0, LIMITES[g]),
    })),
  ];
  const sectionCmds = trouvees.length ? [{ titre: "Commandes", options: trouvees }] : [];
  const avant = trouvees.length && trouvees[0].force > (meilleur?.force ?? 0);
  return (avant ? [...sectionCmds, ...bibliotheque] : [...bibliotheque, ...sectionCmds])
    .filter((s) => s.options.length);
}

/* -------------------------------------------------------------- affichage */

const voile = $("palette");
const champ = $("palette-q");
const listeEl = $("palette-liste");

const etat = {
  options: [], // les options affichées, à plat, dans l'ordre
  actif: 0,
  pistes: [], // pistes rendues par le moteur pour `demande`
  demande: "",
  jeton: 0,
  focusAvant: null,
};

function vignette(option) {
  const v = document.createElement("span");
  v.className = "palette__vignette";
  v.setAttribute("aria-hidden", "true");
  const { genre, item } = option;
  if (genre === "commande") {
    v.classList.add("palette__vignette--commande");
    v.textContent = item.groupe === "Aller à" ? "→" : "⌘";
  } else if (genre === "artistes") {
    v.classList.add("palette__vignette--artiste");
    v.style.setProperty("--teinte", teinteNom(item.name));
    v.textContent = initiales(item.name);
  } else {
    pochette(item.path).then((img) => {
      if (img && v.isConnected) v.style.backgroundImage = `url("${img}")`;
    });
  }
  return v;
}

function textes(option) {
  const { genre, item } = option;
  if (genre === "commande") return [item.libelle, ""];
  if (genre === "artistes") {
    const alb = item.albums === 1 ? "1 album" : `${item.albums} albums`;
    return [item.name, `artiste · ${alb}`];
  }
  if (genre === "albums") {
    return [item.name, ["album", isoler(txt(item.artist, "(sans artiste)")), item.year].filter(Boolean).join(" · ")];
  }
  return [txt(item.title, "(sans titre)"), [isoler(txt(item.artist)), isoler(txt(item.album))].filter(Boolean).join(" · ")];
}

function rendre() {
  const secs = sections(etat.demande, etat.pistes);
  etat.options = secs.flatMap((s) => s.options);
  etat.actif = Math.min(etat.actif, Math.max(0, etat.options.length - 1));
  listeEl.replaceChildren();
  let rang = 0;
  for (const s of secs) {
    const groupe = document.createElement("div");
    groupe.setAttribute("role", "group");
    const titre = document.createElement("p");
    titre.className = "palette__groupe";
    titre.textContent = s.titre;
    titre.id = `palette-g-${rang}`;
    groupe.setAttribute("aria-labelledby", titre.id);
    groupe.appendChild(titre);
    for (const option of s.options) {
      const i = rang++;
      const el = document.createElement("div");
      el.className = "palette__option";
      el.id = `palette-o-${i}`;
      el.setAttribute("role", "option");
      el.appendChild(vignette(option));
      const [principal, secondaire] = textes(option);
      const corps = document.createElement("span");
      corps.className = "palette__corps";
      const nom = document.createElement("span");
      nom.className = "palette__nom";
      nom.textContent = principal;
      corps.appendChild(nom);
      if (secondaire) {
        const sec = document.createElement("span");
        sec.className = "palette__sec";
        sec.textContent = secondaire;
        corps.appendChild(sec);
      }
      el.appendChild(corps);
      if (option.genre === "pistes") {
        const ic = document.createElement("span");
        ic.className = "palette__lire";
        ic.setAttribute("aria-hidden", "true");
        ic.textContent = "▶";
        el.appendChild(ic);
      }
      el.addEventListener("pointermove", () => {
        if (etat.actif !== i) marquer(i, false);
      });
      el.addEventListener("click", () => activer(i));
      groupe.appendChild(el);
    }
    listeEl.appendChild(groupe);
  }
  if (!etat.options.length) {
    const p = document.createElement("p");
    p.className = "palette__rien";
    p.textContent = `Rien ne répond à « ${etat.demande.trim()} ».`;
    listeEl.appendChild(p);
  }
  marquer(etat.actif, true);
}

/// Marque l'option `i` comme active ; `defiler` la ramène dans la vue (au
/// clavier, pas au survol : la liste ne doit pas bouger sous la souris).
function marquer(i, defiler) {
  etat.actif = i;
  listeEl.querySelectorAll(".palette__option").forEach((el, k) =>
    el.setAttribute("aria-selected", String(k === i)),
  );
  const el = $(`palette-o-${i}`);
  if (el) {
    champ.setAttribute("aria-activedescendant", el.id);
    if (defiler) el.scrollIntoView({ block: "nearest" });
  } else {
    champ.removeAttribute("aria-activedescendant");
  }
}

async function chercherPistes(texte) {
  const jeton = ++etat.jeton;
  if (!texte.trim()) {
    etat.pistes = [];
    return;
  }
  try {
    const pistes = await invoke("search", { query: texte, limit: PISTES_DEMANDEES });
    if (jeton !== etat.jeton) return;
    etat.pistes = pistes ?? [];
    rendre();
  } catch (e) {
    remonter(e, "palette");
  }
}

let minuteur;
champ.addEventListener("input", () => {
  etat.demande = champ.value;
  etat.actif = 0;
  // Artistes, albums et commandes tout de suite ; les pistes du moteur suivent.
  etat.pistes = etat.pistes.filter((p) =>
    force(plier(champ.value.trim()), plier(`${p.title} ${p.artist} ${p.album}`)) > 0);
  rendre();
  listeEl.scrollTop = 0;
  clearTimeout(minuteur);
  minuteur = setTimeout(() => chercherPistes(champ.value), 120);
});

champ.addEventListener("keydown", (e) => {
  // Rien ne sort de la palette : ni Échap vers la file d'attente, ni les
  // flèches vers le transport (`lecteur.js`).
  e.stopPropagation();
  const n = etat.options.length;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    if (n) marquer((etat.actif + (e.key === "ArrowDown" ? 1 : n - 1)) % n, true);
  } else if (e.key === "Enter" && !e.isComposing) {
    e.preventDefault();
    activer(etat.actif);
  } else if (e.key === "Escape") {
    e.preventDefault();
    fermerPalette();
  } else if (e.key === "Tab") {
    // Une seule zone de saisie : le focus reste dans la boîte.
    e.preventDefault();
  } else if (e.key.toLowerCase() === "k" && (e.metaKey || e.ctrlKey)) {
    e.preventDefault();
    fermerPalette();
  }
});

// Un clic sur le fond atténué, hors de la boîte, ferme.
voile.addEventListener("pointerdown", (e) => {
  if (e.target === voile) fermerPalette();
});

/// Ouvre la palette, avec `texte` déjà tapé s'il y en a (Entrée dans le filtre
/// du rail).
async function ouvrirPalette(texte = "") {
  if (voile.hidden) etat.focusAvant = document.activeElement;
  voile.hidden = false;
  champ.value = texte;
  etat.demande = texte;
  etat.actif = 0;
  etat.pistes = [];
  champ.focus();
  champ.select();
  rendre();
  try {
    await chargerIndex();
  } catch (e) {
    remonter(e, "palette");
  }
  if (!voile.hidden) {
    rendre();
    chercherPistes(champ.value);
  }
}

function fermerPalette() {
  if (voile.hidden) return;
  voile.hidden = true;
  clearTimeout(minuteur);
  etat.jeton++;
  // Masqué, le champ garderait le focus (WebKit, Chrome) et avalerait le ⌘K
  // suivant : on le rend avant de revenir à ce qui l'avait.
  champ.blur();
  etat.focusAvant?.focus?.();
  etat.focusAvant = null;
}

/// Ouvre ce que l'option désigne. La palette se ferme d'abord : un geste qui
/// ouvre autre chose (le sélecteur de dossier, la file) reçoit le focus.
async function activer(i) {
  const option = etat.options[i];
  if (!option) return;
  fermerPalette();
  const { genre, item } = option;
  try {
    if (genre === "commande") await item.lancer();
    else if (genre === "artistes") await ouvrirAlbumsArtiste(item.name, item.mbid ?? null);
    else if (genre === "albums") await ouvrirAlbumDePiste({ album: item.name, artist: item.artist });
    else await lancerLecture(item);
  } catch (e) {
    remonter(e, "palette");
  }
}

const dansUnChamp = (el) =>
  el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement ||
  el instanceof HTMLSelectElement || el?.isContentEditable;

document.addEventListener("keydown", (e) => {
  if (e.key.toLowerCase() === "k" && (e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey) {
    e.preventDefault();
    if (voile.hidden) ouvrirPalette();
    else fermerPalette();
  } else if (e.key === "/" && !e.metaKey && !e.ctrlKey && !e.altKey && !dansUnChamp(e.target)) {
    e.preventDefault();
    ouvrirPalette();
  }
});

$("ouvrir-palette").addEventListener("click", () => ouvrirPalette($("q").value));

export {
  fermerPalette, force, ouvrirPalette,
};
