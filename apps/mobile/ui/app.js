// SPDX-License-Identifier: GPL-3.0-or-later
// Compagnon iOS — prototype : valises → albums → pistes, mini-lecteur.
'use strict';

const invoke = window.__TAURI__.core.invoke;
const $ = (id) => document.getElementById(id);

const LIBELLES_REGLES = { 'recents': 'Récents', 'peu-ecoutes': 'Peu écoutés' };

const etat = {
  ecran: 'valises',     // valises | albums | album
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
  if (!ms) return '—';
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

function entete(titre, retour) {
  $('titre').textContent = titre;
  $('retour').hidden = !retour;
  $('retour').onclick = retour || null;
}

/* ------------------------------------------------------------ valises */

async function afficherValises() {
  etat.ecran = 'valises';
  entete('Valises', null);
  const contenu = $('contenu');
  contenu.replaceChildren();
  const liste = await invoke('valises');
  if (!liste.length) {
    const p = el('p', 'vide');
    p.append('Aucune valise. Sur le Mac : ',
      el('code', null, 'rusty-music valise exporter --sortie …'),
      ', puis glisser le dossier dans les fichiers de l’app (Finder, iPhone branché).');
    contenu.append(p);
    return;
  }
  for (const v of liste) {
    const b = el('button', 'valise');
    b.append(el('span', null, v.nom), el('span', 'valise__meta', `${v.albums} albums`));
    b.onclick = () => ouvrirValise(v.nom);
    contenu.append(b);
  }
  if (liste.length === 1) ouvrirValise(liste[0].nom);
}

/* ------------------------------------------------------------- albums */

async function ouvrirValise(nom) {
  etat.albums = await invoke('ouvrir_valise', { nom });
  etat.pochettes.clear();
  etat.nomValise = nom;
  afficherAlbums();
}

function afficherAlbums() {
  etat.ecran = 'albums';
  entete(etat.nomValise, afficherValises);
  const contenu = $('contenu');
  contenu.replaceChildren();
  let regle = null;
  let grille = null;
  for (const a of etat.albums) {
    if (a.regle !== regle) {
      regle = a.regle;
      contenu.append(el('div', 'regle', LIBELLES_REGLES[regle] || regle));
      grille = el('div', 'grille');
      contenu.append(grille);
    }
    const b = el('button', 'album');
    b.append(imagePochette(a.id, 'album__pochette'),
      el('div', 'album__nom', a.album),
      el('div', 'album__artiste', a.artiste));
    b.onclick = () => afficherAlbum(a);
    grille.append(b);
  }
}

/* -------------------------------------------------------------- album */

async function afficherAlbum(a) {
  etat.ecran = 'album';
  etat.album = a;
  entete(a.artiste, afficherAlbums);
  const contenu = $('contenu');
  contenu.replaceChildren();
  const tete = el('div', 'entete-album');
  const textes = el('div');
  textes.append(el('div', 'entete-album__nom', a.album),
    el('div', 'entete-album__artiste', a.artiste),
    el('div', 'entete-album__annee', a.annee ? String(a.annee) : ''));
  const lire = el('button', 'lire');
  lire.append(icone('i-lecture'), el('span', null, 'Lire'));
  lire.onclick = () => invoke('jouer', { albumId: a.id, rang: 0 }).then(rafraichir);
  tete.append(imagePochette(a.id, 'entete-album__pochette'), textes, lire);
  contenu.append(tete);

  const pistes = await invoke('pistes', { albumId: a.id });
  pistes.forEach((p, rang) => {
    const b = el('button', 'piste');
    b.dataset.titre = p.titre;
    b.append(el('span', 'piste__no', String(rang + 1)),
      el('span', 'piste__titre', p.titre),
      el('span', 'piste__duree', duree(p.duree_ms)));
    b.onclick = () => invoke('jouer', { albumId: a.id, rang }).then(rafraichir);
    contenu.append(b);
  });
}

/* ------------------------------------------------------------ lecteur */

const lecteur = {
  derniere: null,      // dernier état lu
  albumPochette: null, // album dont la pochette est affichée
  glisse: false,       // le doigt est sur le curseur : ne pas le bouger
};

async function afficherPochetteLecteur(albumId) {
  if (lecteur.albumPochette === albumId) return;
  lecteur.albumPochette = albumId;
  const url = albumId ? await pochette(albumId) : null;
  for (const img of [$('m-pochette'), $('p-pochette')]) {
    if (url) img.src = url; else img.removeAttribute('src');
  }
  $('p-fond').style.backgroundImage = url ? `url("${url}")` : 'none';
}

async function rafraichir() {
  const l = await invoke('etat_lecture').catch(() => null);
  lecteur.derniere = l;
  $('mini').hidden = !l;
  if (!l) {
    fermerPlein();
    return;
  }
  afficherPochetteLecteur(l.album_id);
  const artisteAlbum = [l.artiste, l.album].filter(Boolean).join(' — ');
  $('m-titre').textContent = l.titre || '';
  $('m-artiste').textContent = artisteAlbum;
  $('p-titre').textContent = l.titre || '';
  $('p-artiste').textContent = artisteAlbum;
  const icon = l.pause ? 'i-lecture' : 'i-pause';
  changerIcone($('m-pause'), icon);
  changerIcone($('p-pause'), icon);
  $('plein').classList.toggle('plein--pause', l.pause);

  const part = l.duree_ms ? Math.min(1, l.position_ms / l.duree_ms) : 0;
  $('m-avance').style.width = `${(part * 100).toFixed(2)}%`;
  if (!lecteur.glisse) placerCurseur(part, l.position_ms, l.duree_ms);

  if (etat.ecran === 'album' && etat.album && etat.album.id === l.album_id) {
    for (const b of document.querySelectorAll('.piste')) {
      b.classList.toggle('piste--en-cours', b.dataset.titre === l.titre);
    }
  }
}

function placerCurseur(part, position_ms, duree_ms) {
  const c = $('p-curseur');
  c.value = Math.round(part * 1000);
  c.style.setProperty('--part', `${(part * 100).toFixed(2)}%`);
  $('p-ecoule').textContent = duree(position_ms) === '—' ? '0:00' : duree(position_ms);
  $('p-reste').textContent = duree_ms ? `−${duree(Math.max(0, duree_ms - position_ms))}` : '';
}

function ouvrirPlein() {
  $('plein').classList.add('plein--ouvert');
  $('plein').setAttribute('aria-hidden', 'false');
}
function fermerPlein() {
  $('plein').classList.remove('plein--ouvert');
  $('plein').setAttribute('aria-hidden', 'true');
}

const pause = () => invoke('basculer_pause').then(rafraichir);
const suivante = () => invoke('suivante').then(rafraichir);
const precedente = () => invoke('precedente').then(rafraichir);

$('ouvrir-plein').onclick = ouvrirPlein;
$('fermer-plein').onclick = fermerPlein;
$('m-pause').onclick = pause;
$('p-pause').onclick = pause;
$('m-suivante').onclick = suivante;
$('p-suivante').onclick = suivante;
$('p-precedente').onclick = precedente;

// Curseur : suit le doigt, et ne cherche dans la piste qu'au relâcher.
const curseur = $('p-curseur');
curseur.addEventListener('input', () => {
  lecteur.glisse = true;
  const l = lecteur.derniere;
  const part = curseur.value / 1000;
  const d = l && l.duree_ms;
  placerCurseur(part, d ? part * d : 0, d);
});
curseur.addEventListener('change', () => {
  const l = lecteur.derniere;
  lecteur.glisse = false;
  if (!l || !l.duree_ms) return;
  invoke('aller_a', { positionMs: Math.round((curseur.value / 1000) * l.duree_ms) }).then(rafraichir);
});

// Glisser la feuille vers le bas la referme.
let departY = null;
$('plein').addEventListener('touchstart', (e) => {
  departY = e.target === curseur ? null : e.touches[0].clientY;
}, { passive: true });
$('plein').addEventListener('touchend', (e) => {
  if (departY != null && e.changedTouches[0].clientY - departY > 90) fermerPlein();
  departY = null;
});

setInterval(rafraichir, 500);
afficherValises();
