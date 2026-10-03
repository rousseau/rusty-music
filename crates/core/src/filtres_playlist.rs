// SPDX-License-Identifier: GPL-3.0-or-later
//! Filtres d'une playlist demandée en texte libre (Explorer → champ
//! d'intention) : ce que le LLM *traduit*, ce que le code *applique*.
//!
//! Le LLM ne nomme jamais de morceaux (il en invente) : il rend des arguments
//! typés et bornés — genres pris dans le vocabulaire réel, exclusions, années,
//! BPM, niveau d'énergie, popularité — que ce module applique à la
//! bibliothèque. Même principe qu'AudioMuse-AI (`search_database`) ; voir
//! `docs/recherche-llm-playlist.md`.
//!
//! Tout ici est pur (pas de base, pas de réseau) pour se tester sans rien :
//! [`crate::Library::caracteristiques_pistes`] fournit les données.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// Durée supposée d'un morceau sans durée connue (3 min 30).
pub const DUREE_DEFAUT_MS: i64 = 210_000;

/// Niveau d'énergie demandé. Relatif à **la bibliothèque** (tiers de la
/// distribution des énergies mesurées), pas un seuil fixe : « calme » dans une
/// collection de métal n'est pas « calme » dans une collection d'ambient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NiveauEnergie {
    Calme,
    Moyenne,
    Intense,
}

impl NiveauEnergie {
    /// Tolérant : un modèle écrit « calm », « énergique »… même contraint.
    pub fn depuis_texte(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "calme" | "calm" | "doux" | "low" => Some(Self::Calme),
            "moyenne" | "moyen" | "medium" | "mid" => Some(Self::Moyenne),
            "intense" | "energique" | "énergique" | "high" => Some(Self::Intense),
            _ => None,
        }
    }

    pub fn libelle(self) -> &'static str {
        match self {
            Self::Calme => "calme",
            Self::Moyenne => "énergie moyenne",
            Self::Intense => "intense",
        }
    }
}

/// Popularité demandée : un tiers de la bibliothèque, d'après le rang
/// percentile `track_popularite.relative` (voir `docs/popularite.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NiveauPopularite {
    PeuConnu,
    Connu,
}

impl NiveauPopularite {
    pub fn depuis_texte(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "peu_connu" | "peu connu" | "obscur" | "confidentiel" | "niche" => Some(Self::PeuConnu),
            "connu" | "populaire" | "tube" | "popular" => Some(Self::Connu),
            _ => None,
        }
    }

    pub fn libelle(self) -> &'static str {
        match self {
            Self::PeuConnu => "peu connu",
            Self::Connu => "connu",
        }
    }
}

/// Les contraintes d'une playlist. Tout est optionnel : un filtre vide ne
/// restreint rien ([`FiltresPlaylist::est_vide`]).
///
/// Les **exclusions sont dures** : jamais relâchées, comme les « coupes SQL »
/// d'AudioMuse-AI — « sans rock » ne doit pas devenir « un peu de rock » faute
/// de morceaux. Les désirs (genres, années, BPM, énergie, popularité) peuvent
/// l'être, en le disant ([`selectionner`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FiltresPlaylist {
    pub genres: Vec<String>,
    pub exclure_genres: Vec<String>,
    pub exclure_artistes: Vec<String>,
    pub annee_min: Option<i32>,
    pub annee_max: Option<i32>,
    /// Attention à l'ambiguïté d'octave du détecteur de tempo (un 174 BPM peut
    /// être mesuré 87) — voir `DescripteursVus::bpm`.
    pub bpm_min: Option<f32>,
    pub bpm_max: Option<f32>,
    pub energie: Option<NiveauEnergie>,
    pub popularite: Option<NiveauPopularite>,
}

impl FiltresPlaylist {
    pub fn est_vide(&self) -> bool {
        self.genres.is_empty()
            && self.exclure_genres.is_empty()
            && self.exclure_artistes.is_empty()
            && self.annee_min.is_none()
            && self.annee_max.is_none()
            && self.bpm_min.is_none()
            && self.bpm_max.is_none()
            && self.energie.is_none()
            && self.popularite.is_none()
    }
}

