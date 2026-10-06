// SPDX-License-Identifier: GPL-3.0-or-later
//! Vérification des pochettes, à chaque analyse.
//!
//! Une pochette fausse ne se voit pas dans la base : l'image vit dans les
//! tags des fichiers ou à côté d'eux, et l'application affiche la première
//! trouvée (`tags::read_cover` : intégrée d'abord, puis `cover.jpg`). Trois
//! défauts réels ont motivé cette passe (5 oct. 2026) — une image intégrée
//! sans rapport avec l'album (« Inner City — Music God » sur 18 pistes de
//! Meat Beat Manifesto), un verso à la place de la face avant, un
//! `cover.jpg` tronqué à 6 Mo — et trois signaux les attrapent sans réseau :
//!
//! 1. **illisible** — l'image intégrée ou celle du dossier ne se décode pas,
//!    ou est tronquée (JPEG sans marqueur de fin) ;
//! 2. **divergente** — l'image intégrée et celle du dossier ne représentent
//!    pas la même chose (comparaison de vignettes 16×16, insensible à la
//!    taille et à la compression). Un signal à examiner, pas une certitude :
//!    deux éditions du même album diffèrent aussi ;
//! 3. **partagée** — la même image intégrée sur des albums d'artistes
//!    différents : c'est la signature d'un mauvais étiquetage en lot.
//!
//! Ce qui n'est **pas** détecté : une image fausse sans aucune référence
//! locale (pas de `cover.jpg`, image intégrée propre à un seul album).
//!
//! La passe est **incrémentale** : un dossier n'est rouvert que si la taille
//! ou la date d'une piste ou d'une image a changé ([`signature_dossier`]) —
//! la première passe lit les tags de tout le monde, les suivantes presque
//! rien. Elle ne modifie jamais un fichier de l'utilisateur : elle signale.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use sha2::{Digest, Sha256};
use tracing::warn;

use crate::db::{Library, PisteVerifPochette, VerifPochette};
use crate::error::Result;
use crate::tags;

/// À incrémenter quand la méthode de vérification change : les dossiers déjà
/// vérifiés avec une version antérieure sont alors revus.
pub const VERSION_VERIF: i32 = 1;

/// Seuil de divergence entre deux vignettes (`1 − corrélation`). Mesuré sur la
/// bibliothèque de test : 114 dossiers dont l'image intégrée est la même que
/// celle du dossier restaient sous 0,15 (rééchantillonnage, recompression) ;
/// les écarts réels (autre édition, verso, autre album) valent 0,38 et plus.
pub const SEUIL_DIVERGENCE: f64 = 0.15;

/// Combien de pistes d'un dossier essayer avant de conclure « pas d'image
/// intégrée » — la première n'en porte pas toujours (voir `PISTES_SOEURS_MAX`
/// dans l'application).
const PISTES_ECHANTILLON: usize = 3;

/// Côté de la vignette de comparaison.
const COTE: u32 = 16;

/// Fenêtre, en fin de fichier, où doit se trouver le marqueur de fin d'un
/// JPEG. Large pour tolérer un peu de remplissage après `FFD9`, assez étroite
/// pour ne pas confondre la fin d'une vignette EXIF (au début du fichier) avec
/// celle de l'image.
const FENETRE_FIN_JPEG: usize = 8192;

pub const ILLISIBLE: &str = "illisible";
pub const DIVERGENTE: &str = "divergente";
pub const PARTAGEE: &str = "partagee";

#[derive(Debug, Default, Clone, Copy)]
pub struct Bilan {
    /// Dossiers d'albums de la bibliothèque.
    pub dossiers: usize,
    /// Dossiers (re)vérifiés par cette passe.
    pub verifies: usize,
    /// Pochettes à examiner après la passe (signalements non ignorés).
    pub suspectes: usize,
}

/// Un signalement, tel que l'interface l'affiche.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PochetteSuspecte {
    pub dossier: String,
    pub artiste: String,
    pub album: String,
    /// [`ILLISIBLE`], [`DIVERGENTE`] ou [`PARTAGEE`].
    pub anomalie: String,
    pub detail: String,
    pub mesure: Option<f64>,
}

/* ------------------------------------------------------------ images */

