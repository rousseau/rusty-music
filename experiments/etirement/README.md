# Banc d'écoute de l'étirement (E3)

wsola (ce que fait l'application) contre **Signalsmith Stretch** (MIT) et
**Rubber Band R3** (GPL), au ralenti (0,5 · 0,7 · 0,8×) et en transposition
(±2 demi-tons). Rien n'entre dans l'application avant l'écoute de François.

- wsola : `crates/editor/src/etirement.rs` est **inclus tel quel**
  (`#[path]`) — on mesure le code de l'application, transposition comprise
  (étirer puis rééchantillonner en Catmull-Rom).
- Signalsmith Stretch : `signalsmith-stretch` 0.1.3, préréglage par défaut,
  `exact()` pour les rendus, `process()` en flux.
- Rubber Band : `rubberband` 0.1.5 / `rubberband-sys` 0.1.4 (sources de
  Rubber Band 4.0 embarquées, compilées par `cc`, Accelerate sur macOS ; rien
  à installer). Moteur R3 (`ENGINE_FINER`), hors ligne pour les rendus, temps
  réel (`PITCH_HIGH_CONSISTENCY`, sans fil) pour le coût par bloc.

```bash
cargo run --release -- mesurer "<stem>.wav"   # coûts, contrôle de hauteur ; n'écrit rien
./rendre-ecoute.sh                           # 90 fichiers sur RUSTY_DEV (≈ 450 Mo)
```

## Mesures (9 nov. 2026, MacBook Pro M-series, machine chargée : Firefox, WindowServer)

### Pire salve par bloc de 128 trames, quatre stems

Méthode de `crates/player/examples/cout_bloc.rs` (bruit blanc, une salve =
pousser des blocs jusqu'à ce que l'étireur rende quelque chose), mais sur
**cinq passes de 2 000 salves** : la pire salve retenue est le plus petit des
cinq maxima. Une préemption du système tombe dans une passe, pas dans toutes ;
un pic propre à l'étireur revient à chaque passe. Une passe unique a donné
jusqu'à 327 ms pour wsola et 69 ms pour Rubber Band — du bruit de
l'ordonnanceur, pas des étireurs. Échéance : 11,6 ms (tampon de 512 trames).

| méthode | ×0,25 | ×0,5 | ×0,7 | ×0,8 | +2 demi-tons | −2 demi-tons |
|---|---|---|---|---|---|---|
| wsola | 5,8 ms | 5,7 ms | 5,7-5,9 ms | 5,7-5,9 ms | — (pas en flux) | — |
| Signalsmith | 1,0 ms | 1,0 ms | 1,0 ms | 0,9-1,0 ms | 1,0 ms | 1,0 ms |
| Rubber Band R3 | 1,6-1,7 ms | 1,5-1,6 ms | 1,4 ms | 1,4 ms | 1,5-1,6 ms | 1,5-1,6 ms |

Deux lancements complets ; les fourchettes sont leur écart. wsola retrouve les
6,1 ms du journal (19 août). Médianes : wsola 4,7 ms, Rubber Band 1,0-1,2 ms,
Signalsmith 0,003-0,007 ms — Signalsmith fait tout son travail dans un appel
sur plusieurs dizaines, d'où une médiane quasi nulle et une pire salve à 1 ms.

**Les deux vocodeurs transposent en flux**, à coût égal à celui du ralenti :
la transposition « instantanée » (sans fil ni cache) leur est accessible,
pas à wsola.

### Transposition d'un stem de 184 s, un fil

Basse d'*About Damn Time* (191 s, coupée à 184 s pour la comparer au journal).

| méthode | +2 demi-tons | −2 demi-tons |
|---|---|---|
| wsola + Catmull-Rom (aujourd'hui) | 16,8-17,5 s | 13,1-13,2 s |
| Signalsmith | **1,0 s** | **1,0 s** |
| Rubber Band R3 (hors ligne) | 8,5-8,7 s | 7,2-7,3 s |

### Contrôle de hauteur (sinusoïde de 441 Hz, 6 s)

Durées justes à la trame près pour Signalsmith et Rubber Band (wsola : ±20 ms).
Hauteur juste partout, sauf **Signalsmith en transposition : 495,9 Hz pour
495,0 attendus à +2 (+3 cents), 393,1 pour 392,9 à −2 (+1 cent)**. Rubber Band
et wsola tombent juste au dixième de hertz.

## Écoute à l'aveugle

`rendre-ecoute.sh` exige RUSTY_DEV (le LaCie) : 6 extraits × 5 réglages ×
3 méthodes. Extraits : batterie seule et basse seule (stems démixés de
*Black Crow* et *Love Foolosophy*), pop dense (*About Damn Time*), piano
(Mozart, sonate K. 331), voix (stem de *Donne Moi Ça*), jazz (Bill Evans
trio, *Our Love Is Here to Stay*). La fenêtre de 20 s est choisie entre 15 et
85 % du morceau, la plus pleine (plus haut minimum d'énergie sur des tranches
de 2 s).

- `ecoute/<n>-<extrait>/0-original.wav` puis `<réglage>-A|B|C.wav` ; l'ordre
  des méthodes est tiré par fichier (déterministe).
- La clé (`cle/cle.tsv`) et les sources (`cle/sources.tsv`) sont **hors** du
  dossier d'écoute.
- Mêmes longueurs au octet près pour A, B, C (sinon la taille trahirait la
  méthode), fondus de 20 ms.
- **Un seul niveau par extrait**, original compris : le plus haut niveau
  efficace que tous les fichiers tiennent sous une crête de 0,98. Réglés un à
  un sur l'original, les vocodeurs sortaient jusqu'à 2 dB plus bas que wsola
  (ils montent le facteur de crête) — et plus fort paraît meilleur. Vérifié sur
  la batterie : 16 fichiers entre −20,07 et −19,94 dBFS.
