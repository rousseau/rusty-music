// SPDX-License-Identifier: GPL-3.0-or-later
// Compagnon iOS : valises → albums → pistes, et le lecteur en bas.
// Au plus simple : un toucher sur ▶ d'une pochette lance l'album.
'use strict';

const invoke = window.__TAURI__.core.invoke;
const $ = (id) => document.getElementById(id);

const LIBELLES_REGLES = { 'recents': 'Récents', 'peu-ecoutes': 'Peu écoutés' };

const etat = {
  ecran: 'valises',     // valises | albums | album | file
  nbValises: 0,
  albums: [],
  album: null,          // album affiché dans l'écran « album »
  pochettes: new Map(), // id d'album → URL data: (ou null)
};

function el(balise, classe, texte) {
  const n = document.createElement(balise);
  if (classe) n.className = classe;
  if (texte != null) n.textContent = texte;
  return n;
}

function duree(ms) {
  if (ms == null) return '—';
  const s = Math.round(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

async function pochette(id) {
  if (!etat.pochettes.has(id)) {
    etat.pochettes.set(id, await invoke('pochette', { albumId: id }).catch(() => null));
  }
  return etat.pochettes.get(id);
}

function imagePochette(id, classe) {
  const img = el('img', classe);
  img.alt = '';
  pochette(id).then((url) => { if (url) img.src = url; });
  return img;
}

function icone(id, classe) {
  const ns = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(ns, 'svg');
  svg.setAttribute('class', `icone ${classe || ''}`);
  const use = document.createElementNS(ns, 'use');
  use.setAttribute('href', `#${id}`);
  svg.append(use);
  return svg;
}

/** Remplace l'icône d'un bouton (lecture ↔ pause). */
function changerIcone(bouton, id) {
  const use = bouton.querySelector('use');
  if (use.getAttribute('href') !== `#${id}`) use.setAttribute('href', `#${id}`);
}

/** Le bouton retour flottant : affiché seulement s'il mène quelque part. */
function retour(action) {
  $('retour').hidden = !action;
  $('retour').onclick = action || null;
}

const jouer = (albumId, rang) => invoke('jouer', { albumId, rang }).then(rafraichir);
const aLaSuite = (albumId) => invoke('ajouter_album', { albumId }).then(rafraichir);

/* ------------------------------------------------------------ valises */

async function afficherValises() {
  etat.ecran = 'valises';
  retour(null);
  const contenu = $('contenu');
  contenu.replaceChildren();
  const liste = await invoke('valises');
  etat.nbValises = liste.length;
  if (!liste.length) {
    const p = el('p', 'vide');
    p.append('Aucune valise. Sur le Mac : ',
      el('code', null, 'rusty-music valise exporter --sortie …'),
      ', puis glisser le dossier dans les fichiers de l’app (Finder, iPhone branché).');
    contenu.append(p);
    return;
  }
  // Une seule valise : on l'ouvre directement, sans écran intermédiaire.
  if (liste.length === 1) {
    ouvrirValise(liste[0].nom);
    return;
  }
  for (const v of liste) {
    const b = el('button', 'valise');
    b.append(el('span', null, v.nom), el('span', 'valise__meta', `${v.albums} albums`));
    b.onclick = () => ouvrirValise(v.nom);
    contenu.append(b);
  }
}

/* ------------------------------------------------------------- albums */

async function ouvrirValise(nom) {
  etat.albums = await invoke('ouvrir_valise', { nom });
  etat.pochettes.clear();
  afficherAlbums();
}

function afficherAlbums() {
  etat.ecran = 'albums';
  retour(etat.nbValises > 1 ? afficherValises : null);
  const contenu = $('contenu');
  contenu.replaceChildren();
  let regle = null;
  let grille = null;
  for (const a of etat.albums) {
    if (a.regle !== regle) {
      regle = a.regle;
      contenu.append(el('h2', 'regle', LIBELLES_REGLES[regle] || regle));
      grille = el('div', 'grille');
      contenu.append(grille);
    }
    const carte = el('div', 'album');
    const image = el('div', 'album__image');
    const ouvrir = el('button', 'album__ouvrir');
    ouvrir.setAttribute('aria-label', `${a.album}, ${a.artiste}`);
    ouvrir.append(imagePochette(a.id, 'album__pochette'));
    ouvrir.onclick = () => afficherAlbum(a);
    const lire = el('button', 'album__lire');
    lire.setAttribute('aria-label', `Lire ${a.album}`);
    lire.append(icone('i-lecture'));
    lire.onclick = () => jouer(a.id, 0);
    image.append(ouvrir, lire);
    carte.append(image, el('div', 'album__nom', a.album), el('div', 'album__artiste', a.artiste));
    grille.append(carte);
  }
  window.scrollTo(0, etat.defilement || 0);
}

/* -------------------------------------------------------------- album */

async function afficherAlbum(a) {
  if (etat.ecran === 'albums') etat.defilement = window.scrollY;
  etat.ecran = 'album';
  etat.album = a;
  retour(afficherAlbums);
  const contenu = $('contenu');
  contenu.replaceChildren();
  window.scrollTo(0, 0);
  const tete = el('div', 'entete-album');
  const textes = el('div');
  textes.append(el('div', 'entete-album__nom', a.album),
    el('div', 'entete-album__artiste', a.artiste),
    el('div', 'entete-album__annee', a.annee ? String(a.annee) : ''));
  const lire = el('button', 'lire');
  lire.append(icone('i-lecture'), el('span', null, 'Lire'));
  lire.onclick = () => jouer(a.id, 0);
  const suite = el('button', 'lire lire--second');
  suite.append(icone('i-a-la-suite'), el('span', null, 'À la suite'));
  suite.onclick = () => aLaSuite(a.id).then(() => {
    suite.lastChild.textContent = 'Ajouté';
    setTimeout(() => { suite.lastChild.textContent = 'À la suite'; }, 1500);
  });
  const actions = el('div', 'entete-album__actions');
  actions.append(lire, suite);
  tete.append(imagePochette(a.id, 'entete-album__pochette'), textes, actions);
  contenu.append(tete);

  const pistes = await invoke('pistes', { albumId: a.id });
  pistes.forEach((p, rang) => {
    const b = el('button', 'piste');
    b.dataset.titre = p.titre;
    b.append(el('span', 'piste__no', String(rang + 1)),
      el('span', 'piste__titre', p.titre),
      el('span', 'piste__duree', duree(p.duree_ms)));
    b.onclick = () => jouer(a.id, rang);
    contenu.append(b);
  });
  rafraichir();
}

/* --------------------------------------------------------------- file */

// L'écran d'où l'on est venu, pour que « retour » y ramène.
let avantFile = null;

function ouvrirFile() {
  if (etat.ecran === 'file') return;
  if (etat.ecran === 'albums') etat.defilement = window.scrollY;
  const ecran = etat.ecran;
  const album = etat.album;
  avantFile = () => {
    if (ecran === 'album' && album) afficherAlbum(album);
    else if (ecran === 'albums') afficherAlbums();
    else afficherValises();
  };
  etat.ecran = 'file';
  retour(avantFile);
  afficherFile(true);
}

async function afficherFile(centrer) {
  const f = await invoke('file_lecture');
  if (etat.ecran !== 'file') return;
  lecteur.rangFile = f.rang;
  const contenu = $('contenu');
  contenu.replaceChildren(el('h2', 'regle regle--sous-retour', 'File'));
  if (!f.pistes.length) {
    contenu.append(el('p', 'vide', 'La file est vide.'));
    return;
  }
  let enCours = null;
  f.pistes.forEach((p, rang) => {
    const ligne = el('div', 'piste-file');
    if (f.rang != null && rang < f.rang) ligne.classList.add('piste-file--passee');
    const b = el('button', 'piste');
    if (rang === f.rang) { b.classList.add('piste--en-cours'); enCours = ligne; }
    const textes = el('span', 'piste__textes');
    textes.append(el('span', 'piste__titre', p.titre), el('span', 'piste__artiste', p.artiste || ''));
    b.append(textes, el('span', 'piste__duree', duree(p.duree_ms)));
    b.onclick = () => invoke('sauter_a', { rang }).then(rafraichir);
    ligne.append(b);
    if (rang >= f.retirable) {
      const x = el('button', 'piste-file__retirer');
      x.setAttribute('aria-label', `Retirer ${p.titre}`);
      x.append(icone('i-retirer'));
      x.onclick = () => invoke('retirer_de_la_file', { rang }).then(() => afficherFile(false));
      ligne.append(x);
    }
    contenu.append(ligne);
  });
  if (centrer && enCours) enCours.scrollIntoView({ block: 'center' });
}

/* ------------------------------------------------------------ lecteur */

const lecteur = {
  derniere: null,      // dernier état lu
  albumPochette: null, // album dont la pochette est affichée
  glisse: false,       // le doigt est sur le curseur : ne pas le bouger
  rangFile: null,      // rang en cours au dernier affichage de la file
};

async function afficherPochetteLecteur(albumId) {
  if (lecteur.albumPochette === albumId) return;
  lecteur.albumPochette = albumId;
  const url = albumId ? await pochette(albumId) : null;
  if (url) $('l-pochette').src = url; else $('l-pochette').removeAttribute('src');
}

async function rafraichir() {
  const l = await invoke('etat_lecture').catch(() => null);
  lecteur.derniere = l;
  $('lecteur').hidden = !l;
  if (!l) return;
  afficherPochetteLecteur(l.album_id);
  $('l-titre').textContent = l.titre || '';
  $('l-artiste').textContent = [l.artiste, l.album].filter(Boolean).join(' — ');
  changerIcone($('pause'), l.pause ? 'i-lecture' : 'i-pause');
  const part = l.duree_ms ? Math.min(1, l.position_ms / l.duree_ms) : 0;
  if (!lecteur.glisse) placerCurseur(part, l.position_ms, l.duree_ms);

  if (etat.ecran === 'file' && l.rang !== lecteur.rangFile) afficherFile(false);

  if (etat.ecran === 'album' && etat.album && etat.album.id === l.album_id) {
    for (const b of document.querySelectorAll('.piste')) {
      b.classList.toggle('piste--en-cours', b.dataset.titre === l.titre);
    }
  }
}

function placerCurseur(part, position_ms, duree_ms) {
  const c = $('l-curseur');
  c.value = Math.round(part * 1000);
  c.style.setProperty('--part', `${(part * 100).toFixed(2)}%`);
  $('l-ecoule').textContent = duree(position_ms || 0);
  $('l-reste').textContent = duree_ms ? `−${duree(Math.max(0, duree_ms - position_ms))}` : '';
}

$('file').onclick = ouvrirFile;
$('pause').onclick = () => invoke('basculer_pause').then(rafraichir);
$('suivante').onclick = () => invoke('suivante').then(rafraichir);
$('precedente').onclick = () => invoke('precedente').then(rafraichir);

// Toucher la piste en cours mène à son album.
$('l-piste').onclick = () => {
  const l = lecteur.derniere;
  const a = l && etat.albums.find((x) => x.id === l.album_id);
  if (a) afficherAlbum(a);
};

// Curseur : suit le doigt, et ne cherche dans la piste qu'au relâcher.
const curseur = $('l-curseur');
curseur.addEventListener('input', () => {
  lecteur.glisse = true;
  const d = lecteur.derniere && lecteur.derniere.duree_ms;
  const part = curseur.value / 1000;
  placerCurseur(part, d ? part * d : 0, d);
});
curseur.addEventListener('change', () => {
  const l = lecteur.derniere;
  lecteur.glisse = false;
  if (!l || !l.duree_ms) return;
  invoke('aller_a', { positionMs: Math.round((curseur.value / 1000) * l.duree_ms) }).then(rafraichir);
});

/** Au lancement : la valise de la dernière session, sa file reprise en
 *  pause ; sinon la liste des valises. */
async function demarrer() {
  const r = await invoke('reprendre').catch(() => null);
  if (!r) {
    afficherValises();
    return;
  }
  etat.nbValises = (await invoke('valises')).length;
  etat.albums = r.albums;
  afficherAlbums();
  rafraichir();
}

setInterval(rafraichir, 500);
demarrer();
