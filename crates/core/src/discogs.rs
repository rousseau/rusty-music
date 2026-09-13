// SPDX-License-Identifier: GPL-3.0-or-later
//! Dump mensuel Discogs (CC0) : découverte, téléchargement, lecture en flux.
//!
//! Module d'entrées/sorties pur — aucun accès à la base. L'orchestration
//! (quels identifiants garder, où écrire les crédits) est dans
//! [`crate::discogs_import`].
//!
//! **Jamais l'API Discogs en direct.** Les dumps mensuels sous licence CC0
//! (`https://data.discogs.com/`) donnent le même contenu sans authentification
//! ni limite de débit, en un seul téléchargement — plus simple à obtenir en
//! volume, et c'est ce que demande le projet.
//!
//! **Un seul fichier nécessaire.** `releases.xml.gz` porte déjà les crédits
//! (`<extraartists>`) et, par édition, son label et son numéro de catalogue
//! (`<labels>`) : pas besoin des dumps `artists`/`labels`/`masters` — ce
//! dernier porterait la fiche complète d'un label (filiation, sous-labels),
//! hors de propos ici où seuls nom et numéro de catalogue comptent.
//!
//! **Jamais décompressé sur disque.** Le gzip est décodé en flux
//! (`flate2::read::GzDecoder`) pendant la lecture : le disque ne voit que le
//! fichier compressé téléchargé (≈ 11 Go), jamais un XML intermédiaire de
//! plusieurs dizaines de gigaoctets.
//!
//! **Tolérant aux irrégularités documentées du format.** Le flux n'est jamais
//! analysé par un seul lecteur XML continu : chaque `<release>` est d'abord
//! isolé par une simple recherche de ses bornes (`<release `/`</release>`),
//! puis analysé indépendamment. Un fragment illisible ne coûte que ses propres
//! crédits — jamais tout l'import, et jamais les fragments suivants, puisque
//! le repérage des bornes ne dépend en rien du contenu XML lui-même.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::Path;

use flate2::read::GzDecoder;
use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::error::{Error, Result};

/// Taille des lectures successives depuis le flux gzip décodé.
const TAILLE_LECTURE: usize = 64 * 1024;

/// Un crédit lu dans le `<extraartists>` d'une édition — release-level
/// uniquement (pas les crédits par piste imbriqués dans `<tracklist>`, hors
/// périmètre pour l'instant).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreditBrut {
    pub personne: String,
    pub role: String,
    /// Notation Discogs de la portée ('' = toute l'édition, sinon "A1, A2").
    pub pistes: String,
    pub discogs_artist_id: Option<i64>,
}

/// Un label (et son numéro de catalogue) lu dans le `<labels>` d'une édition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelBrut {
    pub nom: String,
    pub catno: String,
    pub discogs_label_id: Option<i64>,
}

/// Une édition Discogs telle que retenue du dump, réduite à ce qui sert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseDiscogs {
    pub id: u64,
    pub credits: Vec<CreditBrut>,
    pub labels: Vec<LabelBrut>,
}

/// Bilan d'un import.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BilanImport {
    pub editions_vues: u64,
    pub editions_retenues: u64,
    pub fragments_malformes: u64,
}

/// Un agent HTTP pour le téléchargement du dump — pas de cadence particulière
/// (un seul très gros téléchargement, pas des milliers de petites requêtes),
/// mais un délai généreux : le fichier pèse plusieurs gigaoctets.
pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(format!("rusty-music/{}", env!("CARGO_PKG_VERSION")))
        .timeout_global(Some(std::time::Duration::from_secs(3600)))
        .build()
        .into()
}

/// Découvre l'URL de téléchargement du `releases.xml.gz` le plus récent, en
/// parcourant le listing public de `https://data.discogs.com/` (page HTML,
/// pas une API) pour `annee`, et `annee - 1` si celle-ci ne porte encore aucune
/// entrée (début janvier, avant le premier dump du mois).
pub fn derniere_url_dump(agent: &ureq::Agent, annee: i32) -> Result<String> {
    for a in [annee, annee - 1] {
        if let Some(url) = chercher_dump(agent, a)? {
            return Ok(url);
        }
    }
    Err(Error::Reseau(format!(
        "aucun dump Discogs trouvé pour {annee} ni {}",
        annee - 1
    )))
}

