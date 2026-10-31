// SPDX-License-Identifier: GPL-3.0-or-later
// `window.__TAURI__` simulé, chargé avant `app.js` (addInitScript).
//
// Chaque `invoke` répond par `window.__FIXTURES__[commande]` : une valeur
// figée, ou une fonction (args, etat) → valeur. Une commande sans réponse
// rend `null` et reste notée dans `__APPELS__` : le test la voit, rien ne
// casse en silence. `etat` est partagé entre les réponses, pour simuler un
// lecteur qui avance.
(() => {
  const appels = [];
  const ecouteurs = {};
  const etat = {};
  window.__APPELS__ = appels;
  window.__ETAT_FAUX__ = etat;
  const repondre = async (cmd, args = {}) => {
    const f = (window.__FIXTURES__ || {})[cmd];
    appels.push({ cmd, args, connu: f !== undefined });
    if (typeof f === "function") return f(args, etat);
    if (f instanceof Error) throw f.message;
    return f === undefined ? null : structuredClone(f);
  };
  window.__TAURI__ = {
    core: {
      invoke: repondre,
      convertFileSrc: (p) => `fichier://${p}`,
      Channel: class {
        constructor() {
          this.onmessage = () => {};
        }
      },
    },
    event: {
      listen: async (nom, rappel) => {
        (ecouteurs[nom] ||= []).push(rappel);
        return () => {};
      },
    },
    dialog: { open: async () => null },
    opener: { openUrl: async () => {} },
  };
  // Pour qu'un test émette un événement comme le ferait Rust.
  window.__EMETTRE__ = (nom, payload) => (ecouteurs[nom] || []).forEach((r) => r({ payload }));
})();