/// Ce que la bibliothèque sait d'un morceau, pour le filtrer.
#[derive(Debug, Clone, Default)]
pub struct CaracteristiquesPiste {
    pub id: i64,
    pub artiste: Option<String>,
    pub annee: Option<i32>,
    pub duree_ms: Option<i64>,
    pub bpm: Option<f32>,
    pub energie: Option<f32>,
    /// Rang percentile 0..1 dans la bibliothèque (1 = le plus écouté).
    pub popularite: Option<f64>,
    /// Genres résolus, en minuscules.
    pub genres: Vec<String>,
}

/// Les morceaux admissibles, et ce qu'il a fallu lâcher pour en avoir assez.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub ids: HashSet<i64>,
    /// Une phrase par critère abandonné — à montrer à l'utilisateur, jamais à
    /// taire : une playlist « calme » qui ne l'est plus doit le dire.
    pub relaches: Vec<String>,
}

/// Nom d'artiste comparable : minuscules, sans article initial.
fn nom_artiste(s: &str) -> String {
    let s = s.trim().to_lowercase();
    for article in ["the ", "les ", "le ", "la "] {
        if let Some(reste) = s.strip_prefix(article) {
            return reste.trim().to_string();
        }
    }
    s
}

/// Les artistes d'une mention (« James Brown & The Famous Flames », « X feat.
/// Y », « A, B ») : chaque nom séparément, plus la mention entière. Sans cela,
/// « sans James Brown » laisserait passer toute collaboration — mesuré sur la
/// bibliothèque réelle. Une homonymie partielle (« Prince » contre « Prince
/// Royce ») ne correspond pas : on compare des noms entiers, pas des préfixes.
fn artistes_de(mention: &str) -> Vec<String> {
    let mut noms = vec![nom_artiste(mention)];
    let bas = mention.to_lowercase();
    let mut morceaux = vec![bas];
    for sep in [" & ", " and ", " et ", " feat. ", " feat ", " featuring ", " ft. ", " with ", " vs. ", " vs ", " x ", " + ", ", ", "; ", " / "] {
        morceaux = morceaux.iter().flat_map(|m| m.split(sep).map(str::to_string).collect::<Vec<_>>()).collect();
    }
    noms.extend(morceaux.iter().map(|m| nom_artiste(m)));
    noms
}

/// Les mots d'un genre ou d'une demande : minuscules, hors ponctuation, de
/// sorte que « hip-hop », « hip hop » et « rap/hip hop » partagent leurs mots.
pub(crate) fn mots(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|m| !m.is_empty())
        .map(str::to_string)
        .collect()
}

/// Les formulations équivalentes d'une demande de genre : « rap » et « hip
/// hop » désignent la même chose, et la bibliothèque les range sous les deux.
pub(crate) fn variantes_de_genre(demande: &str) -> Vec<String> {
    // Un genre composé (« rap/hip hop », comme la bibliothèque en contient) est
    // une alternative, pas une conjonction : n'exiger que *tous* ses mots
    // laisserait passer le « rap » seul — mesuré en demandant « sans rap ».
    let mut v = Vec::new();
    for part in demande.split(['/', ',', ';', '|']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let m = mots(part);
        v.push(part.to_string());
        if m == ["rap"] {
            v.push("hip hop".into());
        } else if m == ["hip", "hop"] {
            v.push("rap".into());
        }
    }
    v
}

/// Le genre `genre` d'un morceau relève-t-il du genre demandé ? Par **mots** :
/// « rock » couvre « alternative rock », « hard rock » ou « rock;folk », mais
/// pas « rocksteady ». Une exclusion « sans rock » qui n'écartait que le genre
/// exactement « rock » laissait passer toute la famille (mesuré : 8 morceaux
/// sur 20 dans une playlist « sans rock » de la bibliothèque réelle).
pub(crate) fn genre_correspond(genre: &str, demande: &str) -> bool {
    let g = mots(genre);
    variantes_de_genre(demande).iter().any(|d| {
        let dm = mots(d);
        !dm.is_empty() && dm.iter().all(|t| g.contains(t))
    })
}

