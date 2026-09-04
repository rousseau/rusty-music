// SPDX-License-Identifier: GPL-3.0-or-later
//! Le style MapLibre, engendré depuis Rust.
//!
//! **Pourquoi ne pas l'écrire à la main en JSON.** Le style et les tuiles
//! partagent une seule vérité : les paliers de zoom. Ce qui n'est pas dans la
//! tuile ne peut pas s'afficher, et une couche déclarée à un zoom où sa tuile
//! est vide ne montre rien — sans que rien ne signale l'erreur. En dérivant le
//! style des mêmes [`Paliers`](crate::tuiles::Paliers), les deux ne peuvent
//! plus diverger.
//!
//! La révélation par échelle est ici, et c'est le cœur du rendu :
//!
//! | Zoom | Ce qu'on voit |
//! |---|---|
//! | 0-2 | le relief, les territoires, les noms de familles — les continents |
//! | 3-5 | les territoires se précisent, les villes apparaissent |
//! | 6-8 | les morceaux se posent, les familles s'effacent |
//! | 9+ | les titres, un par point |

use serde_json::{json, Value};

use crate::palette::Palette;
use crate::source::Source;
use crate::tuiles::Paliers;

// Les douze teintes de familles — et le gris « fourre-tout » — vivent
// désormais dans [`Palette`] : elles se calent sur le fond de plan (matées
// pour `encre`, vives sur `nuit`…). `Palette::osm_clair` reprend le jeu de
// `--familles` (thème sombre de `apps/desktop/ui/style.css`), inchangé, que le
// nuage de points et la légende continuent d'utiliser côté interface.

// Les couleurs du **fond de plan** — eau, terre, voirie, bâti, toponymes —
// vivent dans [`crate::palette`], une par [`Palette`]. La palette par défaut
// (`Palette::osm_clair`) reprend la palette **claire, à la manière
// d'OpenStreetMap** d'origine : fond clair (terre crème, eau bleu pâle), la
// couleur rare et porteuse de sens (les régions ne sont que des lavis pâles),
// et le réseau qui domine le graphisme parce qu'il est le seul élément
// vraiment coloré. Les autres palettes (`sepia`, `encre`, `nuit`, `bleu-plan`)
// sont portées de maptoposter et ne changent que ce fond — jamais l'overlay
// de familles ci-dessous.

/// Construit le style complet.
///
/// `base` est le préfixe des tuiles (`tuiles://tuiles` — intercepté côté JS par
/// `maplibregl.addProtocol`, l'hôte est arbitraire). `palette` choisit le fond
/// de plan ; l'overlay de familles n'en dépend pas.
pub fn construire(source: &Source, paliers: &Paliers, base: &str, palette: &Palette) -> Value {
    let mut sources = serde_json::Map::new();
    sources.insert(
        "carte".into(),
        json!({
            "type": "vector",
            "tiles": [format!("{base}/carte/{{z}}/{{x}}/{{y}}")],
            "minzoom": 0,
            "maxzoom": paliers.zoom_max,
        }),
    );
    // Le relief n'a de sens que sur le monde fictif : c'est l'ombrage de la
    // nappe de densité traitée comme un modèle numérique de terrain. Paris
    // est plat, et `crate::ville::rassembler` n'engendre aucun champ à
    // ombrer — déclarer la source sans rien pour la servir ne ferait
    // qu'échouer en silence à chaque tuile demandée.
    if !source.est_ville_reelle() {
        sources.insert(
            "relief".into(),
            json!({
                "type": "raster",
                "tiles": [format!("{base}/relief/{{z}}/{{x}}/{{y}}")],
                "tileSize": 512,
                "minzoom": 0,
                "maxzoom": 3,
            }),
        );
    }
    let mut style = json!({
        "version": 8,
        "name": "Rusty Music",
        "glyphs": "vendor/glyphes/{fontstack}/{range}.pbf",
        "sources": sources,
        "layers": couches(source, paliers, palette),
    });
    // Sur le plan de ville réel, la caméra doit s'ouvrir sur Paris — sans
    // cela `apps/desktop/ui/app.js` retombe sur le centre du monde fictif
    // (`[0, 0]`), à des milliers de kilomètres, et la carte réelle générée
    // paraît vide (rien à cette échelle avant le zoom d'une avenue).
    if let Some((ouest, sud, est, nord)) = crate::tuiles::bbox_reelle(source) {
        style["center"] = json!([(ouest + est) / 2.0, (sud + nord) / 2.0]);
        // L'emprise de la limite communale : le bouton « Vue d'ensemble » de
        // l'interface y ramène la caméra (`fitBounds`). Prise sur la frontière
        // réelle, elle ne dépend pas d'éventuels morceaux mal géocodés. Dans
        // `metadata` pour que le moteur de rendu n'y touche pas — c'est une
        // valeur pour l'interface, pas une contrainte de caméra.
        style["metadata"] = json!({ "rusty:bounds": [ouest, sud, est, nord] });
        // À 12 (calibrage initial), la vue d'ouverture tombait sous
        // `paliers.artistes_des`/`morceaux_des` : la carte s'ouvrait sans le
        // moindre morceau ni artiste visible, comme si la bibliothèque
        // n'avait pas été posée sur le plan. 14 tombe juste après les deux
        // seuils par défaut de `Paliers::ville()` — toujours à l'échelle du
        // quartier, mais avec quelque chose de la bibliothèque déjà visible
        // à l'ouverture. Non mesuré à l'œil, à ajuster si ça cadre mal.
        style["zoom"] = json!(14.0);
    }
    style
}

fn couleur_famille(source: &Source, pal: &Palette) -> Value {
    couleur_famille_champ(source, "famille", pal, pal.autres)
}

/// Comme [`couleur_famille`], mais lit un champ MVT arbitraire plutôt que
/// `"famille"` et retombe sur `defaut` plutôt que le gris « autres » du
/// territoire fourre-tout. Sert à colorer un bâtiment habité par la famille
/// de son occupant : cette famille voyage dans le tag `palier`, réutilisé
/// (voir `tuiles::Anneau::palier` et son commentaire), pas dans `famille`
/// (qui, pour tout bâtiment, vaut la constante interne `FAMILLE_BATIMENT_REEL`).
fn couleur_famille_champ(source: &Source, champ: &str, pal: &Palette, defaut: &str) -> Value {
    let mut expr = vec![json!("match"), json!(["get", champ])];
    for f in &source.familles {
        if f.id >= 0 {
            expr.push(json!(f.id));
            expr.push(json!(pal.familles[(f.id as usize) % pal.familles.len()]));
        }
    }
    expr.push(json!(defaut));
    Value::Array(expr)
}

/// Les couches, l'un ou l'autre chemin.
///
/// Le **plan de ville réel** ([`couches_ville`]) est reconstruit couche par
/// couche, façon maptoposter : fond nu (eau, parcs, routes par hiérarchie),
/// puis le bâti, puis l'overlay musical. Le **monde fictif** ([`couches_fictif`])
/// est inchangé — il fonctionne (`docs/carto-etapes.md`).
fn couches(source: &Source, p: &Paliers, pal: &Palette) -> Vec<Value> {
    if source.est_ville_reelle() {
        couches_ville(source, p, pal)
    } else {
        couches_fictif(source, p, pal)
    }
}

