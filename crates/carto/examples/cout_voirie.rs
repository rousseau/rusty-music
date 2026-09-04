//! Carte de **coût de déplacement sur la voirie** depuis l'île de la Cité :
//! visualise ce que donnerait un peuplement piloté par la topographie (des
//! géodésiques de rues) au lieu du disque euclidien actuel.
//!
//! `cargo run --release -p rusty-music-carto --example cout_voirie -- [ville-paris.db] [lon,lat]`
//!
//! Puis : `python3 -m http.server -d /tmp/rusty-music-cout 8098`
//! et ouvrir `http://localhost:8098/`.

use std::path::Path;

use rusty_music_carto::affectation::Repere;
use rusty_music_carto::cout_voirie::{champ_de_cout, friction, isobandes};
use rusty_music_carto::ville::ILE_DE_LA_CITE;
use serde_json::json;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let ville = args.next().unwrap_or_else(|| "ville-paris.db".into());
    let source = match args.next() {
        Some(s) => {
            let (a, b) = s.split_once(',').expect("format attendu : lon,lat");
            [a.trim().parse()?, b.trim().parse()?]
        }
        None => ILE_DE_LA_CITE,
    };

    let t = std::time::Instant::now();
    let extrait = rusty_music_osm::base::lire(Path::new(&ville))?;
    let repere = Repere::centre_de(&extrait);
    println!(
        "lecture : {} tronçons, {} bâtiments, {:.2} s",
        extrait.troncons.len(),
        extrait.batis.len(),
        t.elapsed().as_secs_f64()
    );
    println!(
        "friction : autoroute {:.2}, primaire {:.2}, résidentielle {:.2}, piétonne {:.2}, service {:.2}",
        friction(rusty_music_osm::Classe::Autoroute),
        friction(rusty_music_osm::Classe::Primaire),
        friction(rusty_music_osm::Classe::Residentielle),
        friction(rusty_music_osm::Classe::Pietonne),
        friction(rusty_music_osm::Classe::Service),
    );

    // Comparaison zone « coût de voirie » vs « disque euclidien », pour ~27 000
    // bâtiments : c'est ce que `ville::preparer` choisit maintenant.
    {
        let n = 27_000usize.min(extrait.batis.len());
        let grille = rusty_music_carto::batiments::GrilleBatiments::nouvelle(&extrait, &repere);
        let c_m = repere.vers_m(source);
        let bat: Vec<(i64, [f64; 2])> = grille.tous().iter().map(|b| (b.id, b.centre)).collect();
        let mut par_cout =
            rusty_music_carto::cout_voirie::couts_batiments(&extrait, &repere, &bat, source);
        par_cout.retain(|(_, c)| c.is_finite());
        par_cout.sort_by(|a, b| a.1.total_cmp(&b.1));
        let cout_ids: std::collections::HashSet<i64> =
            par_cout.iter().take(n).map(|(id, _)| *id).collect();
        let eucl_ids: std::collections::HashSet<i64> = grille
            .n_plus_proches(c_m, n)
            .iter()
            .map(|b| b.id)
            .collect();
        let rayon = |ids: &std::collections::HashSet<i64>| {
            let mut d: Vec<f64> = grille
                .tous()
                .iter()
                .filter(|b| ids.contains(&b.id))
                .map(|b| ((b.centre[0] - c_m[0]).powi(2) + (b.centre[1] - c_m[1]).powi(2)).sqrt())
                .collect();
            d.sort_by(f64::total_cmp);
            (
                d.get(d.len() / 2).copied().unwrap_or(0.0),
                d.get((d.len() as f64 * 0.95) as usize).copied().unwrap_or(0.0),
                d.last().copied().unwrap_or(0.0),
            )
        };
        let (cm, c95, cmax) = rayon(&cout_ids);
        let (em, e95, emax) = rayon(&eucl_ids);
        let commun = cout_ids.intersection(&eucl_ids).count();
        println!(
            "\nzone peuplée ({n} bât.) — rayon euclidien depuis le centre (médiane / p95 / max) :\n  \
             par coût de voirie : {cm:.0} / {c95:.0} / {cmax:.0} m\n  \
             disque euclidien   : {em:.0} / {e95:.0} / {emax:.0} m\n  \
             {} % des bâtiments en commun (le reste = la forme qui a changé)\n",
            100 * commun / n
        );
    }

    let t = std::time::Instant::now();
    let champ = champ_de_cout(&extrait, &repere, source, 420);
    println!(
        "champ de coût : grille {0}×{0}, coût max {1:.0} m pondérés, {2:.2} s",
        champ.resolution,
        champ.cout_max,
        t.elapsed().as_secs_f64()
    );

    // Quatorze bandes, pas linéaire jusqu'au 95ᵉ centile fini (le reste de la
    // queue écraserait l'échelle).
    let mut finis: Vec<f64> = champ.valeurs.iter().copied().filter(|v| v.is_finite()).collect();
    finis.sort_by(f64::total_cmp);
    let haut = finis
        .get((finis.len() as f64 * 0.95) as usize)
        .copied()
        .unwrap_or(champ.cout_max)
        .max(1.0);
    let seuils: Vec<f64> = (1..14).map(|i| haut * i as f64 / 14.0).collect();
    println!(
        "seuils : {} … {} m pondérés (95ᵉ centile {haut:.0})",
        seuils.first().copied().unwrap_or(0.0) as i64,
        seuils.last().copied().unwrap_or(0.0) as i64,
    );

    let bandes = isobandes(&champ, &seuils);
    let seuil_max = seuils.last().copied().unwrap_or(1.0);
    let features: Vec<serde_json::Value> = bandes
        .iter()
        .map(|b| {
            let coords: Vec<serde_json::Value> = b
                .polygones
                .iter()
                .map(|poly| {
                    json!(poly
                        .iter()
                        .map(|anneau| anneau
                            .iter()
                            .map(|p| {
                                let ll = repere.depuis_m(*p);
                                json!([ll[0], ll[1]])
                            })
                            .collect::<Vec<_>>())
                        .collect::<Vec<_>>())
                })
                .collect();
            json!({
                "type": "Feature",
                "properties": { "seuil": b.seuil, "t": (b.seuil / seuil_max).min(1.0) },
                "geometry": { "type": "MultiPolygon", "coordinates": coords }
            })
        })
        .collect();

    // Le réseau, en fil de fer, pour lire à quelles voies le coût s'accroche.
    let voirie: Vec<serde_json::Value> = extrait
        .troncons
        .iter()
        .map(|tr| {
            json!({
                "type": "Feature",
                "properties": { "classe": format!("{:?}", tr.classe) },
                "geometry": {
                    "type": "LineString",
                    "coordinates": tr.points.iter().map(|p| json!([p[0], p[1]])).collect::<Vec<_>>()
                }
            })
        })
        .collect();

    let frontiere = extrait.frontiere.as_ref().map(|f| {
        json!({
            "type": "Feature", "properties": {},
            "geometry": {
                "type": "Polygon",
                "coordinates": f.anneaux.iter()
                    .map(|a| a.iter().map(|p| json!([p[0], p[1]])).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            }
        })
    });

    let sortie = std::env::temp_dir().join("rusty-music-cout");
    std::fs::create_dir_all(&sortie)?;
    std::fs::write(
        sortie.join("cout.geojson"),
        serde_json::to_vec(&json!({ "type": "FeatureCollection", "features": features }))?,
    )?;
    std::fs::write(
        sortie.join("voirie.geojson"),
        serde_json::to_vec(&json!({ "type": "FeatureCollection", "features": voirie }))?,
    )?;
    std::fs::write(
        sortie.join("frontiere.geojson"),
        serde_json::to_vec(&json!({
            "type": "FeatureCollection",
            "features": frontiere.into_iter().collect::<Vec<_>>()
        }))?,
    )?;
    std::fs::write(
        sortie.join("index.html"),
        include_str!("cout_voirie.html").replace("__SOURCE__", &format!("[{}, {}]", source[0], source[1])),
    )?;

    println!("\n  écrit dans {}", sortie.display());
    println!("  python3 -m http.server -d {} 8098", sortie.display());
    println!("  http://localhost:8098/\n");
    Ok(())
}