fn quantile(tries: &[f32], q: f32) -> f32 {
    let i = ((tries.len() - 1) as f32 * q).round() as usize;
    tries[i.min(tries.len() - 1)]
}

/// Les deux seuils (tiers inférieur / supérieur) de la distribution des
/// énergies mesurées. `None` si rien n'a été mesuré.
fn seuils_energie(pistes: &[CaracteristiquesPiste]) -> Option<(f32, f32)> {
    let mut e: Vec<f32> = pistes.iter().filter_map(|p| p.energie).collect();
    if e.is_empty() {
        return None;
    }
    e.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some((quantile(&e, 1.0 / 3.0), quantile(&e, 2.0 / 3.0)))
}

fn convient(
    p: &CaracteristiquesPiste,
    f: &FiltresPlaylist,
    seuils: Option<(f32, f32)>,
    exclus: &HashSet<String>,
) -> bool {
    // Exclusions d'abord : dures, et un morceau sans artiste ni genre connu
    // n'est jamais écarté par elles (rien ne dit qu'il y contrevient).
    if let Some(a) = &p.artiste {
        if artistes_de(a).iter().any(|n| exclus.contains(n)) {
            return false;
        }
    }
    if f.exclure_genres.iter().any(|g| p.genres.iter().any(|pg| genre_correspond(pg, g))) {
        return false;
    }
    if !f.genres.is_empty()
        && !f.genres.iter().any(|g| p.genres.iter().any(|pg| genre_correspond(pg, g)))
    {
        return false;
    }
    // Une borne demandée écarte un morceau dont la valeur est inconnue :
    // « années 70 » ne doit pas ramasser des morceaux sans date.
    if let Some(min) = f.annee_min {
        if !p.annee.is_some_and(|a| a >= min) {
            return false;
        }
    }
    if let Some(max) = f.annee_max {
        if !p.annee.is_some_and(|a| a <= max) {
            return false;
        }
    }
    if let Some(min) = f.bpm_min {
        if !p.bpm.is_some_and(|b| b >= min) {
            return false;
        }
    }
    if let Some(max) = f.bpm_max {
        if !p.bpm.is_some_and(|b| b <= max) {
            return false;
        }
    }
    if let Some(niveau) = f.energie {
        let Some((bas, haut)) = seuils else { return false };
        let Some(e) = p.energie else { return false };
        let ok = match niveau {
            NiveauEnergie::Calme => e <= bas,
            NiveauEnergie::Moyenne => e > bas && e < haut,
            NiveauEnergie::Intense => e >= haut,
        };
        if !ok {
            return false;
        }
    }
    if let Some(niveau) = f.popularite {
        let Some(r) = p.popularite else { return false };
        let ok = match niveau {
            NiveauPopularite::PeuConnu => r <= 1.0 / 3.0,
            NiveauPopularite::Connu => r >= 2.0 / 3.0,
        };
        if !ok {
            return false;
        }
    }
    true
}

fn filtrer(pistes: &[CaracteristiquesPiste], f: &FiltresPlaylist) -> HashSet<i64> {
    let seuils = f.energie.and_then(|_| seuils_energie(pistes));
    let exclus: HashSet<String> = f.exclure_artistes.iter().map(|a| nom_artiste(a)).collect();
    pistes
        .iter()
        .filter(|p| convient(p, f, seuils, &exclus))
        .map(|p| p.id)
        .collect()
}