fn couches_fictif(source: &Source, p: &Paliers, pal: &Palette) -> Vec<Value> {
    let couleur = couleur_famille(source, pal);
    let zmax = p.zoom_max as f64;
    let terr_max = (p.territoires_jusqu_a + 3).min(p.zoom_max) as f64;
    let reel = source.est_ville_reelle();
    // La bande où les artistes s'étagent par rang, entre leur première
    // apparition et celle des morceaux (`tuiles::rang_artiste`) — en
    // fraction de cette bande plutôt qu'en incréments fixes, pour que la
    // même formule tienne sur les deux pyramides très différentes du monde
    // fictif (`Paliers::default`, zoom_max 9) et de Paris (`Paliers::ville`,
    // zoom_max 17). Bornée à 3 crans au minimum pour garder quatre paliers
    // distincts même si les deux seuils sont proches.
    let bande_artistes = (p.morceaux_des as f64 - p.artistes_des as f64).max(3.0);
    let art_b2 = p.artistes_des as f64 + bande_artistes / 3.0;
    let art_b3 = p.artistes_des as f64 + bande_artistes * 2.0 / 3.0;
    let art_b4 = p.artistes_des as f64 + bande_artistes;

    // La **pastille** d'artiste (le disque gris) : sur le plan de ville réel,
    // elle n'apparaît qu'une fois entré dans un quartier (`art_b3`), pas dès
    // qu'on voit Paris entier — l'aplat de quartier suffit au repérage de
    // loin, et le nom seul (`artistes-etiquette`, inchangé) l'accompagne. Sur
    // le monde fictif, elle garde son apparition étagée dès `artistes_des`.
    let pt_artiste_des = if reel { art_b3 } else { p.artistes_des as f64 };
    let pt_artiste_opacite: Value = if reel {
        json!(["interpolate", ["linear"], ["zoom"], art_b3, 0.0, art_b4, 0.6])
    } else {
        json!(["interpolate", ["linear"], ["zoom"],
            p.artistes_des as f64, ["match", ["get", "rang"], 3, 0.75, 0.0],
            art_b2, ["match", ["get", "rang"], 3, 0.75, 2, 0.7, 0.0],
            art_b3, ["match", ["get", "rang"], 3, 0.75, 2, 0.7, 1, 0.6, 0.0],
            art_b4, 0.6])
    };

    let mut couches = if !reel {
        vec![
            // La mer. Une carte se lit d'abord parce que la terre s'arrête
            // quelque part : sans cette couleur-là, tout le reste flotte.
            json!({ "id": "mer", "type": "background", "paint": { "background-color": pal.mer } }),
            // La terre : la nappe **globale**, celle qui n'appartient à aucune
            // famille. Elle était calculée et jetée.
            json!({
                "id": "terre",
                "type": "fill",
                "source": "carte",
                "source-layer": "cotes",
                // **Pas de filtre sur le palier.** Les bandes d'isovaleur sont
                // des anneaux emboîtés, pas des disques : n'en garder qu'un
                // laissait la mer transparaître au milieu des continents.
                // Empilées d'une seule teinte, elles pavent la terre sans se
                // recouvrir.
                "paint": { "fill-color": pal.terre, "fill-antialias": true }
            }),
            json!({
                "id": "relief",
                "type": "raster",
                "source": "relief",
                "paint": {
                    // Sur fond clair, l'ombrage doit rester un soupçon :
                    // au-delà, il grise la carte et mange les routes.
                    "raster-opacity": ["interpolate", ["linear"], ["zoom"],
                        0, 0.30, 4, 0.24, 7, 0.14, zmax, 0.08],
                    "raster-fade-duration": 120
                }
            }),
        ]
    } else {
        // Sur le plan de ville réel, la terre est le fond par défaut — pas
        // une nappe à remplir : `eaux-reelles` peint l'eau **par-dessus**
        // (la Seine), dans l'autre sens que le monde fictif (mer dessous,
        // terre dessus). Ni nappe de densité ni relief à ombrer : Paris est
        // plat, et `crate::ville::rassembler` n'en engendre aucun.
        vec![json!({ "id": "terre-reelle", "type": "background", "paint": { "background-color": pal.terre } })]
    };

    // Le reste des couches s'applique tel quel aux deux chemins : sur le plan
    // de ville réel, `territoires`/`cote`/`agglomerations`/`routes`
    // (network sonique) visent des couches absentes des tuiles — MapLibre
    // les affiche silencieusement vides, comme n'importe quelle couche sans
    // contenu à ce zoom. Seul `relief` (au-dessus) devait être retiré : lui
    // n'a pas de couche vide, il a une **source** absente, ce qui échoue.
    couches.extend(vec![
        // Les territoires : **un aplat**, pas sept bandes translucides. Sept
        // paliers empilés se lisaient comme une carte météo ; une carte veut
        // une région d'une seule couleur et une bordure nette.
        json!({
            "id": "territoires",
            "type": "fill",
            "source": "carte",
            "source-layer": "territoires",
            "maxzoom": terr_max,
            "paint": {
                "fill-color": couleur,
                // Un **lavis**, pas un aplat : sur fond clair, 18 % suffisent à
                // distinguer une région de sa voisine. Au-delà, la couleur
                // reprend le dessus et l'on retombe sur la carte de données.
                "fill-opacity": ["interpolate", ["linear"], ["zoom"], 0, 0.22, 6, 0.17, 9, 0.11],
                "fill-antialias": true
            }
        }),
        json!({
            "id": "territoires-contour",
            "type": "line",
            "source": "carte",
            "source-layer": "territoires",
            "filter": ["==", ["get", "palier"], 0],
            "maxzoom": terr_max,
            "paint": {
                "line-color": pal.niveau,
                "line-opacity": ["interpolate", ["linear"], ["zoom"], 0, 0.55, 4, 0.7, 8, 0.8],
                "line-width": ["interpolate", ["linear"], ["zoom"], 0, 0.4, 6, 0.9]
            }
        }),
        // **Les quartiers musicaux du plan de ville réel** — l'aplat par
        // famille du diagramme de puissance de l'étage 1
        // (`FAMILLE_TERRITOIRE_REEL`, famille dans `palier`). C'est la seule
        // information de genre visible quand on dézoome pour voir Paris
        // entier : sans elle, il ne reste que les axes et l'eau. Un lavis un
        // peu plus soutenu que celui du monde fictif — il n'a pas de relief ni
        // de nappe de densité pour l'épauler — qui s'efface quand les
        // bâtiments individuels colorés prennent le relais (`morceaux_des`).
        json!({
            "id": "territoires-reels",
            "type": "fill",
            "source": "carte",
            "source-layer": "territoires-reels",
            "maxzoom": p.morceaux_des as f64 + 1.0,
            "paint": {
                "fill-color": couleur_famille_champ(source, "palier", pal, pal.autres),
                "fill-opacity": ["interpolate", ["linear"], ["zoom"],
                    (p.territoires_jusqu_a as f64) - 4.0, 0.34,
                    (p.territoires_jusqu_a as f64) - 1.0, 0.22,
                    p.territoires_jusqu_a as f64, 0.12,
                    p.morceaux_des as f64 + 1.0, 0.0],
                "fill-antialias": true
            }
        }),
        json!({
            "id": "territoires-reels-contour",
            "type": "line",
            "source": "carte",
            "source-layer": "territoires-reels",
            "maxzoom": p.morceaux_des as f64 + 1.0,
            "paint": {
                "line-color": pal.niveau,
                "line-opacity": ["interpolate", ["linear"], ["zoom"],
                    (p.territoires_jusqu_a as f64) - 4.0, 0.5,
                    p.territoires_jusqu_a as f64, 0.32,
                    p.morceaux_des as f64 + 1.0, 0.0],
                "line-width": ["interpolate", ["linear"], ["zoom"], 8, 0.5, 13, 1.1]
            }
        }),
        // **Les agglomérations.** Le premier repère d'un plan : une tache
        // grise qui tranche sur la campagne. Le palier porte le rang, ce qui
        // les fait apparaître de la métropole à la ferme au fil du zoom.
        json!({
            "id": "agglomerations",
            "type": "fill",
            "source": "carte",
            "source-layer": "agglomerations",
            "paint": {
                "fill-color": pal.bati,
                "fill-opacity": ["interpolate", ["linear"], ["zoom"],
                    2, ["match", ["get", "palier"], 5, 0.9, 0.0],
                    4, ["match", ["get", "palier"], 5, 0.9, 4, 0.9, 0.0],
                    5.5, ["match", ["get", "palier"], 5, 0.9, 4, 0.9, 3, 0.9, 0.0],
                    7, ["match", ["get", "palier"], 0, 0.0, 1, 0.0, 0.9],
                    9, 0.9],
                "fill-antialias": true
            }
        }),
        json!({
            "id": "agglomerations-bord",
            "type": "line",
            "source": "carte",
            "source-layer": "agglomerations",
            "paint": {
                "line-color": pal.bati_bord,
                "line-opacity": ["interpolate", ["linear"], ["zoom"],
                    2, ["match", ["get", "palier"], 5, 0.8, 0.0],
                    4, ["match", ["get", "palier"], 5, 0.8, 4, 0.8, 0.0],
                    5.5, ["match", ["get", "palier"], 5, 0.8, 4, 0.8, 3, 0.8, 0.0],
                    7, ["match", ["get", "palier"], 0, 0.0, 1, 0.0, 0.8],
                    9, 0.8],
                "line-width": ["interpolate", ["linear"], ["zoom"], 2, 0.6, zmax, 1.4]
            }
        }),
        // Le trait de côte, par-dessus le relief : c'est le repère principal.
        json!({
            "id": "cote",
            "type": "line",
            "source": "carte",
            "source-layer": "cotes",
            "filter": ["==", ["get", "palier"], 0],
            "paint": {
                "line-color": pal.cote,
                "line-width": ["interpolate", ["linear"], ["zoom"], 0, 0.8, 5, 1.4, zmax, 2.0]
            }
        }),
        // **Les cours d'eau.** Une carte se reconnaît à ses rivières autant
        // qu'à ses routes : elles donnent au relief une lecture immédiate — on
        // voit où l'eau va, donc la forme du terrain — et cassent la
        // régularité des nappes de densité.
        //
        // Sous les routes : un pont passe au-dessus de l'eau.
        json!({
            "id": "rivieres",
            "type": "line",
            "source": "carte",
            "source-layer": "rivieres",
            "layout": { "line-cap": "round", "line-join": "round" },
            "paint": {
                "line-color": pal.riviere,
                "line-opacity": ["interpolate", ["linear"], ["zoom"],
                    0, ["match", ["get", "classe"], 0, 0.9, 0.0],
                    3, ["match", ["get", "classe"], 0, 0.9, 1, 0.7, 0.0],
                    5, 0.85,
                    zmax, 0.9],
                "line-width": ["interpolate", ["linear"], ["zoom"],
                    2, ["match", ["get", "classe"], 0, 1.4, 1, 0.9, 0.5],
                    6, ["match", ["get", "classe"], 0, 3.2, 1, 2.0, 1.1],
                    zmax, ["match", ["get", "classe"], 0, 6.0, 1, 3.6, 2.0]]
            }
        }),
        // Le réseau : liseré sombre dessous, chaussée dessus. C'est ce liseré
        // qui fait qu'une route se lit comme une route et non comme un trait.
        json!({
            "id": "routes-lisere",
            "type": "line",
            "source": "carte",
            "source-layer": "routes",
            "minzoom": 2.0,
            "layout": { "line-cap": "round", "line-join": "round" },
            "paint": {
                "line-color": ["match", ["get", "classe"],
                    0, pal.autoroute_lisere, 1, pal.nationale_lisere, "#C8C2B6"],
                // **L'opacité du liseré doit suivre celle de la chaussée.**
                // Fixée à 0,7, il dessinait les nationales en noir aux zooms
                // où la route, elle, était masquée : la carte se couvrait de
                // rayures sombres sans qu'aucune route n'apparaisse.
                "line-opacity": ["interpolate", ["linear"], ["zoom"],
                    0, ["match", ["get", "classe"], 0, 1.0, 0.0],
                    3, ["match", ["get", "classe"], 0, 1.0, 0.0],
                    5, ["match", ["get", "classe"], 0, 1.0, 1, 0.9, 0.0],
                    zmax, ["match", ["get", "classe"], 0, 1.0, 1, 0.9, 0.0]],
                // Un liseré n'est pas un second trait : il déborde de peu.
                "line-width": ["interpolate", ["linear"], ["zoom"],
                    2, ["match", ["get", "classe"], 0, 3.4, 2.2],
                    6, ["match", ["get", "classe"], 0, 7.0, 1, 4.6, 2.6],
                    zmax, ["match", ["get", "classe"], 0, 13.0, 1, 8.5, 4.6]]
            }
        }),
        json!({
            "id": "routes",
            "type": "line",
            "source": "carte",
            "source-layer": "routes",
            "layout": { "line-cap": "round", "line-join": "round" },
            "paint": {
                "line-color": ["match", ["get", "classe"],
                    0, pal.autoroute, 1, pal.nationale, 2, pal.secondaire, pal.sentier],
                // Les nationales n'apparaissent qu'à mi-distance. Six mille
                // tronçons courts au planisphère faisaient des hachures, pas
                // un réseau : à cette échelle, une carte routière ne montre
                // que ses autoroutes.
                "line-opacity": ["interpolate", ["linear"], ["zoom"],
                    0, ["match", ["get", "classe"], 0, 0.95, 0.0],
                    3, ["match", ["get", "classe"], 0, 0.95, 0.0],
                    5, ["match", ["get", "classe"], 0, 0.95, 1, 0.55, 0.3],
                    zmax, ["match", ["get", "classe"], 0, 0.9, 1, 0.8, 0.45]],
                // **C'est l'épaisseur qui dit le rang**, avant la couleur : sur
                // un plan, on suit une route large sans avoir à la nommer. Le
                // rapport entre autoroute et route secondaire est de 1 à 4.
                "line-width": ["interpolate", ["linear"], ["zoom"],
                    2, ["match", ["get", "classe"], 0, 1.8, 1, 1.0, 0.5],
                    6, ["match", ["get", "classe"], 0, 4.6, 1, 2.8, 2, 1.4, 0.8],
                    zmax, ["match", ["get", "classe"], 0, 9.0, 1, 5.5, 2, 2.6, 1.4]]
            }
        }),
        // **Les établissements.** Six rangs, six symboles, six seuils de zoom :
        // c'est cette hiérarchie qui produit l'impression de carte d'état-major
        // et qui donne au regard où se poser à chaque échelle.
        //
        // Le rang est un entier de 0 (ferme) à 5 (métropole) ; l'opacité le
        // fait apparaître au bon moment plutôt qu'une couche par rang, ce qui
        // laisse MapLibre arbitrer les collisions sur un seul jeu de symboles.
        json!({
            "id": "etablissements",
            "type": "circle",
            "source": "carte",
            "source-layer": "etablissements",
            "paint": {
                "circle-radius": ["interpolate", ["linear"], ["zoom"],
                    2, ["match", ["get", "rang"], 5, 5.0, 4, 3.0, 3, 2.0, 1.2],
                    6, ["match", ["get", "rang"], 5, 8.0, 4, 5.5, 3, 4.0, 2, 3.0, 2.0],
                    zmax, ["match", ["get", "rang"], 5, 11.0, 4, 8.0, 3, 6.0, 2, 4.5, 1, 3.5, 2.5]],
                "circle-color": "#FFFFFF",
                "circle-opacity": ["interpolate", ["linear"], ["zoom"],
                    2, ["match", ["get", "rang"], 5, 1.0, 0.0],
                    4, ["match", ["get", "rang"], 5, 1.0, 4, 1.0, 0.0],
                    5.5, ["match", ["get", "rang"], 5, 1.0, 4, 1.0, 3, 1.0, 0.0],
                    7, ["match", ["get", "rang"], 5, 1.0, 4, 1.0, 3, 1.0, 2, 1.0, 0.0],
                    8.5, ["match", ["get", "rang"], 1, 1.0, 0, 0.0, 1.0],
                    10, 1.0],
                // **`circle-opacity` ne gouverne que le remplissage.** Le
                // contour a la sienne, qui vaut 1 par défaut : sans cette
                // ligne, les 757 établissements dessinaient leur cerne à tous
                // les zooms — un semis de petits ronds jusque sur la mer,
                // alors que leur disque, lui, était bien invisible.
                "circle-stroke-opacity": ["interpolate", ["linear"], ["zoom"],
                    2, ["match", ["get", "rang"], 5, 1.0, 0.0],
                    4, ["match", ["get", "rang"], 5, 1.0, 4, 1.0, 0.0],
                    5.5, ["match", ["get", "rang"], 5, 1.0, 4, 1.0, 3, 1.0, 0.0],
                    7, ["match", ["get", "rang"], 5, 1.0, 4, 1.0, 3, 1.0, 2, 1.0, 0.0],
                    8.5, ["match", ["get", "rang"], 1, 1.0, 0, 0.0, 1.0],
                    10, 1.0],
                // Cercle blanc cerné de sombre : le symbole de lieu le plus
                // universel, et le seul qui reste lisible sur tous les fonds.
                "circle-stroke-color": "#5A564E",
                "circle-stroke-width": ["interpolate", ["linear"], ["zoom"],
                    2, 1.0, zmax, 2.2]
            }
        }),
        json!({
            "id": "etablissements-etiquette",
            "type": "symbol",
            "source": "carte",
            "source-layer": "etablissements",
            "layout": {
                "text-field": ["get", "nom"],
                // Les métropoles en gras et en capitales, comme les capitales
                // d'une carte routière ; le reste en romain.
                "text-font": ["case", [">=", ["get", "rang"], 4],
                    ["literal", ["Noto Sans Bold"]], ["literal", ["Noto Sans Regular"]]],
                "text-transform": ["case", ["==", ["get", "rang"], 5], "uppercase", "none"],
                "text-letter-spacing": ["case", ["==", ["get", "rang"], 5], 0.12, 0.0],
                "text-size": ["interpolate", ["linear"], ["zoom"],
                    2, ["match", ["get", "rang"], 5, 13.0, 4, 11.0, 9.0],
                    6, ["match", ["get", "rang"], 5, 17.0, 4, 14.0, 3, 12.0, 2, 11.0, 10.0],
                    zmax, ["match", ["get", "rang"], 5, 21.0, 4, 17.0, 3, 14.0, 2, 12.0, 11.0]],
                "text-anchor": "top",
                "text-offset": [0, 0.65],
                "text-max-width": 9,
                "text-padding": 4,
                // Les grands d'abord : MapLibre évince les autres tout seul.
                "symbol-sort-key": ["-", 0, ["*", 1000, ["get", "rang"]]]
            },
            "paint": {
                "text-color": pal.encre,
                "text-halo-color": pal.halo,
                "text-halo-width": 1.8,
                "text-opacity": ["interpolate", ["linear"], ["zoom"],
                    2, ["match", ["get", "rang"], 5, 1.0, 0.0],
                    4, ["match", ["get", "rang"], 5, 1.0, 4, 1.0, 0.0],
                    5.5, ["match", ["get", "rang"], 5, 1.0, 4, 1.0, 3, 1.0, 0.0],
                    7, ["match", ["get", "rang"], 5, 1.0, 4, 1.0, 3, 1.0, 2, 1.0, 0.0],
                    8.5, ["match", ["get", "rang"], 0, 0.0, 1.0],
                    10, 1.0]
            }
        }),
        // **Les artistes, étagés par rang** (`tuiles::rang_artiste`, 3 le
        // plus prolifique à 0 le commun) — même principe que les
        // établissements ci-dessus, mais par quantile plutôt que par seuil
        // de population fixe : personne ne suppose de monde engendré ici.
        // Recalibré pour Paris (`Paliers::ville`, voir son commentaire) —
        // à `artistes_des + 4`/`+5`, hérité du monde fictif, un artiste ne
        // se voyait quasiment jamais (zoom 17/18 sur une pyramide qui
        // culmine à 17).
        json!({
            "id": "artistes-point",
            "type": "circle",
            "source": "carte",
            "source-layer": "artistes",
            "minzoom": pt_artiste_des,
            "paint": {
                "circle-radius": ["interpolate", ["linear"], ["zoom"],
                    pt_artiste_des, ["match", ["get", "rang"], 3, 2.4, 2.0],
                    art_b4, ["match", ["get", "rang"], 3, 5.0, 2, 4.0, 1, 3.2, 2.4],
                    zmax, ["match", ["get", "rang"], 3, 7.0, 2, 6.0, 1, 5.0, 4.0]],
                "circle-color": "#8A8478",
                "circle-opacity": pt_artiste_opacite,
                "circle-stroke-width": 0
            }
        }),
        // Les noms de familles : les « pays ». Grands, espacés, en capitales.
        json!({
            "id": "familles-etiquette",
            "type": "symbol",
            "source": "carte",
            "source-layer": "familles",
            "maxzoom": (p.familles_jusqu_a + 1) as f64,
            "layout": {
                "text-field": ["get", "nom"],
                "text-font": ["Noto Sans Bold"],
                "text-transform": "uppercase",
                "text-letter-spacing": 0.22,
                "text-size": ["interpolate", ["linear"], ["zoom"], 0, 11, 3, 17, 6, 24],
                "text-max-width": 8,
                "text-padding": 6,
                "symbol-sort-key": ["-", 0, ["get", "effectif"]]
            },
            "paint": {
                "text-color": pal.encre_region,
                "text-halo-color": pal.halo,
                "text-halo-width": 2.0,
                "text-opacity": ["interpolate", ["linear"], ["zoom"],
                    0, 0.95, (p.familles_jusqu_a as f64) - 1.0, 0.85, p.familles_jusqu_a as f64, 0.0]
            }
        }),
        // Les noms d'artistes apparaissent **avec** eux, et non deux zooms
        // plus tard : c'est ce qui laissait les échelles moyennes vides.
        // Étagés par rang comme `artistes-point` ci-dessus, un cran plus
        // tard (le point d'abord, le nom ensuite).
        json!({
            "id": "artistes-etiquette",
            "type": "symbol",
            "source": "carte",
            "source-layer": "artistes",
            "minzoom": art_b2,
            "layout": {
                "text-field": ["get", "nom"],
                "text-font": ["Noto Sans Regular"],
                "text-size": ["interpolate", ["linear"], ["zoom"], 8, 9.5, 12, 11.5],
                "text-anchor": "top",
                "text-offset": [0, 0.7],
                "text-max-width": 9,
                "text-padding": 3,
                // MapLibre évite les collisions tout seul ; il lui faut
                // seulement savoir qui compte le plus.
                "symbol-sort-key": ["-", 0, ["get", "effectif"]]
            },
            "paint": {
                "text-color": "#6B665C",
                "text-halo-color": pal.halo,
                "text-halo-width": 1.4,
                "text-opacity": ["interpolate", ["linear"], ["zoom"],
                    art_b2, ["match", ["get", "rang"], 3, 0.85, 0.0],
                    art_b3, ["match", ["get", "rang"], 3, 0.85, 2, 0.8, 0.0],
                    art_b4, ["match", ["get", "rang"], 3, 0.85, 2, 0.8, 1, 0.7, 0.0],
                    zmax, 0.7]
            }
        }),
        // **Les points remarquables.** Trois espèces, et rien de plus : une
        // carte couverte de symboles ne signale plus rien.
        json!({
            "id": "curiosites",
            "type": "circle",
            "source": "carte",
            "source-layer": "curiosites",
            "minzoom": 3.0,
            "paint": {
                "circle-radius": ["interpolate", ["linear"], ["zoom"], 3, 3.0, zmax, 6.0],
                "circle-color": ["match", ["get", "espece"],
                    0, "#B07A2E", 1, "#5E8C6A", "#8A5A9E"],
                "circle-stroke-color": "#FFFFFF",
                "circle-stroke-width": ["interpolate", ["linear"], ["zoom"], 3, 1.2, zmax, 2.2]
            }
        }),
        json!({
            "id": "curiosites-etiquette",
            "type": "symbol",
            "source": "carte",
            "source-layer": "curiosites",
            "minzoom": 5.0,
            "layout": {
                "text-field": ["case",
                    ["has", "annee"],
                    ["concat", ["get", "nom"], " · ", ["to-string", ["get", "annee"]]],
                    ["get", "nom"]],
                "text-font": ["Noto Sans Regular"],
                "text-size": ["interpolate", ["linear"], ["zoom"], 5, 10.0, zmax, 12.5],
                "text-anchor": "left",
                "text-offset": [0.7, 0],
                "text-max-width": 12,
                "text-padding": 4,
                // Les monuments d'abord : ils datent le territoire.
                "symbol-sort-key": ["get", "espece"]
            },
            "paint": {
                "text-color": ["match", ["get", "espece"],
                    0, "#8A5E22", 1, "#456B51", "#6B4478"],
                "text-halo-color": pal.halo,
                "text-halo-width": 1.8
            }
        }),
        json!({
            "id": "morceaux-etiquette",
            "type": "symbol",
            "source": "carte",
            "source-layer": "morceaux",
            // Un cran avant le zoom maximal plutôt qu'à lui seul — sur
            // Paris, un titre se lit dès qu'on approche la façade, pas
            // seulement au tout dernier cran (`docs/carto-etapes.md`).
            "minzoom": zmax - 1.0,
            "layout": {
                "text-field": ["get", "titre"],
                "text-font": ["Noto Sans Regular"],
                "text-size": 11,
                "text-anchor": "left",
                "text-offset": [0.6, 0],
                "text-max-width": 12,
                "text-padding": 2
            },
            "paint": {
                "text-color": "#6B665C",
                "text-halo-color": pal.halo,
                "text-halo-width": 1.2
            }
        }),
    ]);

    // Le point de morceau : chemin fictif seulement. Sur le plan de ville
    // réel, c'est le bâtiment habité qui porte le morceau (« batiments-
    // morceaux » plus bas) — un point de quelques pixels s'y perdait
    // (`carto-ville.md`). `morceaux-etiquette` ci-dessus reste commun aux
    // deux chemins : le titre s'ancre sur le même point, avec ou sans disque.
    if !reel {
        couches.push(json!({
            "id": "morceaux-point",
            "type": "circle",
            "source": "carte",
            "source-layer": "morceaux",
            "minzoom": p.morceaux_des as f64,
            "paint": {
                "circle-radius": ["interpolate", ["linear"], ["zoom"],
                    p.morceaux_des as f64, 1.1, zmax, 3.2, zmax + 4.0, 6.0],
                "circle-color": couleur_famille(source, pal),
                "circle-opacity": ["interpolate", ["linear"], ["zoom"],
                    p.morceaux_des as f64, 0.5, (p.morceaux_des + 2) as f64, 0.9]
            }
        }));
    }

    // --- Le plan de ville réel -----------------------------------------
    //
    // Vides sur le chemin fictif (aucune tuile `frontiere`/`batiments`/
    // `eaux`/`verts`/`routes-reelles`) : ces couches n'y affichent rien,
    // pas plus que `territoires`/`agglomerations` n'affichent rien sur le
    // plan réel.
    couches.extend(vec![
        // La frontière communale : le littoral du plan de ville réel.
        json!({
            "id": "frontiere-ligne",
            "type": "line",
            "source": "carte",
            "source-layer": "frontiere",
            "paint": {
                "line-color": pal.cote,
                "line-width": ["interpolate", ["linear"], ["zoom"], 0, 0.8, 5, 1.4, zmax, 2.0]
            }
        }),
        // L'eau réelle (la Seine) : visible de loin, comme un repère de
        // ville plutôt qu'un détail de rue.
        json!({
            "id": "eaux-reelles",
            "type": "fill",
            "source": "carte",
            "source-layer": "eaux",
            "paint": { "fill-color": pal.mer, "fill-antialias": true }
        }),
        // Les espaces verts — bois, parcs — n'ont de sens qu'à partir du
        // zoom où ils cessent d'être une tache imprécise.
        json!({
            "id": "verts-reels",
            "type": "fill",
            "source": "carte",
            "source-layer": "verts",
            "minzoom": 11.0,
            "paint": { "fill-color": pal.vert, "fill-antialias": true }
        }),
        // **Le bâtiment habité, coloré par la famille de son occupant.** Ce
        // qui portait un morceau — un point de quelques pixels
        // (`morceaux-point`, chemin fictif seulement depuis ce chantier) —
        // c'est maintenant le bâtiment entier qui l'habite : bien plus
        // visible, et ça se lit comme une vraie carte plutôt qu'un semis de
        // points. `palier` porte la famille de l'occupant (`tuiles::Anneau`),
        // `-1` pour un bâtiment vacant — ceux-là restent sous
        // `batiments-reels`, plus bas, en gris neutre. Révélé dès
        // `paliers.morceaux_des` (`tuiles::anneau_visible_a`) : le même seuil
        // auquel les morceaux se révélaient avant ce chantier.
        json!({
            "id": "batiments-morceaux",
            "type": "fill",
            "source": "carte",
            "source-layer": "batiments",
            "filter": [">=", ["get", "palier"], 0],
            "minzoom": p.morceaux_des as f64,
            "paint": {
                "fill-color": couleur_famille_champ(source, "palier", pal, pal.bati),
                // Un lavis d'abord — la densité du quartier se lit avant le
                // détail — puis un aplat franc une fois au zoom de l'îlot,
                // où chaque bâtiment se distingue individuellement.
                "fill-opacity": ["interpolate", ["linear"], ["zoom"],
                    p.morceaux_des as f64, 0.55, 15.0, 0.85],
                "fill-antialias": true
            }
        }),
        json!({
            "id": "batiments-morceaux-bord",
            "type": "line",
            "source": "carte",
            "source-layer": "batiments",
            "filter": [">=", ["get", "palier"], 0],
            "minzoom": 15.0,
            "paint": { "line-color": pal.bati_bord, "line-width": 0.6 }
        }),
        // Le bâti vacant ne se lit qu'au zoom de l'îlot — plus loin, une
        // tache par bâtiment ne serait que du bruit, et il n'y a rien à
        // signaler avant ce zoom (le bâti habité, ci-dessus, a déjà pris ce
        // rôle plus tôt).
        json!({
            "id": "batiments-reels",
            "type": "fill",
            "source": "carte",
            "source-layer": "batiments",
            "filter": ["<", ["get", "palier"], 0],
            "minzoom": 15.0,
            "paint": { "fill-color": pal.bati, "fill-antialias": true }
        }),
        json!({
            "id": "batiments-reels-bord",
            "type": "line",
            "source": "carte",
            "source-layer": "batiments",
            "filter": ["<", ["get", "palier"], 0],
            "minzoom": 15.0,
            "paint": { "line-color": pal.bati_bord, "line-width": 0.6 }
        }),
        // Les rues réelles. `classe` est ici une chaîne — le nom OSM
        // (`rusty_music_osm::Classe::nom`), pas l'entier 0-3 du réseau
        // sonique : les deux hiérarchies ne se confondent pas dans le style.
        json!({
            "id": "routes-reelles-lisere",
            "type": "line",
            "source": "carte",
            "source-layer": "routes-reelles",
            "minzoom": 10.0,
            "layout": { "line-cap": "round", "line-join": "round" },
            "paint": {
                "line-color": ["match", ["get", "classe"],
                    "autoroute", pal.autoroute_lisere, "primaire", pal.nationale_lisere, "#C8C2B6"],
                "line-width": ["interpolate", ["linear"], ["zoom"],
                    10, ["match", ["get", "classe"], "autoroute", 3.4, "primaire", 2.6, 1.6],
                    14, ["match", ["get", "classe"],
                        "autoroute", 7.0, "primaire", 4.6, "secondaire", 3.0, "tertiaire", 3.0, 2.0],
                    17, ["match", ["get", "classe"],
                        "autoroute", 13.0, "primaire", 8.5, "secondaire", 5.5, "tertiaire", 5.5, 3.0]]
            }
        }),
        json!({
            "id": "routes-reelles",
            "type": "line",
            "source": "carte",
            "source-layer": "routes-reelles",
            "minzoom": 10.0,
            "layout": { "line-cap": "round", "line-join": "round" },
            "paint": {
                "line-color": ["match", ["get", "classe"],
                    "autoroute", pal.autoroute, "primaire", pal.nationale,
                    "secondaire", pal.secondaire, "tertiaire", pal.secondaire, pal.sentier],
                "line-width": ["interpolate", ["linear"], ["zoom"],
                    10, ["match", ["get", "classe"], "autoroute", 1.8, "primaire", 1.2, 0.6],
                    14, ["match", ["get", "classe"],
                        "autoroute", 4.6, "primaire", 3.0, "secondaire", 2.2, "tertiaire", 2.2, 1.2],
                    17, ["match", ["get", "classe"],
                        "autoroute", 9.0, "primaire", 6.0, "secondaire", 4.0, "tertiaire", 4.0, 2.0]]
            }
        }),
        json!({
            "id": "routes-reelles-etiquette",
            "type": "symbol",
            "source": "carte",
            "source-layer": "routes-reelles",
            "minzoom": 14.0,
            // L'impasse et la desserte encombreraient l'étiquetage sans
            // aider à s'orienter — la même retenue que les six rangs
            // d'établissement du monde fictif.
            "filter": ["!=", ["get", "classe"], "service"],
            "layout": {
                "symbol-placement": "line",
                "text-field": ["get", "nom"],
                "text-font": ["Noto Sans Regular"],
                "text-size": 11
            },
            "paint": {
                "text-color": pal.encre,
                "text-halo-color": pal.halo,
                "text-halo-width": 1.2
            }
        }),
        // Les repères réels — musées, monuments, lieux de culte. Même parti
        // que `curiosites` (peu de symboles, chacun mérite le détour), mais
        // depuis OSM plutôt que depuis la bibliothèque.
        json!({
            "id": "points-remarquables",
            "type": "circle",
            "source": "carte",
            "source-layer": "points-remarquables",
            "minzoom": 10.0,
            "paint": {
                "circle-radius": ["interpolate", ["linear"], ["zoom"], 10, 3.0, zmax, 6.0],
                "circle-color": ["match", ["get", "genre"],
                    "lieu_de_culte", "#8A5A9E", "musee", "#5E8C6A", "#B07A2E"],
                "circle-stroke-color": "#FFFFFF",
                "circle-stroke-width": ["interpolate", ["linear"], ["zoom"], 10, 1.2, zmax, 2.2]
            }
        }),
        json!({
            "id": "points-remarquables-etiquette",
            "type": "symbol",
            "source": "carte",
            "source-layer": "points-remarquables",
            "minzoom": 11.0,
            "layout": {
                "text-field": ["get", "nom"],
                "text-font": ["Noto Sans Regular"],
                "text-size": ["interpolate", ["linear"], ["zoom"], 11, 10.0, zmax, 12.5],
                "text-anchor": "left",
                "text-offset": [0.7, 0],
                "text-max-width": 12,
                "text-padding": 4
            },
            "paint": {
                "text-color": ["match", ["get", "genre"],
                    "lieu_de_culte", "#6B4478", "musee", "#456B51", "#8A5E22"],
                "text-halo-color": pal.halo,
                "text-halo-width": 1.8
            }
        }),
        // **Les albums** — l'échelon entre l'artiste (une rue) et le morceau
        // (un bâtiment). Vide sur le chemin fictif (`Source.albums`), ces
        // deux couches n'y affichent rien, comme `points-remarquables`
        // ci-dessus. Colorés par famille, comme les bâtiments habités : un
        // album est un regroupement de la même famille musicale que ses
        // morceaux, jamais une famille propre.
        json!({
            "id": "albums-point",
            "type": "circle",
            "source": "carte",
            "source-layer": "albums",
            "minzoom": p.albums_des as f64,
            "paint": {
                "circle-radius": ["interpolate", ["linear"], ["zoom"],
                    p.albums_des as f64, 2.2, p.morceaux_des as f64, 3.6],
                "circle-color": couleur,
                "circle-opacity": ["interpolate", ["linear"], ["zoom"],
                    p.albums_des as f64, 0.0, (p.albums_des as f64) + 0.5, 0.75, p.morceaux_des as f64, 0.6],
                "circle-stroke-color": "#FFFFFF",
                "circle-stroke-width": 1.0,
                // Le cerne doit s'effacer avec le disque — sinon un anneau
                // blanc reste visible seul avant que le disque n'apparaisse.
                "circle-stroke-opacity": ["interpolate", ["linear"], ["zoom"],
                    p.albums_des as f64, 0.0, (p.albums_des as f64) + 0.5, 0.75, p.morceaux_des as f64, 0.6]
            }
        }),
        json!({
            "id": "albums-etiquette",
            "type": "symbol",
            "source": "carte",
            "source-layer": "albums",
            "minzoom": (p.albums_des + 1) as f64,
            "layout": {
                "text-field": ["get", "nom"],
                "text-font": ["Noto Sans Regular"],
                "text-size": ["interpolate", ["linear"], ["zoom"],
                    p.albums_des as f64, 9.0, p.morceaux_des as f64, 11.0],
                "text-anchor": "left",
                "text-offset": [0.6, 0],
                "text-max-width": 10,
                "text-padding": 3,
                "symbol-sort-key": ["-", 0, ["get", "effectif"]]
            },
            "paint": {
                "text-color": "#5C574C",
                "text-halo-color": pal.halo,
                "text-halo-width": 1.4,
                "text-opacity": ["interpolate", ["linear"], ["zoom"],
                    (p.albums_des + 1) as f64, 0.0, (p.albums_des + 2) as f64, 0.85]
            }
        }),
    ]);

    couches
}

