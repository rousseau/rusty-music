// SPDX-License-Identifier: GPL-3.0-or-later
const { defineConfig } = require("@playwright/test");

module.exports = defineConfig({
  testDir: ".",
  timeout: 60_000,
  retries: 0,
  reporter: [["list"]],
  use: {
    // Le Chrome du système : rien à télécharger, ni en local ni en CI (les
    // runners macOS de GitHub l'ont).
    channel: "chrome",
    headless: true,
    viewport: { width: 1440, height: 900 },
  },
});
