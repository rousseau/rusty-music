import "./coach.js"; // l'ordre d'exécution : voir main.js
// Le propre de Créer : la greffe d'un stem pris dans un autre morceau, ses
// candidats et son application. Le commun de l'établi : `etabli.js`.
import {
  $, invoke, remonter, txt,
} from "./socle.js";
import { oublierPartitions } from "./pratiquer.js";
import {
  appliquerReglages, dessinerStems, edition,
} from "./etabli.js";

/// La partie « remplacer » du panneau : le bouton, la greffe posée, ou la
/// liste des morceaux d'où tirer le stem.
function zoneGreffe(s) {
  const zone = document.createElement("div");

  if (s.greffe) {
    const note = document.createElement("p");
    note.className = "stem__note";
    note.textContent = decrireGreffe(s.greffe);
    zone.appendChild(note);

    // Dit une fois, là où c'est utile : la limite est réelle, et le recours
    // est à portée de main, juste au-dessus.
    const garde = document.createElement("p");
    garde.className = "stem__note";
    garde.textContent =
      "Les tempos concordent, pas les temps forts — il n'y a pas de grille de battements. La vitesse de ce stem sert à recaler.";
    zone.appendChild(garde);

    const ligne = document.createElement("div");
    ligne.className = "stem__ligne";
    const retirer = document.createElement("button");
    retirer.className = "stem__b";
    retirer.textContent = "retirer la greffe";
    retirer.addEventListener("click", async () => {
      s.chemin = s.origine;
      s.greffe = null;
      s.spectre = null;
      dessinerStems();
      await appliquerReglages();
    });
    ligne.appendChild(retirer);
    zone.appendChild(ligne);
  }

  if (!s.voisins) {
    // La direction, facultative : « plus funk » oriente CLAP par sa tour
    // texte et dit au classement quel groove récompenser.
    const ligne = document.createElement("div");
    ligne.className = "stem__ligne greffe__guide";
    const guide = document.createElement("input");
    guide.type = "text";
    guide.className = "greffe__champ";
    guide.placeholder = "plus funk, plus sobre… (facultatif)";
    guide.value = s.guide ?? "";
    guide.setAttribute("aria-label", "Dans quel sens chercher");
    guide.setAttribute("list", "greffe-sens");
    const sens = document.createElement("datalist");
    sens.id = "greffe-sens";
    for (const v of ["plus funk", "plus droit", "plus chargé", "plus sobre"]) {
      const o = document.createElement("option");
      o.value = v;
      sens.appendChild(o);
    }
    const b = document.createElement("button");
    b.className = "stem__b";
    b.textContent = s.greffe ? "en essayer un autre" : "remplacer par…";
    b.title = "Cherche parmi les voisins sonores ceux dont le tempo et la tonalité permettent d'échanger ce stem, puis les classe par le groove de leur batterie";
    const lancer = async () => {
      b.disabled = true;
      b.textContent = "recherche…";
      s.guide = guide.value.trim();
      try {
        await chercher(s);
      } catch (e) {
        remonter(e, "recherche du greffon");
        b.disabled = false;
        b.textContent = "remplacer par…";
        return;
      }
      dessinerStems();
      preparer(s);
    };
    b.addEventListener("click", lancer);
    guide.addEventListener("keydown", (e) => {
      if (e.key === "Enter") lancer();
    });
    ligne.append(guide, sens, b);
    zone.appendChild(ligne);
    return zone;
  }

  const r = s.voisins;
  const note = document.createElement("p");
  note.className = "stem__note";
  if (!r.bpm) {
    // Sans tempo, rien n'est calable — et la cause est nommée avec le remède.
    note.textContent =
      "Ce morceau n'a pas de tempo mesuré : rien sur quoi caler un stem. Lance « Descripteurs » depuis les Réglages.";
    zone.appendChild(note);
    return zone;
  }
  note.textContent = resumerRecherche(r);
  zone.appendChild(note);

  if (!r.candidats.length) {
    const vide = document.createElement("p");
    vide.className = "stem__note";
    vide.textContent = "Aucun voisin ne passe les filtres de tempo et de tonalité.";
    zone.appendChild(vide);
    return zone;
  }

  const liste = document.createElement("div");
  liste.className = "candidats";
  for (const c of r.candidats) {
    const b = document.createElement("button");
    b.className = "candidat";
    b.innerHTML = `<b></b><span></span><span></span><small class="candidat__pourquoi"></small>`;
    b.children[0].textContent = `${txt(c.artist, "?")} — ${txt(c.title, "?")}`;
    const octave = c.octaves ? ` ${c.octaves > 0 ? "×" : "÷"}${2 ** Math.abs(c.octaves)}` : "";
    b.children[1].textContent = `${Math.round(c.bpm)} BPM${octave}`;
    // Ce qui décide du clic : prêt, en cours de préparation, ou à séparer.
    b.children[2].textContent = etatCandidat(c);
    // Pourquoi il est là : groove, tempo, tonalité, voisinage.
    b.children[3].textContent = c.raisons.join(" · ");
    b.addEventListener("click", () => greffer(s, c, liste));
    liste.appendChild(b);
  }
  zone.appendChild(liste);
  return zone;
}

