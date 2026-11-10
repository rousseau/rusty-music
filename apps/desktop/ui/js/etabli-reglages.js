import "./etabli.js"; // l'ordre d'exécution : voir main.js
import { $, enStems, invoke, remonter } from "./socle.js";
import { lecture, poserStemsCharges, transport } from "./etat-lecture.js";
import { sonder } from "./lecteur.js";
import { bpmSource, edition, octaveVitesse, pasReglage, reposerBoucle, vitesseEnBpm } from "./etabli.js";
import {
  appliquerNiveaux, dessinerStems, enSerie, niveaux, transposerStems, verifierNoms,
} from "./etabli-stems.js";

/* ------------------------------------------- l'établi : les réglages du dock */

// Les réglages du dock (vitesse, hauteur, et le reste de `REGLAGES`), leurs
// champs et leur application au moteur, stem par stem. Découpé d'`etabli.js`
// (V4), qui garde l'état `edition`.

// Le nom d'un stem à l'écran. Le moteur nomme ses stems en anglais (`drums`,
// `bass`…), et ce nom reste la clé partout (`edition.stems`, `data-stem`,
// fichiers, commandes) ; le rail dit « Basse » et « Batterie » : la pile aussi.
const LIBELLES_STEMS = {
  drums: "Batterie",
  bass: "Basse",
  vocals: "Voix",
  other: "Autres",
  guitar: "Guitare",
  piano: "Piano",
};
function libelleStem(nom) {
  return LIBELLES_STEMS[nom] ?? nom;
}

/// Bornes et pas des deux réglages.
///
/// **Ce sont deux choses différentes, et elles ne coûtent pas la même chose.**
/// La vitesse est immédiate : la lecture avance d'un pas fractionnaire qu'on
/// écrit, rien n'est recalculé — et la hauteur ne bouge pas, `wsola` s'en
/// charge. La hauteur, elle, retraite le signal : quelques secondes de calcul
/// et 31 Mo par stem et par valeur, d'où un pas d'un demi-ton et pas de
/// glissière.
const REGLAGES = {
  vitesse: {
    pas: 0.05,
    min: 0.25,
    max: 4.0,
    // En pour cent par défaut ; en BPM quand le morceau pulse franchement et
    // que l'unité a été basculée — `edition.vitesse` reste le rapport, l'unité
    // n'est qu'une vue (`docs/ui-spec-editeur.md`, décision 9).
    // Le nombre seul : l'unité est dite à côté, par le bouton d'unité
    // (barre) ou un suffixe (ligne d'un stem) — demande du 29 oct.
    ecrire: (v) => `${Math.round(v * (vitesseEnBpm() ? bpmSource() : 100))}`,
    unite: () => (vitesseEnBpm() ? "BPM" : "%"),
    // **Ni pour cent ni BPM ambigus.** « 2 » en pour cent est refusé par la
    // borne basse plutôt qu'interprété comme « deux fois » ; en BPM, le champ
    // affiche « BPM » et lit donc un tempo, converti en rapport.
    lire: (t) => {
      const n = Number(
        String(t)
          .replace(/%|BPM/gi, "")
          .replace(",", ".")
          .trim(),
      );
      if (!Number.isFinite(n)) return null;
      if (vitesseEnBpm()) return bpmSource() ? n / bpmSource() : null;
      return n / 100;
    },
    immediat: true,
  },
  tonalite: {
    pas: 1,
    min: -12,
    max: 12,
    // La valeur reste en demi-tons (le pas des boutons, ce que le moteur
    // transpose) ; elle s'écrit **en tons**, comme on parle d'un accordage :
    // « −½ ton » pour jouer un morceau en mi♭ sans désaccorder la basse.
    ecrire: (v) => {
      if (v === 0) return "±0";
      const a = Math.abs(v);
      const tons = `${Math.floor(a / 2) || ""}${a % 2 ? "½" : ""}`;
      return `${v > 0 ? "+" : "−"}${tons} ton${a >= 4 ? "s" : ""}`;
    },
    // Se relit dans les formes que le champ écrit (« ±0 », « +1½ ton »,
    // « −½ ») et en décimal (« 1,5 ») — des tons, convertis en demi-tons.
    lire: (t) => {
      const brut = String(t)
        .replace(/tons?/gi, "")
        .replace("±", "")
        .replace("−", "-")
        .replace(",", ".")
        .trim();
      const n = Number(brut.replace(/(\d*)½/, (_, e) => `${e || 0}.5`));
      return Number.isFinite(n) ? Math.round(n * 2) : null;
    },
    immediat: false,
  },
};

