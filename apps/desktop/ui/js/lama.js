import "./vues.js"; // l'ordre d'exécution : voir main.js
import {
  $, duree, ecoute, fileAttente, invoke, signalerErreur, txt,
} from "./socle.js";
import {
  ALBUM_HAUT, composerAlchimie, dessinerGrille, grille,
} from "./vues.js";
import {
  majIndexActif, selectionner,
} from "./ecouter.js";
import {
  demarrerLecture,
} from "./lecteur.js";
import {
  carte,
} from "./carte.js";
import {
  longueurChemin,
} from "./maplibre.js";
import {
  bruitChemin,
} from "./fond-de-plan.js";
import {
  positionnerPlayhead,
} from "./pratiquer.js";

/* ------------------------------------------------ Lama : l'interprète (Explorer) */

/// Ce que le panneau Lama retient de la dernière demande : de quoi redessiner
/// le mini-nuage, colorer la route par partie sur les autres affichages, et
/// relier chaque morceau à la partie qui l'a produit.
const lama = {
  prompt: "",
  resultat: null, // le `CompositionTexte` rendu par `path_texte`
  fileComposee: null, // le tableau de pistes tel que posé dans `fileAttente.pistes`
  partieDeId: new Map(), // id de morceau → rang de sa partie
  survol: null, // rang de la partie survolée (barre, liste ou route)
  routeEcran: [], // la route dans le mini-nuage, pour le survol et le clic
  base: null, // canevas hors écran : tous les morceaux, estompés
  cleBase: "", // ce qui invalide `base` (taille, nombre de points, thème)
  // Le dernier plan interprété par Ollama, affiché dans la carte « Ollama a
  // compris » pour correction — `null` tant qu'aucune interprétation n'a réussi.
  planTexte: null,
};

/// Des exemples de demandes, chacun montrant une capacité — tous éprouvés sur
/// le banc `experiments/prompts-playlist`. Un clic remplit le champ.
const EXEMPLES_PROMPTS = [
  { tag: "durée", texte: "Une playlist calme de 60 minutes, sans rock, années 70" },
  { tag: "parties", texte: "Rock, pendant 12 minutes, puis hip hop pendant 20 minutes" },
  { tag: "parties", texte: "30 minutes de calme, puis 30 minutes d'énergique" },
  { tag: "trajet", texte: "Commencer par du Portishead, finir par du Massive Attack, 15 morceaux" },
  { tag: "exclusion", texte: "Une heure de musique énergique pour courir, sans Prince" },
  { tag: "redécouverte", texte: "Des perles méconnues de jazz, un seul morceau par artiste" },
  { tag: "époque", texte: "Du funk des années 70, 20 morceaux, sans James Brown" },
  { tag: "anglais", texte: "A chill 45 minute playlist, no metal" },
];

function dessinerExemples() {
  $("lama-exemples").replaceChildren(
    ...EXEMPLES_PROMPTS.map((ex) => {
      const li = document.createElement("li");
      const b = document.createElement("button");
      b.type = "button";
      b.className = "lama__exemple";
      const tag = document.createElement("span");
      tag.className = "lama__exemple-tag";
      tag.textContent = ex.tag;
      b.append(tag, document.createTextNode(ex.texte));
      b.addEventListener("click", () => {
        $("intention-texte").value = ex.texte;
        $("intention-texte").focus();
      });
      li.appendChild(b);
      return li;
    }),
  );
}
dessinerExemples();

/// « Exemples » rouvre (ou referme) l'accueil une fois qu'une demande l'a
/// effacé : les exemples ne sont pas perdus, ils ne tiennent juste plus la
/// place du récit.
$("lama-exemples-lien").addEventListener("click", () => {
  const accueil = $("lama-accueil");
  accueil.hidden = !accueil.hidden;
  if (!accueil.hidden) $("lama-corps").scrollTop = 0;
});

/// Une erreur du moteur ou d'Ollama, dans le panneau — le bloc « Chemin » du
/// rail, où `signalerErreur` écrivait avant, est masqué ici.
function erreurLama(texte, e, source) {
  signalerErreur("lama-erreur", texte, e, source);
  $("lama-erreur").hidden = false;
}

/// Montre une carte du récit avec son entrée (un fondu, rejoué à chaque fois).
function poserCarte(id, retardMs = 0) {
  const el = $(id);
  el.hidden = false;
  el.classList.remove("lama__carte--pose");
  el.style.animationDelay = `${retardMs}ms`;
  void el.offsetWidth; // relance l'animation
  el.classList.add("lama__carte--pose");
}

/// Interprète le prompt du champ d'intention via Ollama, montre ce qui a été
/// compris, puis enchaîne aussitôt sur la composition (`composerPlanTexte`) —
/// l'utilisateur voit les deux étapes se dérouler l'une après l'autre
/// (l'interprétation d'abord, puis comment la playlist en est sortie) sans
/// avoir de second geste à faire. Le bouton « Recomposer » sert à rejouer la
/// seconde étape seule, après une modification du plan affiché.
async function interpreterIntention() {
  const champ = $("intention-texte");
  const prompt = champ.value.trim();
  if (!prompt) return;

  $("lama-erreur").hidden = true;
  lama.prompt = prompt;
  // Une nouvelle demande efface le récit précédent : on le reconstruit au fil
  // de l'eau, étape par étape. La carte, elle, reste (elle se vide de la
  // route) : c'est le point fixe du panneau.
  for (const id of ["lama-compris", "lama-cherche", "lama-resultat"]) $(id).hidden = true;
  // L'accueil et ses exemples ont servi : le champ porte déjà la demande, le
  // récit prend la place (« Exemples » les rouvre).
  $("lama-accueil").hidden = true;
  lama.resultat = null;
  lama.fileComposee = null;
  lama.partieDeId = new Map();
  dessinerMiniNuage();

  champ.disabled = true;
  const minuteur = demarrerProgresIntention(
    modeleOllama ? `Interrogation d'Ollama (${modeleOllama})…` : "Interrogation d'Ollama…",
  );
  let interprete = false;
  try {
    lama.planTexte = await invoke("path_texte_interpreter", {
      prompt,
      nDefaut: longueurChemin(),
      modele: modeleOllama,
      // Un tirage frais par interprétation : un artiste sans titre précis
      // (« un morceau de Shootyz Groove ») en choisit un au hasard parmi les
      // siens, pas toujours le même — la redécouverte, but de l'application,
      // veut une entrée différente à chaque essai plutôt qu'un pivot figé.
      seed: Math.floor(Math.random() * 2 ** 31),
    });
    montrerPlanTexte(lama.planTexte);
    interprete = true;
  } catch (e) {
    // Le message vient du moteur (`ollama::interpreter`/`error.rs`) — il
    // distingue déjà « serveur injoignable » de « modèle absent », pas la
    // peine de le remplacer par un texte fixe qui les confondrait.
    erreurLama(String(e), e, "path_texte_interpreter");
  } finally {
    champ.disabled = false;
    arreterProgresIntention(minuteur);
  }
  // Hors du `try` : `composerPlanTexte` a sa propre gestion d'erreur
  // (`composerAlchimie`), pas la peine de la faire remonter ici aussi.
  if (interprete) composerPlanTexte();
}

