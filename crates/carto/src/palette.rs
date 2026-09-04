//! Les couleurs du **fond de plan** — l'eau, la terre, la voirie, le bâti, les
//! toponymes — **plus les douze teintes de familles** telles qu'elles
//! s'affichent *sur la carte MapLibre* (bâti habité, quartiers, points de
//! morceau).
//!
//! Séparées de [`crate::style`] pour qu'on puisse en tester plusieurs sur le
//! même plan de ville sans toucher à la structure des couches.
//!
//! **La carte seulement.** Le nuage de points t-SNE, la légende et le reste de
//! l'application gardent la palette de familles de `--familles`
//! (`apps/desktop/ui/style.css`) : un fond de carte peut être sombre sans que
//! l'interface le devienne. Ce qui suit ne colore que les tuiles.
//!
//! Les fonds `sepia`, `encre`, `nuit` et `bleu-plan` sont **repris tels quels**
//! des thèmes de [maptoposter](https://github.com/originalankur/maptoposter)
//! (Ankur Gupta, MIT) — respectivement `terracotta`, `japanese_ink`, `noir`,
//! `blueprint`. maptoposter ne peint que fond + eau + parcs + routes par
//! hiérarchie (5 rangs), et c'est aussi ce que dessine `fond_reel` dans
//! `style.rs`. `osm-clair` garde la palette « à la manière d'OSM » d'origine.
//!
//! Le jeu de familles est calé thème par thème (mat et terreux pour `encre`,
//! chaud pour `sepia`, vif pour `nuit`, pastel refroidi pour `bleu-plan`) et se
//! réajuste avec la preview ouverte (`docs/carto-etapes.md`).

/// Les douze teintes vives d'origine (`--familles`, thème sombre de
/// `style.css`). Assez saturées pour tenir aussi bien sur le fond clair
/// d'`osm-clair` que sur les fonds sombres `nuit` / `bleu-plan`.
const FAMILLES_VIVES: [&str; 12] = [
    "#EF8891", "#EC9066", "#D99E46", "#B7AF47", "#88BC6A", "#4EC497", "#0CC3C3", "#38BBE6",
    "#73AEF8", "#A39FF6", "#C892E1", "#E289BD",
];

/// Un jeu de couleurs de fond de plan, familles comprises.
///
/// `id` sert de clé (fichier `style-<id>.json`, bouton d'interface) et
/// d'allowlist : `apps/desktop/src/main.rs::style_carte` n'ouvre un
/// `style-<id>.json` que si `Palette::par_id(id)` existe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub id: &'static str,
    pub nom: &'static str,
    /// Fond **très** sombre (`nuit`) : le bâti en lavis translucide y disparaît
    /// (sombre sur sombre). Sur ce thème le bâti démarre presque opaque
    /// (`style::bati_reel`). `bleu-plan` n'en est pas — son bleu nuit laisse
    /// passer un lavis.
    pub sombre: bool,
    /// L'eau. Sur le monde fictif, le fond sous les continents ; sur le plan de
    /// ville réel, la Seine peinte par-dessus la terre. maptoposter : `water`.
    pub mer: &'static str,
    /// La terre émergée — le fond par défaut du plan de ville réel.
    /// maptoposter : `bg`.
    pub terre: &'static str,
    /// Le trait de côte / la limite communale. Dérivé (pas de clé maptoposter).
    pub cote: &'static str,
    /// Les courbes de niveau et les contours de territoires. Dérivé.
    pub niveau: &'static str,
    /// L'encre des toponymes. maptoposter : `text`.
    pub encre: &'static str,
    /// Le halo des étiquettes — ce qui les rend lisibles sur n'importe quel
    /// fond. Dérivé (≈ `terre`).
    pub halo: &'static str,
    /// Les noms de régions, plus effacés que les lieux. Dérivé.
    pub encre_region: &'static str,
    /// Le réseau de voirie, **cinq rangs**, comme maptoposter
    /// (`get_edge_colors_by_type`) : `road_motorway` / `road_primary` /
    /// `road_secondary` / `road_tertiary` / `road_residential`.
    pub autoroute: &'static str,
    pub nationale: &'static str,
    pub secondaire: &'static str,
    pub tertiaire: &'static str,
    pub residentielle: &'static str,
    /// Liserés du réseau **sonique** (chemin fictif, `routes-lisere`). Le chemin
    /// ville réelle n'a plus de liseré (routes nues, façon maptoposter).
    pub autoroute_lisere: &'static str,
    pub nationale_lisere: &'static str,
    /// Le sentier du réseau **sonique** (chemin fictif, `routes` rang 3+).
    pub sentier: &'static str,
    /// Le bâti — agglomérations du monde fictif, bâti vacant du plan réel — et
    /// sa bordure. Assez marqué pour que la trame de la ville se lise, jamais
    /// au point d'écraser le bâti *habité* (coloré par famille). Dérivé (un ton
    /// entre `terre` et la voirie).
    pub bati: &'static str,
    pub bati_bord: &'static str,
    /// Les cours d'eau — le même bleu que la mer, un peu plus soutenu.
    pub riviere: &'static str,
    /// Les espaces verts du plan réel — bois, parcs. maptoposter : `parks`.
    pub vert: &'static str,
    /// Les douze teintes de familles **sur la carte** : bâti habité
    /// (`batiments-morceaux`), quartiers (`territoires`, `territoires-reels`),
    /// points de morceau (`morceaux-point`). Calées sur le fond ci-dessus.
    pub familles: [&'static str; 12],
    /// Le gris « fourre-tout » : familles au-delà de la douzième, morceaux sans
    /// famille, bâti d'une famille non isolée. Une valeur neutre du même
    /// registre que le fond.
    pub autres: &'static str,
}

