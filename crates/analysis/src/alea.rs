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
}