// ======================================================================
//  Le plan de ville réel — reconstruit couche par couche, façon maptoposter
// ======================================================================
//
// maptoposter ne peint que `bg` + eau + parcs + routes par hiérarchie (cinq
// rangs). On reprend ce fond nu, puis on empile ce que la bibliothèque ajoute.
// L'ordre, du bas vers le haut :
//
//   1. fond      — terre, Seine, bois/parcs
//   2. quartiers — le lavis de genre, en dézoom seulement
//   3. bâti      — îlots vacants puis habités (colorés par famille), le corps
//                  de la ville
//   4. voirie    — cinq rangs de routes nues, **par-dessus** le bâti
//   5. overlay   — artistes, albums, titres
//
// Retirés du plan de ville (par rapport au monde fictif) : liserés, noms de
// rues et trait de limite communale (fond nu, façon poster), `curiosites` et
// `points-remarquables` (pastilles brunes plus grosses qu'un bâtiment),
// `familles-etiquette`, et toutes les couches du monde fictif sans tuile ici.

fn couches_ville(source: &Source, p: &Paliers, pal: &Palette) -> Vec<Value> {
    let mut v = fond_reel(pal);
    v.extend(quartiers_reels(source, p, pal));
    v.extend(bati_reel(p, pal));
    v.extend(bati_morceaux_reel(source, p, pal));
    v.push(voirie_reelle(pal)); // la voirie passe **sur** le bâti
    v.extend(points_musicaux_reel(source, p, pal));
    v
}