/// Combien de morceaux restent après chaque famille de critères, appliquées
/// l'une après l'autre : la bibliothèque, les genres voulus, les refus, la
/// période, le tempo, l'énergie, la popularité. Rend `(libellé, restants)`,
/// une ligne par famille **réellement demandée** — de quoi montrer pourquoi
/// une sélection est large ou étroite (« 27 385 → 6 340 rock → 5 900 hors
/// Prince »), sans rien inventer : ce sont les mêmes comptes que [`selectionner`]
/// avant tout assouplissement.
pub fn entonnoir(pistes: &[CaracteristiquesPiste], f: &FiltresPlaylist) -> Vec<(String, usize)> {
    let mut etapes = vec![("Bibliothèque".to_string(), pistes.len())];
    let mut courant = FiltresPlaylist::default();
    let ajouter = |libelle: String, courant: &FiltresPlaylist, etapes: &mut Vec<(String, usize)>| {
        etapes.push((libelle, filtrer(pistes, courant).len()));
    };
    if !f.genres.is_empty() {
        courant.genres = f.genres.clone();
        ajouter(format!("genre : {}", f.genres.join(", ")), &courant, &mut etapes);
    }
    if !f.exclure_genres.is_empty() || !f.exclure_artistes.is_empty() {
        courant.exclure_genres = f.exclure_genres.clone();
        courant.exclure_artistes = f.exclure_artistes.clone();
        let refus: Vec<&str> =
            f.exclure_genres.iter().chain(&f.exclure_artistes).map(String::as_str).collect();
        ajouter(format!("sans {}", refus.join(", ")), &courant, &mut etapes);
    }
    if f.annee_min.is_some() || f.annee_max.is_some() {
        courant.annee_min = f.annee_min;
        courant.annee_max = f.annee_max;
        let texte = match (f.annee_min, f.annee_max) {
            (Some(a), Some(b)) => format!("{a}–{b}"),
            (Some(a), None) => format!("depuis {a}"),
            (None, Some(b)) => format!("jusqu'en {b}"),
            (None, None) => unreachable!(),
        };
        ajouter(format!("période : {texte}"), &courant, &mut etapes);
    }
    if f.bpm_min.is_some() || f.bpm_max.is_some() {
        courant.bpm_min = f.bpm_min;
        courant.bpm_max = f.bpm_max;
        let texte = match (f.bpm_min, f.bpm_max) {
            (Some(a), Some(b)) => format!("{a:.0}–{b:.0} BPM"),
            (Some(a), None) => format!("≥ {a:.0} BPM"),
            (None, Some(b)) => format!("≤ {b:.0} BPM"),
            (None, None) => unreachable!(),
        };
        ajouter(format!("tempo : {texte}"), &courant, &mut etapes);
    }
    if let Some(e) = f.energie {
        courant.energie = Some(e);
        ajouter(format!("énergie : {}", e.libelle()), &courant, &mut etapes);
    }
    if let Some(p) = f.popularite {
        courant.popularite = Some(p);
        ajouter(format!("popularité : {}", p.libelle()), &courant, &mut etapes);
    }
    etapes
}

/// Lâche le critère désiré le moins essentiel encore posé, dans l'ordre :
/// popularité, énergie, BPM, années, genres. Les exclusions ne sont jamais
/// touchées. `None` quand il n'y a plus rien à lâcher.
fn relacher(f: &mut FiltresPlaylist) -> Option<&'static str> {
    if f.popularite.take().is_some() {
        return Some("popularité");
    }
    if f.energie.take().is_some() {
        return Some("niveau d'énergie");
    }
    if f.bpm_min.take().is_some() | f.bpm_max.take().is_some() {
        return Some("tempo");
    }
    if f.annee_min.take().is_some() | f.annee_max.take().is_some() {
        return Some("période");
    }
    if !f.genres.is_empty() {
        f.genres.clear();
        return Some("genres");
    }
    None
}

