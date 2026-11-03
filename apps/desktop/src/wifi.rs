// SPDX-License-Identifier: GPL-3.0-or-later
//! Le serveur Wi-Fi du compagnon iOS (étape 4B de `docs/plan-ios-compagnon.md`) :
//! le passage de la lecture dans les deux sens, sans câble (option B). Le
//! protocole et le client de l'iPhone vivent dans `rusty_music_core::appairage`.
//!
//! Il écoute tant que l'app est ouverte, dès qu'un iPhone a été appairé — au
//! câble, par [`appairer`], appelé après un envoi de valise ou un passage par
//! `devicectl`. « Oublier l'iPhone » ([`oublier_iphone`]) efface le jeton et
//! l'arrête. Chaque pas est confirmé par l'autre côté, comme dans le
//! `handoff.ts` de Disc Player (<https://github.com/eudj1n/snowsky-disc-player>,
//! licence MIT, © eudj1n) : le Mac ne se tait qu'à l'accusé de l'iPhone, et
//! l'iPhone qu'à celui du Mac.

use std::io::Read;

use rusty_music_core::appairage::{self, route, Accuse, Appairage, Bonjour, Recu};
use rusty_music_core::passage::Passage;

use super::*;

/// Un passage que l'iPhone n'est pas venu prendre ne vaut plus grand-chose
/// après une demi-heure : la lecture du Mac a continué.
const ATTENTE_MAX: Duration = Duration::from_secs(30 * 60);

/// Un passage tient en quelques kilo-octets ; au-delà, on ne lit pas.
const CORPS_MAX: u64 = 1 << 20;

#[derive(Default)]
pub(crate) struct EtatWifi {
    appairage: Option<Appairage>,
    /// Le serveur en marche : `unblock` l'arrête.
    serveur: Option<Arc<tiny_http::Server>>,
    erreur: Option<String>,
    /// Le passage Mac → iPhone que l'iPhone n'a pas encore pris.
    en_attente: Option<(Passage, Instant)>,
    /// Ce qui s'est passé en dernier, pour le bloc iPhone de Valise.
    dernier: Option<String>,
}

fn fichier(etat: &Etat) -> PathBuf {
    etat.db
        .parent()
        .map_or_else(std::env::temp_dir, Path::to_path_buf)
        .join(appairage::FICHIER)
}

/// Au lancement : relit l'appairage et écoute s'il y en a un.
pub(crate) fn demarrer(app: &tauri::AppHandle) {
    let etat = app.state::<Etat>();
    if let Ok(a) = Appairage::lire(&fichier(&etat)) {
        ecouter(app, &etat, a);
    }
}

fn ecouter(app: &tauri::AppHandle, etat: &Etat, a: Appairage) {
    let mut w = verrou(&etat.wifi);
    if let Some(ancien) = w.serveur.take() {
        ancien.unblock();
    }
    match tiny_http::Server::http(("0.0.0.0", a.port)) {
        Ok(serveur) => {
            let serveur = Arc::new(serveur);
            w.serveur = Some(serveur.clone());
            w.erreur = None;
            let app = app.clone();
            std::thread::spawn(move || {
                for requete in serveur.incoming_requests() {
                    servir(&app, requete);
                }
            });
            tracing::info!(port = a.port, "serveur Wi-Fi du compagnon à l'écoute");
        }
        Err(e) => {
            tracing::warn!(port = a.port, erreur = %e, "serveur Wi-Fi impossible");
            w.erreur = Some(format!("le port {} est pris ({e})", a.port));
        }
    }
    w.appairage = Some(a);
}

/// Appaire l'iPhone `udid` au câble : un jeton neuf la première fois, les
/// adresses du moment à chaque fois, déposés dans les Documents de l'app.
pub(crate) fn appairer(app: &tauri::AppHandle, etat: &Etat, udid: &str) -> Result<(), String> {
    let chemin = fichier(etat);
    let a = match Appairage::lire(&chemin) {
        Ok(a) => Appairage {
            mac: nom_du_mac(),
            adresses: adresses(),
            ..a
        },
        Err(_) => Appairage::nouveau(nom_du_mac(), appairage::PORT, adresses()).map_err(echec)?,
    };
    a.ecrire(&chemin).map_err(echec)?;
    iphone::copier_vers(udid, &chemin, appairage::FICHIER)?;
    let ecoute = verrou(&etat.wifi).serveur.is_some();
    if ecoute {
        verrou(&etat.wifi).appairage = Some(a);
    } else {
        ecouter(app, etat, a);
    }
    Ok(())
}