/// Vignette 16×16 en niveaux de gris, ou la raison pour laquelle l'image est
/// inutilisable.
fn vignette(data: &[u8]) -> std::result::Result<Vec<f32>, String> {
    verifier_fin_jpeg(data)?;
    let img = image::load_from_memory(data).map_err(|e| format!("décodage impossible ({e})"))?;
    let petite = img
        .resize_exact(COTE, COTE, image::imageops::FilterType::Triangle)
        .to_luma8();
    Ok(petite.pixels().map(|p| f32::from(p.0[0])).collect())
}

/// Un JPEG coupé en route se décode « pour moitié » selon les bibliothèques ;
/// le marqueur de fin `FFD9` manquant est le signe sûr. Les autres formats
/// échouent d'eux-mêmes au décodage.
fn verifier_fin_jpeg(data: &[u8]) -> std::result::Result<(), String> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return Ok(());
    }
    let debut = data.len().saturating_sub(FENETRE_FIN_JPEG);
    if data[debut..].windows(2).any(|w| w == [0xFF, 0xD9]) {
        Ok(())
    } else {
        Err(format!("JPEG tronqué ({} octets, pas de marqueur de fin)", data.len()))
    }
}

/// `1 − corrélation de Pearson` entre deux vignettes : 0 = même image (à la
/// luminosité et au contraste près), ~1 = sans rapport, jusqu'à 2 = négatif.
fn distance(a: &[f32], b: &[f32]) -> f64 {
    let n = a.len().min(b.len()) as f64;
    if n == 0.0 {
        return 0.0;
    }
    let ma = a.iter().map(|&x| f64::from(x)).sum::<f64>() / n;
    let mb = b.iter().map(|&x| f64::from(x)).sum::<f64>() / n;
    let (mut cov, mut va, mut vb) = (0.0, 0.0, 0.0);
    for (&x, &y) in a.iter().zip(b) {
        let (dx, dy) = (f64::from(x) - ma, f64::from(y) - mb);
        cov += dx * dy;
        va += dx * dx;
        vb += dy * dy;
    }
    // Une image unie n'a pas de corrélation définie : deux unies sont
    // identiques, une unie face à une image qui a du contenu ne l'est pas.
    if va == 0.0 || vb == 0.0 {
        return if va == vb { 0.0 } else { 1.0 };
    }
    1.0 - cov / (va * vb).sqrt()
}

fn hacher(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|o| format!("{o:02x}")).collect()
}

/* --------------------------------------------------- un dossier d'album */

/// Résumé des fichiers d'un dossier : tant qu'il ne change pas, rien à
/// revérifier. Les images de dossier y entrent (un `cover.jpg` remplacé change
/// la signature sans que les pistes bougent).
fn signature_dossier(dossier: &Path, pistes: &[PisteVerifPochette]) -> String {
    let mut h = Sha256::new();
    for p in pistes {
        h.update(p.path.as_bytes());
        h.update(p.taille.to_le_bytes());
        h.update(p.mtime.to_le_bytes());
    }
    for nom in tags::COVER_FILES {
        if let Ok(m) = std::fs::metadata(dossier.join(nom)) {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            h.update(nom.as_bytes());
            h.update(m.len().to_le_bytes());
            h.update(mtime.to_le_bytes());
        }
    }
    h.finalize().iter().map(|o| format!("{o:02x}")).collect()
}

/// Ce que la vérification d'un dossier conclut. `None` : le dossier n'a pas pu
/// être examiné (fichier illisible, montage absent) — rien n'est écrit, la
/// prochaine passe réessaiera plutôt que de figer un « tout va bien » faux.
struct Verdict {
    emb_hash: Option<String>,
    anomalie: Option<&'static str>,
    detail: String,
    mesure: Option<f64>,
}

