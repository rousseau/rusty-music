// SPDX-License-Identifier: GPL-3.0-or-later
import { $, invoke, journal } from "./socle.js";

/* ------------------------------- pratiquer : les pads de la batterie MIDI */

// E4 (`docs/recherche-coach-pratique.md`, « La batterie : MIDI d'abord ») :
// le profil de correspondance notes → classes, gardé par batterie, se règle
// ici. On frappe un pad, sa note s'affiche avec la classe qu'elle reçoit
// (ou « ignorée ») ; on la change d'un choix. Les notes de chaque classe
// se retirent d'un clic ; la vélocité minimale (diaphonie) et le contrôleur
// d'ouverture du charleston se règlent dessous ; « General MIDI » revient au
// profil de départ. Chaque changement est gardé aussitôt
// (`coach_garder_profil`) et vaut pour les coups suivants.
//
// Le panneau vit dans le repli des réglages de l'entrée (V43), derrière
// « pads » : rien de plus dans le bandeau, rien dans le rail (Règle 12).

const CLASSES = [
  ["grosse_caisse", "Grosse caisse"],
  ["caisse_claire", "Caisse claire"],
  ["toms", "Toms"],
  ["charleston", "Charleston"],
  ["cymbales", "Cymbales"],
];
const NOMS = Object.fromEntries(CLASSES);

/// Sondage des notes reçues tant que le panneau est ouvert.
const SONDE_MS = 150;

const etat = {
  profil: null,
  peripherique: null,
  ouvert: false,
  minuteur: null,
  // La note frappée montrée : `{ t_s, note, velocite, classe, sous_le_seuil }`.
  derniere: null,
  // Les notes reçues avant l'ouverture ne sont pas montrées.
  depuis: null,
  occupe: false,
  message: null,
};

const panneau = $("coach-profil");
const bouton = $("coach-pads");
const choix = $("coach-profil-classe");
choix.replaceChildren(new Option("ignorée", ""), ...CLASSES.map(([v, n]) => new Option(n, v)));

/// Le profil de la batterie écoutée (`null` : plus d'écoute).
function poserProfil(profil, peripherique) {
  etat.profil = profil ?? null;
  etat.peripherique = peripherique ?? null;
  etat.derniere = null;
  etat.message = null;
  if (!profil) fermer();
  dessiner();
}

/// « pads » n'est là que pour une batterie écoutée.
function montrerPads(visible) {
  bouton.hidden = !visible || !etat.profil;
  if (bouton.hidden) fermer();
}

function ouvrir() {
  etat.ouvert = true;
  etat.depuis = null;
  etat.derniere = null;
  etat.message = null;
  panneau.hidden = false;
  bouton.setAttribute("aria-pressed", "true");
  dessiner();
  clearInterval(etat.minuteur);
  etat.minuteur = setInterval(sonder, SONDE_MS);
  sonder();
}

function fermer() {
  etat.ouvert = false;
  panneau.hidden = true;
  bouton.setAttribute("aria-pressed", "false");
  clearInterval(etat.minuteur);
  etat.minuteur = null;
}

async function sonder() {
  if (!etat.ouvert || etat.occupe) return;
  etat.occupe = true;
  try {
    const recues = (await invoke("coach_notes_recues")) ?? [];
    const n = recues.at(-1);
    if (etat.depuis === null) {
      // Premier sondage : le point de départ, rien de montré d'avant.
      etat.depuis = n?.t_s ?? -Infinity;
    } else if (n && n.t_s > etat.depuis && n.t_s !== etat.derniere?.t_s) {
      etat.derniere = n;
      etat.message = null;
      dessiner();
    }
  } catch (e) {
    journal("coach", `notes reçues : ${e}`, "warn");
  } finally {
    etat.occupe = false;
  }
}

/// La classe que le profil donne à une note, ou null.
const classeDe = (note) => etat.profil?.notes?.[note] ?? null;

async function garder(profil, renommer = true) {
  // Un profil changé à la main porte le nom de sa batterie.
  const nouveau = renommer ? { ...profil, nom: etat.peripherique ?? profil.nom } : profil;
  try {
    await invoke("coach_garder_profil", { profil: nouveau });
    etat.profil = nouveau;
    etat.message = null;
  } catch (e) {
    etat.message = `Non gardé : ${e}`;
  }
  dessiner();
}