/// Câble un champ de valeur : on tape, on valide, ça s'applique.
///
/// **Les pas-à-pas restent, et ce n'est pas une hésitation.** Ils servent au
/// réglage fin — chercher la bonne valeur en écoutant. Le champ sert à l'autre
/// geste, sauter d'un coup à une valeur qu'on a déjà en tête, ce qu'une
/// quinzaine de clics ne fait pas.
///
/// Une saisie hors bornes ou illisible **n'est pas corrigée en silence** : le
/// champ se marque et garde ce qui a été tapé, faute de quoi on ne saurait pas
/// ce qu'on a écrit de travers. Échap ou la perte du focus rendent la valeur
/// courante.
function cablerChamp(champ, nom, lire, poser) {
  const r = REGLAGES[nom];
  const rendre = () => {
    champ.classList.remove("reglage__val--faux");
    champ.value = r.ecrire(lire());
  };
  rendre();

  champ.addEventListener("focus", () => champ.select());
  champ.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      rendre();
      champ.blur();
    }
    // Les flèches font ce que font les boutons : c'est ce qu'on attend d'un
    // champ numérique, et cela évite de sortir du clavier pour un pas.
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      poser(calerReglage(nom, lire() + pasReglage(nom) * (e.key === "ArrowUp" ? 1 : -1)));
    }
  });
  champ.addEventListener("change", () => {
    const v = r.lire(champ.value);
    if (v === null || v < r.min - 1e-9 || v > r.max + 1e-9) {
      champ.classList.add("reglage__val--faux");
      return;
    }
    champ.classList.remove("reglage__val--faux");
    poser(calerReglage(nom, v));
  });
  champ.addEventListener("blur", rendre);
  return rendre;
}

/// Cale une valeur sur les bornes et le pas de son réglage.
///
/// L'arrondi n'est pas cosmétique : sans lui, les additions de 0,05 dérivent
/// et le nom du dossier de cache change à chaque passage.
function calerReglage(nom, valeur) {
  const r = REGLAGES[nom];
  const borne = Math.min(r.max, Math.max(r.min, valeur));
  // En mode BPM, on cale sur un tempo entier plutôt que sur un pas de rapport :
  // le champ montre « 128 BPM », pas « 128,3 ».
  if (nom === "vitesse" && vitesseEnBpm()) {
    return Math.round(borne * bpmSource()) / bpmSource();
  }
  const pas = pasReglage(nom);
  return Math.round(borne / pas) * pas;
}

/// Vitesse et hauteur effectives d'un stem : les siennes s'il en a, sinon
/// celles du dock.
function vitesseDe(s) {
  return s.vitesse ?? edition.vitesse;
}
function tonaliteDe(s) {
  return s.tonalite ?? edition.tonalite;
}
/// La transposition qu'on entend sur le stem `nom` (« bass »…), en
/// demi-tons : la sienne, sinon celle du dock (V35 : la partition et le coach
/// de basse la suivent).
function transpositionDe(nom) {
  const s = edition.stems.find((x) => x.nom === nom);
  return s ? tonaliteDe(s) : edition.tonalite;
}
/// Ce stem s'écarte-t-il de l'ensemble, d'une façon ou d'une autre ?
function stemEcarte(s) {
  return s.vitesse !== null || s.tonalite !== null || !!s.greffe;
}

/// Pousse la vitesse d'ensemble, puis les écarts.
///
/// **L'ordre compte.** Régler la vitesse d'ensemble ramène tous les stems
/// dessus — c'est ce qu'on attend d'un réglage global, et c'est ce que fait le
/// moteur — après quoi on repose ceux qui s'en écartent. Rien n'est recalculé :
/// ce sont des flottants que la lecture relit à chaque trame.
async function appliquerVitesses() {
  if (!lecture.stems.charges) return;
  // L'ordre compte : la vitesse d'ensemble écrit sur **tous** les stems, les
  // écarts se reposent donc après, jamais avant.
  await invoke("stems_vitesse", { vitesse: edition.vitesse });
  for (const [i, s] of edition.stems.entries()) {
    if (s.vitesse !== null) {
      await invoke("stems_vitesse_stem", { index: i, vitesse: s.vitesse });
    }
  }

  // **On relit ce que le moteur a retenu.** Il rend déjà ses vitesses dans
  // `stems_state` et personne ne les regardait : un réglage qui n'arrivait pas
  // — commande rejetée, indices décalés — ne se voyait qu'à l'oreille, et
  // « ça ne marche pas » est tout ce qu'on pouvait en dire. Comparé ici, un
  // écart se nomme.
  try {
    const e = await invoke("stems_state");
    const attendu = edition.stems.map(vitesseDe);
    const ecarts = attendu
      .map((v, i) => [i, v, e.vitesses?.[i]])
      .filter(([, v, lu]) => lu === undefined || Math.abs(lu - v) > 1e-3);
    if (ecarts.length) {
      remonter(
        ecarts.map(([i, v, lu]) => `stem ${i} : demandé ${v}, moteur ${lu}`).join(" · "),
        "vitesses non appliquées",
      );
    }
  } catch (e) {
    remonter(e, "relecture des vitesses");
  }
  majDerive();
}