impl Palette {
    /// La palette d'origine — « à la manière d'OpenStreetMap », claire. Défaut,
    /// et garantie sans régression sur le fond : ces valeurs étaient les
    /// constantes de `style.rs`. `tertiaire`/`residentielle` interpolent entre
    /// `secondaire` (blanc) et `sentier`.
    pub const fn osm_clair() -> Palette {
        Palette {
            id: "osm-clair",
            nom: "OSM clair",
            sombre: false,
            mer: "#AAD3DF",
            terre: "#F2EFE9",
            cote: "#8FB6C6",
            niveau: "#D8CDB8",
            encre: "#33312C",
            halo: "#F7F5F0",
            encre_region: "#7C6E55",
            autoroute: "#E892A2",
            nationale: "#FCD6A4",
            secondaire: "#FFFFFF",
            tertiaire: "#F4F1EA",
            residentielle: "#E9E4DA",
            autoroute_lisere: "#C1667A",
            nationale_lisere: "#C9A165",
            sentier: "#E9E4DA",
            bati: "#DEDAD2",
            bati_bord: "#C6C0B4",
            riviere: "#8EBEd0",
            vert: "#D3E0C6",
            familles: FAMILLES_VIVES,
            autres: "#6E6656",
        }
    }

    /// `terracotta` de maptoposter : crème méditerranéenne, voirie terre cuite.
    pub const fn sepia() -> Palette {
        Palette {
            id: "sepia",
            nom: "Sépia chaud",
            sombre: false,
            mer: "#A8C4C4",
            terre: "#F5EDE4",
            cote: "#8FB2B2",
            niveau: "#E3D6C6",
            encre: "#8B4513",
            halo: "#F5EDE4",
            encre_region: "#A9744A",
            autoroute: "#A0522D",
            nationale: "#B8653A",
            secondaire: "#C9846A",
            tertiaire: "#D9A08A",
            residentielle: "#E5C4B0",
            autoroute_lisere: "#7E3F1F",
            nationale_lisere: "#8A4526",
            sentier: "#E5C4B0",
            bati: "#EADBC4",
            bati_bord: "#D6C4A5",
            riviere: "#9BB8B8",
            vert: "#E8E0D0",
            familles: [
                "#B24B58", "#B05323", "#9E6300", "#7E7400", "#4C8227", "#00895D", "#00888A",
                "#0080AC", "#3472BE", "#6B63BC", "#8F56A7", "#A74D83",
            ],
            autres: "#8A7A60",
        }
    }

    /// `japanese_ink` de maptoposter : lavis d'encre, gris chauds, un seul accent
    /// rouge (les autoroutes).
    pub const fn encre() -> Palette {
        Palette {
            id: "encre",
            nom: "Encre",
            sombre: false,
            mer: "#E8E4E0",
            terre: "#FAF8F5",
            cote: "#C9C4BD",
            niveau: "#E4DED5",
            encre: "#2C2C2C",
            halo: "#FAF8F5",
            encre_region: "#7A756C",
            autoroute: "#8B2500",
            nationale: "#4A4A4A",
            secondaire: "#6A6A6A",
            tertiaire: "#909090",
            residentielle: "#B8B8B8",
            autoroute_lisere: "#5E1900",
            nationale_lisere: "#333333",
            sentier: "#B8B8B8",
            bati: "#ECE6DB",
            bati_bord: "#D2CABB",
            riviere: "#E0DBD5",
            vert: "#F0EDE8",
            familles: [
                "#B06A70", "#AE7455", "#A08346", "#7C8347", "#579156", "#2E9480", "#33908F",
                "#4C86A0", "#6981AC", "#7D74A6", "#94709E", "#B06C94",
            ],
            autres: "#8C867A",
        }
    }

    /// `noir` de maptoposter : fond noir, voirie claire — l'esthétique « galerie ».
    /// L'eau (`#0A0A0A` sur `#000000`) y est presque invisible, comme dans le
    /// thème d'origine.
    pub const fn nuit() -> Palette {
        Palette {
            id: "nuit",
            nom: "Nuit",
            sombre: true,
            mer: "#0A0A0A",
            terre: "#000000",
            cote: "#333333",
            niveau: "#222222",
            encre: "#FFFFFF",
            halo: "#000000",
            encre_region: "#9A9A9A",
            autoroute: "#FFFFFF",
            nationale: "#E0E0E0",
            secondaire: "#B0B0B0",
            tertiaire: "#808080",
            residentielle: "#505050",
            autoroute_lisere: "#9A9A9A",
            nationale_lisere: "#7A7A7A",
            sentier: "#505050",
            bati: "#333333",
            bati_bord: "#414141",
            riviere: "#141414",
            vert: "#111111",
            familles: FAMILLES_VIVES,
            autres: "#8C8C90",
        }
    }

