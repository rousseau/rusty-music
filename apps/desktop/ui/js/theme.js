// Le thème de l'interface (`docs/interface-guidelines.md`, Règle 6).
// `style.css` dessine les deux : `:root` (sombre) et `:root[data-theme="clair"]`.
// Trois choix : sombre, clair, ou suivre le système (`prefers-color-scheme`).
// `theme-avant.js` pose le thème avant le premier affichage ; ce module garde
// le choix ensuite, suit le système quand on le lui demande et prévient les
// canevas, qui lisent leurs couleurs dans les jetons au moment de dessiner
// (évènement `theme` sur `document`). Hors de la chaîne d'exécution (voir
// `main.js`) : il ne fait que déclarer, plus un écouteur du système.

const CLE = "rusty-music.theme";
const CHOIX = ["sombre", "clair", "systeme"];
const systemeClair = matchMedia("(prefers-color-scheme: light)");

/// Le choix mémorisé ; sombre par défaut, le thème d'origine de l'application.
function themeChoisi() {
  try {
    const c = localStorage.getItem(CLE);
    if (CHOIX.includes(c)) return c;
  } catch {
    // Stockage refusé : défaut.
  }
  return "sombre";
}

/// Vrai si le thème clair est affiché.
const clairActif = () => document.documentElement.dataset.theme === "clair";

/// Affiche le thème qui découle de `choix` et prévient qui dessine.
function appliquer(choix) {
  const clair = choix === "clair" || (choix === "systeme" && systemeClair.matches);
  if (clair === clairActif()) return;
  if (clair) document.documentElement.dataset.theme = "clair";
  else delete document.documentElement.dataset.theme;
  document.dispatchEvent(new CustomEvent("theme"));
}

/// Retient `choix` (« sombre », « clair » ou « systeme ») et l'affiche.
function choisirTheme(choix) {
  try {
    localStorage.setItem(CLE, choix);
  } catch {
    // Stockage refusé : le choix vaut pour la session.
  }
  appliquer(choix);
  document.dispatchEvent(new CustomEvent("theme-choisi", { detail: choix }));
}

/// Passe d'un thème à l'autre — un choix explicite, qui cesse de suivre le
/// système.
const basculerTheme = () => choisirTheme(clairActif() ? "sombre" : "clair");

systemeClair.addEventListener("change", () => {
  if (themeChoisi() === "systeme") appliquer("systeme");
});

export {
  basculerTheme, choisirTheme, clairActif, themeChoisi,
};
