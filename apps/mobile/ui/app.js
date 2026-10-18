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
  tete.append(imagePochette(a.id, 'entete-album__pochette'), textes);
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

async function rafraichir() {
  const l = await invoke('etat_lecture').catch(() => null);
  $('lecteur').hidden = !l;
  if (!l) return;
  $('l-titre').textContent = l.titre || '';
  $('l-artiste').textContent = [l.artiste, l.album].filter(Boolean).join(' — ');
  $('pause').textContent = l.pause ? '▶' : '⏸';
  const part = l.duree_ms ? Math.min(1, l.position_ms / l.duree_ms) : 0;
  $('avance').style.width = `${(part * 100).toFixed(2)}%`;
  if (etat.ecran === 'album' && etat.album && etat.album.id === l.album_id) {
    for (const b of document.querySelectorAll('.piste')) {
      b.classList.toggle('piste--en-cours', b.dataset.titre === l.titre);
    }
  }
}

$('pause').onclick = () => invoke('basculer_pause').then(rafraichir);
$('suivante').onclick = () => invoke('suivante').then(rafraichir);
$('precedente').onclick = () => invoke('precedente').then(rafraichir);

setInterval(rafraichir, 500);
afficherValises();
