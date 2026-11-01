// Les noms dans toutes les écritures (`docs/interface-guidelines.md`, Règle 11).
// Titres, artistes et albums viennent des fichiers de l'utilisateur, dans
// n'importe quelle écriture ; l'interface est en français. Deux choses que la
// langue de l'interface ne décide pas :
//
// - **La direction.** Chaque élément en ligne est son propre isolat bidi
//   (`style.css`) ; un nom joint à d'autres textes dans une même chaîne
//   (« Artiste — Album », « فيروز · 1991 ») passe par `isoler`, qui l'entoure
//   d'isolats U+2068 … U+2069 s'il porte une lettre de droite à gauche — sans
//   quoi l'année s'afficherait avant le nom.
// - **Les formes des idéogrammes.** Chinois, japonais et coréen partagent des
//   idéogrammes dessinés différemment selon la langue, et le document est en
//   `fr`. `observerEcritures` donne `lang="ja"` à un élément dont le texte
//   propre a des kana, `lang="ko"` à un élément qui a du hangul ; des
//   idéogrammes seuls gardent la langue du document (rien ne distingue alors
//   le chinois du japonais). Il ne touche jamais un `lang` posé par le gabarit.
//
// Méthode reprise de Disc Player (https://github.com/eudj1n/snowsky-disc-player,
// `docs/typography.md`, `src/lib/scriptLanguages.ts`, licence MIT), réécrite ici.
// Hors de la chaîne d'exécution (voir `main.js`) : `socle.js` lance l'observateur.

const DROITE_A_GAUCHE = /[֐-ࣿיִ-﷿ﹰ-﻿]/;
const KANA = /[぀-ヿㇰ-ㇿｦ-ﾟ]/;
const HANGUL = /[ᄀ-ᇿ㄰-㆏가-힯]/;

/// `nom`, isolé pour le bidi s'il porte une écriture de droite à gauche ;
/// tel quel sinon (le texte ne change pas pour les autres écritures).
function isoler(nom) {
  const s = String(nom ?? "");
  return DROITE_A_GAUCHE.test(s) ? `⁨${s}⁩` : s;
}

/// La langue que trahit `texte` : « ja » pour des kana, « ko » pour du hangul,
/// `null` sinon.
function langueDe(texte) {
  if (KANA.test(texte)) return "ja";
  if (HANGUL.test(texte)) return "ko";
  return null;
}

/// Le texte propre d'un élément : ses nœuds texte directs, pas ceux de ses
/// enfants (qui reçoivent leur propre `lang`).
function textePropre(el) {
  let t = "";
  for (const n of el.childNodes) if (n.nodeType === Node.TEXT_NODE) t += n.data;
  return t;
}

/// Pose ou retire le `lang` automatique d'un élément. Un élément réutilisé
/// (le titre du transport, une case de la grille) change de texte : le `lang`
/// posé ici suit, celui du gabarit n'est jamais touché.
function marquer(el) {
  if (el.hasAttribute("lang") && !el.hasAttribute("data-lang-auto")) return;
  const langue = langueDe(textePropre(el));
  if (langue) {
    if (el.getAttribute("lang") !== langue) {
      el.setAttribute("lang", langue);
      el.setAttribute("data-lang-auto", "");
    }
  } else if (el.hasAttribute("data-lang-auto")) {
    el.removeAttribute("lang");
    el.removeAttribute("data-lang-auto");
  }
}

function marquerSousArbre(racine) {
  if (racine.nodeType === Node.TEXT_NODE) {
    if (racine.parentElement) marquer(racine.parentElement);
    return;
  }
  if (racine.nodeType !== Node.ELEMENT_NODE) return;
  marquer(racine);
  for (const el of racine.querySelectorAll("*")) marquer(el);
}

/// Surveille le document : tout texte ajouté ou changé reçoit sa langue.
function observerEcritures(racine = document.body) {
  marquerSousArbre(racine);
  new MutationObserver((mutations) => {
    for (const m of mutations) {
      if (m.type === "characterData") {
        if (m.target.parentElement) marquer(m.target.parentElement);
      } else {
        if (m.target.nodeType === Node.ELEMENT_NODE) marquer(m.target);
        m.addedNodes.forEach(marquerSousArbre);
      }
    }
  }).observe(racine, { childList: true, characterData: true, subtree: true });
}

export {
  isoler, langueDe, observerEcritures,
};
