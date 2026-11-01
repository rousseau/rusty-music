// Le thème de l'interface, sombre ou clair (`docs/interface-guidelines.md`,
// Règle 6). `style.css` dessine les deux : `:root` (sombre) et
// `:root[data-theme="clair"]`. Ce module pose l'attribut et mémorise le choix.
// Hors de la chaîne d'exécution (voir `main.js`) : il ne fait que déclarer.

const CLE = "rusty-music.theme";

/// Vrai si le thème clair est affiché.
const clairActif = () => document.documentElement.dataset.theme === "clair";

/// Affiche le thème `theme` (« clair » ou « sombre ») et le mémorise.
function choisirTheme(theme) {
  if (theme === "clair") document.documentElement.dataset.theme = "clair";
  else delete document.documentElement.dataset.theme;
  try {
    localStorage.setItem(CLE, theme);
  } catch {
    // Stockage refusé : le choix vaut pour la session.
  }
}

/// Passe d'un thème à l'autre.
const basculerTheme = () => choisirTheme(clairActif() ? "sombre" : "clair");

export {
  basculerTheme, choisirTheme, clairActif,
};