function affecter(note, classe) {
  const notes = { ...etat.profil.notes };
  if (classe) notes[note] = classe;
  else delete notes[note];
  return garder({ ...etat.profil, notes });
}

function dessiner() {
  bouton.title = `Pads de la batterie : quelle note va à quelle pièce (profil « ${etat.profil?.nom ?? "General MIDI"} », gardé pour cette batterie)`;
  if (!etat.profil) return;
  const d = etat.derniere;
  const ligne = $("coach-profil-dernier");
  const choixLabel = $("coach-profil-choix");
  if (etat.message) {
    ligne.textContent = etat.message;
  } else if (!d) {
    ligne.textContent = "Frapper un pad : sa note s'affiche ici, et la pièce qu'elle compte.";
  } else {
    const c = classeDe(d.note);
    const seuil = d.sous_le_seuil ? ` — sous la vélocité minimale (${etat.profil.velocite_min}) : pas compté` : "";
    ligne.textContent = `Note ${d.note} · vélocité ${d.velocite} → ${c ? NOMS[c] : "ignorée"}${seuil}`;
  }
  choixLabel.hidden = !d;
  if (d) choix.value = classeDe(d.note) ?? "";

  const table = $("coach-profil-table");
  table.replaceChildren(
    ...CLASSES.map(([classe, nom]) => {
      const li = document.createElement("li");
      li.className = "coach__profil-classe";
      const titre = document.createElement("span");
      titre.className = "coach__profil-nom";
      titre.textContent = nom;
      li.append(titre);
      const notes = Object.entries(etat.profil.notes)
        .filter(([, c]) => c === classe)
        .map(([n]) => Number(n))
        .sort((a, b) => a - b);
      if (!notes.length) {
        const vide = document.createElement("span");
        vide.className = "coach__profil-vide";
        vide.textContent = "aucune note";
        li.append(vide);
      }
      for (const n of notes) {
        const b = document.createElement("button");
        b.type = "button";
        b.className = "coach__profil-note";
        if (d && d.note === n) b.classList.add("coach__profil-note--frappee");
        b.dataset.note = String(n);
        b.textContent = `${n} ×`;
        b.setAttribute("aria-label", `Retirer la note ${n} (${nom})`);
        b.title = `Ignorer la note ${n} : un coup sur elle ne comptera plus`;
        li.append(b);
      }
      return li;
    }),
  );
  $("coach-profil-seuil").value = String(etat.profil.velocite_min ?? 1);
  $("coach-profil-cc").value = etat.profil.cc_charleston ?? "";
}

/// Un entier entre 0 et 127, ou null.
function entier(valeur, min) {
  if (valeur === "") return null;
  const n = Math.round(Number(valeur));
  return Number.isFinite(n) && n >= min && n <= 127 ? n : undefined;
}

bouton.addEventListener("click", () => (etat.ouvert ? fermer() : ouvrir()));
// Le repli des réglages refermé arrête le sondage.
$("coach-reglages").addEventListener("toggle", (e) => {
  if (!e.target.open) fermer();
});
choix.addEventListener("change", () => {
  if (etat.derniere) affecter(etat.derniere.note, choix.value || null);
});
$("coach-profil-table").addEventListener("click", (e) => {
  const b = e.target.closest(".coach__profil-note");
  if (b) affecter(Number(b.dataset.note), null);
});
$("coach-profil-seuil").addEventListener("change", (e) => {
  const v = entier(e.target.value, 1);
  if (v === undefined || v === null) return dessiner();
  garder({ ...etat.profil, velocite_min: v });
});
$("coach-profil-cc").addEventListener("change", (e) => {
  const v = entier(e.target.value, 0);
  if (v === undefined) return dessiner();
  garder({ ...etat.profil, cc_charleston: v });
});
$("coach-profil-gm").addEventListener("click", async () => {
  try {
    const gm = await invoke("coach_profil_general_midi");
    if (gm) await garder(gm, false);
  } catch (e) {
    etat.message = `General MIDI indisponible : ${e}`;
    dessiner();
  }
});

export { montrerPads, poserProfil };