/// 1. Le fond — `bg` + eau + parcs. Pas de trait de limite : maptoposter
///    recadre sans bordure, le réseau remplit le cadre et s'arrête où il
///    s'arrête (la frontière sert encore à découper les tuiles, pas à se voir).
fn fond_reel(pal: &Palette) -> Vec<Value> {
    vec![
        json!({ "id": "terre-reelle", "type": "background",
            "paint": { "background-color": pal.terre } }),
        json!({
            "id": "eaux-reelles", "type": "fill",
            "source": "carte", "source-layer": "eaux",
            "paint": { "fill-color": pal.mer, "fill-antialias": true }
        }),
        // Les parcs et bois apparaissent avec le fond, pas plus tard (z10, comme
        // la voirie) : sur maptoposter l'eau et les parcs sont toujours là.
        json!({
            "id": "verts-reels", "type": "fill",
            "source": "carte", "source-layer": "verts",
            "minzoom": 10.0,
            "paint": { "fill-color": pal.vert, "fill-antialias": true }
        }),
    ]
}

/// La voirie, **nue** : cinq rangs, ni liseré ni nom de rue. Au dézoom (z11-13)
/// des cheveux fins — c'est la texture de mille rues qui fait l'aspect poster ;
/// ratio de largeur ≈ 3:1 de l'autoroute à la résidentielle, comme
/// `get_edge_widths_by_type` de maptoposter. **Au-dessus du bâti** : sur un plan
/// (OSM, maptoposter) le réseau passe sur les îlots, pas dessous.
fn voirie_reelle(pal: &Palette) -> Value {
    json!({
        "id": "routes-reelles", "type": "line",
        "source": "carte", "source-layer": "routes-reelles",
        "minzoom": 10.0,
        "layout": { "line-cap": "round", "line-join": "round" },
        "paint": {
            "line-color": ["match", ["get", "classe"],
                "autoroute", pal.autoroute,
                "primaire", pal.nationale,
                "secondaire", pal.secondaire,
                "tertiaire", pal.tertiaire,
                pal.residentielle],
            "line-width": ["interpolate", ["linear"], ["zoom"],
                11, ["match", ["get", "classe"],
                    "autoroute", 0.9, "primaire", 0.7, "secondaire", 0.5, "tertiaire", 0.4, 0.35],
                14, ["match", ["get", "classe"],
                    "autoroute", 2.6, "primaire", 2.0, "secondaire", 1.4, "tertiaire", 1.0, 0.7],
                17, ["match", ["get", "classe"],
                    "autoroute", 7.0, "primaire", 5.5, "secondaire", 4.0, "tertiaire", 2.8, 2.0]]
        }
    })
}