/// Tient `passage` à la disposition de l'iPhone ; `false` sans appairage.
pub(crate) fn mettre_en_attente(etat: &Etat, passage: Passage) -> bool {
    let mut w = verrou(&etat.wifi);
    if w.serveur.is_none() {
        return false;
    }
    w.en_attente = Some((passage, Instant::now()));
    true
}

/// Un passage parti par le câble rend caduc celui qui attendait.
pub(crate) fn retirer_attente(etat: &Etat) {
    verrou(&etat.wifi).en_attente = None;
}

/// « Mac de François » : le nom choisi dans les Réglages.
fn nom_du_mac() -> String {
    scutil("ComputerName").unwrap_or_else(|| "Mac".into())
}

fn scutil(cle: &str) -> Option<String> {
    let sortie = std::process::Command::new("scutil")
        .args(["--get", cle])
        .output()
        .ok()?;
    let nom = String::from_utf8_lossy(&sortie.stdout).trim().to_string();
    (sortie.status.success() && !nom.is_empty()).then_some(nom)
}

/// Où l'iPhone cherchera le Mac : son nom `.local`, puis son adresse sur la
/// route par défaut, puis son adresse Tailscale. Une socket UDP « connectée »
/// n'envoie rien : elle dit seulement quelle adresse locale servirait.
fn adresses() -> Vec<String> {
    let mut v: Vec<String> = scutil("LocalHostName")
        .map(|n| format!("{n}.local"))
        .into_iter()
        .collect();
    // 192.0.2.1 (TEST-NET-1) suit la route par défaut ; 100.100.100.100 est
    // l'adresse de service de Tailscale, joignable par son seul réseau.
    for (cible, tailscale) in [("192.0.2.1:9", false), ("100.100.100.100:53", true)] {
        let Some(ip) = std::net::UdpSocket::bind("0.0.0.0:0")
            .and_then(|s| s.connect(cible).and_then(|()| s.local_addr()))
            .ok()
            .map(|a| a.ip())
        else {
            continue;
        };
        let ip = ip.to_string();
        if ip != "0.0.0.0" && (!tailscale || ip.starts_with("100.")) && !v.contains(&ip) {
            v.push(ip);
        }
    }
    v
}

/// Une requête, réduite à ce que [`traiter`] lit.
pub(crate) struct Requete<'a> {
    pub(crate) methode: &'a str,
    pub(crate) chemin: &'a str,
    pub(crate) autorisation: Option<&'a str>,
    pub(crate) corps: &'a [u8],
}

/// Ce que le serveur demande au Mac ; à part pour tester [`traiter`] sans
/// lecteur audio.
pub(crate) trait Cote {
    fn bonjour(&self) -> Bonjour;
    fn en_attente(&self) -> Option<Passage>;
    /// L'iPhone a pris le passage émis à `emis_le`.
    fn recu(&self, emis_le: i64);
    /// L'iPhone rend la lecture.
    fn recevoir(&self, passage: Passage) -> Result<Accuse, String>;
}

/// Le protocole : (code HTTP, corps JSON).
pub(crate) fn traiter(a: &Appairage, r: &Requete, cote: &impl Cote) -> (u16, Option<Vec<u8>>) {
    if !a.autorise(r.autorisation) {
        return (401, None);
    }
    match (r.methode, r.chemin) {
        ("GET", route::BONJOUR) => json(&cote.bonjour()),
        ("GET", route::PASSAGE) => match cote.en_attente() {
            Some(p) => json(&p),
            None => (204, None),
        },
        ("POST", route::PASSAGE_RECU) => match serde_json::from_slice::<Recu>(r.corps) {
            Ok(recu) => {
                cote.recu(recu.emis_le);
                (204, None)
            }
            Err(e) => (400, Some(e.to_string().into_bytes())),
        },
        ("POST", route::PASSAGE) => match Passage::depuis_json(r.corps) {
            Ok(p) => match cote.recevoir(p) {
                Ok(accuse) => json(&accuse),
                Err(e) => (422, Some(e.into_bytes())),
            },
            Err(e) => (400, Some(e.to_string().into_bytes())),
        },
        _ => (404, None),
    }
}

fn json(v: &impl serde::Serialize) -> (u16, Option<Vec<u8>>) {
    (200, Some(serde_json::to_vec(v).unwrap_or_default()))
}