/// Affiche la jauge indéterminée du champ d'intention avec `texte`, complété
/// d'un compteur de secondes écoulées — sans lui, une réponse qui prend
/// plus d'une minute (mesuré sur un gros modèle « qui réfléchit ») se lit
/// comme un blocage plutôt qu'un calcul en cours. Rend l'identifiant du
/// minuteur, à couper par `arreterProgresIntention`.
function demarrerProgresIntention(texte) {
  $("intention-progres-barre").classList.add("file__compo-barre--indetermine");
  $("intention-progres").hidden = false;
  // Le lama de l'icône marche pendant le calcul — pas un lama de plus ajouté
  // ailleurs, celui qui sert déjà à choisir le modèle.
  $("intention-ollama").classList.add("intention__ollama--travail");
  const debut = Date.now();
  const majTexte = () => {
    const s = Math.round((Date.now() - debut) / 1000);
    $("intention-progres-phase").textContent = s > 0 ? `${texte} (${s} s)` : texte;
  };
  majTexte();
  return setInterval(majTexte, 1000);
}

function arreterProgresIntention(minuteur) {
  clearInterval(minuteur);
  $("intention-progres").hidden = true;
  $("intention-ollama").classList.remove("intention__ollama--travail");
}

$("intention-texte").addEventListener("keydown", (e) => {
  if (e.key === "Enter") interpreterIntention();
});

/// Le modèle Ollama choisi par l'utilisateur (icône 🦙) — mémorisé d'une
/// session à l'autre. `null` tant qu'aucun choix n'a été fait : le moteur
/// retombe alors sur le plus petit modèle installé (`ollama::modele_par_defaut`),
/// pas sur un nom fixe qui n'existe peut-être pas sur cette machine (observé :
/// `qwen2.5:3b` absent rend un 404 qu'Ollama ne distingue pas clairement d'un
/// serveur injoignable).
const CLE_MODELE_OLLAMA = "ollama-modele";
let modeleOllama = localStorage.getItem(CLE_MODELE_OLLAMA) || null;

/// Icône 🦙 : déplie la liste des modèles déjà installés (`ollama list`),
/// repliée par défaut (Règle 5, sobriété) — pas un réglage à garder ouvert en
/// permanence pour un choix qui change rarement.
$("intention-ollama").addEventListener("click", async () => {
  const liste = $("intention-modele-liste");
  $("intention-ollama").classList.toggle("intention__ollama--actif", liste.hidden);
  if (!liste.hidden) {
    liste.hidden = true;
    return;
  }

  $("intention-modele-etat").hidden = true;
  try {
    const modeles = await invoke("ollama_modeles");
    if (modeles.length === 0) {
      $("intention-modele-etat").textContent =
        "Aucun modèle Ollama installé — « ollama pull <modèle> »";
      $("intention-modele-etat").hidden = false;
      $("intention-ollama").classList.remove("intention__ollama--actif");
      return;
    }
    liste.replaceChildren(
      ...modeles.map((m) => {
        const opt = document.createElement("option");
        opt.value = m;
        opt.textContent = m;
        return opt;
      }),
    );
    if (!modeleOllama || !modeles.includes(modeleOllama)) modeleOllama = modeles[0];
    liste.value = modeleOllama;
    liste.hidden = false;
    liste.focus();
  } catch (e) {
    erreurLama(String(e), e, "ollama_modeles");
    $("intention-ollama").classList.remove("intention__ollama--actif");
  }
});

$("intention-modele-liste").addEventListener("change", () => {
  modeleOllama = $("intention-modele-liste").value;
  localStorage.setItem(CLE_MODELE_OLLAMA, modeleOllama);
  $("intention-modele-liste").hidden = true;
  $("intention-ollama").classList.remove("intention__ollama--actif");
});

/* ---- ce qu'Ollama a compris ---- */

/// Le nom d'une partie : ses genres, sinon le début de sa description.
function libellePartie(p) {
  const g = (p.genres || []).join(" / ");
  const t = g || p.description || "partie";
  return t.charAt(0).toUpperCase() + t.slice(1);
}

