//! Générateur pseudo-aléatoire déterministe, partagé par le module 2.
//!
//! xorshift64* : une vingtaine de lignes, aucune dépendance. Le déterminisme
//! n'est pas une commodité mais une exigence — deux passes sur les mêmes
//! données doivent donner les mêmes familles, sinon la légende de la carte
//! change à chaque analyse, et la même graine doit redonner la même errance,
//! sinon on ne peut pas rejouer une promenade qu'on a aimée.

/// Générateur déterministe, une graine par suite.
pub struct Alea(u64);

impl Alea {
    /// Une graine nulle est remplacée : xorshift resterait bloqué à zéro.
    pub fn depuis(graine: u64) -> Self {
        Alea(if graine == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            graine
        })
    }

    /// Prochain entier 64 bits.
    pub fn entier(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Prochain réel dans [0, 1).
    pub fn reel(&mut self) -> f32 {
        (self.entier() >> 40) as f32 / (1 << 24) as f32
    }

    /// Prochain entier dans [0, `n`). Rend 0 si `n` est nul.
    pub fn borne(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.entier() % n as u64) as usize
        }
    }

    /// Tirage pondéré : l'indice `i` sort avec une probabilité proportionnelle
    /// à `poids[i]`. Un seul tirage continu (`reel`) plus un cumul — inutile
    /// de construire une méthode des alias pour des listes de la taille d'un
    /// voisinage (une douzaine d'éléments dans le graphe des voisins).
    pub fn categorique(&mut self, poids: &[f32]) -> usize {
        let total: f32 = poids.iter().sum();
        if poids.is_empty() || total <= 0.0 {
            return self.borne(poids.len());
        }
        let cible = self.reel() * total;
        let mut cumul = 0.0;
        for (i, &p) in poids.iter().enumerate() {
            cumul += p;
            if cible < cumul {
                return i;
            }
        }
        poids.len() - 1 // filet pour l'arrondi flottant en bout de cumul
    }

    /// Tirage gaussien centré réduit (Box-Muller). Sert au pont brownien du
    /// bruit sur les chemins « direct »/« dessiné » : la littérature du
    /// sujet parle d'écart-type, pas d'un intervalle uniforme.
    pub fn normale(&mut self) -> f32 {
        let u1 = self.reel().max(1e-9); // évite ln(0)
        let u2 = self.reel();
        (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deux_suites_de_meme_graine_sont_identiques() {
        let (mut a, mut b) = (Alea::depuis(7), Alea::depuis(7));
        for _ in 0..64 {
            assert_eq!(a.entier(), b.entier());
        }
        assert_ne!(Alea::depuis(7).entier(), Alea::depuis(8).entier());
    }

    #[test]
    fn la_graine_nulle_ne_bloque_pas() {
        let mut a = Alea::depuis(0);
        let premiers: Vec<u64> = (0..8).map(|_| a.entier()).collect();
        assert!(premiers.iter().any(|&x| x != 0), "suite bloquée à zéro");
    }

    #[test]
    fn les_tirages_restent_dans_leurs_bornes() {
        let mut a = Alea::depuis(1);
        for _ in 0..1000 {
            let r = a.reel();
            assert!((0.0..1.0).contains(&r), "réel hors bornes : {r}");
            assert!(a.borne(5) < 5);
        }
        assert_eq!(a.borne(0), 0, "borne nulle");
    }

    #[test]
    fn categorique_suit_les_poids_relatifs() {
        // Un poids dix fois plus grand doit sortir environ dix fois plus
        // souvent — tolérance large, ce n'est pas un test de précision
        // statistique mais une garde contre un tirage resté uniforme.
        let mut a = Alea::depuis(3);
        let poids = [1.0, 10.0, 1.0];
        let mut comptes = [0u32; 3];
        for _ in 0..6000 {
            comptes[a.categorique(&poids)] += 1;
        }
        assert!(
            comptes[1] > comptes[0] * 5 && comptes[1] > comptes[2] * 5,
            "l'indice le plus lourd ne domine pas assez : {comptes:?}"
        );

        // Poids nuls : repli sur le tirage uniforme plutôt qu'un blocage.
        assert!(Alea::depuis(1).categorique(&[0.0, 0.0]) < 2);
        assert_eq!(Alea::depuis(1).categorique(&[]), 0);
    }

    #[test]
    fn normale_est_centree_et_reduite() {
        // Pas un test de précision statistique — une garde contre un signe
        // inversé ou une échelle très fausse dans Box-Muller.
        let mut a = Alea::depuis(9);
        let tirages: Vec<f32> = (0..20_000).map(|_| a.normale()).collect();
        let moyenne = tirages.iter().sum::<f32>() / tirages.len() as f32;
        let variance =
            tirages.iter().map(|x| (x - moyenne).powi(2)).sum::<f32>() / tirages.len() as f32;
        assert!(moyenne.abs() < 0.05, "moyenne trop loin de 0 : {moyenne}");
        assert!((variance - 1.0).abs() < 0.1, "variance trop loin de 1 : {variance}");
    }
}