fn servir(app: &tauri::AppHandle, mut requete: tiny_http::Request) {
    let Some(etat) = app.try_state::<Etat>() else {
        return;
    };
    let Some(a) = verrou(&etat.wifi).appairage.clone() else {
        let _ = requete.respond(tiny_http::Response::empty(401));
        return;
    };
    let autorisation = requete
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .map(|h| h.value.as_str().to_string());
    let mut corps = Vec::new();
    let _ = requete.as_reader().take(CORPS_MAX).read_to_end(&mut corps);
    let methode = requete.method().as_str().to_string();
    let chemin = requete.url().split('?').next().unwrap_or("").to_string();
    let (code, reponse) = traiter(
        &a,
        &Requete {
            methode: &methode,
            chemin: &chemin,
            autorisation: autorisation.as_deref(),
            corps: &corps,
        },
        &CoteMac { app, etat: &etat },
    );
    if code == 401 {
        tracing::warn!(%chemin, "requête Wi-Fi sans le bon jeton, refusée");
    }
    let mut r = tiny_http::Response::from_data(reponse.unwrap_or_default()).with_status_code(code);
    if code == 200 {
        r.add_header(
            tiny_http::Header::from_bytes("Content-Type", "application/json")
                .expect("en-tête valide"),
        );
    }
    let _ = requete.respond(r);
}

struct CoteMac<'a> {
    app: &'a tauri::AppHandle,
    etat: &'a Etat,
}

impl Cote for CoteMac<'_> {
    fn bonjour(&self) -> Bonjour {
        Bonjour {
            mac: verrou(&self.etat.wifi)
                .appairage
                .as_ref()
                .map_or_else(nom_du_mac, |a| a.mac.clone()),
            version: appairage::VERSION,
        }
    }

    fn en_attente(&self) -> Option<Passage> {
        let mut w = verrou(&self.etat.wifi);
        if w.en_attente
            .as_ref()
            .is_some_and(|(_, depuis)| depuis.elapsed() > ATTENTE_MAX)
        {
            w.en_attente = None;
        }
        w.en_attente.as_ref().map(|(p, _)| p.clone())
    }

    fn recu(&self, emis_le: i64) {
        // Ordre des verrous : `wifi` avant `player`, ici seulement.
        let mut w = verrou(&self.etat.wifi);
        let Some((p, _)) = w.en_attente.take_if(|(p, _)| p.emis_le == emis_le) else {
            return;
        };
        // Comme par le câble : un geste fait sur le Mac entre-temps garde la
        // main, le Mac ne se tait que s'il joue encore ce qu'il a confié.
        let confie = p.file.get(p.rang).map(PathBuf::from);
        let player = verrou(&self.etat.player);
        let en_pause = confie.is_some() && player.current() == confie.as_deref();
        if en_pause {
            player.pause();
        }
        w.dernier = Some(
            if en_pause {
                "L'iPhone a pris la lecture ; le Mac s'est mis en pause."
            } else {
                "L'iPhone a pris la lecture ; elle avait changé ici, le Mac continue."
            }
            .into(),
        );
        tracing::info!(en_pause, "passage pris par l'iPhone");
    }

    fn recevoir(&self, passage: Passage) -> Result<Accuse, String> {
        // La file de l'iPhone est en chemins d'origine, ceux du Mac.
        let r = passage.reduire(|c| Path::new(c).is_file());
        if r.file.is_empty() {
            return Err("aucun morceau de cette file n'est sur le Mac".into());
        }
        {
            let mut player = verrou(&self.etat.player);
            // La lecture rendue remplace celle du Mac ; elle revient en pause,
            // au volume du Mac (celui de l'iPhone valait pour ses écouteurs).
            let volume = player.volume();
            player.stop();
            player
                .restaurer(rusty_music_player::Instantane {
                    file: r.file.iter().map(PathBuf::from).collect(),
                    avant_melange: r.avant_melange.iter().map(PathBuf::from).collect(),
                    rang: r.rang,
                    position: Duration::from_millis(r.position_ms),
                    alea: passage.alea,
                    repetition: rusty_music_player::Repetition::depuis_nom(&passage.repetition),
                    volume,
                })
                .map_err(echec)?;
        }
        {
            let mut w = verrou(&self.etat.wifi);
            w.en_attente = None;
            w.dernier = Some(match r.absents {
                0 => "Lecture reprise de l'iPhone, en pause.".into(),
                n => format!(
                    "Lecture reprise de l'iPhone, en pause ; {n} morceau{} de sa file introuvable{} ici.",
                    if n > 1 { "x" } else { "" },
                    if n > 1 { "s" } else { "" }
                ),
            });
        }
        // La page ne sonde le lecteur que quand il joue (`session-restauree`,
        // comme la reprise de session au lancement).
        let _ = self.app.emit("session-restauree", ());
        tracing::info!(
            pistes = r.file.len(),
            absents = r.absents,
            "lecture reprise de l'iPhone"
        );
        Ok(Accuse {
            pistes: r.file.len(),
            absents: r.absents,
            courante_presente: r.courante_presente,
        })
    }
}