fn verifier_dossier(pistes: &[PathBuf]) -> Option<Verdict> {
    let mut integree = None;
    for p in pistes.iter().take(PISTES_ECHANTILLON) {
        match tags::read_embedded_cover(p) {
            Ok(Some(c)) => {
                integree = Some(c);
                break;
            }
            Ok(None) => {}
            // Le fichier ne se lit pas du tout : pas de verdict.
            Err(_) => return None,
        }
    }
    let dossier = pistes.first().and_then(|p| tags::read_folder_cover(p));

    let emb_hash = integree.as_ref().map(|c| hacher(&c.data));
    let vig_integree = integree.as_ref().map(|c| vignette(&c.data));
    let vig_dossier = dossier.as_ref().map(|c| vignette(&c.data));

    let sans_anomalie = |emb_hash| Verdict {
        emb_hash,
        anomalie: None,
        detail: String::new(),
        mesure: None,
    };

    if let Some(Err(raison)) = &vig_integree {
        return Some(Verdict {
            emb_hash,
            anomalie: Some(ILLISIBLE),
            detail: format!("image intégrée aux tags : {raison}"),
            mesure: None,
        });
    }
    if let Some(Err(raison)) = &vig_dossier {
        return Some(Verdict {
            emb_hash,
            anomalie: Some(ILLISIBLE),
            detail: format!("image du dossier : {raison}"),
            mesure: None,
        });
    }
    if let (Some(Ok(a)), Some(Ok(b))) = (&vig_integree, &vig_dossier) {
        let d = distance(a, b);
        if d > SEUIL_DIVERGENCE {
            return Some(Verdict {
                emb_hash,
                anomalie: Some(DIVERGENTE),
                detail: "l'image intégrée aux tags n'est pas celle du dossier \
                         (l'application affiche l'intégrée)"
                    .to_string(),
                mesure: Some(d),
            });
        }
    }
    Some(sans_anomalie(emb_hash))
}

/* ------------------------------------------------------------- la passe */

/// Vérifie les pochettes des dossiers dont les fichiers ont changé depuis la
/// dernière passe — ou de tous avec `force`. Écrit dans `pochettes_verif` ;
/// la lecture se fait par [`suspectes`].
///
/// Même patron que `loudness::actualiser` : `travailleurs` fils lisent les
/// tags et décodent les images, le fil appelant, seul, écrit en base.
pub fn actualiser(
    lib: &Library,
    travailleurs: usize,
    force: bool,
    mut avancement: impl FnMut(usize, usize) + Send,
) -> Result<Bilan> {
    let pistes = lib.pistes_pour_verif_pochettes()?;
    let mut par_dossier: BTreeMap<String, Vec<PisteVerifPochette>> = BTreeMap::new();
    for p in pistes {
        let Some(dir) = Path::new(&p.path).parent() else { continue };
        par_dossier
            .entry(dir.to_string_lossy().into_owned())
            .or_default()
            .push(p);
    }

    let mut bilan = Bilan {
        dossiers: par_dossier.len(),
        ..Default::default()
    };

    let connus = lib.signatures_verif_pochettes()?;
    // (dossier, signature, artiste, album, chemins) des dossiers à revoir.
    let mut a_faire: Vec<(String, String, String, String, Vec<PathBuf>)> = Vec::new();
    for (dossier, pistes) in &par_dossier {
        let signature = signature_dossier(Path::new(dossier), pistes);
        let a_jour = connus
            .get(dossier)
            .is_some_and(|(s, v)| *s == signature && *v == VERSION_VERIF);
        if a_jour && !force {
            continue;
        }
        let premiere = &pistes[0];
        a_faire.push((
            dossier.clone(),
            signature,
            premiere.artiste.clone(),
            premiere.album.clone(),
            pistes.iter().map(|p| PathBuf::from(&p.path)).collect(),
        ));
    }

    let total = a_faire.len();
    if total > 0 {
        let curseur = AtomicUsize::new(0);
        let (tx, rx) = mpsc::sync_channel::<(usize, Option<Verdict>)>(travailleurs.max(1) * 2);
        std::thread::scope(|pool| {
            for _ in 0..travailleurs.max(1) {
                let tx = tx.clone();
                let (curseur, a_faire) = (&curseur, &a_faire);
                pool.spawn(move || loop {
                    let i = curseur.fetch_add(1, Ordering::Relaxed);
                    let Some((dossier, _, _, _, chemins)) = a_faire.get(i) else {
                        break;
                    };
                    // Un décodage d'image qui panique sur un fichier
                    // hasardeux n'emporte pas la passe — `crate::panique`.
                    let verdict = match crate::panique::sans_panique(|| verifier_dossier(chemins)) {
                        Ok(v) => v,
                        Err(message) => {
                            warn!(dossier, message, "vérification de pochette interrompue");
                            None
                        }
                    };
                    if tx.send((i, verdict)).is_err() {
                        break;
                    }
                });
            }
            drop(tx);

            let mut vus = 0usize;
            for (i, verdict) in rx {
                vus += 1;
                avancement(vus, total);
                let Some(v) = verdict else { continue };
                let (dossier, signature, artiste, album, _) = &a_faire[i];
                let ligne = VerifPochette {
                    dossier: dossier.clone(),
                    artiste: artiste.clone(),
                    album: album.clone(),
                    signature: signature.clone(),
                    version: VERSION_VERIF,
                    emb_hash: v.emb_hash,
                    anomalie: v.anomalie.map(str::to_string),
                    detail: v.detail,
                    mesure: v.mesure,
                };
                match lib.enregistrer_verif_pochette(&ligne) {
                    Ok(()) => bilan.verifies += 1,
                    Err(e) => warn!(dossier, error = %e, "écriture de la vérification impossible"),
                }
            }
        });
    }

    // Même prudence que `prune_missing` : une bibliothèque vue vide (montage
    // absent) ne doit pas effacer tous les résultats.
    if !par_dossier.is_empty() {
        let presents: HashSet<String> = par_dossier.keys().cloned().collect();
        lib.elaguer_verif_pochettes(&presents)?;
    }

    bilan.suspectes = suspectes(lib)?.len();
    Ok(bilan)
}