/// Applique un réglage, et **montre l'échec s'il y en a un**.
///
/// Les gestionnaires de clic n'avaient pas de garde : une commande rejetée
/// partait dans le rapporteur global, invisible depuis l'interface, et le
/// réglage paraissait simplement sans effet.
async function appliquerUn(nom) {
  try {
    if (REGLAGES[nom].immediat) await appliquerVitesses();
    else await appliquerReglages();
  } catch (e) {
    $("dock-aide").textContent = `échec du réglage « ${nom} »`;
    remonter(e, `réglage ${nom}`);
  }
}

/// Recalcule les stems transposés et les remet en lecture au même instant.
///
/// C'est le seul chemin qui recharge le multipiste : transposition, greffe et
/// retrait de greffe y passent tous. Le calcul dure quelques secondes par stem
/// transposé, et le moteur garde chaque valeur sur le disque — revenir à une
/// hauteur déjà entendue est immédiat, et changer celle d'un seul stem ne
/// recalcule que celui-là.
async function appliquerReglages() {
  return enSerie(async () => {
    if (!edition.stems.length) return;
    const boutons = document.querySelectorAll(".reglage button");
    boutons.forEach((b) => (b.disabled = true));
    try {
      // Dans le `try` avec le reste : un échec ici laissait auparavant les
      // boutons désactivés pour de bon, faute de passer par le `finally`
      // ci-dessous.
      $("dock-aide").textContent = "calcul…";
      const cibles = edition.stems;
      const traites = await transposerStems(
        edition.stems.map((s) => [s.nom, s.chemin]),
        edition.stems.map(tonaliteDe),
      );
      // Le calcul dure des dizaines de secondes : on a pu changer de morceau
      // ou de mode entre-temps. Charger alors ces stems-là remplacerait ceux
      // de l'écran par ceux d'un autre morceau.
      if (edition.stems !== cibles || !enStems()) {
        $("dock-aide").textContent = "";
        return;
      }
      // **L'état de lecture se relève après le calcul, pas avant** : la
      // lecture a continué (ou été mise en pause) pendant qu'il tournait, et
      // rendre la position d'avant le calcul faisait revenir en arrière d'autant.
      const avant = lecture.stems.charges ? await invoke("stems_state") : null;
      if (avant && !avant.en_pause) transport.ignorerEtatJusqua = Date.now() + 2000;
      // Le nouveau jeu naît en pause, à ses niveaux : ni la batterie coupée ni
      // le mélange depuis zéro ne s'entendent pendant qu'on reposera position
      // et vitesses.
      const noms = await invoke("stems_play", { stems: traites, niveaux: niveaux() });
      poserStemsCharges(true);
      verifierNoms(noms);
      await appliquerVitesses();
      await reposerBoucle();
      await appliquerNiveaux();
      if (avant) {
        // La position se conserve **en proportion** : le rechargement remet
        // toutes les têtes à zéro, y compris celles qui avaient dérivé.
        const frac = avant.duree_ms ? avant.position_ms / avant.duree_ms : 0;
        const e = await invoke("stems_state");
        await invoke("stems_transport", {
          action: "deplacer",
          position: (frac * e.duree_ms) / 1000,
        });
      }
      if (!avant?.en_pause) await invoke("stems_transport", { action: "reprendre", position: null });
      $("dock-aide").textContent = "clic sur un spectrogramme : se déplacer";
      // V35 : la partition et le coach suivent la hauteur qu'on entend.
      document.dispatchEvent(new CustomEvent("hauteur-appliquee"));
    } catch (e) {
      $("dock-aide").textContent = "";
      remonter(e, "vitesse et hauteur");
    } finally {
      boutons.forEach((b) => (b.disabled = false));
    }
    sonder(true);
  });
}