    /// `blueprint` de maptoposter : fond bleu nuit, traits bleu pâle — « calque
    /// d'architecte ».
    pub const fn bleu_plan() -> Palette {
        Palette {
            id: "bleu-plan",
            nom: "Bleu plan",
            sombre: false,
            mer: "#0F2840",
            terre: "#1A3A5C",
            cote: "#4E7BA0",
            niveau: "#2C517A",
            encre: "#E8F4FF",
            halo: "#1A3A5C",
            encre_region: "#9FC5E8",
            autoroute: "#E8F4FF",
            nationale: "#C5DCF0",
            secondaire: "#9FC5E8",
            tertiaire: "#7BAED4",
            residentielle: "#5A96C0",
            autoroute_lisere: "#9FC5E8",
            nationale_lisere: "#7BAED4",
            sentier: "#5A96C0",
            bati: "#234870",
            bati_bord: "#3C6498",
            riviere: "#0F2840",
            vert: "#1E4570",
            familles: [
                "#E6A6AD", "#E3AB8A", "#D4B47C", "#BCC088", "#9CCA94", "#74CDB4", "#6CC9D2",
                "#86C6E2", "#A2C8F5", "#C0BEF5", "#D6B0E7", "#E6ADD0",
            ],
            autres: "#7C93AE",
        }
    }

    /// Toutes les palettes, `osm-clair` en tête (c'est celle du `style.json`
    /// sans suffixe).
    pub fn toutes() -> &'static [Palette] {
        &TOUTES
    }

    /// La palette d'`id` donné, ou `None`. Sert d'allowlist côté desktop.
    pub fn par_id(id: &str) -> Option<&'static Palette> {
        TOUTES.iter().find(|p| p.id == id)
    }
}

static TOUTES: [Palette; 5] = [
    Palette::osm_clair(),
    Palette::sepia(),
    Palette::encre(),
    Palette::nuit(),
    Palette::bleu_plan(),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osm_clair_est_la_premiere() {
        assert_eq!(Palette::toutes()[0].id, "osm-clair");
    }

    #[test]
    fn les_identifiants_sont_uniques() {
        let mut ids: Vec<&str> = Palette::toutes().iter().map(|p| p.id).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "deux palettes partagent un id");
    }

    #[test]
    fn par_id_retrouve_chaque_palette_et_rejette_le_reste() {
        for p in Palette::toutes() {
            assert_eq!(Palette::par_id(p.id).map(|q| q.id), Some(p.id));
        }
        assert!(Palette::par_id("../style").is_none());
        assert!(Palette::par_id("").is_none());
        assert!(Palette::par_id("inconnu").is_none());
    }

    /// Garde-fou anti-dérive : `osm-clair` garde le fond d'origine de `style.rs`.
    #[test]
    fn osm_clair_reprend_les_anciennes_constantes() {
        let p = Palette::osm_clair();
        assert_eq!(p.mer, "#AAD3DF");
        assert_eq!(p.terre, "#F2EFE9");
        assert_eq!(p.autoroute, "#E892A2");
        assert_eq!(p.secondaire, "#FFFFFF");
        assert_eq!(p.bati, "#DEDAD2");
        assert_eq!(p.familles[0], "#EF8891");
        assert_eq!(p.autres, "#6E6656");
    }

    /// Les quatre fonds portés reprennent exactement le `bg` de leur thème
    /// maptoposter (terracotta / japanese_ink / noir / blueprint).
    #[test]
    fn les_fonds_portes_reprennent_maptoposter() {
        assert_eq!(Palette::sepia().terre, "#F5EDE4"); // terracotta bg
        assert_eq!(Palette::sepia().autoroute, "#A0522D"); // road_motorway
        assert_eq!(Palette::encre().terre, "#FAF8F5"); // japanese_ink bg
        assert_eq!(Palette::nuit().terre, "#000000"); // noir bg
        assert_eq!(Palette::bleu_plan().terre, "#1A3A5C"); // blueprint bg
        assert_eq!(Palette::bleu_plan().residentielle, "#5A96C0"); // road_residential
    }

    #[test]
    fn chaque_palette_a_douze_familles_en_hex() {
        for p in Palette::toutes() {
            assert_eq!(p.familles.len(), 12, "{}", p.id);
            for c in p.familles.iter().chain(std::iter::once(&p.autres)).chain([
                &p.mer, &p.terre, &p.cote, &p.niveau, &p.encre, &p.halo, &p.encre_region,
                &p.autoroute, &p.nationale, &p.secondaire, &p.tertiaire, &p.residentielle,
                &p.autoroute_lisere, &p.nationale_lisere, &p.sentier,
                &p.bati, &p.bati_bord, &p.riviere, &p.vert,
            ]) {
                assert!(
                    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|d| d.is_ascii_hexdigit()),
                    "{} : couleur invalide {c:?}", p.id
                );
            }
        }
    }
}