/// Les pochettes à examiner : les signalements enregistrés, plus les images
/// intégrées partagées par des albums d'artistes différents (déduit ici, pas
/// stocké — il dépend de tous les dossiers à la fois). Les signalements
/// écartés par l'utilisateur sont omis.
pub fn suspectes(lib: &Library) -> Result<Vec<PochetteSuspecte>> {
    let lignes = lib.verifs_pochettes()?;

    // hachage → (artistes distincts, albums distincts), tout en minuscules.
    let mut groupes: HashMap<&str, (HashSet<String>, HashSet<String>)> = HashMap::new();
    for (v, _) in &lignes {
        if let Some(h) = &v.emb_hash {
            let g = groupes.entry(h).or_default();
            g.0.insert(v.artiste.to_lowercase());
            g.1.insert(v.album.to_lowercase());
        }
    }

    let mut sortie = Vec::new();
    for (v, ignoree) in &lignes {
        if *ignoree {
            continue;
        }
        let mut pousser = |anomalie: &str, detail: String, mesure: Option<f64>| {
            sortie.push(PochetteSuspecte {
                dossier: v.dossier.clone(),
                artiste: v.artiste.clone(),
                album: v.album.clone(),
                anomalie: anomalie.to_string(),
                detail,
                mesure,
            });
        };
        if let Some(a) = &v.anomalie {
            pousser(a, v.detail.clone(), v.mesure);
        }
        if let Some(h) = &v.emb_hash {
            if let Some((artistes, albums)) = groupes.get(h.as_str()) {
                if artistes.len() > 1 && albums.len() > 1 {
                    pousser(
                        PARTAGEE,
                        format!(
                            "la même image intégrée sert à {} albums d'artistes différents",
                            albums.len()
                        ),
                        None,
                    );
                }
            }
        }
    }
    Ok(sortie)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgb, RgbImage};
    use std::io::Cursor;

    /// Dégradé diagonal, dont le sens change l'image — de quoi distinguer
    /// « même image » et « autre image » sur une vignette.
    fn degrade(cote: u32, inverse: bool) -> RgbImage {
        RgbImage::from_fn(cote, cote, |x, y| {
            let t = ((x + y) * 255 / (2 * cote)) as u8;
            let v = if inverse { 255 - t } else { t };
            Rgb([v, v / 2, 255 - v])
        })
    }

    fn encoder(img: &RgbImage, format: ImageFormat) -> Vec<u8> {
        let mut octets = Vec::new();
        img.write_to(&mut Cursor::new(&mut octets), format).unwrap();
        octets
    }

    #[test]
    fn meme_image_a_deux_tailles_et_deux_formats_ne_diverge_pas() {
        let grande = encoder(&degrade(600, false), ImageFormat::Jpeg);
        let petite = encoder(&degrade(120, false), ImageFormat::Png);
        let d = distance(&vignette(&grande).unwrap(), &vignette(&petite).unwrap());
        assert!(d < SEUIL_DIVERGENCE, "distance {d}");
    }

    #[test]
    fn deux_images_differentes_divergent() {
        let a = encoder(&degrade(300, false), ImageFormat::Jpeg);
        let b = encoder(&degrade(300, true), ImageFormat::Jpeg);
        let d = distance(&vignette(&a).unwrap(), &vignette(&b).unwrap());
        assert!(d > SEUIL_DIVERGENCE, "distance {d}");
    }

    #[test]
    fn un_jpeg_tronque_est_illisible() {
        let entier = encoder(&degrade(400, false), ImageFormat::Jpeg);
        assert!(vignette(&entier).is_ok());
        let coupe = &entier[..entier.len() * 6 / 10];
        let raison = vignette(coupe).unwrap_err();
        assert!(raison.contains("tronqué"), "{raison}");
    }

    #[test]
    fn un_png_tronque_ou_des_octets_quelconques_sont_illisibles() {
        let entier = encoder(&degrade(200, false), ImageFormat::Png);
        assert!(vignette(&entier[..entier.len() / 2]).is_err());
        assert!(vignette(b"pas une image du tout").is_err());
    }

    #[test]
    fn une_image_unie_ne_ressemble_qu_a_une_image_unie() {
        let unie = vec![128.0f32; 256];
        let autre = vec![30.0f32; 256];
        let motif: Vec<f32> = (0..256).map(|i| (i % 7) as f32 * 30.0).collect();
        assert_eq!(distance(&unie, &autre), 0.0);
        assert_eq!(distance(&unie, &motif), 1.0);
    }

    fn ligne(dossier: &str, artiste: &str, album: &str, hash: Option<&str>) -> VerifPochette {
        VerifPochette {
            dossier: dossier.into(),
            artiste: artiste.into(),
            album: album.into(),
            signature: "s".into(),
            version: VERSION_VERIF,
            emb_hash: hash.map(str::to_string),
            anomalie: None,
            detail: String::new(),
            mesure: None,
        }
    }

    #[test]
    fn partagee_seulement_entre_artistes_et_albums_differents() {
        let lib = Library::open_in_memory().unwrap();
        // Même image sur deux artistes différents : signalé des deux côtés.
        lib.enregistrer_verif_pochette(&ligne("/a", "Meat Beat Manifesto", "Subliminal", Some("h1"))).unwrap();
        lib.enregistrer_verif_pochette(&ligne("/b", "Inner City", "Paradise", Some("h1"))).unwrap();
        // Deux disques d'un même album (même artiste, même titre) : normal.
        lib.enregistrer_verif_pochette(&ligne("/c/cd1", "Moriarty", "Epitaph", Some("h2"))).unwrap();
        lib.enregistrer_verif_pochette(&ligne("/c/cd2", "Moriarty", "Epitaph", Some("h2"))).unwrap();
        // Deux albums du même artiste : une série, pas un défaut.
        lib.enregistrer_verif_pochette(&ligne("/d", "Aphex Twin", "Vol 1", Some("h3"))).unwrap();
        lib.enregistrer_verif_pochette(&ligne("/e", "Aphex Twin", "Vol 2", Some("h3"))).unwrap();

        let s = suspectes(&lib).unwrap();
        let dossiers: Vec<&str> = s.iter().map(|p| p.dossier.as_str()).collect();
        // Trié par artiste : Inner City avant Meat Beat Manifesto.
        assert_eq!(dossiers, vec!["/b", "/a"]);
        assert!(s.iter().all(|p| p.anomalie == PARTAGEE));
    }

    #[test]
    fn ignorer_ecarte_le_signalement_et_reecrire_le_rend() {
        let lib = Library::open_in_memory().unwrap();
        let mut v = ligne("/a", "X", "Y", None);
        v.anomalie = Some(DIVERGENTE.into());
        v.mesure = Some(0.7);
        lib.enregistrer_verif_pochette(&v).unwrap();
        assert_eq!(suspectes(&lib).unwrap().len(), 1);

        lib.ignorer_pochette_suspecte("/a").unwrap();
        assert!(suspectes(&lib).unwrap().is_empty());

        // Les fichiers ont changé : le dossier est revérifié, l'ancien « ignorer »
        // ne vaut plus.
        lib.enregistrer_verif_pochette(&v).unwrap();
        assert_eq!(suspectes(&lib).unwrap().len(), 1);
    }

    #[test]
    fn elaguer_retire_les_dossiers_disparus() {
        let lib = Library::open_in_memory().unwrap();
        lib.enregistrer_verif_pochette(&ligne("/a", "X", "Y", None)).unwrap();
        lib.enregistrer_verif_pochette(&ligne("/b", "X", "Z", None)).unwrap();
        let presents: HashSet<String> = ["/a".to_string()].into();
        assert_eq!(lib.elaguer_verif_pochettes(&presents).unwrap(), 1);
        assert_eq!(lib.verifs_pochettes().unwrap().len(), 1);
    }
}