/// 2. Les quartiers musicaux — **lavis** de genre au dézoom (≤ z15), la seule
///    information de famille quand Paris tient dans l'écran. Un lavis, pas un
///    aplat : le réseau doit rester lisible dessous (c'est lui l'aspect poster).
///    S'efface quand le bâti habité prend le relais (`morceaux_des`).
fn quartiers_reels(source: &Source, p: &Paliers, pal: &Palette) -> Vec<Value> {
    let bas = p.morceaux_des as f64 + 1.0;
    let jusqua = p.territoires_jusqu_a as f64;
    vec![
        json!({
            "id": "territoires-reels", "type": "fill",
            "source": "carte", "source-layer": "territoires-reels",
            "maxzoom": bas,
            "paint": {
                "fill-color": couleur_famille_champ(source, "palier", pal, pal.autres),
                "fill-opacity": ["interpolate", ["linear"], ["zoom"],
                    jusqua - 4.0, 0.2, jusqua - 1.0, 0.14, jusqua, 0.08, bas, 0.0],
                "fill-antialias": true
            }
        }),
        json!({
            "id": "territoires-reels-contour", "type": "line",
            "source": "carte", "source-layer": "territoires-reels",
            "maxzoom": bas,
            "paint": {
                "line-color": pal.niveau,
                "line-opacity": ["interpolate", ["linear"], ["zoom"],
                    jusqua - 4.0, 0.35, jusqua, 0.2, bas, 0.0],
                "line-width": ["interpolate", ["linear"], ["zoom"], 8, 0.4, 13, 0.9]
            }
        }),
    ]
}