fn chercher_dump(agent: &ureq::Agent, annee: i32) -> Result<Option<String>> {
    let url = format!("https://data.discogs.com/?prefix=data%2F{annee}%2F");
    let mut r = agent
        .get(&url)
        .call()
        .map_err(|e| Error::Reseau(format!("listing {url} : {e}")))?;
    let page = r
        .body_mut()
        .read_to_string()
        .map_err(|e| Error::Reseau(format!("lecture du listing : {e}")))?;

    // Les liens sont `?download=data%2F{annee}%2Fdiscogs_YYYYMMDD_releases.xml.gz`,
    // dans l'ordre chronologique : le dernier trouvé est le plus récent.
    let motif = "_releases.xml.gz";
    let Some(fin) = page.match_indices(motif).last().map(|(i, _)| i + motif.len()) else {
        return Ok(None);
    };
    let Some(debut) = page[..fin].rfind("discogs_") else {
        return Err(Error::Reseau("listing Discogs illisible : nom de fichier introuvable".into()));
    };
    let fichier = &page[debut..fin];
    Ok(Some(format!("https://data.discogs.com/?download=data%2F{annee}%2F{fichier}")))
}

/// Télécharge `url` dans `dest`, en flux — jamais entièrement en mémoire.
pub fn telecharger(agent: &ureq::Agent, url: &str, dest: &Path) -> Result<()> {
    telecharger_avec_avancement(agent, url, dest, |_, _| {})
}

/// Taille des lots de copie pendant le téléchargement — assez gros pour ne
/// pas multiplier les appels à `avancer`, assez petit pour qu'un avancement
/// affiché reste vivant sur un fichier de plusieurs gigaoctets.
const TAILLE_LOT_TELECHARGEMENT: usize = 4 * 1024 * 1024;

