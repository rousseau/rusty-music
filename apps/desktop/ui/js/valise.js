import "./modes.js"; // l'ordre d'exécution : voir main.js
import {
  $, duree, invoke, isoler, remonter,
} from "./socle.js";
import { lecture } from "./etat-lecture.js";
import {
  carte,
} from "./carte.js";
import {
  chargerFamilles, couleursFamilles,
} from "./maplibre.js";

/* ------------------------------------------------------ mode Valise */

/// Le mode Valise : composer la valise du compagnon iOS et l'envoyer sur
/// l'iPhone (`docs/plan-ios-compagnon.md`). Le moteur choisit les albums
/// (`valise_apercu`, qui garde aussi les réglages pour la prochaine fois) ;
/// l'iPhone est lu par `devicectl` (`valise_iphone`), quelques secondes en
/// Wi-Fi — d'où la lecture en tâche de fond, jamais à chaque réglage.
const etatValise = {
  reglages: null, // `valise::Reglages`, tel que le moteur le rend
  apercu: null, // { albums, octets, budget_octets }
  iphone: null, // `EtatIphone`
  minuterie: null, // l'aperçu attend la fin d'une frappe
  suivi: null, // sondage de l'envoi et de la réinstallation
  occupe: false, // un envoi ou une réinstallation tourne
  lectureIphone: null, // la lecture de l'iPhone en cours (promesse)
  emporte: false, // un passage est en cours de dépôt
  wifi: null, // `EtatWifiVue` (`wifi.rs`)
  suiviWifi: false, // la ligne Wi-Fi suit une lecture en attente
};

const REGLES_VALISE = [
  ["recents", "Récents"],
  ["peu-ecoutes", "Peu écoutés"],
  ["famille", "Famille"],
  ["hasard", "Au hasard"],
];
const GO = 1024 ** 3;
const enGo = (o) => (o / GO).toLocaleString("fr-FR", { maximumFractionDigits: 1 });

async function entrerValise() {
  await chargerFamilles();
  if (!etatValise.reglages) {
    const r = await invoke("valise_reglages");
    // La ligne de commande partage le budget à parts égales ; ici, chaque
    // ligne compte ses albums.
    for (const p of r.parts) p.albums ??= 20;
    etatValise.reglages = r;
  }
  dessinerReglagesValise();
  await rafraichirApercuValise();
  lireIphone().catch((e) => remonter(e, "iPhone"));
  suivreValise();
}

function dessinerReglagesValise() {
  const r = etatValise.reglages;
  const hote = $("valise-parts");
  hote.replaceChildren();
  r.parts.forEach((part, i) => {
    const ligne = document.createElement("div");
    ligne.className = "valise-part";

    const regle = document.createElement("select");
    regle.className = "chercher";
    for (const [code, nom] of REGLES_VALISE) regle.add(new Option(nom, code, false, part.regle === code));
    regle.addEventListener("change", () => {
      part.regle = regle.value;
      if (part.regle === "famille") part.famille ??= carte.familles?.[0]?.[0] ?? 0;
      dessinerReglagesValise();
      changerValise();
    });

    // La famille n'a de sens que pour la règle du même nom ; la case reste
    // pour que les colonnes ne bougent pas d'une ligne à l'autre.
    let famille = document.createElement("span");
    if (part.regle === "famille") {
      famille = document.createElement("select");
      famille.className = "chercher";
      for (const [c, nom] of carte.familles ?? []) {
        famille.add(new Option(nom || `famille ${c + 1}`, c, false, part.famille === c));
      }
      famille.addEventListener("change", () => {
        part.famille = +famille.value;
        changerValise();
      });
    }

    const nombre = document.createElement("input");
    nombre.className = "chercher";
    nombre.type = "number";
    nombre.min = 0;
    nombre.max = 500;
    nombre.value = part.albums ?? 0;
    nombre.addEventListener("input", () => {
      part.albums = Math.max(0, parseInt(nombre.value, 10) || 0);
      changerValise();
    });

    const unite = document.createElement("span");
    unite.textContent = "albums";

    const retirer = document.createElement("button");
    retirer.className = "lien valise-part__retirer";
    retirer.textContent = "✕";
    retirer.title = "Retirer cette ligne";
    retirer.addEventListener("click", () => {
      r.parts.splice(i, 1);
      dessinerReglagesValise();
      changerValise();
    });

    ligne.append(regle, famille, nombre, unite, retirer);
    hote.appendChild(ligne);
  });

  $("valise-budget").value = Math.round((r.budget_octets / GO) * 10) / 10;
  $("valise-par-artiste").value = r.par_artiste;
  $("valise-pistes-min").value = r.pistes_min;
}

