//! Lecture simultanée de plusieurs stems, avec solo et coupure à la volée.
//!
//! Le [`Player`](crate::Player) du module 1 enchaîne une file : une piste après
//! l'autre. L'éditeur demande l'inverse — **toutes les pistes en même temps**,
//! rigoureusement alignées, avec un niveau réglable par piste pendant la
//! lecture.
//!
//! Deux façons de s'y prendre, et une seule tient :
//!
//! - une sortie audio par stem, quatre lectures lancées ensemble. Rien ne
//!   garantit qu'elles démarrent au même échantillon, et un décalage de
//!   quelques millisecondes entre la batterie et la basse s'entend
//!   immédiatement ;
//! - **une seule source qui somme les stems échantillon par échantillon.**
//!   L'alignement est exact par construction — il n'y a qu'un flux — et le
//!   niveau de chaque piste se lit à chaque échantillon dans un atomique, donc
//!   un solo s'applique sans interrompre la lecture.
//!
//! Les stems sont chargés en mémoire, en `i16` : ils sortent de notre écriture
//! WAV en 16 bits, la conversion ne perd donc rien, et c'est deux fois moins
//! lourd que du `f32`. Un morceau de quatre minutes en quatre stems tient dans
//! 186 Mo — le prix d'un solo instantané et d'un déplacement sans relecture
//! disque.
//!
//! **Une tête de lecture par stem.** Il n'y en avait qu'une tant qu'il n'y
//! avait qu'une vitesse : on sommait les stems, et la somme passait dans un
//! étireur unique. Le réglage par stem l'interdit — deux vitesses veulent deux
//! têtes, donc deux étireurs, et le mélange se fait après. C'est précisément ce
//! qui désynchronise les stems, et c'est l'effet demandé
//! (`docs/ui-spec-editeur.md`, décision 4). Une position de référence, à la
//! vitesse globale, reste là pour le transport : sans elle, la barre du bas
//! n'aurait plus de position à montrer.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use std::sync::Arc;
use std::time::Duration;

use rodio::source::UniformSourceIterator;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Source};

use crate::{Error, Result};

/// Fréquence de travail de l'éditeur — celle des stems produits.
pub const SR: u32 = 44_100;
const CANAUX: u16 = 2;

/// Niveau d'une piste, partagé entre l'interface et le fil audio.
///
/// Un `f32` dans un `AtomicU32` : le fil audio le relit à chaque échantillon,
/// l'interface l'écrit quand on clique. Aucun verrou sur le chemin du son —
/// un verrou tenu une milliseconde de trop s'entend.
/// `BAS` et `HAUT` sont les bornes, en millièmes : les paramètres génériques
/// n'acceptent pas de flottants.
#[derive(Clone)]
pub struct Reglage<const BAS: i32, const HAUT: i32>(Arc<std::sync::atomic::AtomicU32>);