/// Montre le plan interprété : la reformulation, les parties (barres
/// proportionnelles à leur durée, éditables), les contraintes, le départ et
/// l'arrivée. C'est la carte « Ollama a compris » — tout s'y corrige sans
/// repasser par Ollama, « Recomposer » rejoue la composition.
function montrerPlanTexte(plan) {
  $("lama-haut").hidden = false;
  poserCarte("lama-compris");
  $("lama-reformulation").textContent = plan.reformulation ? `« ${plan.reformulation} »` : "";
  $("lama-reformulation").hidden = !plan.reformulation;
  $("lama-brut").textContent = plan.brut || "(pas de réponse brute conservée)";

  $("plan-texte-depart-nom").textContent = plan.depart
    ? `${txt(plan.depart.title, "(sans titre)")} — ${txt(plan.depart.artist, "(sans artiste)")}`
    : "par description (aucun départ nommé)";
  $("plan-texte-depart-effacer").hidden = !plan.depart;

  // Absente d'une simple dérive : ne s'affiche que si une arrivée a été
  // demandée (« arriver à X »), résolue ou non — une arrivée demandée mais
  // introuvable (régression du 14 septembre : « RATM » cherché tel quel,
  // jamais trouvé, la playlist dérivait quand même sans que rien ne le
  // dise) doit rester visible, pas disparaître comme si de rien n'était.
  const arriveeDemandee = plan.arrivee || plan.arrivee_demandee;
  $("plan-texte-arrivee-titre").hidden = !arriveeDemandee;
  $("plan-texte-arrivee").hidden = !arriveeDemandee;
  $("plan-texte-arrivee-effacer").hidden = !plan.arrivee;
  if (plan.arrivee) {
    $("plan-texte-arrivee").classList.remove("borne--introuvable");
    $("plan-texte-arrivee-nom").textContent =
      `${txt(plan.arrivee.title, "(sans titre)")} — ${txt(plan.arrivee.artist, "(sans artiste)")}`;
  } else if (plan.arrivee_demandee) {
    $("plan-texte-arrivee").classList.add("borne--introuvable");
    $("plan-texte-arrivee-nom").textContent =
      `« ${plan.arrivee_demandee} » introuvable dans la bibliothèque`;
  }

  const liste = $("plan-texte-etapes");
  liste.replaceChildren();
  plan.etapes.forEach((etape, i) => {
    const li = document.createElement("li");
    const champ = document.createElement("input");
    champ.type = "text";
    champ.className = "chercher";
    champ.value = etape;
    // Édition directe dans la liste plutôt qu'un aller-retour par Ollama :
    // une étape reformulée par l'utilisateur n'a pas besoin d'une nouvelle
    // interprétation, seulement d'une nouvelle empreinte CLAP-texte, que
    // `composerPlanTexte` calcule de toute façon à la composition.
    champ.addEventListener("change", () => {
      lama.planTexte.etapes[i] = champ.value;
    });
    li.appendChild(champ);
    liste.appendChild(li);
  });

  $("plan-texte-n").value = plan.n;
  $("plan-texte-duree").value = plan.duree_minutes ?? "";
  // Une durée posée prime sur le nombre de morceaux.
  $("plan-texte-n").disabled = !!plan.duree_minutes;
  montrerPartiesPlan(plan);
  montrerFiltresPlan(plan);
  dessinerMiniNuage(); // la carte vient d'apparaître : elle a enfin une taille
}

const COULEUR_PARTIE = (k) => `var(--partie-${(k % 6) + 1})`;

/// Les parties d'une playlist « X puis Y » : une barre dont chaque segment est
/// proportionnel à la durée de sa partie, et dessous chaque partie éditable
/// (description anglaise, genres retirables, durée). Moins de deux parties :
/// le plan est une seule partie implicite, montrée par ses étapes.
function montrerPartiesPlan(plan) {
  const parties = plan.parties || [];
  const multi = parties.length >= 2;
  $("lama-parties-barre").hidden = !multi;
  $("lama-parties").hidden = !multi;
  $("lama-sans-partie").hidden = multi;
  if (!multi) return;

  const poids = parties.map(
    (p) => p.duree_minutes ?? (p.n ? p.n * 3.5 : Math.max(3, Math.round(plan.n / parties.length)) * 3.5),
  );
  const barre = $("lama-parties-barre");
  barre.replaceChildren();
  parties.forEach((p, k) => {
    const seg = document.createElement("div");
    seg.className = "lama__segment";
    seg.dataset.partie = k;
    seg.style.flexGrow = poids[k];
    seg.style.background = COULEUR_PARTIE(k);
    const taille = p.duree_minutes ? `${p.duree_minutes} min` : p.n ? `${p.n} morceaux` : "";
    seg.textContent = taille ? `${libellePartie(p)} · ${taille}` : libellePartie(p);
    seg.title = p.description || "";
    seg.addEventListener("mouseenter", () => survolerPartie(k));
    seg.addEventListener("mouseleave", () => survolerPartie(null));
    barre.appendChild(seg);
  });

  const ol = $("lama-parties");
  ol.replaceChildren();
  parties.forEach((p, k) => {
    const li = document.createElement("li");
    li.className = "lama__partie";
    li.style.setProperty("--couleur", COULEUR_PARTIE(k));
    li.addEventListener("mouseenter", () => survolerPartie(k));
    li.addEventListener("mouseleave", () => survolerPartie(null));

    const nom = document.createElement("span");
    nom.className = "lama__partie-nom";
    nom.textContent = `${k + 1}. ${libellePartie(p)}`;

    const desc = document.createElement("input");
    desc.type = "text";
    desc.className = "chercher";
    desc.value = p.description || "";
    desc.title = "Ce que la marche vise (anglais, pour CLAP)";
    desc.addEventListener("change", () => {
      p.description = desc.value;
      montrerPartiesPlan(plan);
    });

    const duree = document.createElement("input");
    duree.type = "number";
    duree.className = "chercher lama__partie-duree";
    duree.min = 1;
    duree.max = 1440;
    duree.placeholder = "min";
    duree.value = p.duree_minutes ?? "";
    duree.title = "Durée de cette partie, en minutes";
    duree.addEventListener("change", () => {
      const d = Number.parseInt(duree.value, 10);
      p.duree_minutes = Number.isFinite(d) && d >= 1 ? Math.min(1440, d) : null;
      montrerPartiesPlan(plan);
    });

    const x = document.createElement("button");
    x.type = "button";
    x.className = "borne__x";
    x.textContent = "×";
    x.title = "Retirer cette partie";
    x.addEventListener("click", () => retirerPartie(plan, k));

    const chips = document.createElement("span");
    chips.className = "lama__partie-genres";
    (p.genres || []).forEach((g, i) => {
      const c = document.createElement("button");
      c.type = "button";
      c.className = "lama__chip";
      c.textContent = `${g} ×`;
      c.title = "Retirer ce genre de la partie";
      c.addEventListener("click", () => {
        p.genres.splice(i, 1);
        montrerPartiesPlan(plan);
      });
      chips.appendChild(c);
    });

    const l1 = document.createElement("div");
    l1.className = "lama__partie-ligne";
    l1.append(nom, chips);
    const l2 = document.createElement("div");
    l2.className = "lama__partie-ligne";
    l2.append(desc, duree, x);
    li.append(l1, l2);
    ol.appendChild(li);
  });
}