/// Un réglage a changé : l'aperçu suit, une fois la frappe finie.
function changerValise() {
  clearTimeout(etatValise.minuterie);
  etatValise.minuterie = setTimeout(() => {
    etatValise.minuterie = null;
    rafraichirApercuValise().catch((e) => remonter(e, "valise"));
  }, 250);
}

async function rafraichirApercuValise() {
  etatValise.apercu = await invoke("valise_apercu", { reglages: etatValise.reglages });
  dessinerApercuValise();
}

function dessinerApercuValise() {
  const r = etatValise.reglages;
  const ap = etatValise.apercu;
  if (!ap) return;
  const duree = ap.albums.reduce((s, a) => s + a.duree_ms, 0) / 3_600_000;
  $("valise-resume").textContent =
    `${ap.albums.length} albums · ${enGo(ap.octets)} Go sur ${enGo(ap.budget_octets)} Go` +
    ` · ${Math.round(duree)} h d'écoute`;
  $("valise-remplissage").style.width = `${Math.min(100, (ap.octets / ap.budget_octets) * 100 || 0)}%`;

  const exclus = $("valise-exclus");
  exclus.hidden = r.exclus.length === 0;
  if (r.exclus.length) {
    exclus.textContent = `${r.exclus.length} album${r.exclus.length > 1 ? "s" : ""} écarté${r.exclus.length > 1 ? "s" : ""} à la main · `;
    const tout = document.createElement("button");
    tout.className = "lien lien--inline";
    tout.textContent = "Tout rétablir";
    tout.addEventListener("click", () => {
      r.exclus = [];
      changerValise();
    });
    exclus.appendChild(tout);
  }

  const teintes = couleursFamilles();
  const libelles = Object.fromEntries(REGLES_VALISE);
  libelles.choisi = "Gardé";
  const nomsFamilles = new Map((carte.familles ?? []).map(([c, nom]) => [c, nom || `famille ${c + 1}`]));
  const hote = $("valise-albums");
  hote.replaceChildren();
  for (const a of ap.albums) {
    const garde = r.epingles.includes(a.id);
    const ligne = document.createElement("div");
    ligne.className = "verif" + (garde ? " verif--garde" : "");

    const pastille = document.createElement("span");
    pastille.className = "famille__pastille";
    pastille.style.background = a.famille == null ? "transparent" : teintes[a.famille % teintes.length];
    pastille.title = a.famille == null ? "" : nomsFamilles.get(a.famille) ?? "";

    const corps = document.createElement("div");
    corps.className = "verif__corps";
    const titre = document.createElement("div");
    titre.className = "verif__ligne";
    titre.textContent = `${isoler(a.artiste)} — ${isoler(a.album)}${a.annee ? ` (${a.annee})` : ""}`;
    const detail = document.createElement("div");
    detail.className = "verif__detail";
    const ecoutes = a.ecoutes === 0 ? "jamais écouté" : `écouté ${a.ecoutes} fois`;
    const regle = a.regle === "famille" ? nomsFamilles.get(a.famille) ?? "Famille" : libelles[a.regle];
    detail.textContent = `${regle} · ${a.nb_pistes} pistes · ${Math.round(a.octets / 1048576)} Mo · ${ecoutes}`;
    corps.append(titre, detail);

    const actions = document.createElement("div");
    actions.className = "verif__actions";
    const garder = document.createElement("button");
    garder.className = "verif__geste" + (garde ? " verif__geste--actif" : "");
    garder.textContent = garde ? "gardé" : "garder";
    garder.title = garde
      ? "Ne plus garder : l'album repasse par les règles"
      : "Garder cet album dans les prochaines valises, quel que soit le tirage";
    garder.addEventListener("click", () => {
      r.epingles = garde ? r.epingles.filter((id) => id !== a.id) : [...r.epingles, a.id];
      changerValise();
    });
    const ecarter = document.createElement("button");
    ecarter.className = "verif__geste";
    ecarter.textContent = "écarter";
    ecarter.title = "Écarter cet album : aucune règle ne le reprendra";
    ecarter.addEventListener("click", () => {
      r.epingles = r.epingles.filter((id) => id !== a.id);
      r.exclus = [...r.exclus, a.id];
      changerValise();
    });
    actions.append(garder, ecarter);

    ligne.append(pastille, corps, actions);
    hote.appendChild(ligne);
  }
  majBoutonEnvoyer();
}