impl<const BAS: i32, const HAUT: i32> Reglage<BAS, HAUT> {
    fn nouveau(v: f32) -> Self {
        let r = Self(Arc::new(std::sync::atomic::AtomicU32::new(0)));
        r.ecrire(v);
        r
    }
    pub fn lire(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    pub fn ecrire(&self, v: f32) {
        let borne = v.clamp(BAS as f32 / 1000.0, HAUT as f32 / 1000.0);
        self.0.store(borne.to_bits(), Ordering::Relaxed);
    }
}

/// Niveau d'une piste : de zéro à pleine échelle.
pub type Niveau = Reglage<0, 1000>;

/// Vitesse de lecture, en trames lues par trame rendue.
///
/// **Bornes distinctes de celles d'un niveau, et c'est tout l'intérêt de les
/// séparer** : la vitesse avait d'abord réutilisé le type des niveaux, borné à
/// 1,0. Toute accélération était silencieusement ramenée à 100 %, et seuls les
/// tests l'ont dit.
pub type Vitesse = Reglage<250, 4000>;

/// Résolution de la position, en bits de partie fractionnaire.
///
/// La position avance d'un pas fractionnaire — c'est ce qui permet de changer
/// la vitesse — et un flottant dériverait sur des millions d'échantillons. Un
/// entier en virgule fixe 32.32 tient la position à 2⁻³² de trame près sur des
/// heures.
const FRAC: u32 = 32;
const UNITE: f64 = (1u64 << FRAC) as f64;

/// Trames poussées d'un coup dans l'étireur. 4096 à 44,1 kHz, soit 93 ms :
/// assez pour qu'il travaille, assez peu pour qu'un changement de vitesse
/// s'entende tout de suite.
///
/// **128, et c'est mesuré — 4096 craquait.** `remplir()` est appelé depuis le
/// rappel audio : un appel sur quelques milliers fait tout le travail, et c'est
/// ce pic-là qui doit tenir dans le tampon du périphérique, pas la moyenne. La
/// moyenne allait très bien — WSOLA tient 3,2 fois le temps réel sur quatre
/// stems —, et c'est pour cela que le défaut a survécu à la première mesure.
///
/// Pire salve pour quatre stems, contre les 11,6 ms d'un tampon de 512 trames
/// (`examples/cout_bloc.rs`) :
///
/// | bloc | ×0,25 | ×0,5 | ×1,5 | ×4 |
/// |---|---|---|---|---|
/// | 4096 | 122,5 | 65,5 | 23,9 | 10,4 |
/// | 1024 | 33,9 | 20,6 | 9,6 | 5,3 |
/// | 256 | 10,0 | 5,2 | 5,1 | 5,1 |
/// | **128** | **6,1** | **5,8** | **5,1** | **5,1** |
///
/// **Le pire cas est la vitesse la plus lente, et c'est ce qui a failli être
/// manqué.** À ×0,25, un bloc d'entrée rend quatre fois sa durée, donc quatre
/// fois plus de pas d'étireur à calculer d'un coup. Mesuré au seul tempo 1,5,
/// un bloc de 512 paraissait tenir — il craque à 19,6 ms dès qu'on ralentit,
/// c'est-à-dire précisément quand on ralentit pour écouter un détail.
///
/// **Le plancher n'est pas le bloc, c'est le pas de l'étireur.** `wsola` rend
/// sa sortie par sauts de `hop_ms` — 15 ms, soit 661 trames — et n'en rend
/// jamais moins : d'où les 5,1 ms qui ne descendent plus, quel que soit le
/// bloc. 128 est le plus grand bloc qui reste à ce plancher sur toute la plage
/// de l'interface (25 % à 400 %).
///
/// Ce qui reste à savoir : 6,1 ms sur 11,6, c'est de la marge, pas du confort.
/// Un périphérique demandant des tampons de 256 trames redeviendrait juste. La
/// réponse de fond serait de sortir l'étirement du rappel audio — un fil
/// producteur et un tampon circulaire —, ce que cette mesure ne rend pas
/// nécessaire aujourd'hui.
const BLOC: usize = 128;

/// Un stem et sa tête de lecture : matière, niveau, vitesse, position.
///
/// **Le niveau s'applique en sortie, après l'étireur.** Il s'appliquait avant,
/// du temps où l'étireur recevait la somme des stems ; un solo mettait alors
/// jusqu'à 93 ms — un bloc — à s'entendre en vitesse variable, puisqu'il
/// fallait vider ce que l'étireur avait déjà produit. Un facteur constant ne
/// change pas l'endroit où WSOLA trouve sa meilleure ressemblance : appliquer
/// le niveau après revient au même, et s'entend tout de suite.
struct Voix {
    /// Matière du stem, entrelacée.
    piste: Vec<i16>,
    niveau: Niveau,
    /// Trames lues par trame rendue. 2,0 lit deux fois plus vite.
    vitesse: Vitesse,
    /// Position **en trames**, virgule fixe, partagée pour que le déplacement
    /// et la vitesse puissent l'écrire depuis l'extérieur.
    curseur: Arc<AtomicU64>,
    trames: usize,
    /// Position capturée au début de la trame : les deux canaux doivent lire
    /// le même instant, sinon la stéréo se décale d'un demi-échantillon.
    position: u64,