/// Retire une partie. Il n'en reste qu'une : ce n'est plus une suite, on la
/// ramène aux champs du plan entier (comme le fait le moteur).
function retirerPartie(plan, k) {
  plan.parties.splice(k, 1);
  if (plan.parties.length === 1) {
    const [p] = plan.parties;
    plan.parties = [];
    plan.etapes = [p.description || "music"];
    plan.filtres.genres = p.genres || [];
    plan.duree_minutes = p.duree_minutes ?? null;
  }
  montrerPlanTexte(plan);
}

const LIBELLE_ENERGIE = { calme: "calme", moyenne: "énergie moyenne", intense: "intense" };
const LIBELLE_POPULARITE = { peu_connu: "peu connu", connu: "connu" };
const LIBELLE_ECOUTE = { oublie: "oubliés (plus écoutés depuis 6 mois)", jamais: "jamais écoutés" };

/// Les contraintes du plan, une ligne retirable chacune (`× `) : retirer
/// n'appelle pas Ollama, ça modifie le plan affiché, que « Recomposer »
/// rejoue. `exclu` : un refus, signalé par la bordure d'alerte.
function contraintesDuPlan(plan) {
  const f = plan.filtres || {};
  const lignes = [];
  const ajouter = (texte, retirer, exclu = false) => lignes.push({ texte, retirer, exclu });
  (f.genres || []).forEach((g, i) => ajouter(`+ ${g}`, () => f.genres.splice(i, 1)));
  (f.exclure_genres || []).forEach((g, i) => ajouter(`− ${g}`, () => f.exclure_genres.splice(i, 1), true));
  (f.exclure_artistes || []).forEach((a, i) => ajouter(`− ${a}`, () => f.exclure_artistes.splice(i, 1), true));
  if (f.annee_min != null || f.annee_max != null) {
    const texte =
      f.annee_min != null && f.annee_max != null
        ? `${f.annee_min}–${f.annee_max}`
        : f.annee_min != null
          ? `à partir de ${f.annee_min}`
          : `jusqu'à ${f.annee_max}`;
    ajouter(texte, () => { f.annee_min = null; f.annee_max = null; });
  }
  if (f.bpm_min != null || f.bpm_max != null) {
    const texte =
      f.bpm_min != null && f.bpm_max != null
        ? `${Math.round(f.bpm_min)}–${Math.round(f.bpm_max)} BPM`
        : f.bpm_min != null
          ? `≥ ${Math.round(f.bpm_min)} BPM`
          : `≤ ${Math.round(f.bpm_max)} BPM`;
    ajouter(texte, () => { f.bpm_min = null; f.bpm_max = null; });
  }
  if (f.energie) ajouter(`énergie : ${LIBELLE_ENERGIE[f.energie] ?? f.energie}`, () => { f.energie = null; });
  if (f.popularite) ajouter(LIBELLE_POPULARITE[f.popularite] ?? f.popularite, () => { f.popularite = null; });
  if (f.ecoute) ajouter(LIBELLE_ECOUTE[f.ecoute] ?? f.ecoute, () => { f.ecoute = null; });
  if (plan.plafond_par_artiste)
    ajouter(`${plan.plafond_par_artiste} morceau(x) max par artiste`, () => { plan.plafond_par_artiste = null; });
  return lignes;
}

function montrerFiltresPlan(plan) {
  const liste = $("plan-texte-filtres");
  liste.replaceChildren();
  const lignes = contraintesDuPlan(plan);
  $("plan-texte-contraintes").hidden = lignes.length === 0;
  for (const l of lignes) {
    const li = document.createElement("li");
    if (l.exclu) li.className = "filtre--exclu";
    const nom = document.createElement("span");
    nom.textContent = l.texte;
    const x = document.createElement("button");
    x.className = "borne__x";
    x.textContent = "×";
    x.title = "Retirer cette contrainte";
    x.addEventListener("click", () => {
      l.retirer();
      montrerFiltresPlan(plan);
    });
    li.append(nom, x);
    liste.appendChild(li);
  }
}

$("plan-texte-depart-effacer").addEventListener("click", () => {
  if (!lama.planTexte) return;
  lama.planTexte.depart = null;
  montrerPlanTexte(lama.planTexte);
});

$("plan-texte-arrivee-effacer").addEventListener("click", () => {
  if (!lama.planTexte) return;
  // Revient à une simple dérive, sans arrivée visée — `path_texte` ne fait
  // alors plus le raccordement exact (`Graphe::sonique`) en fin de marche.
  lama.planTexte.arrivee = null;
  montrerPlanTexte(lama.planTexte);
});

$("plan-texte-n").addEventListener("change", () => {
  if (!lama.planTexte) return;
  const n = Number.parseInt($("plan-texte-n").value, 10);
  lama.planTexte.n = Number.isFinite(n) ? Math.min(200, Math.max(1, n)) : lama.planTexte.n;
});