function majBoutonEnvoyer() {
  const i = etatValise.iphone;
  $("valise-envoyer").disabled =
    etatValise.occupe || !i?.udid || !i.app_installee || !etatValise.apercu?.albums.length;
}

/// Une seule lecture de l'iPhone à la fois : « Emporter la lecture » depuis ⌘K
/// entre dans Valise, qui en lance une, et attend la même.
function lireIphone() {
  etatValise.lectureIphone ??= (async () => {
    $("iphone-nom").textContent = "Recherche de l'iPhone…";
    $("iphone-etat").textContent = "";
    try {
      [etatValise.iphone, etatValise.wifi] = await Promise.all([
        invoke("valise_iphone", { udid: etatValise.iphone?.udid ?? null }),
        invoke("wifi_etat"),
      ]);
    } finally {
      etatValise.lectureIphone = null;
    }
    dessinerIphone();
  })();
  return etatValise.lectureIphone;
}

function dessinerIphone() {
  const i = etatValise.iphone;
  const nom = $("iphone-nom");
  const etat = $("iphone-etat");
  const echeance = $("iphone-echeance");
  echeance.hidden = true;
  $("valise-renouveler").hidden = true;
  $("iphone-emporter").hidden = !(parCable() || etatValise.wifi?.ecoute);
  majBoutonEmporter();
  dessinerWifi();
  $("iphone-valises").textContent = "—";
  majBoutonEnvoyer();
  if (!i.xcode) {
    nom.textContent = "Xcode absent";
    etat.textContent = "L'envoi et le renouvellement passent par devicectl, l'outil d'Xcode.";
    return;
  }
  const a = i.appareils.find((x) => x.udid === i.udid);
  if (!a && etatValise.wifi?.appaire) {
    nom.textContent = "iPhone appairé pour le Wi-Fi";
    etat.textContent = i.erreur ?? "Hors de portée de devicectl : l'envoi d'une valise attend le câble.";
    return;
  }
  if (!a) {
    nom.textContent = "Aucun iPhone appairé";
    etat.textContent = i.erreur ?? "Branchez l'iPhone en USB une première fois et faites confiance à ce Mac.";
    return;
  }
  nom.textContent = a.nom;
  etat.textContent = i.erreur ?? [a.modele, a.filaire ? "USB" : "Wi-Fi",
    i.app_installee ? null : "Rusty Valise n'y est pas installée"].filter(Boolean).join(" · ");

  if (i.renouvelable) {
    const b = $("valise-renouveler");
    b.hidden = false;
    b.textContent = i.app_installee ? "Renouveler l'app" : "Installer l'app";
    b.disabled = etatValise.occupe;
  }
  if (i.echeance) {
    const fin = new Date(i.echeance);
    const jours = (fin - Date.now()) / 86_400_000;
    const quand = fin.toLocaleString("fr-FR", { weekday: "long", day: "numeric", month: "long", hour: "2-digit", minute: "2-digit" });
    echeance.hidden = false;
    echeance.classList.toggle("iphone__echeance--proche", jours < 2);
    echeance.textContent = jours < 0
      ? `L'app a expiré le ${quand}.`
      : `L'app expire dans ${jours < 1 ? "moins d'un jour" : `${Math.floor(jours)} jour${jours >= 2 ? "s" : ""}`} — ${quand}.`;
  }
  if (i.app_installee) {
    $("iphone-valises").replaceChildren(
      ...(i.valises.length ? i.valises : [null]).map((v) => {
        const l = document.createElement("div");
        l.textContent = v
          ? `${v.nom}${v.le ? ` · ${new Date(v.le).toLocaleDateString("fr-FR", { day: "numeric", month: "short" })}` : ""}`
          : "Aucune valise.";
        return l;
      }),
    );
  }
}

/// Le passage par le câble est possible : Xcode, un iPhone, l'app dessus.
function parCable() {
  const i = etatValise.iphone;
  return Boolean(i?.xcode && i.udid && i.app_installee);
}