/// Comme [`telecharger`], en rapportant l'avancement en octets déjà écrits —
/// `avancer(vus, total)`, `total` étant `None` si le serveur n'a pas annoncé
/// de `Content-Length` (ne devrait pas arriver pour un fichier statique, mais
/// mieux vaut ne rien y supposer côté appelant).
pub fn telecharger_avec_avancement(
    agent: &ureq::Agent,
    url: &str,
    dest: &Path,
    mut avancer: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    let mut r = agent
        .get(url)
        .call()
        .map_err(|e| Error::Reseau(format!("téléchargement {url} : {e}")))?;
    let total = r
        .headers()
        .get(ureq::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let mut fichier = File::create(dest)?;
    let mut lecteur = r.body_mut().as_reader();
    let mut tampon = vec![0u8; TAILLE_LOT_TELECHARGEMENT];
    let mut vus = 0u64;
    loop {
        let n = lecteur.read(&mut tampon)?;
        if n == 0 {
            break;
        }
        fichier.write_all(&tampon[..n])?;
        vus += n as u64;
        avancer(vus, total);
    }
    Ok(())
}

/// Toutes les combien d'éditions vues `avancer` est rappelé — l'immense
/// majorité des ~18 M éditions du dump n'intéresse pas notre bibliothèque, ce
/// n'est donc pas `editions_retenues` qui dirait la progression.
const PROGRES_TOUS_LES: u64 = 5_000;

/// Parcourt `chemin` (un `releases.xml.gz` déjà téléchargé) et appelle `f`
/// pour chaque édition dont l'identifiant figure dans `voulus`. Les autres
/// sont ignorées sans être analysées en détail — la plupart des ~18 M
/// éditions du dump ne concernent pas notre bibliothèque.
pub fn pour_chaque_release(
    chemin: &Path,
    voulus: &HashSet<u64>,
    mut f: impl FnMut(ReleaseDiscogs),
    mut avancer: impl FnMut(&BilanImport),
) -> Result<BilanImport> {
    let source = GzDecoder::new(BufReader::new(File::open(chemin)?));
    let mut flux = FluxReleases::new(source);
    let mut bilan = BilanImport::default();

    while let Some((id, fragment)) = flux.suivant()? {
        bilan.editions_vues += 1;
        if voulus.contains(&id) {
            match parser_fragment(&fragment) {
                Ok((credits, labels)) => {
                    bilan.editions_retenues += 1;
                    f(ReleaseDiscogs { id, credits, labels });
                }
                Err(e) => {
                    bilan.fragments_malformes += 1;
                    tracing::warn!(erreur = %e, id, "édition Discogs illisible, ignorée");
                }
            }
        }
        if bilan.editions_vues % PROGRES_TOUS_LES == 0 {
            avancer(&bilan);
        }
    }
    avancer(&bilan);
    Ok(bilan)
}

/// Isole les fragments `<release ...>...</release>` d'un flux décompressé,
/// sans jamais garder plus qu'un fragment (et le prochain bout lu) en
/// mémoire.
struct FluxReleases<R> {
    source: R,
    tampon: Vec<u8>,
}

impl<R: Read> FluxReleases<R> {
    fn new(source: R) -> Self {
        Self { source, tampon: Vec::new() }
    }

    /// Le prochain fragment `(id, xml)`, ou `None` en fin de flux. Un
    /// `<release` resté sans `</release>` à la fin du fichier (téléchargement
    /// tronqué) est silencieusement abandonné : il ne peut de toute façon pas
    /// être analysé.
    fn suivant(&mut self) -> Result<Option<(u64, Vec<u8>)>> {
        loop {
            if let Some(deb) = trouver(&self.tampon, b"<release ") {
                if let Some(rel_fin) = trouver(&self.tampon[deb..], b"</release>") {
                    let fin = deb + rel_fin + b"</release>".len();
                    let fragment = self.tampon[deb..fin].to_vec();
                    self.tampon.drain(..fin);
                    let id = id_depuis_tag(&fragment).ok_or_else(|| {
                        Error::Parsing("édition Discogs sans identifiant lisible".into())
                    })?;
                    return Ok(Some((id, fragment)));
                }
            }
            let mut lot = [0u8; TAILLE_LECTURE];
            let n = self.source.read(&mut lot)?;
            if n == 0 {
                return Ok(None);
            }
            self.tampon.extend_from_slice(&lot[..n]);
        }
    }
}

/// Position de la première occurrence de `motif` dans `texte`.
fn trouver(texte: &[u8], motif: &[u8]) -> Option<usize> {
    texte.windows(motif.len()).position(|f| f == motif)
}

/// L'attribut `id="…"` de la balise ouvrante `<release id="…" …>`.
fn id_depuis_tag(fragment: &[u8]) -> Option<u64> {
    let fin_ouverture = fragment.iter().position(|&b| b == b'>')?;
    let ouverture = std::str::from_utf8(&fragment[..fin_ouverture]).ok()?;
    let apres = ouverture.split("id=\"").nth(1)?;
    let chiffres: String = apres.chars().take_while(|c| c.is_ascii_digit()).collect();
    chiffres.parse().ok()
}

/// L'état d'un `<artist>` de `<extraartists>` en cours de lecture. Le texte
/// s'accumule par `push_str` plutôt que d'être affecté d'un coup : depuis
/// quick-xml 0.38, une entité (`&amp;`) coupe le texte en plusieurs
/// évènements (`Event::Text` autour d'un `Event::GeneralRef`) — un nom comme
/// « Stock, Aitken & Waterman » arrive donc en trois morceaux.
#[derive(Debug, Default)]
struct CreditEnCours {
    id: String,
    name: String,
    anv: String,
    role: String,
    tracks: String,
}

impl CreditEnCours {
    /// `anv` (nom tel que crédité sur cette édition précise) prime sur `name`
    /// (nom canonique de l'artiste chez Discogs) quand il est renseigné.
    /// Un crédit sans nom ni rôle est une entrée malformée : ignorée plutôt
    /// que mal attribuée.
    fn finaliser(self) -> Option<CreditBrut> {
        let anv = self.anv.trim();
        let name = self.name.trim();
        let personne = if !anv.is_empty() { anv } else { name };
        let role = self.role.trim();
        if personne.is_empty() || role.is_empty() {
            return None;
        }
        Some(CreditBrut {
            personne: personne.to_string(),
            role: role.to_string(),
            pistes: self.tracks.trim().to_string(),
            discogs_artist_id: self.id.trim().parse().ok(),
        })
    }

    /// Ajoute un fragment de texte au champ actuellement ouvert (`champ`).
    fn accumuler(courant: &mut Option<Self>, champ: Option<&str>, texte: &str) {
        let (Some(c), Some(nom)) = (courant.as_mut(), champ) else { return };
        match nom {
            "id" => c.id.push_str(texte),
            "name" => c.name.push_str(texte),
            "anv" => c.anv.push_str(texte),
            "role" => c.role.push_str(texte),
            "tracks" => c.tracks.push_str(texte),
            _ => {}
        }
    }
}

/// Résout une référence d'entité générale (`Event::GeneralRef`) en texte —
/// prédéfinie (`&amp;`, `&lt;`…) ou numérique (`&#38;`, `&#x26;`). Une
/// entité inconnue (hors XML strict — DTD non chargée) est tue plutôt que de
/// faire échouer tout le fragment : `personne`/`role` seront simplement
/// amputés du caractère manquant.
fn resoudre_ref(r: &quick_xml::events::BytesRef) -> String {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        return c.to_string();
    }
    quick_xml::escape::resolve_predefined_entity(r)
        .unwrap_or_default()
        .to_string()
}