    /// **L'étireur ne sert que hors vitesse normale.** À 100 %, on lit la
    /// matière telle quelle : y faire passer le signal ajouterait le traitement
    /// d'un étirement qui n'a rien à étirer.
    ///
    /// C'est `wsola` qui l'assure — recouvrement-addition par similarité de
    /// forme d'onde, la méthode d'`atempo` chez ffmpeg. Méthode temporelle,
    /// donc **la hauteur ne bouge pas** et il n'y a pas d'artefact de phase.
    etireur: wsola::TimeStretch,
    /// Tampon d'entrée de l'étireur, **réalloué jamais**. Allouer dans le
    /// rappel audio est ce qu'on évite en priorité : la latence d'une
    /// allocation n'est pas bornée, et elle tombe précisément sur l'appel qui
    /// travaille déjà le plus.
    bloc: Vec<f32>,
    /// Ce que l'étireur a rendu et qui n'est pas encore joué, entrelacé.
    sortie: Vec<f32>,
    lus: usize,
    /// Vitesse du bloc précédent, pour repérer le passage d'une voie à l'autre.
    derniere: f32,
}

impl Voix {
    /// Pousse un bloc dans l'étireur et récupère ce qu'il rend.
    fn remplir(&mut self) -> bool {
        let depart = (self.curseur.load(Ordering::Relaxed) >> FRAC) as usize;
        if depart >= self.trames {
            return false;
        }
        let fin = (depart + BLOC).min(self.trames);
        self.bloc.clear();
        for t in depart..fin {
            for c in 0..CANAUX as usize {
                let v = self
                    .piste
                    .get(t * CANAUX as usize + c)
                    .copied()
                    .unwrap_or(0);
                self.bloc.push(v as f32 / i16::MAX as f32);
            }
        }
        self.etireur.push(&self.bloc);
        self.sortie = self.etireur.pull(usize::MAX);
        self.lus = 0;
        self.curseur
            .fetch_add(((fin - depart) as u64) << FRAC, Ordering::Relaxed);
        !self.sortie.is_empty() || fin < self.trames
    }

    /// Bascule entre la voie directe et l'étireur sans laisser traîner l'état
    /// de l'un dans l'autre.
    fn changer_de_voie(&mut self) {
        self.etireur.reset();
        self.sortie.clear();
        self.lus = 0;
    }

    /// L'échantillon suivant de ce stem, niveau appliqué. `None` quand la
    /// matière est épuisée — le stem se tait, les autres continuent.
    fn echantillon(&mut self, canal: usize) -> Option<f32> {
        let vitesse = self.vitesse.lire();
        let etire = (vitesse - 1.0).abs() > 1e-3;
        if etire != ((self.derniere - 1.0).abs() > 1e-3) {
            self.changer_de_voie();
        }
        self.derniere = vitesse;

        if etire {
            self.etireur.set_tempo(vitesse);
            while self.lus >= self.sortie.len() {
                if !self.remplir() {
                    return None;
                }
            }
            let v = self.sortie[self.lus];
            self.lus += 1;
            return Some(v * self.niveau.lire());
        }

        if canal == 0 {
            self.position = self.curseur.load(Ordering::Relaxed);
        }
        let trame = (self.position >> FRAC) as usize;
        if trame >= self.trames {
            return None;
        }
        // Interpolation linéaire entre deux trames : à vitesse non entière, on
        // lit entre deux échantillons.
        let frac = (self.position & ((1u64 << FRAC) - 1)) as f32 / UNITE as f32;
        let i = trame * CANAUX as usize + canal;
        let a = self.piste.get(i).copied().unwrap_or(0);
        let b = self.piste.get(i + CANAUX as usize).copied().unwrap_or(a);
        let v = a as f32 + (b as f32 - a as f32) * frac;

        if canal + 1 >= CANAUX as usize {
            // Les bornes sont celles du type : rien à revérifier ici.
            let pas = (vitesse as f64 * UNITE) as u64;
            self.curseur.fetch_add(pas, Ordering::Relaxed);
        }
        Some((v / i16::MAX as f32) * self.niveau.lire())
    }
}

/// La source qui somme les voix. Un seul flux, donc une seule sortie.
struct Melange {
    voix: Vec<Voix>,
    /// Position de référence : celle qu'aurait un stem à la vitesse globale.
    ///
    /// C'est elle que le transport montre et que le déplacement vise. Les
    /// stems s'en écartent dès que leurs vitesses diffèrent — la mesure de cet
    /// écart est ce que l'interface appelle la dérive.
    maitre: Arc<AtomicU64>,
    vitesse_maitre: Vitesse,
    /// Canal en cours dans la trame courante.
    canal: usize,
    trames: usize,
}

impl Iterator for Melange {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        let canal = self.canal;
        let mut somme = 0.0f32;
        let mut vivante = false;
        for v in &mut self.voix {
            if let Some(x) = v.echantillon(canal) {
                somme += x;
                vivante = true;
            }
        }

        self.canal += 1;
        if self.canal >= CANAUX as usize {
            self.canal = 0;
            let pas = (self.vitesse_maitre.lire() as f64 * UNITE) as u64;
            self.maitre.fetch_add(pas, Ordering::Relaxed);
        }