/// La ligne Wi-Fi du bloc iPhone (`wifi.rs`) : appairé ou non, une lecture
/// en attente, ce qui s'est passé en dernier.
function dessinerWifi() {
  const w = etatValise.wifi;
  const ligne = $("iphone-wifi");
  $("iphone-oublier").hidden = !w?.appaire;
  if (!w?.appaire) {
    ligne.hidden = !parCable();
    ligne.textContent = "Wi-Fi : le prochain envoi ou passage par le câble appairera l'iPhone.";
    return;
  }
  ligne.hidden = false;
  ligne.textContent = [
    w.ecoute
      ? "Wi-Fi : appairé, l'iPhone retrouve ce Mac sur le réseau local."
      : `Wi-Fi : appairé, mais ce Mac n'écoute pas : ${w.erreur ?? "raison inconnue"}.`,
    w.en_attente ? "Une lecture attend que l'iPhone la prenne." : null,
    w.dernier,
  ].filter(Boolean).join(" ");
}

/// Tant qu'une lecture attend l'iPhone, la ligne Wi-Fi suit : elle dit quand
/// il l'a prise.
async function suivreAttente() {
  if (etatValise.suiviWifi) return;
  etatValise.suiviWifi = true;
  try {
    while (!$("bloc-valise").hidden) {
      await new Promise((r) => setTimeout(r, 2000));
      etatValise.wifi = await invoke("wifi_etat");
      dessinerWifi();
      if (!etatValise.wifi.en_attente) break;
    }
  } finally {
    etatValise.suiviWifi = false;
  }
}

function majBoutonEmporter() {
  $("iphone-emporter").disabled = etatValise.emporte || etatValise.occupe;
}