/// Lit `name`, `catno` et `id` d'un `<label .../>` auto-fermant. Contrairement
/// à `<artist>`, pas de texte imbriqué à accumuler sur plusieurs évènements :
/// une valeur d'attribut se résout en un seul appel (`normalized_value`). Une
/// entrée sans nom lisible est ignorée plutôt que mal attribuée — même
/// principe que [`CreditEnCours::finaliser`].
fn label_depuis_attributs(e: &quick_xml::events::BytesStart) -> Option<LabelBrut> {
    let mut nom = String::new();
    let mut catno = String::new();
    let mut discogs_label_id = None;
    for attr in e.attributes().flatten() {
        let Ok(valeur) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) else { continue };
        match attr.key.local_name().as_ref() {
            "name" => nom = valeur.trim().to_string(),
            "catno" => catno = valeur.trim().to_string(),
            "id" => discogs_label_id = valeur.trim().parse().ok(),
            _ => {}
        }
    }
    if nom.is_empty() {
        return None;
    }
    Some(LabelBrut { nom, catno, discogs_label_id })
}

/// Analyse un fragment `<release>...</release>` déjà isolé et rend ses
/// crédits `<extraartists>` de portée édition (pas ceux, imbriqués, d'une
/// piste précise dans `<tracklist>` — hors périmètre) et ses labels
/// `<labels>` (nom + numéro de catalogue).
///
/// Une erreur ici ne concerne que ce fragment : elle ne se propage jamais à
/// la lecture du flux, qui a déjà avancé sur les bornes de texte brut avant
/// même cet appel.
fn parser_fragment(fragment: &[u8]) -> Result<(Vec<CreditBrut>, Vec<LabelBrut>)> {
    // Pas de `trim_text(true)` : depuis quick-xml 0.38, une entité coupe le
    // texte d'un champ en plusieurs évènements, et un rabotage par évènement
    // mangerait l'espace de part et d'autre de l'entité (« Stock, Aitken »
    // + « & » + « Waterman » → « Stock, Aitken&Waterman »). Le rabotage se
    // fait une seule fois, sur le texte déjà réassemblé, dans `finaliser`.
    let mut reader = Reader::from_reader(fragment);
    let mut buf = Vec::new();
    let mut credits = Vec::new();
    let mut labels = Vec::new();
    let mut dans_extraartists = false;
    let mut dans_labels = false;
    let mut courant: Option<CreditEnCours> = None;
    let mut champ: Option<&'static str> = None;

    loop {
        let evt = reader
            .read_event_into(&mut buf)
            .map_err(|e| Error::Parsing(format!("XML Discogs illisible : {e}")))?;
        match evt {
            Event::Eof => break,
            Event::Start(e) => match e.local_name().as_ref() {
                "extraartists" => dans_extraartists = true,
                "artist" if dans_extraartists => courant = Some(CreditEnCours::default()),
                "id" if courant.is_some() => champ = Some("id"),
                "name" if courant.is_some() => champ = Some("name"),
                "anv" if courant.is_some() => champ = Some("anv"),
                "role" if courant.is_some() => champ = Some("role"),
                "tracks" if courant.is_some() => champ = Some("tracks"),
                "labels" => dans_labels = true,
                // Non garanti par le format (les éditions observées portent
                // toutes un `<label .../>` auto-fermant), mais une balise
                // ouvrante non auto-fermante ne doit pas être perdue en
                // silence : on la lit au même titre.
                "label" if dans_labels => {
                    if let Some(l) = label_depuis_attributs(&e) {
                        labels.push(l);
                    }
                }
                _ => {}
            },
            Event::Empty(e) if dans_labels && e.local_name().as_ref() == "label" => {
                if let Some(l) = label_depuis_attributs(&e) {
                    labels.push(l);
                }
            }
            Event::Text(t) => {
                CreditEnCours::accumuler(&mut courant, champ, &t);
            }
            Event::GeneralRef(r) => {
                CreditEnCours::accumuler(&mut courant, champ, &resoudre_ref(&r));
            }
            Event::End(e) => match e.local_name().as_ref() {
                "extraartists" => dans_extraartists = false,
                "artist" if dans_extraartists => {
                    if let Some(c) = courant.take() {
                        if let Some(credit) = c.finaliser() {
                            credits.push(credit);
                        }
                    }
                }
                "labels" => dans_labels = false,
                "id" | "name" | "anv" | "role" | "tracks" => champ = None,
                _ => {}
            },
            _ => {}
        }
        buf.clear();
    }
    Ok((credits, labels))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parser_credits_lit_les_extraartists_de_ledition() {
        let xml = br#"<release id="249504" status="Accepted">
            <artists><artist><id>72872</id><name>Rick Astley</name></artist></artists>
            <extraartists>
                <artist><id>34067</id><name>Pete Hammond</name><anv>Mixmaster Pete Hammond</anv>
                    <role>Mixed By</role><tracks></tracks></artist>
                <artist><id>20942</id><name>Stock, Aitken &amp; Waterman</name>
                    <role>Producer, Written-By</role><tracks>A1, A2</tracks></artist>
            </extraartists>
        </release>"#;
        let (credits, _) = parser_fragment(xml).expect("fragment lisible");
        assert_eq!(credits.len(), 2);
        assert_eq!(credits[0].personne, "Mixmaster Pete Hammond");
        assert_eq!(credits[0].role, "Mixed By");
        assert_eq!(credits[0].pistes, "");
        assert_eq!(credits[1].personne, "Stock, Aitken & Waterman");
        assert_eq!(credits[1].pistes, "A1, A2");
        assert_eq!(credits[1].discogs_artist_id, Some(20942));
    }

    #[test]
    fn parser_credits_ignore_un_credit_sans_role() {
        let xml = br#"<release id="1"><extraartists>
            <artist><id>1</id><name>Sans Role</name><role></role><tracks></tracks></artist>
        </extraartists></release>"#;
        assert!(parser_fragment(xml).expect("fragment lisible").0.is_empty());
    }

    #[test]
    fn parser_credits_dune_edition_sans_extraartists_rend_une_liste_vide() {
        let xml = br#"<release id="1"><artists></artists></release>"#;
        assert!(parser_fragment(xml).expect("fragment lisible").0.is_empty());
    }

    #[test]
    fn parser_fragment_lit_les_labels_de_ledition() {
        // Sous-label + label parent, comme Big Dada / Counter Records sous
        // Ninja Tune : deux `<label>` auto-fermants dans le même `<labels>`.
        let xml = br#"<release id="1">
            <labels>
                <label name="Ninja Tune" catno="ZEN123" id="222"/>
                <label name="Big Dada" catno="BD456" id="333"/>
            </labels>
        </release>"#;
        let (_, labels) = parser_fragment(xml).expect("fragment lisible");
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].nom, "Ninja Tune");
        assert_eq!(labels[0].catno, "ZEN123");
        assert_eq!(labels[0].discogs_label_id, Some(222));
        assert_eq!(labels[1].nom, "Big Dada");
        assert_eq!(labels[1].catno, "BD456");
    }

    #[test]
    fn parser_fragment_dune_edition_sans_labels_rend_une_liste_vide() {
        let xml = br#"<release id="1"><extraartists></extraartists></release>"#;
        assert!(parser_fragment(xml).expect("fragment lisible").1.is_empty());
    }

    #[test]
    fn label_depuis_attributs_ignore_un_label_sans_nom() {
        let xml = br#"<release id="1"><labels><label catno="X"/></labels></release>"#;
        assert!(parser_fragment(xml).expect("fragment lisible").1.is_empty());
    }

    #[test]
    fn id_depuis_tag_lit_lattribut() {
        assert_eq!(id_depuis_tag(br#"<release id="249504" status="Accepted">"#), Some(249504));
        assert_eq!(id_depuis_tag(b"<release>"), None);
    }

    #[test]
    fn flux_releases_isole_chaque_fragment_meme_coupe_entre_deux_lectures() {
        let xml = b"<releases>\
            <release id=\"1\"><extraartists></extraartists></release>\
            <release id=\"2\"><extraartists></extraartists></release>\
            </releases>";
        // Un lecteur qui ne rend qu'un octet à la fois force `suivant` à
        // accumuler sur plusieurs lectures avant de trouver une borne.
        struct UnOctet<'a>(Cursor<&'a [u8]>);
        impl<'a> Read for UnOctet<'a> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n = 1.min(buf.len());
                self.0.read(&mut buf[..n])
            }
        }
        let mut flux = FluxReleases::new(UnOctet(Cursor::new(xml)));
        let (id1, _) = flux.suivant().unwrap().expect("premier fragment");
        let (id2, _) = flux.suivant().unwrap().expect("second fragment");
        assert_eq!((id1, id2), (1, 2));
        assert!(flux.suivant().unwrap().is_none());
    }

    /// Bout en bout : un vrai `.xml.gz` sur disque, lu par
    /// [`pour_chaque_release`] — le chemin réellement emprunté par un import.
    #[test]
    fn pour_chaque_release_lit_un_vrai_dump_gzippe() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<releases>
