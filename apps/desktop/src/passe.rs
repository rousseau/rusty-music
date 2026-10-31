// SPDX-License-Identifier: GPL-3.0-or-later
//! Les passes de fond (scan, analyse, enrichissements, démixage, « HD »,
//! valise…) : un même cycle de vie, un seul type.
//!
//! L'interface sonde `<passe>_state` et lit `en_cours`, l'avancement propre à
//! la passe et `resultat`. [`lancer`] tient le reste : refuser un second
//! lancement, marquer la passe en cours, travailler dans un fil sous
//! [`GardePasse`], écrire le bilan.

use super::*;

/// Une passe de fond telle que l'interface la sonde. L'avancement est aplati
/// dans le JSON : `{ en_cours, faits, total, resultat }` pour un [`Compte`].
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct Passe<A> {
    pub(crate) en_cours: bool,
    #[serde(flatten)]
    pub(crate) avancement: A,
    pub(crate) resultat: Option<String>,
}

/// L'avancement le plus courant : `faits` sur `total`.
#[derive(Clone, Copy, Default, serde::Serialize)]
pub(crate) struct Compte {
    pub(crate) faits: usize,
    pub(crate) total: usize,
}

/// Ce qu'un état de passe de fond expose à la garde anti-panique
/// [`GardePasse`] : le signal d'avancement (`en_cours`) que l'interface
/// sonde, et comment le clore proprement si la passe s'arrête autrement
/// qu'en l'écrivant elle-même.
pub(crate) trait EtatPasse {
    fn en_cours(&self) -> bool;
    /// Repositionne l'état comme si la passe avait échoué en interne —
    /// appelé uniquement quand `en_cours` est encore vrai à la destruction de
    /// la garde, c'est-à-dire seulement si le fil s'est arrêté (panique)
    /// avant d'avoir écrit son résultat par le chemin normal.
    fn marquer_echec_interne(&mut self);
}

impl<A> EtatPasse for Passe<A> {
    fn en_cours(&self) -> bool {
        self.en_cours
    }
    fn marquer_echec_interne(&mut self) {
        self.en_cours = false;
        self.resultat = Some("échec : panique interne".to_string());
    }
}

/// Posée en tête d'un fil de passe de fond (juste après avoir récupéré
/// `etat`) : si le fil se termine — panique comprise — sans avoir écrit son
/// résultat par le chemin normal, `en_cours` redescend quand même à `false`
/// avec un résultat par défaut. Sans elle, une panique dans le travail
/// (décodage, inférence…) laissait la passe « en cours » à vie côté
/// interface, `start_*` refusant tout relancement jusqu'au redémarrage.
///
/// Silencieuse sur le chemin normal : celui-ci écrit déjà `en_cours = false`
/// avant que le fil ne se termine, donc `en_cours()` rend faux à la
/// destruction et la garde ne fait rien.
pub(crate) struct GardePasse<'a, T: EtatPasse> {
    etat: &'a Mutex<T>,
}

impl<'a, T: EtatPasse> GardePasse<'a, T> {
    pub(crate) fn nouvelle(etat: &'a Mutex<T>) -> Self {
        Self { etat }
    }
}

impl<T: EtatPasse> Drop for GardePasse<'_, T> {
    fn drop(&mut self) {
        let mut g = verrou(self.etat);
        if g.en_cours() {
            g.marquer_echec_interne();
            tracing::warn!("passe de fond interrompue par une panique interne");
        }
    }
}

/// Lance une passe de fond.
///
/// Refuse avec `deja` si elle tourne encore ; sinon la marque en cours avec
/// l'avancement `depart`, et exécute `travail` dans un fil à part, sous
/// [`GardePasse`]. `travail` publie son avancement dans `passe(etat)` et rend
/// le bilan, écrit dans `resultat` au moment où `en_cours` retombe.
pub(crate) fn lancer<A, F>(
    app: tauri::AppHandle,
    passe: fn(&Etat) -> &Mutex<Passe<A>>,
    deja: &str,
    depart: A,
    travail: F,
) -> Result<(), String>
where
    A: Send + 'static,
    F: FnOnce(&Etat) -> String + Send + 'static,
{
    {
        let etat = app.state::<Etat>();
        let mut p = verrou(passe(&etat));
        if p.en_cours {
            return Err(deja.into());
        }
        *p = Passe {
            en_cours: true,
            avancement: depart,
            resultat: None,
        };
    }
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let _garde = GardePasse::nouvelle(passe(&etat));
        let bilan = travail(&etat);
        let mut p = verrou(passe(&etat));
        p.en_cours = false;
        p.resultat = Some(bilan);
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le JSON que l'interface sonde : l'avancement aplati entre `en_cours`
    /// et `resultat`, comme du temps des structs `EtatXxx` écrites à la main.
    #[test]
    fn le_json_garde_la_forme_lue_par_linterface() {
        let p = Passe {
            en_cours: true,
            avancement: Compte { faits: 2, total: 5 },
            resultat: None,
        };
        assert_eq!(
            serde_json::to_value(&p).unwrap(),
            serde_json::json!({ "en_cours": true, "faits": 2, "total": 5, "resultat": null })
        );
    }

    /// Une panique survenue pendant qu'une `GardePasse` est en vie doit
    /// laisser `en_cours` à `false` et un résultat posé — sans quoi la passe
    /// resterait « en cours » à vie côté interface. Le chemin normal (pas de
    /// panique) doit rester sans effet, la garde restant silencieuse.
    #[test]
    fn garde_passe_cloture_en_cours_apres_une_panique() {
        let etat = Mutex::new(Passe {
            en_cours: true,
            ..Passe::<Compte>::default()
        });
        let issue = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _garde = GardePasse::nouvelle(&etat);
            panic!("simulation d'un décodage qui tourne mal");
        }));
        assert!(issue.is_err(), "la panique doit bien traverser le test");
        let s = verrou(&etat);
        assert!(
            !s.en_cours,
            "la garde doit clore la passe malgré la panique"
        );
        assert_eq!(s.resultat.as_deref(), Some("échec : panique interne"));
    }

    /// Sur le chemin normal, la garde ne doit rien écraser : le résultat posé
    /// explicitement avant sa destruction doit survivre.
    #[test]
    fn garde_passe_silencieuse_sur_le_chemin_normal() {
        let etat = Mutex::new(Passe {
            en_cours: true,
            ..Passe::<Compte>::default()
        });
        {
            let _garde = GardePasse::nouvelle(&etat);
            let mut s = verrou(&etat);
            s.en_cours = false;
            s.resultat = Some("terminé normalement".to_string());
        }
        let s = verrou(&etat);
        assert!(!s.en_cours);
        assert_eq!(s.resultat.as_deref(), Some("terminé normalement"));
    }
}
