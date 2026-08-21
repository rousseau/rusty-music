//! Lecture des métadonnées embarquées, agnostique au format (lofty).

use std::path::{Path, PathBuf};

use lofty::config::ParseOptions;
use lofty::picture::PictureType;
use lofty::prelude::*;
use lofty::probe::Probe;

use crate::error::{Error, Result};

/// Métadonnées d'un morceau, telles que lues sur le disque.
#[derive(Debug, Clone, Default)]
pub struct TrackMeta {
    pub path: PathBuf,
    pub size_bytes: i64,
    pub mtime: i64,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i64>,
    pub track_no: Option<i64>,
    pub duration_ms: Option<i64>,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    /// Débit audio en kb/s — `None` pour un format sans débit constant à
    /// annoncer (FLAC, WavPack…), pas une mesure manquante.
    pub bitrate: Option<i64>,
    /// Format du conteneur, tel que lofty le nomme (« MP3 », « FLAC »…).
    pub codec: Option<String>,
    pub mb_recording_id: Option<String>,
    /// Identifiant MusicBrainz de l'artiste de piste. Peut correspondre à
    /// plusieurs artistes sur un « X feat. Y » : on ne retient que le premier,
    /// il ne sert donc pas de clé de regroupement.
    pub mb_artist_id: Option<String>,
    /// Identifiant MusicBrainz de l'artiste d'album — unique, même sur les
    /// pistes en featuring. C'est lui qui regroupe les artistes.
    pub mb_album_artist_id: Option<String>,
}

/// Lit les tags et les propriétés audio d'un fichier.
///
/// Un fichier sans aucun tag n'est pas une erreur : on retombe sur le nom de
/// fichier comme titre, pour qu'il apparaisse quand même dans la bibliothèque.
pub fn read(path: &Path) -> Result<TrackMeta> {
    let fs_meta = std::fs::metadata(path)?;
    let mtime = fs_meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // On saute les pochettes embarquées : elles ne sont pas stockées ici et
    // leur lecture domine le coût du scan (plusieurs Mo par fichier sur un
    // support lent). Le lecteur (module 1) les relira à la demande.
    let opts = ParseOptions::new().read_cover_art(false);

    let tagged = Probe::open(path)
        .map_err(|source| Error::Tags {
            path: path.to_path_buf(),
            source,
        })?
        .options(opts)
        .read()
        .map_err(|source| Error::Tags {
            path: path.to_path_buf(),
            source,
        })?;

    let props = tagged.properties();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

    let fallback_title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string());

    Ok(TrackMeta {
        path: path.to_path_buf(),
        size_bytes: fs_meta.len() as i64,
        mtime,
        title: tag
            .and_then(|t| t.title().map(|s| s.to_string()))
            .or(fallback_title),
        artist: tag.and_then(|t| t.artist().map(|s| s.to_string())),
        album: tag.and_then(|t| t.album().map(|s| s.to_string())),
        // album_artist n'a pas d'accesseur dédié : on passe par la clé générique.
        album_artist: tag.and_then(|t| t.get_string(&ItemKey::AlbumArtist).map(|s| s.to_string())),
        genre: tag.and_then(|t| t.genre().map(|s| s.to_string())),
        // `0` (« 0000 ») est un espace réservé de tagueur pour « année
        // absente », jamais une vraie date d'enregistrement : le garder
        // écraserait les bornes de tout ce qui affiche ou classe par année.
        year: tag
            .and_then(|t| t.year())
            .map(|y| y as i64)
            .filter(|&y| y > 0),
        track_no: tag.and_then(|t| t.track()).map(|n| n as i64),
        duration_ms: Some(props.duration().as_millis() as i64),
        sample_rate: props.sample_rate().map(|v| v as i64),
        channels: props.channels().map(|v| v as i64),
        // Le débit audio, pas le débit global (`overall_bitrate`, qui inclut
        // les tags embarqués) — c'est celui qui dit la qualité de l'encodage.
        // Certains décodeurs ne le distinguent pas et ne rendent que le
        // global : on l'accepte alors en repli plutôt que de perdre la
        // mesure.
        bitrate: props
            .audio_bitrate()
            .or_else(|| props.overall_bitrate())
            .map(|v| v as i64),
        codec: Some(nom_du_format(tagged.file_type())),
        mb_recording_id: tag.and_then(|t| {
            t.get_string(&ItemKey::MusicBrainzRecordingId)
                .map(|s| s.to_string())
        }),
        mb_artist_id: tag.and_then(|t| {
            t.get_string(&ItemKey::MusicBrainzArtistId)
                .map(|s| s.to_string())
        }),
        mb_album_artist_id: tag.and_then(|t| {
            t.get_string(&ItemKey::MusicBrainzReleaseArtistId)
                .map(|s| s.to_string())
        }),
    })
}

