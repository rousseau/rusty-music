// SPDX-License-Identifier: GPL-3.0-or-later
//! Deux passes bien séparées dans le temps pour les crédits Discogs.
//!
//! **a) Liaison** (`lier`) — légère : pour chaque édition MusicBrainz déjà
//! connue (`tracks.mb_release_id`), retrouve son édition Discogs par la
//! relation d'URL MusicBrainz ([`crate::musicbrainz::Client::discogs_release_id`]),
//! jamais par recherche de nom. Peut tourner à chaque enrichissement, comme
//! [`crate::popularite`].
//!
//! **b) Import** (`importer`) — lourd, mensuel, séparé du scan normal :
//! télécharge le dump `releases.xml.gz` (CC0) et en extrait, dans la même
//! passe, les crédits et les labels des éditions déjà reliées. Voir
//! [`crate::discogs`] pour le détail du téléchargement et de la lecture en
//! flux.

use std::path::Path;

use crate::db::{CreditDiscogs, LabelDiscogs, Library};
use crate::discogs;
use crate::error::Result;
use crate::musicbrainz;

/// Ce qu'une passe de liaison a produit.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BilanLiaison {
    pub interrogees: usize,
    pub liees: usize,
    pub faits: usize,
    pub total: usize,
}

/// Relie au plus `limite` éditions MusicBrainz à leur édition Discogs.
pub fn lier(
    lib: &mut Library,
    client: &musicbrainz::Client,
    limite: usize,
    mut avancer: impl FnMut(&BilanLiaison),
) -> Result<BilanLiaison> {
    let mut bilan = BilanLiaison::default();
    let a_faire = lib.discogs_liaison_candidats(limite)?;
    bilan.total = a_faire.len();
    avancer(&bilan);

    for mb_release_id in a_faire {
        match client.discogs_release_id(&mb_release_id) {
            Ok(id) => {
                if id.is_some() {
                    bilan.liees += 1;
                }
                lib.discogs_lier_edition(&mb_release_id, id.map(|v| v as i64))?;
            }
            Err(e) => {
                // Pas rangée : l'édition reviendra au prochain passage.
                tracing::warn!(erreur = %e, %mb_release_id, "liaison Discogs non vérifiée");
                bilan.interrogees += 1;
                bilan.faits += 1;
                avancer(&bilan);
                continue;
            }
        }
        bilan.interrogees += 1;
        bilan.faits += 1;
        avancer(&bilan);
    }
    Ok(bilan)
}

/// Télécharge le dernier dump Discogs dans `dest` — séparé de [`importer`]
/// pour qu'un import puisse réutiliser un fichier déjà là (`--fichier`,
/// tests, reprise après coupure). `avancer(octets_vus, octets_total)` rapporte
/// la progression ; `octets_total` est `None` tant que le serveur n'a pas
/// répondu (ou n'a pas annoncé de taille).
pub fn telecharger_dernier_dump(dest: &Path, avancer: impl FnMut(u64, Option<u64>)) -> Result<()> {
    let agent = discogs::agent();
    let annee = annee_courante();
    let url = discogs::derniere_url_dump(&agent, annee)?;
    tracing::info!(%url, "téléchargement du dump Discogs");
    discogs::telecharger_avec_avancement(&agent, &url, dest, avancer)
}

/// Année civile courante (UTC), sans dépendance de calendrier — juste de quoi
/// choisir le bon dossier `data/{annee}/` du listing Discogs.
fn annee_courante() -> i32 {
    let mut jours = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    let mut annee = 1970i32;
    loop {
        let jours_annee = if bissextile(annee) { 366 } else { 365 };
        if jours < jours_annee {
            return annee;
        }
        jours -= jours_annee;
        annee += 1;
    }
}

fn bissextile(annee: i32) -> bool {
    (annee % 4 == 0 && annee % 100 != 0) || annee % 400 == 0
}

/// Importe les crédits et les labels des éditions déjà reliées (voir
/// [`lier`]) depuis un dump `releases.xml.gz` déjà présent sur le disque en
/// `chemin`.
///
/// N'écrit rien pour une édition reliée mais absente du dump (bibliothèque
/// vaste, dump réduit à ce qu'on cherche) : ses crédits et labels, s'il y en
/// avait, restent ceux du dernier import réussi — et elle n'est pas marquée
/// dans `discogs_importes`, donc redevient candidate tant qu'un import ne
/// l'a pas réellement rencontrée dans le dump.
pub fn importer(
    lib: &mut Library,
    chemin: &Path,
    avancer: impl FnMut(&discogs::BilanImport),
) -> Result<discogs::BilanImport> {
    let voulus = lib.discogs_wanted_ids()?;
    discogs::pour_chaque_release(
        chemin,
        &voulus,
        |edition| {
            let credits: Vec<CreditDiscogs> = edition
                .credits
                .into_iter()
                .map(|c| CreditDiscogs {
                    personne: c.personne,
                    role: c.role,
                    pistes: c.pistes,
                    discogs_artist_id: c.discogs_artist_id,
                })
                .collect();
            if let Err(e) = lib.credits_poser(edition.id, &credits) {
                tracing::warn!(erreur = %e, id = edition.id, "crédits Discogs non rangés");
            }
            let labels: Vec<LabelDiscogs> = edition
                .labels
                .into_iter()
                .map(|l| LabelDiscogs {
                    nom: l.nom,
                    catno: l.catno,
                    discogs_label_id: l.discogs_label_id,
                })
                .collect();
            if let Err(e) = lib.labels_poser(edition.id, &labels) {
                tracing::warn!(erreur = %e, id = edition.id, "labels Discogs non rangés");
            }
            // Après credits_poser/labels_poser, même sans rien à ranger —
            // voir `discogs_importes` et `Library::discogs_import_utile`.
            if let Err(e) = lib.discogs_marquer_importe(edition.id) {
                tracing::warn!(erreur = %e, id = edition.id, "édition Discogs non marquée importée");
            }
        },
        avancer,
    )
}