        // Tant qu'un seul stem a de la matière, la lecture continue : à
        // vitesses différentes, le plus rapide finit le premier et ce n'est pas
        // une raison pour couper les autres.
        if !vivante {
            return None;
        }
        // La somme des stems reconstitue le mélange d'origine, qui pouvait
        // déjà frôler la pleine échelle : sans écrêtage, un solo à plein
        // niveau saturerait.
        Some(somme.clamp(-1.0, 1.0))
    }
}

impl Source for Melange {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        CANAUX.try_into().expect("2 canaux")
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        SR.try_into().expect("44,1 kHz")
    }
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(self.trames as f64 / SR as f64))
    }
}

/// Un jeu de stems chargé, prêt à être écouté ensemble.
pub struct Multipiste {
    // `inner` avant `_output` : le lecteur doit être détruit avant la sortie
    // dont il dépend. Même contrainte d'ordre que dans `Player`.
    inner: rodio::Player,
    _output: MixerDeviceSink,
    noms: Vec<String>,
    niveaux: Vec<Niveau>,
    vitesses: Vec<Vitesse>,
    curseurs: Vec<Arc<AtomicU64>>,
    /// Longueur propre de chaque stem : ils ne finissent pas tous ensemble.
    fins: Vec<usize>,
    maitre: Arc<AtomicU64>,
    vitesse_maitre: Vitesse,
    trames: usize,
}

impl Multipiste {
    /// Charge des stems et prépare leur lecture simultanée.
    ///
    /// `stems` associe un nom au chemin de son WAV. L'ordre est conservé :
    /// c'est celui dans lequel l'interface affichera les pistes.
    pub fn charger(stems: &[(String, std::path::PathBuf)]) -> Result<Self> {
        if stems.is_empty() {
            return Err(Error::Vide);
        }

        let mut pistes = Vec::with_capacity(stems.len());
        let mut noms = Vec::with_capacity(stems.len());
        for (nom, chemin) in stems {
            pistes.push(lire_entrelace(chemin)?);
            noms.push(nom.clone());
        }

        // La plus longue commande : une piste plus courte se tait à la fin
        // plutôt que d'arrêter tout le monde.
        let fins: Vec<usize> = pistes.iter().map(|p| p.len() / CANAUX as usize).collect();
        let trames = fins.iter().copied().max().unwrap_or(0);
        let niveaux: Vec<Niveau> = stems.iter().map(|_| Niveau::nouveau(1.0)).collect();
        let vitesses: Vec<Vitesse> = stems.iter().map(|_| Vitesse::nouveau(1.0)).collect();
        let curseurs: Vec<Arc<AtomicU64>> =
            stems.iter().map(|_| Arc::new(AtomicU64::new(0))).collect();
        let maitre = Arc::new(AtomicU64::new(0));
        let vitesse_maitre = Vitesse::nouveau(1.0);

        let mut voix = Vec::with_capacity(stems.len());
        for (i, piste) in pistes.into_iter().enumerate() {
            voix.push(Voix {
                piste,
                niveau: niveaux[i].clone(),
                vitesse: vitesses[i].clone(),
                curseur: Arc::clone(&curseurs[i]),
                trames: fins[i],
                position: 0,
                etireur: wsola::TimeStretch::new(SR, CANAUX)
                    .map_err(|e| Error::Etirement(format!("{e}")))?,
                bloc: Vec::with_capacity(BLOC * CANAUX as usize),
                sortie: Vec::new(),
                lus: 0,
                derniere: 1.0,
            });
        }

        let mut sortie = DeviceSinkBuilder::open_default_sink()?;
        // Même raison que dans `Player` : le message de fermeture de `rodio`
        // n'apprend rien et pollue la sortie.
        sortie.log_on_drop(false);
        let lecteur = rodio::Player::connect_new(sortie.mixer());
        lecteur.append(Melange {
            voix,
            maitre: Arc::clone(&maitre),
            vitesse_maitre: vitesse_maitre.clone(),
            canal: 0,
            trames,
        });

        Ok(Self {
            inner: lecteur,
            _output: sortie,
            noms,
            niveaux,
            vitesses,
            curseurs,
            fins,
            maitre,
            vitesse_maitre,
            trames,
        })
    }

    /// Noms des pistes, dans l'ordre de chargement.
    pub fn noms(&self) -> &[String] {
        &self.noms
    }

    /// Règle le niveau d'une piste. Prend effet à l'échantillon suivant.
    pub fn regler(&self, piste: usize, niveau: f32) {
        if let Some(n) = self.niveaux.get(piste) {
            n.ecrire(niveau);
        }
    }

    /// Niveaux courants, dans l'ordre des pistes.
    pub fn niveaux(&self) -> Vec<f32> {
        self.niveaux.iter().map(Niveau::lire).collect()
    }