/// Nom affichable d'un format de fichier — celui de lofty (`Mpeg`, `Mp4`…)
/// ne parlerait à personne dans un graphe de qualité.
fn nom_du_format(t: lofty::file::FileType) -> String {
    use lofty::file::FileType;
    match t {
        FileType::Mpeg => "MP3",
        FileType::Flac => "FLAC",
        FileType::Mp4 => "MP4",
        FileType::Aac => "AAC",
        FileType::Vorbis => "Ogg Vorbis",
        FileType::Opus => "Opus",
        FileType::Wav => "WAV",
        FileType::Aiff => "AIFF",
        FileType::Ape => "APE",
        FileType::WavPack => "WavPack",
        FileType::Mpc => "Musepack",
        FileType::Speex => "Speex",
        FileType::Custom(s) => s,
        // `FileType` est `#[non_exhaustive]` : lofty peut en ajouter sans que
        // ce soit une rupture de compatibilité pour ses utilisateurs.
        _ => "inconnu",
    }
    .to_string()
}

/// D'où vient une pochette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverSource {
    /// Image embarquée dans les tags du fichier.
    Embedded,
    /// Fichier image posé à côté du morceau (convention `cover.jpg` de beets).
    Folder,
}

/// Une pochette et son type MIME.
#[derive(Debug, Clone)]
pub struct Cover {
    pub data: Vec<u8>,
    pub mime: Option<String>,
    pub source: CoverSource,
}

/// Noms de fichiers cherchés à côté du morceau, par ordre de préférence.
const COVER_FILES: &[&str] = &[
    "cover.jpg",
    "cover.jpeg",
    "cover.png",
    "folder.jpg",
    "folder.png",
    "front.jpg",
];

/// Cherche la pochette d'un morceau : embarquée d'abord, sinon dans le dossier.
///
/// Volontairement hors du scan — celui-ci saute les pochettes, qui pèsent
/// 4,9 Go sur la bibliothèque de test et ne sont pas stockées en base. Le
/// lecteur appelle ceci pour un seul morceau à la fois, quand il l'affiche.
///
/// Le repli sur le dossier n'est pas décoratif : certains albums n'ont aucune
/// image embarquée et ne comptent que sur leur `cover.jpg`.
pub fn read_cover(path: &Path) -> Result<Option<Cover>> {
    if let Some(cover) = read_embedded_cover(path)? {
        return Ok(Some(cover));
    }
    Ok(read_folder_cover(path))
}

fn read_embedded_cover(path: &Path) -> Result<Option<Cover>> {
    // Les propriétés audio ne servent à rien ici et coûtent une estimation de
    // durée sur les MPEG : on ne demande que les tags.
    let opts = ParseOptions::new()
        .read_properties(false)
        .read_cover_art(true);

    let tagged = Probe::open(path)
        .map_err(|source| Error::Tags {
            path: path.to_path_buf(),
            source,
        })?
        .options(opts)
        .read()
        .map_err(|source| Error::Tags {
            path: path.to_path_buf(),
            source,
        })?;

    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Ok(None);
    };

    // Un fichier peut embarquer plusieurs images (pochette arrière, photo
    // d'artiste…) : on veut la face avant, à défaut la première venue.
    let pictures = tag.pictures();
    let picture = pictures
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first());

    Ok(picture.map(|p| Cover {
        data: p.data().to_vec(),
        mime: p.mime_type().map(|m| m.to_string()),
        source: CoverSource::Embedded,
    }))
}

fn read_folder_cover(path: &Path) -> Option<Cover> {
    let dir = path.parent()?;
    for nom in COVER_FILES {
        let candidat = dir.join(nom);
        // Une erreur de lecture ne doit pas masquer les candidats suivants.
        if let Ok(data) = std::fs::read(&candidat) {
            return Some(Cover {
                mime: mime_depuis_extension(&candidat),
                data,
                source: CoverSource::Folder,
            });
        }
    }
    None
}

fn mime_depuis_extension(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => Some("image/jpeg".into()),
        "png" => Some("image/png".into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire propre à chaque test.
    fn dossier(nom: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("rusty-music-cover-{}-{nom}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn repli_dossier_trouve_la_pochette_a_cote() {
        let d = dossier("trouve");
        std::fs::write(d.join("cover.jpg"), b"donnees-jpeg").unwrap();

        let cover = read_folder_cover(&d.join("01 piste.mp3")).unwrap();
        assert_eq!(cover.data, b"donnees-jpeg");
        assert_eq!(cover.mime.as_deref(), Some("image/jpeg"));
        assert_eq!(cover.source, CoverSource::Folder);

        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn repli_dossier_respecte_lordre_de_preference() {
        let d = dossier("ordre");
        // `folder.png` est un candidat plus faible que `cover.jpg`.
        std::fs::write(d.join("folder.png"), b"png").unwrap();
        std::fs::write(d.join("cover.jpg"), b"jpg").unwrap();

        let cover = read_folder_cover(&d.join("01 piste.mp3")).unwrap();
        assert_eq!(cover.data, b"jpg");

        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn repli_dossier_sans_image_ne_renvoie_rien() {
        let d = dossier("vide");
        assert!(read_folder_cover(&d.join("01 piste.mp3")).is_none());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn mime_deduit_de_lextension() {
        let cas = [
            ("cover.jpg", Some("image/jpeg")),
            ("cover.JPEG", Some("image/jpeg")),
            ("cover.png", Some("image/png")),
            ("cover.webp", None),
            ("cover", None),
        ];
        for (nom, attendu) in cas {
            assert_eq!(
                mime_depuis_extension(Path::new(nom)).as_deref(),
                attendu,
                "pour {nom}"
            );
        }
    }
}