$("plan-texte-duree").addEventListener("change", () => {
  if (!lama.planTexte) return;
  const d = Number.parseInt($("plan-texte-duree").value, 10);
  // Vide ou invalide : plus de durée, on revient au nombre de morceaux.
  lama.planTexte.duree_minutes = Number.isFinite(d) && d >= 1 ? Math.min(1440, d) : null;
  $("plan-texte-duree").value = lama.planTexte.duree_minutes ?? "";
  $("plan-texte-n").disabled = !!lama.planTexte.duree_minutes;
});

/* ---- ce que Rusty Music a cherché, et ce qui en sort ---- */

const dureeMin = (ms) => `${Math.round(ms / 60000)} min`;
const nombreFr = (n) => n.toLocaleString("fr-FR");

/// L'entonnoir d'une partie : combien de morceaux restent après chaque
/// critère. Les barres sont proportionnelles à la bibliothèque entière — on
/// voit d'un coup d'œil ce qui restreint le plus.
function dessinerEntonnoir(etapes) {
  const ul = document.createElement("ul");
  ul.className = "lama__entonnoir";
  const total = Math.max(1, etapes[0]?.[1] ?? 1);
  for (const [libelle, n] of etapes) {
    const li = document.createElement("li");
    const nom = document.createElement("span");
    nom.className = "lama__entonnoir-nom";
    nom.textContent = libelle;
    const barre = document.createElement("span");
    barre.className = "lama__entonnoir-barre";
    const i = document.createElement("i");
    i.style.width = `${Math.max(1.5, (100 * n) / total)}%`;
    barre.appendChild(i);
    const compte = document.createElement("span");
    compte.className = "lama__entonnoir-n";
    compte.textContent = nombreFr(n);
    li.append(nom, barre, compte);
    ul.appendChild(li);
  }
  return ul;
}

/// Les cartes « Rusty Music a cherché » et « Résultat » : ce que le moteur a
/// fait de la demande, partie par partie, et ce qui en sort. Rien n'y est
/// inventé — ce sont les comptes et les choix du moteur (`PartieResultat`).
function montrerComposition(r) {
  lama.resultat = r;
  lama.fileComposee = r.pistes;
  lama.partieDeId = new Map();
  r.parties.forEach((p, k) => {
    for (let i = p.debut; i < p.fin; i++) lama.partieDeId.set(r.pistes[i].id, k);
  });

  const multi = r.parties.length > 1;
  const corps = $("lama-cherche-corps");
  corps.replaceChildren();
  r.parties.forEach((p, k) => {
    const bloc = document.createElement("div");
    bloc.className = "lama__cherche-partie";
    bloc.dataset.partie = k;
    if (multi) bloc.style.setProperty("--couleur", COULEUR_PARTIE(k));
    bloc.addEventListener("mouseenter", () => survolerPartie(k));
    bloc.addEventListener("mouseleave", () => survolerPartie(null));
    if (multi) {
      const h = document.createElement("h4");
      const point = document.createElement("i");
      point.className = "lama__point";
      point.style.background = COULEUR_PARTIE(k);
      h.append(point, `${p.libelle}${p.cible_ms ? ` — ${dureeMin(p.cible_ms)} visées` : ""}`);
      bloc.appendChild(h);
    }
    bloc.appendChild(dessinerEntonnoir(p.entonnoir));
    const note = (texte, classe = "") => {
      const el = document.createElement("p");
      el.className = `aide ${classe}`.trim();
      el.textContent = texte;
      bloc.appendChild(el);
    };
    if (p.n_admissibles !== (p.entonnoir.at(-1)?.[1] ?? p.n_admissibles))
      note(`${nombreFr(p.n_admissibles)} admissibles après assouplissement et hors des parties précédentes.`);
    note(`Départ : ${p.depart_pourquoi}.`);
    note(
      `Obtenu : ${p.fin - p.debut} morceau${p.fin - p.debut > 1 ? "x" : ""}, ${dureeMin(p.duree_ms)}` +
        (p.cible_ms ? ` (visé : ${dureeMin(p.cible_ms)}).` : "."),
    );
    for (const rel of p.relaches) note(`Assoupli — ${rel}`, "lama__relache");
    corps.appendChild(bloc);
  });
  poserCarte("lama-cherche", 120);

  // Le résultat : la playlist par parties, cliquable (lecture à partir d'ici).
  const total = r.pistes.length;
  $("lama-resultat-resume").textContent =
    `${total} morceaux · ${dureeMin(r.duree_ms)}` +
    (lama.planTexte?.duree_minutes ? ` (visé : ${lama.planTexte.duree_minutes} min)` : "") +
    (multi ? ` · ${r.parties.length} parties` : "");
  const res = $("lama-resultat-corps");
  res.replaceChildren();
  r.parties.forEach((p, k) => {
    const groupe = document.createElement("div");
    groupe.className = "lama__groupe";
    groupe.dataset.partie = k;
    groupe.addEventListener("mouseenter", () => survolerPartie(k));
    groupe.addEventListener("mouseleave", () => survolerPartie(null));
    const tete = document.createElement("div");
    tete.className = "lama__groupe-tete";
    if (multi) {
      const point = document.createElement("i");
      point.className = "lama__point";
      point.style.background = COULEUR_PARTIE(k);
      const nom = document.createElement("b");
      nom.textContent = p.libelle;
      tete.append(point, nom);
    }
    const info = document.createElement("span");
    info.className = "aide";
    info.textContent = `${p.fin - p.debut} morceau${p.fin - p.debut > 1 ? "x" : ""} · ${dureeMin(p.duree_ms)}`;
    tete.appendChild(info);
    groupe.appendChild(tete);
    const ol = document.createElement("ol");
    ol.className = "lama__pistes";
    for (let i = p.debut; i < p.fin; i++) {
      const t = r.pistes[i];
      const li = document.createElement("li");
      li.value = i + 1;
      const titre = document.createElement("span");
      titre.textContent = `${txt(t.title, "(sans titre)")} — ${txt(t.artist, "(sans artiste)")}`;
      const d = document.createElement("span");
      d.className = "lama__piste-duree";
      d.textContent = duree(t.duration_ms);
      li.append(titre, d);
      li.title = "Lire à partir d'ici";
      li.addEventListener("click", async () => {
        selectionner(t);
        await demarrerLecture(() => invoke("jump_to", { index: i }));
      });
      ol.appendChild(li);
    }
    groupe.appendChild(ol);
    res.appendChild(groupe);
  });
  poserCarte("lama-resultat", 240);
  dessinerMiniNuage();
}