/// L'en-tête de la liste : la référence du classement, et ce qui a été
/// écarté — sans les comptes, une liste courte passe pour une bibliothèque
/// pauvre en voisins.
function resumerRecherche(r) {
  const virgule = (x, n) => x.toFixed(n).replace(".", ",");
  const bouts = [`${r.candidats.length} candidat${r.candidats.length > 1 ? "s" : ""} au tempo de ${Math.round(r.bpm)} BPM`];
  if (r.texte) bouts.push(`guidés par « ${r.texte} »`);
  if (r.groove) {
    bouts.push(`classés contre la batterie d'origine (syncope ${virgule(r.groove.syncope, 2)}, ${virgule(r.groove.densite, 1)} attaques par temps)`);
  } else {
    bouts.push("pas de batterie d'origine : classés dans l'absolu");
  }
  const ecarts = [`${r.ecartes_tempo} pour leur tempo`];
  if (r.a_hauteur) ecarts.push(`${r.ecartes_tonalite} pour leur tonalité`);
  if (r.ecartes_metrique) ecarts.push(`${r.ecartes_metrique} pour leur métrique`);
  ecarts.push(`${r.sans_tempo} sans tempo mesuré`);
  let texte = `${bouts.join(", ")}. Écartés : ${ecarts.join(", ")}.`;
  if (!r.a_hauteur) texte += " Tonalité ignorée : une batterie n'a pas de hauteur.";
  const attente = r.candidats.filter((c) => !c.groove).length;
  if (attente) texte += ` ${attente} à mesurer : les premiers se séparent en tâche de fond, le classement s'affine à mesure.`;
  return texte;
}

/// Prêt, en préparation, ou à séparer : ce qui décide du clic.
function etatCandidat(c) {
  if (preparation.courant === c.id) return "en cours…";
  if (c.separe) return c.groove ? "prêt" : "séparé";
  return preparation.file.includes(c.id) ? "en file" : "~30 s";
}

/// La recherche, avec la direction donnée.
async function chercher(s) {
  s.voisins = await invoke("chercher_greffon", {
    id: edition.source.id, stem: s.nom, guide: s.guide || null, count: 12,
  });
}

/// Ce que la préparation en tâche de fond fait, vu d'ici.
const preparation = { courant: null, file: [], vus: 0, sonde: null };

/// Les premiers candidats sans groove mesuré (5 à 10 au plan, 8 ici) : à
/// démixer en tâche de fond, dans le cache de stems existant, pour que leur
/// batterie puisse être classée. Le classement est relu à chaque candidat
/// prêt.
async function preparer(s, devant = null) {
  const r = s.voisins;
  if (!r?.candidats) return;
  let ids = r.candidats.filter((c) => !c.groove).map((c) => c.id).slice(0, 8);
  if (devant != null) ids = [devant, ...ids.filter((i) => i !== devant)];
  if (!ids.length) return;
  preparation.file = ids;
  await invoke("greffons_preparer", { ids, variant: edition.variante });
  if (preparation.sonde) return;
  preparation.vus = 0;
  preparation.sonde = setInterval(async () => {
    let e;
    try {
      e = await invoke("greffons_etat");
    } catch (err) {
      clearInterval(preparation.sonde);
      preparation.sonde = null;
      remonter(err, "préparation des greffons");
      return;
    }
    if (!e) return;
    preparation.courant = e.courant;
    const vus = e.prets.length + e.echecs.length;
    const fini = !e.en_cours;
    if (fini) {
      clearInterval(preparation.sonde);
      preparation.sonde = null;
      preparation.file = [];
    }
    // Un candidat de plus est prêt : le classement change, on le relit —
    // sauf si la liste a été fermée ou un autre stem cherché entre-temps.
    if ((vus !== preparation.vus || fini) && s.voisins && !s.greffe_en_cours) {
      preparation.vus = vus;
      try {
        await chercher(s);
        dessinerStems();
      } catch (err) {
        remonter(err, "recherche du greffon");
      }
    }
  }, 1500);
}

