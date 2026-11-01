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