/// La composition a échoué : le message du moteur dit pourquoi (exclusions qui
/// vident la bibliothèque, marche impossible…) — montré dans la carte « a
/// cherché », pas seulement au journal.
function montrerErreurComposition(texte) {
  lama.resultat = null;
  const corps = $("lama-cherche-corps");
  corps.replaceChildren();
  const p = document.createElement("p");
  p.className = "aide lama__relache lama__relache--erreur";
  p.textContent = texte;
  corps.appendChild(p);
  poserCarte("lama-cherche");
  $("lama-resultat").hidden = true;
  dessinerMiniNuage();
}

/* ---- le mini-nuage : la demande et la bibliothèque se répondent ---- */

/// Met en valeur une partie (barre, liste, résultat ou route) — ou aucune.
function survolerPartie(k) {
  lama.survol = k;
  document.querySelectorAll("[data-partie], .lama__partie").forEach((el) => {
    const rang = el.dataset.partie != null ? Number(el.dataset.partie) : -1;
    el.classList.toggle("lama--survol", k != null && rang === k);
  });
  dessinerMiniNuage();
}

/// Le nuage de la bibliothèque, en petit, au service de la demande :
///  - tous les morceaux, estompés (Règle 2 : estomper, jamais masquer) ;
///  - ceux que les filtres de chaque partie retiennent, dans la couleur de la
///    partie (la partie survolée seule, si on en survole une) ;
///  - la route de la playlist, un trait par partie.
/// Positions t-SNE (`p.x`, `p.y`) comme le Nuage, jamais celles de la ville.
function dessinerMiniNuage() {
  const cv = $("lama-nuage");
  if (!cv || $("lama-vue").hidden) return;
  const w = cv.clientWidth;
  const h = cv.clientHeight;
  if (!w || !h) return;
  const dpr = window.devicePixelRatio || 1;
  if (cv.width !== Math.round(w * dpr) || cv.height !== Math.round(h * dpr)) {
    cv.width = Math.round(w * dpr);
    cv.height = Math.round(h * dpr);
  }
  const ctx = cv.getContext("2d");
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);

  const css = getComputedStyle(document.documentElement);
  const mut = css.getPropertyValue("--mut").trim() || "#999";
  const encre = css.getPropertyValue("--txt").trim() || "#fff";
  const pts = carte.points;
  $("lama-nuage-legende").textContent = pts.length
    ? lama.resultat
      ? "Un point par morceau. En couleur, ce que les filtres retiennent pour chaque partie ; le trait relie la playlist dans l'ordre. Survolez une partie ou un point du trait."
      : "Chaque point est un morceau de votre bibliothèque, placé par similarité sonore."
    : "La carte de la bibliothèque se charge…";
  if (!pts.length) return;

  const c = Math.min(w, h) / 2 - 8;
  const ecran = (p) => [w / 2 + p.x * c, h / 2 + p.y * c];

  // Fond : tous les morceaux, estompés. Mis en cache (27 000 points).
  const cle = `${w}x${h}@${dpr}:${pts.length}:${mut}`;
  if (lama.cleBase !== cle) {
    const base = document.createElement("canvas");
    base.width = Math.round(w * dpr);
    base.height = Math.round(h * dpr);
    const b = base.getContext("2d");
    b.setTransform(dpr, 0, 0, dpr, 0, 0);
    b.fillStyle = mut;
    b.globalAlpha = 0.28;
    for (const p of pts) {
      const [x, y] = ecran(p);
      b.fillRect(x - 0.5, y - 0.5, 1.1, 1.1);
    }
    lama.base = base;
    lama.cleBase = cle;
  }
  ctx.drawImage(lama.base, 0, 0, w, h);

  const r = lama.resultat;
  lama.routeEcran = [];
  if (!r) return;

  // Ce que les filtres retiennent : une couleur par partie. Une partie sans
  // filtre admet tout — rien à mettre en valeur.
  const parId = new Map(pts.map((p) => [p.id, p]));
  r.parties.forEach((part, k) => {
    if (lama.survol != null && lama.survol !== k) return;
    if (!part.ids_admissibles.length) return;
    ctx.fillStyle = getComputedStyle(document.documentElement).getPropertyValue(`--partie-${(k % 6) + 1}`).trim();
    ctx.globalAlpha = lama.survol === k ? 0.85 : 0.55;
    for (const id of part.ids_admissibles) {
      const p = parId.get(id);
      if (!p) continue;
      const [x, y] = ecran(p);
      ctx.fillRect(x - 0.9, y - 0.9, 1.9, 1.9);
    }
  });
  ctx.globalAlpha = 1;

  // La route : un trait par partie, un halo d'encre dessous pour qu'elle
  // ressorte sur n'importe quelle couleur.
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  const route = r.pistes.map((t, i) => ({ t, i, p: parId.get(t.id) })).filter((e) => e.p);
  const partieDe = (i) => r.parties.findIndex((q) => i >= q.debut && i < q.fin);
  lama.routeEcran = route.map((e) => {
    const [x, y] = ecran(e.p);
    return { x, y, piste: e.t, index: e.i, partie: partieDe(e.i) };
  });
  const tracer = (couleur, largeur, alpha, filtre) => {
    ctx.strokeStyle = couleur;
    ctx.lineWidth = largeur;
    ctx.globalAlpha = alpha;
    for (let i = 1; i < lama.routeEcran.length; i++) {
      const a = lama.routeEcran[i - 1];
      const b = lama.routeEcran[i];
      if (!filtre(b)) continue;
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);
      ctx.stroke();
    }
  };
  const couleurPartie = (k) =>
    getComputedStyle(document.documentElement).getPropertyValue(`--partie-${(Math.max(k, 0) % 6) + 1}`).trim();
  tracer(encre, 4.5, 0.45, () => true);
  for (let k = 0; k < r.parties.length; k++) {
    const fort = lama.survol === k;
    tracer(couleurPartie(k), fort ? 3.4 : 2.2, lama.survol != null && !fort ? 0.45 : 0.98, (b) => b.partie === k);
  }
  for (const e of lama.routeEcran) {
    ctx.beginPath();
    ctx.arc(e.x, e.y, lama.survol === e.partie ? 3.8 : 3, 0, Math.PI * 2);
    ctx.fillStyle = couleurPartie(e.partie);
    ctx.globalAlpha = lama.survol != null && lama.survol !== e.partie ? 0.5 : 1;
    ctx.fill();
    ctx.lineWidth = 1;
    ctx.strokeStyle = encre;
    ctx.globalAlpha = 0.7;
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
}