/// Attend qu'un candidat soit préparé (séparé, groove mesuré) ou abandonné.
function attendreGreffon(id) {
  return new Promise((resolve, reject) => {
    const t = setInterval(async () => {
      let e;
      try {
        e = await invoke("greffons_etat");
      } catch (err) {
        clearInterval(t);
        reject(err);
        return;
      }
      if (!e) return;
      if (e.prets.includes(id)) {
        clearInterval(t);
        resolve();
      } else if (e.echecs.includes(id) || !e.en_cours) {
        // Un échec de la mesure du groove n'empêche pas la greffe si les
        // stems sont là : `stems_greffer` le dira sinon.
        clearInterval(t);
        resolve();
      }
    }, 1000);
  });
}

/// Ce qu'il a fallu faire au greffon, dit plutôt que laissé à deviner.
function decrireGreffe(g) {
  const bouts = [g.origine];
  if (g.mesures > 0) {
    // Mesure par mesure : chaque mesure étirée sur la sienne ; le facteur
    // dit l'étirement typique.
    const pct = Math.round((g.facteur - 1) * 1000) / 10;
    bouts.push(`calé mesure par mesure (${g.mesures} mesures)`);
    if (g.mesures_par_section > 0) {
      bouts.push(g.mesures_par_section === g.mesures ? "section sur section" : `${g.mesures_par_section} section sur section`);
    }
    if (pct !== 0) bouts.push(`étiré d'environ ${pct > 0 ? "+" : ""}${pct} %`);
    if (g.octaves > 0) bouts.push(`${2 ** g.octaves} mesures pour une`);
    if (g.octaves < 0) bouts.push(`une demi-mesure pour une`);
    return bouts.join(" · ");
  }
  const pct = Math.round((g.facteur - 1) * 1000) / 10;
  bouts.push(pct === 0 ? "au même tempo" : `étiré de ${pct > 0 ? "+" : ""}${pct} %`);
  if (g.octaves > 0) bouts.push(`joué à ${2 ** g.octaves} × son tempo`);
  if (g.octaves < 0) bouts.push(`joué à son tempo divisé par ${2 ** -g.octaves}`);
  if (g.retard_s > 0.05) bouts.push(`entrée à ${g.retard_s.toFixed(1)} s`);
  bouts.push(g.boucles > 1 ? `${g.boucles} passages` : "un passage");
  // Le calage change ce qu'on entend, pas seulement ce qu'on a calculé : sans
  // lui les deux matières pulsent au même tempo sans tomber sur le même temps,
  // et il faut le rattraper à la main avec la vitesse de ce stem.
  bouts.push(g.cale_aux_temps ? "calé sur les temps" : "calé sur la première attaque");
  if (g.repli) bouts.push(`pas mesure par mesure : ${g.repli}`);
  return bouts.join(" · ");
}

/// Va chercher le stem d'un autre morceau et le met à la place de celui-ci.
///
/// Deux attentes possibles, et toutes deux annoncées : la séparation du
/// morceau voisin s'il n'est pas déjà séparé, puis le calage lui-même —
/// l'étirement d'un stem entier se compte en dizaines de secondes.
async function greffer(s, candidat, liste) {
  liste.querySelectorAll("button").forEach((b) => (b.disabled = true));
  s.greffe_en_cours = true;
  try {
    if (!candidat.separe) {
      // Il passe devant les autres dans la préparation de fond, qui le
      // sépare dans le cache comme le ferait « Séparer ».
      $("dock-aide").textContent = "séparation du morceau voisin…";
      await preparer(s, candidat.id);
      await attendreGreffon(candidat.id);
      oublierPartitions(candidat.id);
    }
    $("dock-aide").textContent = "calage du stem…";
    const g = await invoke("stems_greffer", {
      id: edition.source.id,
      stem: s.nom,
      // Toujours le stem d'origine : greffer sur une greffe empilerait les
      // étirements et le retard.
      remplace: s.origine,
      voisin: candidat.id,
    });
    $("dock-aide").textContent = "";
    s.chemin = g.chemin;
    s.greffe = g;
    s.spectre = null; // le spectrogramme montre autre chose, maintenant
    s.voisins = null;
    dessinerStems();
    await appliquerReglages();
  } catch (e) {
    $("dock-aide").textContent = "";
    remonter(e, "greffe");
    liste.querySelectorAll("button").forEach((b) => (b.disabled = false));
  } finally {
    s.greffe_en_cours = false;
  }
}

export {
  zoneGreffe,
};