    pub fn pause(&self) {
        self.inner.pause();
    }

    pub fn reprendre(&self) {
        self.inner.play();
    }

    pub fn en_pause(&self) -> bool {
        self.inner.is_paused()
    }

    /// Position de lecture, celle de référence.
    ///
    /// Lue sur le curseur maître et non sur le lecteur : c'est lui qui fait
    /// foi, puisque c'est lui que le déplacement écrit. Quand les stems ont
    /// des vitesses différentes, aucun d'eux n'est *la* position — le maître
    /// est celle qu'aurait un stem resté à la vitesse globale.
    pub fn position(&self) -> Duration {
        let t = (self.maitre.load(Ordering::Relaxed) >> FRAC) as usize;
        Duration::from_secs_f64(t.min(self.trames) as f64 / SR as f64)
    }

    /// Durée **du morceau**, pas du temps qu'il reste à l'entendre.
    ///
    /// À vitesse double, la lecture dure moitié moins longtemps mais le
    /// morceau, lui, n'a pas raccourci : la tête se déplace deux fois plus vite
    /// sur la même étendue. C'est ce que montrent VLC et les autres.
    pub fn duree(&self) -> Duration {
        Duration::from_secs_f64(self.trames as f64 / SR as f64)
    }

    /// Vitesse globale. 2,0 lit deux fois plus vite.
    ///
    /// **Immédiate** : c'est un flottant que la lecture relit à chaque trame,
    /// rien n'est recalculé ni rechargé. La hauteur ne bouge pas — c'est
    /// `wsola` qui s'en charge dès qu'on quitte 100 %.
    ///
    /// Remet tous les stems à cette vitesse : régler la vitesse d'ensemble,
    /// c'est effacer les écarts, et l'interface les repose ensuite si elle en
    /// tient.
    pub fn vitesse(&self, v: f32) {
        self.vitesse_maitre.ecrire(v);
        for s in &self.vitesses {
            s.ecrire(v);
        }
    }

    /// Vitesse d'un seul stem. **C'est ce qui désynchronise** : sa tête de
    /// lecture n'avance plus au même pas que les autres, et l'écart grandit
    /// tant que la lecture continue.
    pub fn vitesse_stem(&self, piste: usize, v: f32) {
        if let Some(s) = self.vitesses.get(piste) {
            s.ecrire(v);
        }
    }

    /// Vitesse de chaque stem, dans l'ordre des pistes.
    pub fn vitesses(&self) -> Vec<f32> {
        self.vitesses.iter().map(Vitesse::lire).collect()
    }

    pub fn vitesse_courante(&self) -> f32 {
        self.vitesse_maitre.lire()
    }

    /// De combien le stem le plus éloigné s'est écarté de la référence.
    ///
    /// C'est la dérive, et elle se mesure plutôt qu'elle ne se devine : « les
    /// stems ont dérivé de 1,4 s » se vérifie à l'oreille, « attention, ils
    /// peuvent se désynchroniser » ne dit rien.
    pub fn derive(&self) -> Duration {
        let maitre = (self.maitre.load(Ordering::Relaxed) >> FRAC) as i64;
        let ecart = self
            .curseurs
            .iter()
            .map(|c| ((c.load(Ordering::Relaxed) >> FRAC) as i64 - maitre).abs())
            .max()
            .unwrap_or(0);
        Duration::from_secs_f64(ecart as f64 / SR as f64)
    }

    /// Se déplace dans le morceau, tous stems ensemble.
    ///
    /// Tout est en mémoire : déplacer, c'est écrire un entier. Aucun décodage,
    /// aucune relecture disque, et les stems repartent alignés.
    pub fn deplacer(&self, ou: Duration) {
        let t = (ou.as_secs_f64() * SR as f64) as usize;
        // La position est comptée en trames : la stéréo ne peut plus se
        // décaler d'un canal, ce qui inversait les côtés pour tout le reste
        // de la lecture.
        let brut = (t.min(self.trames) as u64) << FRAC;
        self.maitre.store(brut, Ordering::Relaxed);
        for c in &self.curseurs {
            c.store(brut, Ordering::Relaxed);
        }
    }

    /// Ramène tous les stems sur la position de référence, **sans toucher aux
    /// vitesses**.
    ///
    /// Deux gestes distincts, et il faut les deux : remettre les vitesses à
    /// égalité arrête la dérive mais laisse l'écart déjà pris ; réaligner
    /// efface l'écart mais le laisse se reformer. On garde souvent l'effet et
    /// on veut seulement recaler.
    pub fn realigner(&self) {
        let ou = self.maitre.load(Ordering::Relaxed);
        for c in &self.curseurs {
            c.store(ou, Ordering::Relaxed);
        }
    }