/// 3. Le bâti vacant — les îlots, le **corps de la ville**. Apparaît avec le
///    bâti habité (`morceaux_des`, z14), d'abord en lavis (la densité du
///    quartier se lit avant le détail), puis en aplat franc au zoom de l'îlot.
///    Teinté sur le thème (`pal.bati`), entre la terre et la voirie.
fn bati_reel(p: &Paliers, pal: &Palette) -> Vec<Value> {
    let des = p.morceaux_des as f64;
    // Sur fond sombre, un lavis à 0,4 disparaît (sombre sur sombre) : le bâti y
    // démarre presque opaque, c'est sa couleur seule qui le retient de la nuit.
    let o0 = if pal.sombre { 0.75 } else { 0.4 };
    vec![
        json!({
            "id": "batiments-reels", "type": "fill",
            "source": "carte", "source-layer": "batiments",
            "filter": ["<", ["get", "palier"], 0],
            "minzoom": des,
            "paint": {
                "fill-color": pal.bati,
                "fill-opacity": ["interpolate", ["linear"], ["zoom"],
                    des, o0, des + 1.5, (o0 + 0.9) / 2.0, des + 2.5, 0.92],
                "fill-antialias": true
            }
        }),
        // Les bords ne comptent qu'une fois les bâtiments assez gros pour en
        // avoir : un cran après leur apparition.
        json!({
            "id": "batiments-reels-bord", "type": "line",
            "source": "carte", "source-layer": "batiments",
            "filter": ["<", ["get", "palier"], 0],
            "minzoom": des + 2.0,
            "paint": {
                "line-color": pal.bati_bord,
                "line-width": ["interpolate", ["linear"], ["zoom"], des + 2.0, 0.3, 17.0, 0.7]
            }
        }),
    ]
}

/// 4a. Le bâti habité, coloré par la famille de l'occupant. Même révélation que
///     le bâti vacant (lavis puis aplat), au-dessus de lui, sous la voirie.
fn bati_morceaux_reel(source: &Source, p: &Paliers, pal: &Palette) -> Vec<Value> {
    let des = p.morceaux_des as f64;
    let o0 = if pal.sombre { 0.8 } else { 0.5 };
    vec![
        json!({
            "id": "batiments-morceaux", "type": "fill",
            "source": "carte", "source-layer": "batiments",
            "filter": [">=", ["get", "palier"], 0],
            "minzoom": des,
            "paint": {
                "fill-color": couleur_famille_champ(source, "palier", pal, pal.bati),
                "fill-opacity": ["interpolate", ["linear"], ["zoom"],
                    des, o0, des + 1.5, (o0 + 0.92) / 2.0, des + 2.5, 0.92],
                "fill-antialias": true
            }
        }),
        json!({
            "id": "batiments-morceaux-bord", "type": "line",
            "source": "carte", "source-layer": "batiments",
            "filter": [">=", ["get", "palier"], 0],
            "minzoom": des + 2.0,
            "paint": {
                "line-color": pal.bati_bord,
                "line-width": ["interpolate", ["linear"], ["zoom"], des + 2.0, 0.3, 17.0, 0.7]
            }
        }),
    ]
}