/// Les morceaux qui satisfont `filtres`, en lâchant des critères désirés (un à
/// la fois, voir [`relacher`]) tant que moins de `minimum` morceaux
/// conviennent. `None` si `filtres` est vide : aucune restriction, le graphe
/// entier sert.
///
/// Un résultat vide à l'issue de tout cela veut dire que les **exclusions**
/// seules vident la bibliothèque — à l'appelant de le dire.
pub fn selectionner(
    pistes: &[CaracteristiquesPiste],
    filtres: &FiltresPlaylist,
    minimum: usize,
) -> Option<Selection> {
    if filtres.est_vide() {
        return None;
    }
    let mut courant = filtres.clone();
    let mut relaches = Vec::new();
    loop {
        let ids = filtrer(pistes, &courant);
        if ids.len() >= minimum {
            return Some(Selection { ids, relaches });
        }
        match relacher(&mut courant) {
            Some(critere) => relaches.push(format!(
                "critère « {critere} » abandonné : {} morceau(x) admissible(s) pour {minimum} voulus",
                ids.len()
            )),
            None => return Some(Selection { ids, relaches }),
        }
    }
}

/// Durée moyenne (ms) des morceaux de `ids`, ou de toute la bibliothèque si
/// `None`. [`DUREE_DEFAUT_MS`] faute de durée connue.
pub fn duree_moyenne_ms(pistes: &[CaracteristiquesPiste], ids: Option<&HashSet<i64>>) -> i64 {
    let (somme, n) = pistes
        .iter()
        .filter(|p| ids.is_none_or(|s| s.contains(&p.id)))
        .filter_map(|p| p.duree_ms)
        .filter(|d| *d > 0)
        .fold((0i64, 0i64), |(s, n), d| (s + d, n + 1));
    if n == 0 { DUREE_DEFAUT_MS } else { somme / n }
}

/// Retire d'une `route` les morceaux d'un artiste déjà présent `plafond` fois
/// (les `proteges` — départ, arrivée — ne sont jamais retirés). Si `impose`
/// est faux (plafond par défaut, pas demandé) et qu'il reste moins de `voulu`
/// morceaux, le plafond monte d'un cran à la fois jusqu'à y arriver — même
/// relâchement progressif qu'AudioMuse-AI ; un plafond demandé par
/// l'utilisateur, lui, n'est jamais relâché.
pub fn plafonner_par_artiste(
    route: &[i64],
    artiste_de: &HashMap<i64, String>,
    voulu: usize,
    plafond: usize,
    impose: bool,
    proteges: &HashSet<i64>,
) -> Vec<i64> {
    let mut plafond = plafond.max(1);
    loop {
        let mut compte: HashMap<String, usize> = HashMap::new();
        let garde: Vec<i64> = route
            .iter()
            .copied()
            .filter(|id| {
                let Some(a) = artiste_de.get(id) else { return true };
                let n = compte.entry(nom_artiste(a)).or_insert(0);
                if proteges.contains(id) || *n < plafond {
                    *n += 1;
                    true
                } else {
                    false
                }
            })
            .collect();
        if impose || garde.len() >= voulu || garde.len() == route.len() {
            return garde;
        }
        plafond += 1;
    }
}