<release id="1"><extraartists></extraartists></release>
<release id="249504"><extraartists>
    <artist><id>34067</id><name>Pete Hammond</name><anv>Mixmaster Pete Hammond</anv>
        <role>Mixed By</role><tracks></tracks></artist>
</extraartists><labels>
    <label name="Ninja Tune" catno="ZEN123" id="222"/>
</labels></release>
<release id="3"><extraartists></extraartists></release>
</releases>"#;

        let dir = std::env::temp_dir().join(format!("rusty-music-discogs-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let chemin = dir.join("releases.xml.gz");
        {
            use flate2::write::GzEncoder;
            use flate2::Compression;
            use std::io::Write;
            let mut enc = GzEncoder::new(File::create(&chemin).unwrap(), Compression::default());
            enc.write_all(xml).unwrap();
            enc.finish().unwrap();
        }

        let voulus: HashSet<u64> = [249504].into_iter().collect();
        let mut trouvees = Vec::new();
        let bilan = pour_chaque_release(&chemin, &voulus, |r| trouvees.push(r), |_| {}).unwrap();

        assert_eq!(bilan.editions_vues, 3);
        assert_eq!(bilan.editions_retenues, 1);
        assert_eq!(bilan.fragments_malformes, 0);
        assert_eq!(trouvees.len(), 1);
        assert_eq!(trouvees[0].id, 249504);
        assert_eq!(trouvees[0].credits[0].personne, "Mixmaster Pete Hammond");
        assert_eq!(trouvees[0].labels[0].nom, "Ninja Tune");
        assert_eq!(trouvees[0].labels[0].catno, "ZEN123");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parser_credits_dun_fragment_invalide_rend_une_erreur_locale() {
        // Balise fermante qui ne correspond pas à l'ouvrante : une erreur XML
        // confinée à ce fragment, sans toucher au reste du flux (voir
        // `pour_chaque_release`, qui l'attrape et compte le fragment).
        let xml = br#"<release id="1"><extraartists><artist></desaccord></extraartists></release>"#;
        assert!(parser_fragment(xml).is_err());
    }
}