    /// Vrai quand plus aucun stem n'a de matière — pas quand le premier finit.
    pub fn fini(&self) -> bool {
        self.curseurs
            .iter()
            .zip(&self.fins)
            .all(|(c, fin)| (c.load(Ordering::Relaxed) >> FRAC) as usize >= *fin)
    }
}

/// Décode un WAV en `i16` entrelacé stéréo 44,1 kHz.
fn lire_entrelace(chemin: &Path) -> Result<Vec<i16>> {
    let fichier = std::fs::File::open(chemin).map_err(|source| Error::Open {
        path: chemin.to_path_buf(),
        source,
    })?;
    let decodeur = Decoder::try_from(fichier).map_err(|source| Error::Decode {
        path: chemin.to_path_buf(),
        source,
    })?;
    let uniforme = UniformSourceIterator::new(
        decodeur,
        CANAUX.try_into().expect("2 canaux"),
        SR.try_into().expect("44,1 kHz"),
    );
    Ok(uniforme
        .map(|v: f32| (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_niveau_borne_et_survit_au_partage() {
        let n = Niveau::nouveau(1.0);
        let copie = n.clone();
        n.ecrire(0.5);
        assert!((copie.lire() - 0.5).abs() < 1e-6, "partagé entre copies");
        n.ecrire(3.0);
        assert_eq!(copie.lire(), 1.0, "borné en haut");
        n.ecrire(-1.0);
        assert_eq!(copie.lire(), 0.0, "borné en bas");
    }

    /// Monte un mélange de test et rend de quoi l'observer : les curseurs de
    /// chaque voix, leurs vitesses, et le curseur maître.
    struct Banc {
        melange: Melange,
        curseurs: Vec<Arc<AtomicU64>>,
        vitesses: Vec<Vitesse>,
        maitre: Arc<AtomicU64>,
        vitesse_maitre: Vitesse,
    }

    fn banc(pistes: Vec<Vec<i16>>, niveaux: Vec<Niveau>) -> Banc {
        let fins: Vec<usize> = pistes.iter().map(|p| p.len() / CANAUX as usize).collect();
        let trames = fins.iter().copied().max().unwrap_or(0);
        let curseurs: Vec<Arc<AtomicU64>> =
            pistes.iter().map(|_| Arc::new(AtomicU64::new(0))).collect();
        let vitesses: Vec<Vitesse> = pistes.iter().map(|_| Vitesse::nouveau(1.0)).collect();
        let maitre = Arc::new(AtomicU64::new(0));
        let vitesse_maitre = Vitesse::nouveau(1.0);

        let voix = pistes
            .into_iter()
            .enumerate()
            .map(|(i, piste)| Voix {
                piste,
                niveau: niveaux[i].clone(),
                vitesse: vitesses[i].clone(),
                curseur: Arc::clone(&curseurs[i]),
                trames: fins[i],
                position: 0,
                etireur: wsola::TimeStretch::new(SR, CANAUX).expect("étireur de test"),
                bloc: Vec::with_capacity(BLOC * CANAUX as usize),
                sortie: Vec::new(),
                lus: 0,
                derniere: 1.0,
            })
            .collect();

        Banc {
            melange: Melange {
                voix,
                maitre: Arc::clone(&maitre),
                vitesse_maitre: vitesse_maitre.clone(),
                canal: 0,
                trames,
            },
            curseurs,
            vitesses,
            maitre,
            vitesse_maitre,
        }
    }

    /// Le mélange doit sommer les pistes audibles et taire les autres — c'est
    /// tout ce que solo et coupure demandent.
    #[test]
    fn le_melange_somme_ce_qui_est_audible() {
        let niveaux = vec![Niveau::nouveau(1.0), Niveau::nouveau(1.0)];
        let mut b = banc(vec![vec![1000i16; 8], vec![2000i16; 8]], niveaux.clone());

        let attendu = 3000.0 / i16::MAX as f32;
        assert!(
            (b.melange.next().unwrap() - attendu).abs() < 1e-4,
            "les deux pistes"
        );

        // On coupe la seconde : la somme retombe sur la première seule.
        niveaux[1].ecrire(0.0);
        let attendu = 1000.0 / i16::MAX as f32;
        assert!(
            (b.melange.next().unwrap() - attendu).abs() < 1e-4,
            "une seule piste"
        );

        // Épuisement au bout des trames disponibles — pour toutes les voix.
        for c in &b.curseurs {
            c.store(4 << FRAC, Ordering::Relaxed);
        }
        assert_eq!(b.melange.next(), None);
    }

    /// Une piste plus courte se tait ; elle n'arrête pas les autres. C'est la
    /// même règle qu'avant le réglage par stem, mais elle porte maintenant sur
    /// des têtes de lecture distinctes.
    #[test]
    fn un_stem_epuise_ne_coupe_pas_les_autres() {
        let mut b = banc(
            vec![vec![1000i16; 4], vec![2000i16; 16]],
            vec![Niveau::nouveau(1.0), Niveau::nouveau(1.0)],
        );
        // Deux trames : la première piste est épuisée, la seconde continue.
        let rendu: Vec<f32> = (0..8).filter_map(|_| b.melange.next()).collect();
        assert_eq!(rendu.len(), 8, "la piste longue doit continuer");
        let seule = 2000.0 / i16::MAX as f32;
        assert!(
            (rendu[6] - seule).abs() < 1e-4,
            "après épuisement de la courte, il reste la longue : {}",
            rendu[6]
        );
    }

    /// À vitesse normale, la voie directe consomme exactement une trame par
    /// trame rendue. C'est ce qui garantit qu'à 100 % rien n'est traité.
    #[test]
    fn a_vitesse_normale_on_lit_la_matiere_telle_quelle() {
        let piste: Vec<i16> = (0..64).map(|i| i as i16 * 100).collect();
        let mut b = banc(vec![piste.clone()], vec![Niveau::nouveau(1.0)]);

        // Huit trames de sortie, soit seize échantillons en stéréo.
        let rendu: Vec<f32> = (0..16).filter_map(|_| b.melange.next()).collect();
        assert_eq!(rendu.len(), 16);
        assert_eq!(
            (b.curseurs[0].load(Ordering::Relaxed) >> FRAC) as usize,
            8,
            "une trame lue par trame rendue"
        );
        assert_eq!(
            (b.maitre.load(Ordering::Relaxed) >> FRAC) as usize,
            8,
            "le maître suit, puisque la vitesse globale est la même"
        );
        // Et les valeurs sont celles de la matière, sans traitement.
        for (i, v) in rendu.iter().enumerate() {
            let attendu = piste[i] as f32 / i16::MAX as f32;
            assert!((v - attendu).abs() < 1e-4, "échantillon {i} altéré");
        }
    }

    /// Au-delà de 100 %, c'est `wsola` qui rend le son : il consomme la matière
    /// plus vite qu'il ne la restitue, et **c'est là que la hauteur est
    /// préservée**. On vérifie ici le contrat de la voie, pas le détail de
    /// l'algorithme — celui-ci est éprouvé dans son propre crate.
    /// **L'invariant qui empêche le craquement de revenir**, et il se vérifie
    /// par arithmétique plutôt que par chronomètre — un test de durée serait
    /// capricieux sur une machine chargée.
    ///
    /// `remplir()` pousse `BLOC` trames d'entrée dans l'étireur, qui en rend
    /// `BLOC / vitesse` et travaille par pas de `hop`. Un appel qui couvre
    /// plusieurs pas les calcule tous d'un coup, et c'est cette salve qui
    /// dépasse l'échéance du tampon audio. À la vitesse la plus lente — le
    /// pire cas, celui qui avait échappé à la première mesure — un bloc doit
    /// donc tenir dans un seul pas.
    #[test]
    fn un_bloc_ne_couvre_jamais_plus_dun_pas_detireur() {
        // `hop_ms` vaut 15 ms par défaut chez `wsola`, soit 661 trames à
        // 44,1 kHz. Recalculé plutôt qu'écrit en dur : le jour où la crate
        // change ses réglages, c'est ce test qui doit le dire.
        let hop = (SR as f32 * 0.015) as usize;
        // La borne basse de `Vitesse`, celle que l'interface expose à 25 %.
        let plus_lent = 250.0 / 1000.0;
        let rendu = (BLOC as f32 / plus_lent) as usize;
        assert!(
            rendu <= hop,
            "un bloc de {BLOC} rend {rendu} trames à ×{plus_lent}, \
             soit {:.1} pas d'étireur de {hop} — la salve dépassera le tampon",
            rendu as f32 / hop as f32
        );
    }

    #[test]
    fn au_dela_de_cent_pour_cent_letireur_prend_le_relais() {
        // Une seconde de matière : l'étireur a besoin de plus qu'une fenêtre.
        let n = SR as usize * CANAUX as usize;
        let piste: Vec<i16> = (0..n)
            .map(|i| {
                let t = i as f32 / SR as f32;
                ((std::f32::consts::TAU * 440.0 * t).sin() * 8000.0) as i16
            })
            .collect();
        let mut b = banc(vec![piste], vec![Niveau::nouveau(1.0)]);
        b.vitesses[0].ecrire(2.0);
        b.vitesse_maitre.ecrire(2.0);

        let rendu: Vec<f32> = (0..8192).filter_map(|_| b.melange.next()).collect();
        assert_eq!(rendu.len(), 8192, "l'étireur doit rendre du son");
        assert!(
            rendu.iter().any(|v| v.abs() > 0.01),
            "sortie silencieuse : l'étireur ne reçoit rien"
        );

        // La position musicale avance plus vite que la sortie : c'est ce que
        // « deux fois plus vite » veut dire.
        let trames = (b.curseurs[0].load(Ordering::Relaxed) >> FRAC) as usize;
        assert!(
            trames > 4096,
            "seulement {trames} trames consommées pour 4096 rendues"
        );
    }

    /// **Le cœur du réglage par stem** : deux vitesses, deux têtes de lecture,
    /// et un écart qui grandit. Sans cela le réglage n'aurait aucun effet, et
    /// l'avertissement de désynchronisation ne parlerait de rien.
    #[test]
    fn des_vitesses_differentes_desynchronisent() {
        let piste: Vec<i16> = vec![1000i16; 8192];
        let mut b = banc(
            vec![piste.clone(), piste],
            vec![Niveau::nouveau(1.0), Niveau::nouveau(1.0)],
        );
        // Le second stem lit une trame et demie par trame rendue — hors
        // vitesse normale, donc par l'étireur. C'est l'écart des têtes qu'on
        // mesure ici, pas le son.
        b.vitesses[1].ecrire(1.5);

        for _ in 0..2048 {
            b.melange.next();
        }
        let a = (b.curseurs[0].load(Ordering::Relaxed) >> FRAC) as i64;
        let c = (b.curseurs[1].load(Ordering::Relaxed) >> FRAC) as i64;
        assert!(
            c > a,
            "le stem accéléré doit avoir consommé plus de matière : {c} contre {a}"
        );

        // Le maître reste sur la vitesse globale, inchangée : c'est la
        // position que le transport montrera.
        let m = (b.maitre.load(Ordering::Relaxed) >> FRAC) as i64;
        assert_eq!(m, 1024, "1024 trames rendues à vitesse globale 1,0");
        assert_eq!(a, m, "le stem laissé à la vitesse globale suit le maître");
    }

    /// Réaligner ramène les têtes sur la référence sans toucher aux vitesses :
    /// on garde l'effet, on efface l'écart.
    #[test]
    fn realigner_ramene_les_stems_sur_la_reference() {
        let curseurs: Vec<Arc<AtomicU64>> = (0..3).map(|_| Arc::new(AtomicU64::new(0))).collect();
        let maitre = Arc::new(AtomicU64::new(500 << FRAC));
        curseurs[0].store(400 << FRAC, Ordering::Relaxed);
        curseurs[1].store(500 << FRAC, Ordering::Relaxed);
        curseurs[2].store(830 << FRAC, Ordering::Relaxed);

        // La dérive est le plus grand écart, en valeur absolue.
        let m = (maitre.load(Ordering::Relaxed) >> FRAC) as i64;
        let ecart = curseurs
            .iter()
            .map(|c| ((c.load(Ordering::Relaxed) >> FRAC) as i64 - m).abs())
            .max()
            .unwrap();
        assert_eq!(ecart, 330);

        // Réaligner : tout le monde sur le maître.
        for c in &curseurs {
            c.store(maitre.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        for c in &curseurs {
            assert_eq!(c.load(Ordering::Relaxed) >> FRAC, 500);
        }
    }

    /// Le déplacement doit tomber sur une frontière de trame : atterrir sur le
    /// canal droit inverserait la stéréo pour tout le reste de la lecture.
    #[test]
    fn le_deplacement_reste_sur_une_frontiere_de_trame() {
        for brut in [0usize, 1, 2, 3, 44_099, 44_100, 44_101] {
            let aligne = brut - brut % CANAUX as usize;
            assert_eq!(aligne % CANAUX as usize, 0, "désaligné pour {brut}");
            assert!(aligne <= brut);
        }
    }
}