/// Réduit `route` à la longueur dont la durée cumulée approche le mieux
/// `cible_ms`. Deux familles de candidats : la route **échantillonnée** à n
/// morceaux (`reduire`, typiquement `chemin::echantillonner`) — elle garde les
/// deux extrémités et l'allure du trajet — et, sauf `garder_fin`, ses
/// **préfixes** (les k premiers morceaux de la marche). Les préfixes affinent
/// la durée : avec des morceaux de 4-6 minutes, l'échantillon n'a que de gros
/// pas (mesuré : 9 min pour 12 visées sur une partie « rock »). `garder_fin`
/// quand le dernier morceau est une arrivée nommée, qu'on ne peut pas couper.
/// Une route trop courte pour atteindre la cible est rendue telle quelle : la
/// durée est alors manquée, et l'appelant le signale.
pub fn ajuster_a_duree(
    route: &[i64],
    duree_ms_de: &HashMap<i64, i64>,
    cible_ms: i64,
    reduire: impl Fn(&[i64], usize) -> Vec<i64>,
    garder_fin: bool,
) -> Vec<i64> {
    let total = |r: &[i64]| -> i64 {
        r.iter().map(|id| duree_ms_de.get(id).copied().filter(|d| *d > 0).unwrap_or(DUREE_DEFAUT_MS)).sum()
    };
    let mut meilleur = route.to_vec();
    let mut ecart = (total(&meilleur) - cible_ms).abs();
    let mut essayer = |candidat: Vec<i64>| {
        let e = (total(&candidat) - cible_ms).abs();
        if e < ecart {
            ecart = e;
            meilleur = candidat;
        }
    };
    for n in 2..route.len() {
        essayer(reduire(route, n));
        if !garder_fin {
            essayer(route[..n].to_vec());
        }
    }
    meilleur
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piste(id: i64, artiste: &str, genre: &str, annee: i32, energie: f32) -> CaracteristiquesPiste {
        CaracteristiquesPiste {
            id,
            artiste: Some(artiste.into()),
            annee: Some(annee),
            duree_ms: Some(200_000),
            bpm: Some(100.0 + id as f32),
            energie: Some(energie),
            popularite: Some(id as f64 / 10.0),
            genres: vec![genre.into()],
        }
    }

    /// 9 morceaux, énergies 0.1…0.9 : tiers bas = 0.1-0.3, haut = 0.7-0.9.
    fn corpus() -> Vec<CaracteristiquesPiste> {
        (1..=9)
            .map(|i| {
                let (art, genre, annee) = if i % 2 == 0 {
                    ("The Rockers", "rock", 1975)
                } else {
                    ("Jazz Club", "jazz", 1962)
                };
                piste(i, art, genre, annee, i as f32 / 10.0)
            })
            .collect()
    }

    #[test]
    fn un_filtre_vide_ne_restreint_rien() {
        assert!(selectionner(&corpus(), &FiltresPlaylist::default(), 3).is_none());
    }

    #[test]
    fn calme_prend_le_tiers_bas_de_la_distribution() {
        let f = FiltresPlaylist { energie: Some(NiveauEnergie::Calme), ..Default::default() };
        let s = selectionner(&corpus(), &f, 1).unwrap();
        assert!(s.relaches.is_empty());
        assert!(s.ids.iter().all(|id| *id <= 4), "{:?}", s.ids);
        assert!(s.ids.contains(&1));
    }

    #[test]
    fn exclure_un_genre_est_dur_meme_sans_assez_de_morceaux() {
        let f = FiltresPlaylist { exclure_genres: vec!["Rock".into()], ..Default::default() };
        let s = selectionner(&corpus(), &f, 100).unwrap();
        assert!(s.ids.iter().all(|id| id % 2 == 1), "aucun rock ne doit rester");
        assert!(s.relaches.is_empty(), "une exclusion ne se relâche jamais");
    }

    /// Mesuré sur la vraie bibliothèque : « sans rock » laissait passer
    /// `alternative rock`, `hard rock`… et « sans James Brown » une collaboration.
    #[test]
    fn exclure_un_genre_couvre_sa_famille_pas_les_mots_voisins() {
        assert!(genre_correspond("alternative rock", "rock"));
        assert!(genre_correspond("rock;folk", "rock"));
        assert!(genre_correspond("Hard Rock", "rock"));
        assert!(!genre_correspond("rocksteady", "rock"));
        assert!(!genre_correspond("jazz", "rock"));
        // rap et hip hop sont une même famille, sous toutes leurs graphies.
        for g in ["rap", "hip hop", "hip-hop", "rap/hip hop", "east coast hip hop"] {
            assert!(genre_correspond(g, "rap"), "{g} / rap");
            assert!(genre_correspond(g, "hip hop"), "{g} / hip hop");
        }
        assert!(!genre_correspond("trip hop", "rap"));
        // Une demande composée est une alternative : « sans rap/hip hop » écarte « rap » seul.
        assert!(genre_correspond("rap", "rap/hip hop"));
        assert!(genre_correspond("hip-hop", "rap/hip hop"));
        assert!(!genre_correspond("jazz", "rap/hip hop"));
        // Un genre demandé à plusieurs mots les exige tous.
        assert!(!genre_correspond("hip", "hip hop"));
    }

    #[test]
    fn exclure_un_artiste_couvre_ses_collaborations_pas_ses_homonymes() {
        let mut c = corpus();
        c[0].artiste = Some("James Brown & The Famous Flames".into());
        c[1].artiste = Some("Prince Royce".into());
        c[2].artiste = Some("Prince".into());
        c[3].artiste = Some("Santana feat. Prince".into());
        let f = FiltresPlaylist {
            exclure_artistes: vec!["James Brown".into(), "Prince".into()],
            ..Default::default()
        };
        let s = selectionner(&c, &f, 1).unwrap();
        assert!(!s.ids.contains(&1), "collaboration de James Brown");
        assert!(s.ids.contains(&2), "Prince Royce n'est pas Prince");
        assert!(!s.ids.contains(&3), "Prince");
        assert!(!s.ids.contains(&4), "feat. Prince");
    }

    #[test]
    fn lentonnoir_decroit_et_ne_montre_que_ce_qui_est_demande() {
        let c = corpus(); // 9 morceaux : impairs jazz 1962, pairs rock 1975
        let f = FiltresPlaylist {
            exclure_genres: vec!["rock".into()],
            annee_max: Some(1970),
            energie: Some(NiveauEnergie::Calme),
            ..Default::default()
        };
        let e = entonnoir(&c, &f);
        let libelles: Vec<&str> = e.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(libelles, ["Bibliothèque", "sans rock", "période : jusqu'en 1970", "énergie : calme"]);
        let comptes: Vec<usize> = e.iter().map(|(_, n)| *n).collect();
        assert_eq!(comptes[0], 9);
        assert!(comptes.windows(2).all(|w| w[0] >= w[1]), "{comptes:?}");
        // Cohérent avec la sélection elle-même (sans assouplissement).
        let s = selectionner(&c, &f, 1).unwrap();
        assert_eq!(*comptes.last().unwrap(), s.ids.len());
        // Aucun critère : la bibliothèque seule.
        assert_eq!(entonnoir(&c, &FiltresPlaylist::default()).len(), 1);
    }

    #[test]
    fn exclure_un_artiste_ignore_larticle_et_la_casse() {
        let f = FiltresPlaylist { exclure_artistes: vec!["rockers".into()], ..Default::default() };
        let s = selectionner(&corpus(), &f, 1).unwrap();
        assert!(s.ids.iter().all(|id| id % 2 == 1));
    }

    #[test]
    fn une_borne_dannee_ecarte_les_morceaux_sans_date() {
        let mut c = corpus();
        c[0].annee = None;
        let f = FiltresPlaylist { annee_max: Some(1970), ..Default::default() };
        let s = selectionner(&c, &f, 1).unwrap();
        assert!(!s.ids.contains(&1));
        assert!(s.ids.contains(&3));
    }

    #[test]
    fn relache_dans_lordre_et_le_dit() {
        // Jazz 1962 énergie calme : 1, 3 (0.1, 0.3) → 2 morceaux ; on en veut 5.
        let f = FiltresPlaylist {
            genres: vec!["jazz".into()],
            energie: Some(NiveauEnergie::Calme),
            popularite: Some(NiveauPopularite::Connu),
            ..Default::default()
        };
        let s = selectionner(&corpus(), &f, 5).unwrap();
        assert_eq!(s.relaches.len(), 2, "{:?}", s.relaches);
        assert!(s.relaches[0].contains("popularité"));
        assert!(s.relaches[1].contains("énergie"));
        assert!(s.ids.len() >= 5);
    }

    #[test]
    fn les_exclusions_seules_peuvent_vider_la_bibliotheque() {
        let f = FiltresPlaylist {
            exclure_genres: vec!["rock".into(), "jazz".into()],
            ..Default::default()
        };
        let s = selectionner(&corpus(), &f, 3).unwrap();
        assert!(s.ids.is_empty());
    }

    #[test]
    fn plafond_par_artiste_se_relache_sauf_sil_est_impose() {
        let route: Vec<i64> = (1..=8).collect();
        let artistes: HashMap<i64, String> =
            route.iter().map(|id| (*id, if id % 2 == 0 { "A".into() } else { "B".into() })).collect();
        let aucun = HashSet::new();
        // Plafond par défaut 1, 8 voulus : monte jusqu'à 4 (4 A + 4 B).
        let r = plafonner_par_artiste(&route, &artistes, 8, 1, false, &aucun);
        assert_eq!(r.len(), 8);
        // Plafond imposé : jamais relâché.
        let r = plafonner_par_artiste(&route, &artistes, 8, 2, true, &aucun);
        assert_eq!(r.len(), 4);
        // Un morceau protégé reste même au-delà du plafond.
        let proteges = HashSet::from([8]);
        let r = plafonner_par_artiste(&route, &artistes, 1, 1, true, &proteges);
        assert!(r.contains(&8));
    }

    #[test]
    fn ajuster_a_duree_vise_la_cible() {
        let route: Vec<i64> = (1..=20).collect();
        let durees: HashMap<i64, i64> = route.iter().map(|id| (*id, 180_000)).collect();
        let reduire = |r: &[i64], max: usize| -> Vec<i64> {
            if r.len() <= max || max < 2 {
                return r.to_vec();
            }
            (0..max).map(|i| r[i * (r.len() - 1) / (max - 1)]).collect()
        };
        // 30 min à 3 min le morceau → 10 morceaux.
        let r = ajuster_a_duree(&route, &durees, 30 * 60_000, reduire, true);
        assert_eq!(r.len(), 10);
        assert_eq!(r.first(), Some(&1));
        assert_eq!(r.last(), Some(&20), "les extrémités sont gardées");
        // Cible hors de portée : la route entière.
        let r = ajuster_a_duree(&route, &durees, 600 * 60_000, reduire, true);
        assert_eq!(r.len(), 20);
        // Sans `garder_fin`, les préfixes affinent : des morceaux de durées
        // inégales (4, 5 et 6 min…) tombent plus près de la cible.
        let inegales: HashMap<i64, i64> =
            route.iter().map(|id| (*id, if id % 2 == 0 { 300_000 } else { 240_000 })).collect();
        let cible = 12 * 60_000;
        let ecart = |r: &[i64]| (r.iter().map(|id| inegales[id]).sum::<i64>() - cible).abs();
        let avec = ajuster_a_duree(&route, &inegales, cible, reduire, false);
        let sans = ajuster_a_duree(&route, &inegales, cible, reduire, true);
        assert!(ecart(&avec) <= ecart(&sans), "préfixes : {} contre {}", ecart(&avec), ecart(&sans));
        assert_eq!(avec.first(), Some(&1), "le départ reste");
    }

    #[test]
    fn niveaux_tolerent_les_variantes_de_modele() {
        assert_eq!(NiveauEnergie::depuis_texte(" Calm "), Some(NiveauEnergie::Calme));
        assert_eq!(NiveauEnergie::depuis_texte("énergique"), Some(NiveauEnergie::Intense));
        assert_eq!(NiveauEnergie::depuis_texte("bof"), None);
        assert_eq!(NiveauPopularite::depuis_texte("peu connu"), Some(NiveauPopularite::PeuConnu));
    }

    #[test]
    fn duree_moyenne_retombe_sur_le_defaut() {
        assert_eq!(duree_moyenne_ms(&[], None), DUREE_DEFAUT_MS);
        assert_eq!(duree_moyenne_ms(&corpus(), None), 200_000);
    }
}
