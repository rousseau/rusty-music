// SPDX-License-Identifier: GPL-3.0-or-later
// Pose le thème mémorisé avant que la feuille de style ne peigne : sans ce
// script, un utilisateur du thème clair verrait d'abord l'interface en sombre.
// Classique et synchrone, chargé dans <head> : la politique de sécurité de
// Tauri interdit les scripts en ligne, et un module attendrait la fin de
// l'analyse du document. `js/theme.js` reprend la main ensuite.
//
// Même principe que `public/theme.js` de Disc Player
// (https://github.com/eudj1n/snowsky-disc-player, licence MIT).
(function () {
  var choix;
  try {
    choix = localStorage.getItem("rusty-music.theme");
  } catch (e) {
    choix = null;
  }
  var clair =
    choix === "clair" ||
    (choix === "systeme" && matchMedia("(prefers-color-scheme: light)").matches);
  if (clair) document.documentElement.dataset.theme = "clair";
})();

// Les largeurs du rail et du panneau droit, mémorisées par `js/panneaux.js`,
// posées elles aussi avant la première peinture (bornées là-bas à la fenêtre).
(function () {
  var l;
  try {
    l = JSON.parse(localStorage.getItem("rusty-music.panneaux") || "{}") || {};
  } catch (e) {
    l = {};
  }
  var s = document.documentElement.style;
  if (l.rail > 0) s.setProperty("--largeur-rail", Math.min(420, Math.max(200, l.rail)) + "px");
  if (l.droite > 0) s.setProperty("--largeur-droite", Math.min(560, Math.max(260, l.droite)) + "px");
})();
