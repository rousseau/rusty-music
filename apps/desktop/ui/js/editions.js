import "./en-cours.js"; // l'ordre d'exécution : voir main.js
import {
  $, duree, ecoute, invoke, isoler, remonter, txt, vue,
} from "./socle.js";
import {
  contactMb,
} from "./decouvrir.js";

/* -------------------------------------------- choisir l'édition, face à face
 *
 * Depuis l'album ouvert au centre, « Choisir l'édition… » ouvre une boîte
 * au-dessus de l'écran : à gauche les éditions que propose MusicBrainz, à
 * droite l'album de la bibliothèque, dessous les pistes alignées rang à rang,
 * les différences marquées (`docs/plan-ecouter-v0.3.md`, point 3 ; idée de
 * Disc Player, `src/layout/AlbumCompare.vue`, licence MIT). Une phrase dit ce
 * qui part sur le réseau. Rien n'est appliqué sans « Retenir » ; une coche
 * n'est qu'une concordance, jamais une décision (l'esprit du « nom incertain »
 * de `docs/nommage-familles.md`). Retenir écrit en base, jamais dans les
 * fichiers ; « Oublier ce choix » rend aux pistes les identifiants de leurs tags.
 */

const voile = $("editions");
const liste = $("editions-liste");
const etatEl = $("editions-etat");
const retenir = $("editions-retenir");

const etat = {
  album: null, // { name, artist }
  propositions: [],
  choisie: null, // id de l'édition sélectionnée
  comparaison: null,
  jeton: 0,
  focusAvant: null,
};

const ouverte = () => !voile.hidden;
const dureeTotale = (ms) => (ms ? duree(ms) : "—");

function dire(texte) {
  etatEl.textContent = texte;
}

/// « Night Drive · Lumen LR-02 · Japon · Vinyl · 2010 · 3 pistes »
function description(e) {
  const label = [e.label, e.catalogue].filter(Boolean).join(" ");
  return [label, e.pays, e.format, e.date, `${e.pistes} piste${e.pistes > 1 ? "s" : ""}`]
    .filter(Boolean).join(" · ");
}

function rendreListe() {
  liste.replaceChildren();
  for (const e of etat.propositions) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "editions__option";
    b.setAttribute("role", "option");
    b.setAttribute("aria-selected", String(e.id === etat.choisie));
    const nom = document.createElement("span");
    nom.className = "editions__titre";
    nom.textContent = e.titre;
    const sec = document.createElement("span");
    sec.className = "editions__sec";
    sec.textContent = description(e);
    b.append(nom, sec);
    if (e.concorde) {
      const coche = document.createElement("span");
      coche.className = "editions__coche";
      coche.textContent = "✓";
      coche.title = "Même nombre de pistes que l'album de la bibliothèque — une concordance, pas une décision.";
      b.appendChild(coche);
    }
    b.addEventListener("click", () => choisir(e.id));
    liste.appendChild(b);
  }
}

function cellule(texte, cls = "") {
  const td = document.createElement("td");
  td.textContent = texte;
  if (cls) td.className = cls;
  return td;
}

/// Les pistes alignées : locale à gauche, édition à droite ; une ligne marquée
/// dit ce qui diffère (piste seule, écart de durée, titre).
function rendrePistes() {
  const corps = $("editions-pistes-corps");
  corps.replaceChildren();
  const c = etat.comparaison;
  $("editions-pistes").hidden = !c;
  if (!c) return;
  for (const p of c.paires) {
    const l = p.locale != null ? c.locales[p.locale] : null;
    const e = p.edition != null ? c.edition.pistes[p.edition] : null;
    const tr = document.createElement("tr");
    const marques = [];
    if (!l) marques.push("absente de l'album");
    if (!e) marques.push("absente de l'édition");
    if (p.ecart_ms != null && Math.abs(p.ecart_ms) >= 3000) {
      const s = Math.round(p.ecart_ms / 1000);
      marques.push(`${s > 0 ? "+" : "−"}${Math.abs(s)} s`);
    }
    if (p.titre_differe) marques.push("titre différent");
    if (marques.length) tr.className = "editions__ligne--marquee";
    tr.append(
      cellule(l ? String(l.track_no ?? "") : ""),
      cellule(l ? txt(l.title, "(sans titre)") : "—", "editions__nom"),
      cellule(l ? dureeTotale(l.duration_ms) : ""),
      cellule(e ? `${c.edition.pistes.some((x) => x.disque > 1) ? `${e.disque}-` : ""}${e.position}` : ""),
      cellule(e ? e.titre : "—", "editions__nom"),
      cellule(e ? dureeTotale(e.duree_ms) : ""),
      cellule(marques.join(" · "), "editions__marque"),
    );
    corps.appendChild(tr);
  }
  const ecarts = c.paires.filter((p) =>
    p.locale == null || p.edition == null || p.titre_differe ||
    (p.ecart_ms != null && Math.abs(p.ecart_ms) >= 3000)).length;
  const total = c.edition.pistes.reduce((s, x) => s + (x.duree_ms ?? 0), 0);
  $("editions-total-edition").textContent =
    `${c.edition.pistes.length} pistes · ${dureeTotale(total)}`;
  dire(ecarts
    ? `${ecarts} ligne${ecarts > 1 ? "s" : ""} diffère${ecarts > 1 ? "nt" : ""} — à vérifier avant de retenir.`
    : "Les pistes concordent, rang à rang.");
}