/// Emporte ce qui joue sur l'iPhone (`emporter_lecture`,
/// `docs/plan-ios-compagnon.md`) : par le câble si possible, le Mac se met en
/// pause une fois le dépôt confirmé ; sinon par le Wi-Fi, et il se met en
/// pause quand l'iPhone l'a prise. L'app reprend en pause, à la position.
async function emporterLecture() {
  const statut = $("iphone-emporter-etat");
  const dire = (texte) => {
    statut.hidden = false;
    statut.textContent = texte;
  };
  if (etatValise.lectureIphone) await etatValise.lectureIphone;
  else if (!etatValise.iphone) await lireIphone();
  const cable = parCable();
  if (!cable && !etatValise.wifi?.ecoute) {
    dire("Aucun iPhone branché ni appairé pour le Wi-Fi : voir ci-dessus.");
    return;
  }
  if (!lecture.courant) {
    dire("Rien en lecture à emporter.");
    return;
  }
  etatValise.emporte = true;
  majBoutonEmporter();
  dire(cable ? "Dépôt de la lecture sur l'iPhone…" : "La lecture attend l'iPhone sur le Wi-Fi…");
  try {
    const r = await invoke("emporter_lecture", { udid: cable ? etatValise.iphone.udid : null });
    const quoi = r.titre ? `« ${isoler(r.titre)} » à ${duree(r.position_ms)}` : `La file (${r.pistes} pistes)`;
    if (r.par_wifi) {
      dire(`${quoi} attend l'iPhone sur le Wi-Fi${r.cable_echec ? ` (le câble n'a pas servi : ${r.cable_echec})` : ""}. `
        + "Ouvrez Rusty Valise : le Mac se mettra en pause quand elle l'aura prise.");
      etatValise.wifi = await invoke("wifi_etat");
      dessinerWifi();
      suivreAttente();
    } else {
      dire(r.mac_en_pause
        ? `${quoi} attend sur l'iPhone, en pause ; le Mac s'est arrêté. Ouvrez Rusty Valise.`
        : `${quoi} attend sur l'iPhone. La lecture a changé ici pendant le dépôt : le Mac continue.`);
      // Le câble appaire au passage : la ligne Wi-Fi peut avoir changé.
      etatValise.wifi = await invoke("wifi_etat");
      dessinerWifi();
    }
  } catch (e) {
    dire(`Rien n'a été emporté : ${e}`);
    remonter(e, "emporter la lecture");
  } finally {
    etatValise.emporte = false;
    majBoutonEmporter();
  }
}

/// « valise-2026-10-19-2041 » : l'heure locale, pour qu'elles se rangent.
function nomValise() {
  const d = new Date();
  const z = (n) => String(n).padStart(2, "0");
  return `valise-${d.getFullYear()}-${z(d.getMonth() + 1)}-${z(d.getDate())}-${z(d.getHours())}${z(d.getMinutes())}`;
}

/// Sonde l'envoi et la réinstallation tant que l'un tourne ; relit l'iPhone
/// quand ils finissent.
function suivreValise() {
  if (etatValise.suivi) return;
  const tour = async () => {
    let e;
    try {
      e = await invoke("valise_etat");
    } catch (err) {
      remonter(err, "valise");
      return;
    }
    const { envoi, renouvellement: ren } = e;
    const occupe = envoi.en_cours || ren.en_cours;
    const fini = etatValise.occupe && !occupe;
    etatValise.occupe = occupe;

    const jauge = $("valise-jauge");
    const avancement = $("valise-avancement");
    jauge.hidden = !envoi.en_cours;
    if (envoi.en_cours) {
      if (envoi.total) {
        jauge.max = envoi.total;
        jauge.value = envoi.faits;
      } else jauge.removeAttribute("value");
      avancement.hidden = false;
      avancement.textContent = envoi.total ? `${envoi.etape} — ${envoi.faits + 1} / ${envoi.total}` : `${envoi.etape}…`;
    } else {
      avancement.hidden = !envoi.resultat;
      avancement.textContent = envoi.resultat ?? "";
    }

    const r = $("valise-renouveler-etat");
    r.hidden = !ren.en_cours && !ren.resultat;
    r.textContent = ren.en_cours ? ren.ligne : ren.resultat ?? "";
    $("valise-renouveler").disabled = occupe;
    majBoutonEnvoyer();
    majBoutonEmporter();

    if (fini) lireIphone().catch((err) => remonter(err, "iPhone"));
    if (!occupe) arreterSuiviValise();
  };
  etatValise.suivi = setInterval(tour, 800);
  tour();
}

function arreterSuiviValise() {
  clearInterval(etatValise.suivi);
  etatValise.suivi = null;
}

$("valise-ajouter-part").addEventListener("click", () => {
  etatValise.reglages.parts.push({ regle: "hasard", albums: 10, famille: null });
  dessinerReglagesValise();
  changerValise();
});
$("valise-budget").addEventListener("input", (e) => {
  const go = parseFloat(String(e.target.value).replace(",", "."));
  if (go > 0) {
    etatValise.reglages.budget_octets = Math.round(go * GO);
    changerValise();
  }
});
$("valise-par-artiste").addEventListener("input", (e) => {
  etatValise.reglages.par_artiste = Math.max(1, parseInt(e.target.value, 10) || 1);
  changerValise();
});
$("valise-pistes-min").addEventListener("input", (e) => {
  etatValise.reglages.pistes_min = Math.max(1, parseInt(e.target.value, 10) || 1);
  changerValise();
});
$("valise-tirage").addEventListener("click", () => {
  etatValise.reglages.graine = Date.now();
  changerValise();
});
$("iphone-actualiser").addEventListener("click", () => lireIphone().catch((e) => remonter(e, "iPhone")));
$("iphone-emporter").addEventListener("click", () => emporterLecture());
$("iphone-oublier").addEventListener("click", async () => {
  try {
    await invoke("oublier_iphone");
    etatValise.wifi = await invoke("wifi_etat");
    dessinerIphone();
  } catch (e) {
    remonter(e, "oublier l'iPhone");
  }
});
$("valise-envoyer").addEventListener("click", async () => {
  // Les réglages partent avec l'aperçu : finir celui qui attend.
  if (etatValise.minuterie) {
    clearTimeout(etatValise.minuterie);
    etatValise.minuterie = null;
    await rafraichirApercuValise();
  }
  try {
    await invoke("valise_envoyer", { udid: etatValise.iphone.udid, nom: nomValise() });
  } catch (e) {
    $("valise-avancement").hidden = false;
    $("valise-avancement").textContent = `échec : ${e}`;
    return;
  }
  etatValise.occupe = true;
  majBoutonEnvoyer();
  suivreValise();
});
$("valise-renouveler").addEventListener("click", async () => {
  try {
    await invoke("valise_renouveler", { udid: etatValise.iphone.udid });
  } catch (e) {
    $("valise-renouveler-etat").hidden = false;
    $("valise-renouveler-etat").textContent = `échec : ${e}`;
    return;
  }
  etatValise.occupe = true;
  suivreValise();
});

export {
  arreterSuiviValise, emporterLecture, entrerValise,
};