/// 4b. Points et étiquettes musicaux — artistes, albums, titres. Inchangé
///     (retouché étape 3). Ni `curiosites` ni `points-remarquables` ici.
fn points_musicaux_reel(source: &Source, p: &Paliers, pal: &Palette) -> Vec<Value> {
    let zmax = p.zoom_max as f64;
    let bande = (p.morceaux_des as f64 - p.artistes_des as f64).max(3.0);
    let art_b2 = p.artistes_des as f64 + bande / 3.0;
    let art_b3 = p.artistes_des as f64 + bande * 2.0 / 3.0;
    let art_b4 = p.artistes_des as f64 + bande;
    let couleur = couleur_famille(source, pal);
    vec![
        json!({
            "id": "artistes-point", "type": "circle",
            "source": "carte", "source-layer": "artistes",
            "minzoom": art_b3,
            "paint": {
                "circle-radius": ["interpolate", ["linear"], ["zoom"],
                    art_b3, ["match", ["get", "rang"], 3, 2.4, 2.0],
                    art_b4, ["match", ["get", "rang"], 3, 5.0, 2, 4.0, 1, 3.2, 2.4],
                    zmax, ["match", ["get", "rang"], 3, 7.0, 2, 6.0, 1, 5.0, 4.0]],
                "circle-color": pal.autres,
                "circle-opacity": ["interpolate", ["linear"], ["zoom"], art_b3, 0.0, art_b4, 0.6],
                "circle-stroke-width": 0
            }
        }),
        json!({
            "id": "artistes-etiquette", "type": "symbol",
            "source": "carte", "source-layer": "artistes",
            "minzoom": art_b2,
            "layout": {
                "text-field": ["get", "nom"],
                "text-font": ["Noto Sans Regular"],
                "text-size": ["interpolate", ["linear"], ["zoom"], 8, 9.5, 12, 11.5],
                "text-anchor": "top",
                "text-offset": [0, 0.7],
                "text-max-width": 9,
                "text-padding": 3,
                "symbol-sort-key": ["-", 0, ["get", "effectif"]]
            },
            "paint": {
                "text-color": pal.encre_region,
                "text-halo-color": pal.halo,
                "text-halo-width": 1.4,
                "text-opacity": ["interpolate", ["linear"], ["zoom"],
                    art_b2, ["match", ["get", "rang"], 3, 0.85, 0.0],
                    art_b3, ["match", ["get", "rang"], 3, 0.85, 2, 0.8, 0.0],
                    art_b4, ["match", ["get", "rang"], 3, 0.85, 2, 0.8, 1, 0.7, 0.0],
                    zmax, 0.7]
            }
        }),
        json!({
            "id": "albums-point", "type": "circle",
            "source": "carte", "source-layer": "albums",
            "minzoom": p.albums_des as f64,
            "paint": {
                "circle-radius": ["interpolate", ["linear"], ["zoom"],
                    p.albums_des as f64, 2.2, p.morceaux_des as f64, 3.6],
                "circle-color": couleur,
                "circle-opacity": ["interpolate", ["linear"], ["zoom"],
                    p.albums_des as f64, 0.0, (p.albums_des as f64) + 0.5, 0.75, p.morceaux_des as f64, 0.6],
                "circle-stroke-color": pal.halo,
                "circle-stroke-width": 1.0,
                "circle-stroke-opacity": ["interpolate", ["linear"], ["zoom"],
                    p.albums_des as f64, 0.0, (p.albums_des as f64) + 0.5, 0.75, p.morceaux_des as f64, 0.6]
            }
        }),
        json!({
            "id": "albums-etiquette", "type": "symbol",
            "source": "carte", "source-layer": "albums",
            "minzoom": (p.albums_des + 1) as f64,
            "layout": {
                "text-field": ["get", "nom"],
                "text-font": ["Noto Sans Regular"],
                "text-size": ["interpolate", ["linear"], ["zoom"],
                    p.albums_des as f64, 9.0, p.morceaux_des as f64, 11.0],
                "text-anchor": "left",
                "text-offset": [0.6, 0],
                "text-max-width": 10,
                "text-padding": 3,
                "symbol-sort-key": ["-", 0, ["get", "effectif"]]
            },
            "paint": {
                "text-color": pal.encre_region,
                "text-halo-color": pal.halo,
                "text-halo-width": 1.4,
                "text-opacity": ["interpolate", ["linear"], ["zoom"],
                    (p.albums_des + 1) as f64, 0.0, (p.albums_des + 2) as f64, 0.85]
            }
        }),
        json!({
            "id": "morceaux-etiquette", "type": "symbol",
            "source": "carte", "source-layer": "morceaux",
            "minzoom": zmax - 1.0,
            "layout": {
                "text-field": ["get", "titre"],
                "text-font": ["Noto Sans Regular"],
                "text-size": 11,
                "text-anchor": "left",
                "text-offset": [0.6, 0],
                "text-max-width": 12,
                "text-padding": 2
            },
            "paint": {
                "text-color": pal.encre_region,
                "text-halo-color": pal.halo,
                "text-halo-width": 1.2
            }
        }),
        // Le nom **inventé** de la rue (« Rue <artiste> ») : c'est une donnée,
        // pas juste un repère d'orientation. Une fois dans le quartier (z14),
        // discret, sans l'impasse ni la desserte qui n'apprennent rien.
        json!({
            "id": "routes-reelles-etiquette", "type": "symbol",
            "source": "carte", "source-layer": "routes-reelles",
            "minzoom": 14.0,
            "filter": ["!=", ["get", "classe"], "service"],
            "layout": {
                "symbol-placement": "line",
                "text-field": ["get", "nom"],
                "text-font": ["Noto Sans Regular"],
                "text-size": ["interpolate", ["linear"], ["zoom"], 14, 9.5, 17, 11.5]
            },
            "paint": {
                "text-color": pal.encre_region,
                "text-halo-color": pal.halo,
                "text-halo-width": 1.2,
                "text-opacity": ["interpolate", ["linear"], ["zoom"], 14, 0.0, 15, 0.85]
            }
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Famille;

    fn source() -> Source {
        Source {
            familles: (0..3)
                .map(|id| Famille {
                    id,
                    nom: format!("famille {id}"),
                    effectif: 10,
                })
                .collect(),
            ..Default::default()
        }
    }

    /// Un plan de ville réel minimal — assez pour que `est_ville_reelle()`
    /// bascule le style.
    fn source_ville() -> Source {
        Source {
            frontiere: Some(vec![vec![[2.3, 48.8], [2.4, 48.8], [2.4, 48.9], [2.3, 48.9]]]),
            ..source()
        }
    }

    /// Sur le plan de ville réel, la source `relief` n'existe pas — la
    /// déclarer sans rien pour la servir échouerait en silence à chaque
    /// tuile demandée, contrairement à une couche visant une source-layer
    /// simplement absente des tuiles.
    #[test]
    fn le_relief_disparait_sur_le_plan_de_ville_reel() {
        let s = construire(&source_ville(), &Paliers::ville(), "x", &Palette::osm_clair());
        assert!(s["sources"].get("relief").is_none(), "source « relief » toujours déclarée");
        let ids: Vec<&str> = s["layers"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
        assert!(!ids.contains(&"relief"), "couche « relief » toujours présente");
        for attendu in [
            "terre-reelle", "eaux-reelles", "verts-reels", "routes-reelles",
            "territoires-reels", "batiments-reels", "batiments-morceaux", "albums-point",
        ] {
            assert!(ids.contains(&attendu), "couche « {attendu} » absente du plan de ville réel : {ids:?}");
        }
    }

    /// Le plan de ville est **nu** : ni liserés ni noms de rues dans le fond,
    /// aucune pastille brune (`curiosites` / `points-remarquables`), aucun grand
    /// nom de famille — et rien du monde fictif (mer/relief/agglomérations/…).
    #[test]
    fn le_plan_de_ville_est_nu() {
        let s = construire(&source_ville(), &Paliers::ville(), "x", &Palette::osm_clair());
        let ids: Vec<&str> = s["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap())
            .collect();
        // Fond nu : pas de liseré de route, pas de bordure de commune, pas de
        // pastille brune, rien du monde fictif. Les noms de rues (inventés) sont
        // une donnée de l'overlay, eux, et reviennent au zoom du quartier.
        for absent in [
            "routes-reelles-lisere", "frontiere-ligne",
            "curiosites", "curiosites-etiquette",
            "points-remarquables", "points-remarquables-etiquette",
            "familles-etiquette",
            "mer", "terre", "relief", "cote", "rivieres", "routes", "routes-lisere",
            "agglomerations", "agglomerations-bord", "etablissements", "territoires",
            "morceaux-point",
        ] {
            assert!(!ids.contains(&absent), "couche « {absent} » ne devrait pas être sur le plan de ville : {ids:?}");
        }
    }

    /// La règle `["zoom"]` en tête d'expression vaut aussi pour le plan de ville
    /// (le test `aucune_expression_de_zoom_nest_imbriquee` ne couvre que le
    /// monde fictif).
    #[test]
    fn le_plan_de_ville_respecte_les_expressions_de_zoom() {
        let s = construire(&source_ville(), &Paliers::ville(), "x", &Palette::encre());
        verifier_zoom_en_tete(&s);
    }

    /// L'ordre d'empilement du plan de ville : fond < quartiers < bâti < voirie
    /// < points. Sur un plan (OSM, maptoposter) la route passe **sur** l'îlot.
    #[test]
    fn le_plan_de_ville_empile_la_voirie_sur_le_bati() {
        let s = construire(&source_ville(), &Paliers::ville(), "x", &Palette::osm_clair());
        let ids: Vec<&str> = s["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap())
            .collect();
        let rang = |id: &str| ids.iter().position(|x| *x == id).unwrap_or_else(|| panic!("{id} absent : {ids:?}"));
        assert!(rang("eaux-reelles") < rang("territoires-reels"));
        assert!(rang("territoires-reels") < rang("batiments-reels"));
        assert!(rang("batiments-reels") < rang("batiments-morceaux"));
        assert!(rang("batiments-morceaux") < rang("routes-reelles"), "la voirie doit passer sur le bâti");
        assert!(rang("routes-reelles") < rang("artistes-point"));
    }

    /// Sans `center`/`zoom` explicites, `app.js` retombe sur le centre du
    /// monde fictif (`[0, 0]`) — sur un plan de ville réel, la caméra doit
    /// s'ouvrir sur la ville, pas au large du golfe de Guinée.
    #[test]
    fn le_style_reel_porte_son_centre_sur_la_ville() {
        let s = construire(&source_ville(), &Paliers::ville(), "x", &Palette::osm_clair());
        let centre = s["center"].as_array().expect("centre absent du style réel");
        // `source_ville()` a pour frontière le carré [2.3, 48.8]..[2.4, 48.9].
        assert!((centre[0].as_f64().unwrap() - 2.35).abs() < 1e-9);
        assert!((centre[1].as_f64().unwrap() - 48.85).abs() < 1e-9);
        assert!(s["zoom"].as_f64().unwrap() > 0.0);

        // Le monde fictif, lui, n'en porte pas : `app.js` doit pouvoir
        // continuer à retomber sur ses propres valeurs par défaut.
        let f = construire(&source(), &Paliers::default(), "x", &Palette::osm_clair());
        assert!(f.get("center").is_none());
        assert!(f.get("zoom").is_none());
    }

    /// Chaque couche vectorielle du style doit viser une couche qui existe
    /// vraiment dans les tuiles. Une faute de frappe ici n'affiche rien et ne
    /// signale rien : c'est le mode de défaillance le plus coûteux du format.
    #[test]
    fn les_couches_du_style_existent_dans_les_tuiles() {
        // Doit rester d'accord avec les couches qu'écrit `tuiles::encoder_tuile`.
        let connues = [
            "cotes", "agglomerations", "territoires", "routes", "rivieres",
            "familles", "etablissements", "curiosites", "artistes", "morceaux",
            // Le plan de ville réel.
            "frontiere", "batiments", "eaux", "verts", "routes-reelles", "points-remarquables", "albums",
            "territoires-reels",
        ];
        let verifier = |s: &Value| {
            for couche in s["layers"].as_array().unwrap() {
                if let Some(sl) = couche.get("source-layer").and_then(|v| v.as_str()) {
                    assert!(connues.contains(&sl), "couche inconnue dans les tuiles : {sl}");
                }
            }
        };
        verifier(&construire(&source(), &Paliers::default(), "x", &Palette::osm_clair()));
        verifier(&construire(&source_ville(), &Paliers::ville(), "x", &Palette::osm_clair()));
    }

    /// La révélation par échelle n'est correcte que si les bornes du style
    /// tombent dans celles des tuiles. Une couche visible là où sa tuile est
    /// vide donne un écran noir sans erreur.
    #[test]
    fn les_bornes_du_style_tiennent_dans_celles_des_tuiles() {
        let p = Paliers::default();
        let s = construire(&source(), &p, "tuiles://localhost", &Palette::osm_clair());
        let par_id: std::collections::HashMap<&str, &Value> = s["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (c["id"].as_str().unwrap(), c))
            .collect();

        let minzoom = |id: &str| par_id[id].get("minzoom").and_then(|v| v.as_f64());
        let maxzoom = |id: &str| par_id[id].get("maxzoom").and_then(|v| v.as_f64());

        assert!(minzoom("artistes-point").unwrap() >= p.artistes_des as f64);
        assert!(minzoom("morceaux-point").unwrap() >= p.morceaux_des as f64);
        assert!(maxzoom("familles-etiquette").unwrap() <= (p.familles_jusqu_a + 1) as f64);
        // Les territoires ont le droit d'être sur-zoomés au-delà du dernier
        // zoom produit — c'est le but — mais jamais au-delà du zoom maximal.
        assert!(maxzoom("territoires").unwrap() <= p.zoom_max as f64);
    }

    /// Chaque famille reçoit sa couleur, et le reste tombe sur « autres ».
    #[test]
    fn chaque_famille_a_sa_couleur() {
        let s = construire(&source(), &Paliers::default(), "x", &Palette::osm_clair());
        // Par identifiant, pas par indice : l'ordre des couches change dès
        // qu'on en ajoute une, et un test qui compte les rangs se casse pour
        // rien.
        let territoires = s["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "territoires")
            .expect("couche des territoires");
        let tab = territoires["paint"]["fill-color"].as_array().unwrap();
        assert_eq!(tab[0], "match");
        // 2 (match + get) + 3 × 2 (id, couleur) + 1 (défaut)
        assert_eq!(tab.len(), 2 + 3 * 2 + 1);
        assert_eq!(tab.last().unwrap(), Palette::osm_clair().autres);
    }

    /// La règle « `["zoom"]` en tête de l'expression la plus extérieure ».
    /// La violer fait rejeter **tout** le style : carte noire, `load` jamais
    /// déclenché, rien écrit nulle part. Partagée entre le monde fictif et le
    /// plan de ville.
    fn verifier_zoom_en_tete(s: &Value) {
        fn contient_zoom(v: &Value) -> bool {
            match v {
                Value::Array(t) => {
                    if t.len() == 1 && t[0] == "zoom" {
                        return true;
                    }
                    t.iter().any(contient_zoom)
                }
                _ => false,
            }
        }
        fn zoom_en_tete(v: &Value) -> bool {
            let Some(t) = v.as_array() else { return false };
            match t.first().and_then(|x| x.as_str()) {
                Some("interpolate") => t.get(2).is_some_and(|e| e == &json!(["zoom"])),
                Some("step") => t.get(1).is_some_and(|e| e == &json!(["zoom"])),
                _ => false,
            }
        }

        for couche in s["layers"].as_array().unwrap() {
            let id = couche["id"].as_str().unwrap();
            for bloc in ["paint", "layout"] {
                let Some(objet) = couche.get(bloc).and_then(|v| v.as_object()) else {
                    continue;
                };
                for (propriete, valeur) in objet {
                    if !contient_zoom(valeur) {
                        continue;
                    }
                    assert!(
                        zoom_en_tete(valeur),
                        "{id}.{bloc}.{propriete} : le zoom doit être l'entrée de \
                         l'expression la plus extérieure — {valeur}"
                    );
                    // Et une fois en tête, il ne doit pas réapparaître dans
                    // les sorties.
                    let t = valeur.as_array().unwrap();
                    let debut = if t[0] == "interpolate" { 3 } else { 2 };
                    for sortie in t.iter().skip(debut) {
                        assert!(
                            !contient_zoom(sortie),
                            "{id}.{bloc}.{propriete} : zoom réutilisé dans une sortie"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn aucune_expression_de_zoom_nest_imbriquee() {
        verifier_zoom_en_tete(&construire(
            &source(),
            &Paliers::default(),
            "x",
            &Palette::osm_clair(),
        ));
    }

    /// Une carte se lit parce que la terre s'arrête : la mer, le littoral et
    /// le réseau doivent être dans le style, et dans cet ordre — la mer
    /// dessous, les routes au-dessus des territoires.
    #[test]
    fn la_mer_le_littoral_et_les_routes_sont_la() {
        let s = construire(&source(), &Paliers::default(), "x", &Palette::osm_clair());
        let ids: Vec<&str> = s["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap())
            .collect();
        for attendu in [
            "mer", "terre", "cote", "routes", "routes-lisere",
            "etablissements", "etablissements-etiquette",
            "agglomerations", "agglomerations-bord",
            "rivieres", "curiosites", "curiosites-etiquette",
        ] {
            assert!(ids.contains(&attendu), "couche « {attendu} » absente : {ids:?}");
        }
        let rang = |id: &str| ids.iter().position(|x| *x == id).unwrap();
        assert!(rang("mer") < rang("terre"), "la mer doit être sous la terre");
        assert!(rang("terre") < rang("territoires"), "la terre sous les territoires");
        assert!(rang("routes-lisere") < rang("routes"), "le liseré sous la chaussée");
        assert!(rang("territoires") < rang("routes"), "les routes sur les territoires");
        assert!(
            rang("agglomerations") < rang("routes"),
            "les routes traversent les agglomérations, pas l'inverse"
        );
        assert!(rang("routes") < rang("artistes-etiquette"), "les noms au-dessus");
        assert!(rang("rivieres") < rang("routes"), "un pont passe au-dessus de l'eau");
        assert!(
            rang("curiosites") > rang("etablissements"),
            "les points remarquables se posent au-dessus des lieux"
        );
    }

    /// Un cercle a **deux** opacités, et la seconde s'oublie : `circle-opacity`
    /// ne gouverne que le disque, `circle-stroke-opacity` gouverne le cerne et
    /// vaut 1 par défaut. Une couche qui règle l'une sans l'autre laisse des
    /// ronds vides à tous les zooms.
    #[test]
    fn un_cercle_qui_sefface_efface_aussi_son_cerne() {
        let s = construire(&source(), &Paliers::default(), "x", &Palette::osm_clair());
        for couche in s["layers"].as_array().unwrap() {
            if couche["type"] != "circle" {
                continue;
            }
            let p = &couche["paint"];
            let id = couche["id"].as_str().unwrap();
            let cerne = p.get("circle-stroke-width").is_some_and(|w| w != &json!(0));
            if !cerne || p.get("circle-opacity").is_none() {
                continue;
            }
            assert_eq!(
                p.get("circle-opacity"),
                p.get("circle-stroke-opacity"),
                "{id} : le cerne ne suit pas l'opacité du disque"
            );
        }
    }

    #[test]
    fn les_tuiles_sont_lues_sous_la_base_donnee() {
        let s = construire(&source(), &Paliers::default(), "tuiles://localhost", &Palette::osm_clair());
        assert_eq!(
            s["sources"]["carte"]["tiles"][0],
            "tuiles://localhost/carte/{z}/{x}/{y}"
        );
        assert_eq!(
            s["sources"]["relief"]["tiles"][0],
            "tuiles://localhost/relief/{z}/{x}/{y}"
        );
    }

    /// Une palette ne change que les couleurs : mêmes couches, mêmes bornes de
    /// zoom, même structure — que ce soit le fond ou les familles qui bougent.
    #[test]
    fn une_palette_ne_change_que_les_couleurs() {
        let clair = construire(&source(), &Paliers::default(), "x", &Palette::osm_clair());
        let encre = construire(&source(), &Paliers::default(), "x", &Palette::encre());

        let ids = |s: &Value| -> Vec<String> {
            s["layers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["id"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(ids(&clair), ids(&encre), "la liste des couches a changé");

        // Le fond bouge.
        let fond = |s: &Value| s["layers"][0]["paint"]["background-color"].clone();
        assert_ne!(fond(&clair), fond(&encre), "le fond n'a pas changé de couleur");
        assert_eq!(fond(&encre), json!(Palette::encre().mer));

        // Les familles aussi (elles se calent sur le fond).
        let couleur_territoires = |s: &Value| {
            s["layers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == "territoires")
                .unwrap()["paint"]["fill-color"]
                .clone()
        };
        assert_ne!(couleur_territoires(&clair), couleur_territoires(&encre));
        // La première teinte de famille du style encre est bien celle de sa palette.
        let tab = couleur_territoires(&encre);
        assert_eq!(tab[2], json!(0));
        assert_eq!(tab[3], json!(Palette::encre().familles[0]));
    }
}