/// Le point de la route le plus proche du curseur, à moins de 10 px.
function pointDeRouteSous(ev) {
  const cv = $("lama-nuage");
  const rect = cv.getBoundingClientRect();
  const x = ev.clientX - rect.left;
  const y = ev.clientY - rect.top;
  let meilleur = null;
  let d2min = 100;
  for (const e of lama.routeEcran) {
    const d2 = (e.x - x) ** 2 + (e.y - y) ** 2;
    if (d2 < d2min) {
      d2min = d2;
      meilleur = e;
    }
  }
  return meilleur ? { e: meilleur, x, y } : null;
}

$("lama-nuage").addEventListener("mousemove", (ev) => {
  const hit = pointDeRouteSous(ev);
  const bulle = $("lama-nuage-info");
  if (!hit) {
    bulle.hidden = true;
    $("lama-nuage").style.cursor = "";
    if (lama.survol != null) survolerPartie(null);
    return;
  }
  $("lama-nuage").style.cursor = "pointer";
  const { e } = hit;
  const part = lama.resultat.parties[e.partie];
  bulle.textContent =
    `${e.index + 1}. ${txt(e.piste.title, "(sans titre)")} — ${txt(e.piste.artist, "(sans artiste)")}` +
    (lama.resultat.parties.length > 1 && part ? ` · ${part.libelle}` : "");
  bulle.style.left = `${Math.min(hit.x + 12, $("lama-nuage").clientWidth - 160)}px`;
  bulle.style.top = `${hit.y + 12}px`;
  bulle.hidden = false;
  if (lama.survol !== e.partie) survolerPartie(e.partie);
});

$("lama-nuage").addEventListener("mouseleave", () => {
  $("lama-nuage-info").hidden = true;
  if (lama.survol != null) survolerPartie(null);
});

$("lama-nuage").addEventListener("click", async (ev) => {
  const hit = pointDeRouteSous(ev);
  if (!hit) return;
  selectionner(hit.e.piste);
  await demarrerLecture(() => invoke("jump_to", { index: hit.e.index }));
});

window.addEventListener("resize", () => {
  if (!$("lama-vue").hidden) dessinerMiniNuage();
});

/// La taille à réserver dans la file avant que les vraies pistes n'arrivent :
/// à 3 min 30 le morceau pour une durée, la somme des parties sinon.
function cibleDuPlan(plan) {
  const parties = plan.parties || [];
  if (parties.length >= 2) {
    const morceaux = parties.reduce(
      (t, p) => t + (p.duree_minutes ? p.duree_minutes / 3.5 : p.n || Math.max(3, Math.round(plan.n / parties.length))),
      0,
    );
    return Math.max(2, Math.round(morceaux));
  }
  return plan.duree_minutes ? Math.max(2, Math.round(plan.duree_minutes / 3.5)) : plan.n;
}

/// Compose la playlist du plan courant : une empreinte CLAP-texte par étape
/// ou par partie, puis une marche guidée dans le graphe des voisins
/// (`chemin::guidee`) — même jauge de composition que les boutons ✦
/// (`composerAlchimie`).
///
/// Déclenchée automatiquement dès que l'interprétation est confirmée
/// (`interpreterIntention`), pas seulement sur clic : l'utilisateur voit ce
/// qu'Ollama a compris, puis voit tout de suite la playlist s'assembler à
/// partir de là, sans geste de plus à faire. Le bouton « Recomposer » reste
/// disponible pour relancer après une modification du plan.
function composerPlanTexte() {
  const plan = lama.planTexte;
  if (!plan || (plan.etapes.length === 0 && !(plan.parties || []).length)) return;
  $("lama-cherche").hidden = true;
  $("lama-resultat").hidden = true;
  composerAlchimie({
    bouton: $("plan-texte-composer"),
    // La demande faite à Lama : elle nomme la playlist et se garde avec elle.
    origine: lama.prompt || null,
    cible: cibleDuPlan(lama.planTexte),
    chemin: async () => {
      try {
        const r = await invoke("path_texte", {
          plan: lama.planTexte,
          seed: Math.floor(Math.random() * 2 ** 31),
          bruit: bruitChemin,
        });
        montrerComposition(r);
        return r.pistes;
      } catch (e) {
        montrerErreurComposition(String(e));
        throw e;
      }
    },
    demarrer: (pistes) => invoke("play", { paths: pistes.map((t) => t.path) }),
  });
}

$("plan-texte-composer").addEventListener("click", composerPlanTexte);

/// Ouvre la file sur la longueur cible : autant d'emplacements vides que de
/// pistes attendues, la phase en cours au-dessus. `revelerFile` viendra
/// ensuite y poser les vraies pistes une à une.
function demarrerCompositionFile(cible) {
  fileAttente.compositionActive = true;
  $("file-compo").hidden = false;
  $("file-compo-phase").textContent = "Préparation du graphe des voisins…";
  majBarreCompo(0, 0);
  $("file-compte").textContent = `0 / ${cible}`;
  $("file-duree").textContent = ""; // la file en cours n'est plus celle qui s'affiche
  const hote = $("file-liste");
  hote.replaceChildren();
  for (let i = 0; i < cible; i++) hote.appendChild(ligneSquelette(i));
}