async function choisir(id) {
  etat.choisie = id;
  etat.comparaison = null;
  rendreListe();
  rendrePistes();
  retenir.disabled = true;
  const jeton = ++etat.jeton;
  dire("Lecture de l'édition chez MusicBrainz…");
  try {
    const c = await invoke("edition_comparee", {
      contact: contactMb(), id, album: etat.album.name, artist: etat.album.artist,
    });
    if (jeton !== etat.jeton) return;
    etat.comparaison = c;
    rendrePistes();
    retenir.disabled = false;
  } catch (e) {
    if (jeton !== etat.jeton) return;
    dire(`MusicBrainz n'a pas répondu : ${e}`);
    remonter(e, "édition comparée");
  }
}

async function majRetenue() {
  const r = await invoke("edition_retenue", { album: etat.album.name, artist: etat.album.artist });
  $("editions-retenue").hidden = !r;
  if (r) {
    const le = new Date(r.choisie_le * 1000).toLocaleDateString("fr-FR", { day: "numeric", month: "long", year: "numeric" });
    $("editions-retenue-texte").textContent = `Édition retenue le ${le} : ${r.titre}.`;
  }
}

function resumeLocal() {
  const pistes = vue.quoi === "pistes" ? vue.lignes : [];
  const total = pistes.reduce((s, t) => s + (t.duration_ms ?? 0), 0);
  $("editions-local-titre").textContent = etat.album.name;
  $("editions-local-artiste").textContent = txt(etat.album.artist, "(sans artiste)");
  $("editions-total-local").textContent = `${pistes.length} pistes · ${dureeTotale(total)}`;
}

async function ouvrirEditions() {
  const album = ecoute.albumOuvert;
  if (!album) return;
  etat.album = album;
  etat.propositions = [];
  etat.choisie = null;
  etat.comparaison = null;
  etat.focusAvant = document.activeElement;
  voile.hidden = false;
  $("editions-album").textContent = album.name;
  $("editions-reseau").textContent =
    `Envoie le titre « ${isoler(album.name)} »${album.artist ? ` et l'artiste « ${isoler(album.artist)} »` : ""} ` +
    "à musicbrainz.org, avec votre adresse de contact, et liste les éditions de l'album : label, pays, " +
    "format, année, pistes. Rien n'est écrit dans vos fichiers.";
  resumeLocal();
  rendreListe();
  rendrePistes();
  retenir.disabled = true;
  $("editions-fermer").focus();
  majRetenue().catch((e) => remonter(e, "édition retenue"));
  if (!contactMb().includes("@")) {
    dire("MusicBrainz demande une adresse de contact : Bibliothèque → Options de la chaîne.");
    return;
  }
  const jeton = ++etat.jeton;
  dire("Recherche des éditions chez MusicBrainz…");
  try {
    etat.propositions = await invoke("editions_proposees", {
      contact: contactMb(), album: album.name, artist: album.artist,
    });
    if (jeton !== etat.jeton || !ouverte()) return;
    rendreListe();
    if (!etat.propositions.length) {
      dire("Aucune édition trouvée chez MusicBrainz pour ce titre et cet artiste.");
      return;
    }
    // La première est montrée en comparaison, sans rien retenir.
    choisir(etat.propositions[0].id);
  } catch (e) {
    if (jeton !== etat.jeton) return;
    dire(`MusicBrainz n'a pas répondu : ${e}`);
    remonter(e, "éditions proposées");
  }
}

function fermerEditions() {
  if (!ouverte()) return;
  voile.hidden = true;
  etat.jeton++;
  etat.focusAvant?.focus?.();
}

retenir.addEventListener("click", async () => {
  if (!etat.choisie) return;
  retenir.disabled = true;
  dire("Enregistrement…");
  try {
    await invoke("retenir_edition", {
      contact: contactMb(), id: etat.choisie, album: etat.album.name, artist: etat.album.artist,
    });
    await majRetenue();
    dire("Édition retenue. Genres et crédits suivront au prochain « Tout rafraîchir ».");
  } catch (e) {
    dire(`Rien n'a été retenu : ${e}`);
    remonter(e, "retenir l'édition");
    retenir.disabled = false;
  }
});

$("editions-oublier").addEventListener("click", async () => {
  try {
    const n = await invoke("oublier_edition", { album: etat.album.name, artist: etat.album.artist });
    await majRetenue();
    dire(`Choix oublié : ${n} piste${n > 1 ? "s" : ""} reprennent les identifiants de leurs tags.`);
  } catch (e) {
    dire(`Le choix n'a pas pu être oublié : ${e}`);
    remonter(e, "oublier l'édition");
  }
});

$("editions-fermer").addEventListener("click", fermerEditions);
$("editions-annuler").addEventListener("click", fermerEditions);
$("choisir-edition").addEventListener("click", () => ouvrirEditions().catch((e) => remonter(e, "éditions")));
voile.addEventListener("pointerdown", (e) => {
  if (e.target === voile) fermerEditions();
});
window.addEventListener("keydown", (e) => {
  if (e.key !== "Escape" || !ouverte()) return;
  e.stopPropagation();
  fermerEditions();
}, true);

export {
  fermerEditions, ouvrirEditions,
};