/// Ce que le bloc iPhone de Valise montre du Wi-Fi.
#[derive(serde::Serialize)]
pub(crate) struct EtatWifiVue {
    appaire: bool,
    ecoute: bool,
    port: Option<u16>,
    adresses: Vec<String>,
    erreur: Option<String>,
    en_attente: bool,
    dernier: Option<String>,
}

#[tauri::command]
pub(crate) fn wifi_etat(etat: State<Etat>) -> EtatWifiVue {
    let w = verrou(&etat.wifi);
    EtatWifiVue {
        appaire: w.appairage.is_some(),
        ecoute: w.serveur.is_some(),
        port: w.appairage.as_ref().map(|a| a.port),
        adresses: w
            .appairage
            .as_ref()
            .map(|a| a.adresses.clone())
            .unwrap_or_default(),
        erreur: w.erreur.clone(),
        en_attente: w
            .en_attente
            .as_ref()
            .is_some_and(|(_, depuis)| depuis.elapsed() <= ATTENTE_MAX),
        dernier: w.dernier.clone(),
    }
}

/// Efface le jeton et arrête le serveur : l'iPhone ne sera plus servi tant
/// qu'il n'aura pas été rebranché.
#[tauri::command(async)]
pub(crate) fn oublier_iphone(etat: State<Etat>) -> Result<(), String> {
    match std::fs::remove_file(fichier(&etat)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(echec(e)),
    }
    let mut w = verrou(&etat.wifi);
    if let Some(serveur) = w.serveur.take() {
        serveur.unblock();
    }
    *w = EtatWifi::default();
    tracing::info!("iPhone oublié : serveur Wi-Fi arrêté");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Faux {
        attente: Option<Passage>,
        recus: RefCell<Vec<i64>>,
        rendus: RefCell<Vec<Passage>>,
    }

    impl Cote for Faux {
        fn bonjour(&self) -> Bonjour {
            Bonjour {
                mac: "Mac".into(),
                version: appairage::VERSION,
            }
        }
        fn en_attente(&self) -> Option<Passage> {
            self.attente.clone()
        }
        fn recu(&self, emis_le: i64) {
            self.recus.borrow_mut().push(emis_le);
        }
        fn recevoir(&self, p: Passage) -> Result<Accuse, String> {
            if p.file.is_empty() {
                return Err("vide".into());
            }
            let pistes = p.file.len();
            self.rendus.borrow_mut().push(p);
            Ok(Accuse {
                pistes,
                absents: 0,
                courante_presente: true,
            })
        }
    }

    fn passage(emis_le: i64) -> Passage {
        Passage {
            version: rusty_music_core::passage::VERSION,
            emis_le,
            file: vec!["/m/a.mp3".into()],
            avant_melange: Vec::new(),
            rang: 0,
            position_ms: 1000,
            alea: false,
            repetition: "aucune".into(),
            volume: 1.0,
        }
    }

    fn appairage() -> Appairage {
        Appairage::nouveau("Mac".into(), appairage::PORT, Vec::new()).unwrap()
    }

    fn requete<'a>(a: &'a str, m: &'a str, c: &'a str, corps: &'a [u8]) -> Requete<'a> {
        Requete {
            methode: m,
            chemin: c,
            autorisation: Some(a),
            corps,
        }
    }

    #[test]
    fn sans_le_jeton_rien_ne_passe() {
        let a = appairage();
        let f = Faux {
            attente: Some(passage(1)),
            ..Default::default()
        };
        let r = Requete {
            methode: "GET",
            chemin: route::PASSAGE,
            autorisation: Some("Bearer faux"),
            corps: b"",
        };
        assert_eq!(traiter(&a, &r, &f), (401, None));
        let sans = Requete {
            autorisation: None,
            ..r
        };
        assert_eq!(traiter(&a, &sans, &f).0, 401);
    }

    #[test]
    fn le_passage_en_attente_se_prend_puis_s_accuse() {
        let a = appairage();
        let jeton = a.autorisation();
        let vide = Faux::default();
        assert_eq!(
            traiter(&a, &requete(&jeton, "GET", route::PASSAGE, b""), &vide).0,
            204
        );

        let f = Faux {
            attente: Some(passage(42)),
            ..Default::default()
        };
        let (code, corps) = traiter(&a, &requete(&jeton, "GET", route::PASSAGE, b""), &f);
        assert_eq!(code, 200);
        assert_eq!(Passage::depuis_json(&corps.unwrap()).unwrap(), passage(42));

        let recu = serde_json::to_vec(&Recu { emis_le: 42 }).unwrap();
        assert_eq!(
            traiter(&a, &requete(&jeton, "POST", route::PASSAGE_RECU, &recu), &f).0,
            204
        );
        assert_eq!(*f.recus.borrow(), [42]);
    }

    #[test]
    fn la_lecture_rendue_par_l_iphone_est_accusee() {
        let a = appairage();
        let jeton = a.autorisation();
        let f = Faux::default();
        let corps = serde_json::to_vec(&passage(7)).unwrap();
        let (code, reponse) = traiter(&a, &requete(&jeton, "POST", route::PASSAGE, &corps), &f);
        assert_eq!(code, 200);
        let accuse: Accuse = serde_json::from_slice(&reponse.unwrap()).unwrap();
        assert_eq!(accuse.pistes, 1);
        assert_eq!(f.rendus.borrow().len(), 1);

        // Un refus du Mac revient en 422 avec sa raison ; un corps illisible en 400.
        let mut vide = passage(8);
        vide.file.clear();
        let corps = serde_json::to_vec(&vide).unwrap();
        let (code, raison) = traiter(&a, &requete(&jeton, "POST", route::PASSAGE, &corps), &f);
        assert_eq!((code, raison.as_deref()), (422, Some(&b"vide"[..])));
        assert_eq!(
            traiter(&a, &requete(&jeton, "POST", route::PASSAGE, b"{"), &f).0,
            400
        );
        assert_eq!(
            traiter(&a, &requete(&jeton, "GET", "/ailleurs", b""), &f).0,
            404
        );
    }

    /// Le client de l'iPhone (`rusty_music_core::appairage::Client`) contre un
    /// vrai `tiny_http` sur la boucle locale.
    #[test]
    fn le_client_de_l_iphone_parle_au_serveur() {
        let serveur = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
        let port = serveur.server_addr().to_ip().unwrap().port();
        let a = Appairage {
            port,
            adresses: vec!["127.0.0.2".into(), "127.0.0.1".into()],
            ..appairage()
        };
        let pour_le_fil = (serveur.clone(), a.clone());
        let fil = std::thread::spawn(move || {
            let (serveur, a) = pour_le_fil;
            let f = Faux {
                attente: Some(passage(5)),
                ..Default::default()
            };
            for mut req in serveur.incoming_requests() {
                let autorisation = req
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Authorization"))
                    .map(|h| h.value.as_str().to_string());
                let mut corps = Vec::new();
                req.as_reader().read_to_end(&mut corps).unwrap();
                let methode = req.method().as_str().to_string();
                let chemin = req.url().to_string();
                let (code, rep) = traiter(
                    &a,
                    &Requete {
                        methode: &methode,
                        chemin: &chemin,
                        autorisation: autorisation.as_deref(),
                        corps: &corps,
                    },
                    &f,
                );
                req.respond(
                    tiny_http::Response::from_data(rep.unwrap_or_default()).with_status_code(code),
                )
                .unwrap();
            }
            f.recus.into_inner()
        });

        let (client, bonjour) = appairage::Client::joindre(&a, None).unwrap();
        assert_eq!(bonjour.mac, "Mac");
        assert_eq!(client.adresse(), "127.0.0.1");
        let p = client.passage_en_attente().unwrap().unwrap();
        client.accuser(p.emis_le).unwrap();
        assert_eq!(client.rendre(&passage(6)).unwrap().pistes, 1);

        // Un autre jeton : le client le dit, sans chercher plus loin.
        let intrus = Appairage {
            jeton: "intrus".into(),
            ..a.clone()
        };
        assert!(matches!(
            appairage::Client::joindre(&intrus, Some("127.0.0.1")),
            Err(rusty_music_core::Error::AccesRefuse(_))
        ));

        serveur.unblock();
        assert_eq!(fil.join().unwrap(), [5]);
    }
}