function phaseCompositionFile(texte) {
  $("file-compo-phase").textContent = texte;
  majBarreCompo(0, 0); // indéterminé le temps de la composition
}

function finirCompositionFile() {
  fileAttente.compositionActive = false;
  $("file-compo").hidden = true;
}

/// `fait`/`total` de la jauge de composition. `total` à 0 : va-et-vient
/// indéterminé (graphe déjà en cache, ou composition en cours).
function majBarreCompo(fait, total) {
  const barre = $("file-compo-barre");
  if (total > 0) {
    barre.classList.remove("file__compo-barre--indetermine");
    barre.style.marginLeft = "0";
    barre.style.width = `${Math.round((fait / total) * 100)}%`;
  } else {
    barre.classList.add("file__compo-barre--indetermine");
    barre.style.marginLeft = "";
    barre.style.width = "";
  }
}

/// Sonde `graphe_progress` pendant l'attente du graphe des voisins. Rend
/// l'identifiant d'intervalle, à couper dès que `preparerGraphe` a rendu.
function sonderGrapheProgres() {
  return setInterval(async () => {
    try {
      const [fait, total] = await invoke("graphe_progress");
      if (total > 0) {
        $("file-compo-phase").textContent = "Préparation du graphe des voisins…";
        majBarreCompo(fait, total);
      }
    } catch {
      /* le sondage suivant réessaiera */
    }
  }, 200);
}

function ligneSquelette(i) {
  const el = document.createElement("div");
  el.className = "file__ligne file__ligne--squelette";
  el.innerHTML = `<span class="file__rang"></span>
                  <span class="file__txt"><b></b><span></span></span>
                  <span class="file__duree"></span>`;
  el.children[0].textContent = i + 1;
  return el;
}

/// Une piste posée dans la file en composition — même gabarit que
/// `dessinerFile`, avec l'animation d'entrée et, pour la première, la marque
/// « graine » : c'est le morceau d'où part la dérive. `graine = false` pour
/// une playlist mélangée (`shuffle_artist`) : pas de pivot, la première
/// piste n'a rien de spécial.
function ligneComposee(t, i, graine = true) {
  const estGraine = graine && i === 0;
  const el = document.createElement("div");
  el.className = "file__ligne file__ligne--pose";
  if (estGraine) el.classList.add("file__ligne--graine");
  el.innerHTML = `<span class="file__rang"></span>
                  <span class="file__txt"><b></b><span></span></span>
                  <span class="file__duree"></span>`;
  el.children[0].textContent = estGraine ? "✦" : i + 1;
  el.children[1].children[0].textContent = txt(t.title, "(sans titre)");
  el.children[1].children[1].textContent = estGraine
    ? `graine · ${txt(t.artist, "(sans artiste)")}`
    : txt(t.artist, "(sans artiste)");
  el.children[2].textContent = duree(t.duration_ms);
  el.addEventListener("click", async () => {
    selectionner(t);
    await demarrerLecture(() => invoke("jump_to", { index: i }));
  });
  return el;
}

/// Remplace les emplacements vides par les vraies pistes, une par battement,
/// pour donner à voir la playlist se construire. Le trajet est déjà entier —
/// l'errance est instantanée — c'est un rythme d'affichage, pas de calcul, et
/// l'ordre est le vrai ordre de la dérive (ou du mélange, voir `graine`).
async function revelerFile(pistes, { graine = true } = {}) {
  const hote = $("file-liste");
  // La longueur cible affichée d'emblée peut différer du trajet rendu
  // (échantillonnage côté moteur) : on réajuste les emplacements.
  while (hote.children.length < pistes.length)
    hote.appendChild(ligneSquelette(hote.children.length));
  while (hote.children.length > pistes.length) hote.lastChild.remove();

  for (let i = 0; i < pistes.length; i++) {
    hote.replaceChild(ligneComposee(pistes[i], i, graine), hote.children[i]);
    $("file-compte").textContent = `${i + 1} / ${pistes.length}`;
    majBarreCompo(i + 1, pistes.length);
    await new Promise((r) => setTimeout(r, 55));
  }
}

// Le défilement émet des dizaines d'évènements par seconde, et `dessinerGrille`
// reconstruit toute la bande de cases visibles à chaque appel (DOM neuf,
// écouteurs, demande de pochette). Sans ce garde-fou, la grille se traînait au
// défilement et le bouton ▶ d'une case disparaissait sous le pointeur au
// moindre coup de molette juste avant le clic — d'où l'impression qu'il ne
// répondait pas. On ne redessine qu'une fois par trame, et seulement quand la
// bande de rangées visibles a réellement changé (un cran = `ALBUM_HAUT`).
let grilleRedessinArme = false;
function dessinerGrilleAuDefilement() {
  if (grilleRedessinArme) return;
  grilleRedessinArme = true;
  requestAnimationFrame(() => {
    grilleRedessinArme = false;
    const rang = Math.floor(grille.scrollTop / ALBUM_HAUT);
    if (rang === ecoute.grilleDernierRang) {
      majIndexActif();
      return;
    }
    ecoute.grilleDernierRang = rang;
    dessinerGrille();
  });
}

grille.addEventListener("scroll", dessinerGrilleAuDefilement, { passive: true });
window.addEventListener("resize", () => {
  if (!grille.hidden) {
    ecoute.grilleDernierRang = -1;
    dessinerGrille();
  }
  positionnerPlayhead();
});


export {
  demarrerCompositionFile, dessinerMiniNuage, finirCompositionFile, interpreterIntention, lama,
  montrerComposition, montrerPlanTexte, phaseCompositionFile, revelerFile, sonderGrapheProgres,
};
