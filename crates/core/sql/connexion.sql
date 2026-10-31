-- Réglages de connexion, joués à chaque ouverture, hors migration : ils
-- valent pour la connexion et non pour la base (sauf `journal_mode`), et
-- `journal_mode`/`foreign_keys` sont sans effet dans une transaction.

PRAGMA journal_mode = WAL;
-- Plusieurs processus partagent la base : l'application, la CLI, une passe
-- d'analyse qui écrit pendant des heures. Sans délai d'attente, la moindre
-- contention rend un échec immédiat au lieu de patienter quelques
-- millisecondes. WAL autorise un rédacteur et des lecteurs simultanés ; ce
-- délai couvre le cas où deux rédacteurs se croisent.
PRAGMA busy_timeout = 5000;
PRAGMA foreign_keys = ON;