function dessinerReglages() {
  for (const [nom, r] of Object.entries(REGLAGES)) {
    const champ = $(`val-${nom}`);
    // Ne pas écraser une saisie en cours : le sondage et les redessins
    // passent ici plusieurs fois par seconde.
    if (document.activeElement === champ) continue;
    champ.classList.remove("reglage__val--faux");
    champ.value = r.ecrire(edition[nom]);
  }

  // Bascule d'unité : toujours là, puisque la valeur s'écrit sans unité ;
  // inactive (« % ») tant que le morceau ne pulse pas franchement. Elle porte
  // l'unité courante — cliquer passe à l'autre.
  const unite = $("vitesse-unite");
  unite.hidden = false;
  unite.disabled = !edition.tempoSource;
  unite.textContent = REGLAGES.vitesse.unite();
  unite.classList.toggle("reglage__unite--actif", vitesseEnBpm());

  // Repliement d'octave : « ½ » si viser ce BPM étire de plus d'une demi-octave
  // vers le haut, « ×2 » vers le bas, rien sinon.
  const oct = $("vitesse-octave");
  const f = octaveVitesse();
  oct.hidden = f === 0;
  oct.textContent = f === 0.5 ? "½" : f === 2 ? "×2" : "";

  // Au-delà de ±20 % d'étirement d'ensemble, `wsola` s'entend : on le dit, dans
  // les deux unités — c'est le rapport qui compte, pas la façon de l'écrire.
  $("vitesse-alerte").hidden = Math.abs(edition.vitesse - 1) <= 0.2;

  // La vitesse atteinte (Pratiquer) : celle de la boucle posée, ou du
  // morceau sans boucle ; dans l'unité du réglage.
  const at = $("vitesse-atteinte");
  const v = edition.boucle ? edition.boucle.atteinte : edition.vitesseAtteinte;
  at.hidden = edition.usage !== "pratiquer" || !v;
  if (v) at.textContent = `atteinte : ${REGLAGES.vitesse.ecrire(v)} ${REGLAGES.vitesse.unite()}`;
}

for (const nom of Object.keys(REGLAGES)) {
  cablerChamp(
    $(`val-${nom}`),
    nom,
    () => edition[nom],
    async (v) => {
      edition[nom] = v;
      dessinerReglages();
      if (REGLAGES[nom].immediat) await appliquerVitesses();
      else await appliquerReglages();
      dessinerStems();
    },
  );
}

dessinerReglages();

document.querySelectorAll(".dock__tete .reglage button[data-r]").forEach((b) => {
  b.addEventListener("click", async () => {
    const nom = b.dataset.r;
    edition[nom] = calerReglage(nom, edition[nom] + pasReglage(nom) * Number(b.dataset.d));
    dessinerReglages();
    if (REGLAGES[nom].immediat) {
      // Un flottant à écrire, rien de plus : ni recalcul, ni rechargement, ni
      // perte de position. Les stems qui s'en écartent sont reposés ensuite.
      await appliquerVitesses();
    } else {
      await appliquerReglages();
    }
    dessinerStems();
  });
});

/// Bascule le réglage « vitesse » entre pour cent et BPM. Ne touche pas à
/// `edition.vitesse` : c'est le même rapport, lu autrement. Caler ensuite fait
/// tomber l'affichage sur un tempo entier.
$("vitesse-unite").addEventListener("click", () => {
  if (!edition.tempoSource) return;
  edition.vitesseEnBpm = !edition.vitesseEnBpm;
  try {
    localStorage.setItem("vitesse-unite", edition.vitesseEnBpm ? "bpm" : "%");
  } catch {}
  if (edition.vitesseEnBpm) edition.vitesse = calerReglage("vitesse", edition.vitesse);
  dessinerReglages();
});

/// Replie le rapport de tempo à l'octave : le morceau se lit à demi- ou
/// double-tempo, et viser le BPM ne demande plus qu'un étirement léger.
$("vitesse-octave").addEventListener("click", async () => {
  const f = octaveVitesse();
  if (!f) return;
  edition.vitesse = calerReglage("vitesse", edition.vitesse * f);
  dessinerReglages();
  await appliquerVitesses();
  dessinerStems();
});

/// Montre la dérive — et seulement quand il y en a une à montrer.
///
/// **La mesure, pas la mise en garde.** « Les stems peuvent se
/// désynchroniser » ne dit rien qu'on puisse vérifier ; « 1,4 s d'écart » se
/// contrôle à l'oreille et se corrige d'un bouton. Un avertissement permanent,
/// lui, ne serait pas lu.
function majDerive() {
  const el = $("derive");
  if (!el) return;
  const ecarte = edition.stems.some(
    (s) => s.vitesse !== null && Math.abs(s.vitesse - edition.vitesse) > 1e-3,
  );
  el.hidden = !ecarte || !lecture.stems.charges;
  if (el.hidden) return;
  const ms = lecture.stems.deriveMs || 0;
  $("derive-txt").textContent =
    ms > 250 ? `vitesses différentes — ${(ms / 1000).toFixed(1)} s d'écart` : "vitesses différentes";
}

$("realigner").addEventListener("click", async () => {
  try {
    await invoke("stems_transport", { action: "realigner", position: null });
  } catch (e) {
    remonter(e, "réalignement");
  }
});

export {
  appliquerReglages, appliquerUn, appliquerVitesses, cablerChamp, calerReglage, dessinerReglages,
  libelleStem, majDerive, REGLAGES, stemEcarte, tonaliteDe, transpositionDe, vitesseDe,
};
