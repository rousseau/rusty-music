// SPDX-License-Identifier: GPL-3.0-or-later
//! E7 : le rappel d'entrée ([`Entree::pousser`]) et celui de sortie
//! ([`Moteur::remplir`]) du retour de l'instrument n'allouent rien, chaîne
//! changée comprise. Un allocateur qui compte, propre à ce binaire de test.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use rusty_music_player::instrument::{Chaine, Prereglage, Reglage};
use rusty_music_player::retour::retour;

struct Compteur;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Compteur {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCATIONS.with(|n| n.set(n.get() + 1));
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Compteur = Compteur;

#[test]
fn les_rappels_du_retour_n_allouent_rien() {
    for (sr_e, sr_s) in [(48_000, 48_000), (44_100, 48_000), (48_000, 44_100)] {
        let chaine = Chaine::new(&Reglage::de(Prereglage::Funky), sr_s as f32);
        let (mut e, mut m, mut c) = retour(sr_e, sr_s, chaine).unwrap();
        c.partage.couper(false);
        c.partage.regler_volume(0.8);
        let autre = Chaine::new(&Reglage::de(Prereglage::Overdrive), sr_s as f32);
        let bloc: Vec<f32> = (0..64).map(|i| (i as f32 * 0.05).sin() * 0.7).collect();
        let mut sortie = vec![0.0f32; 2 * 128];
        let avant = ALLOCATIONS.with(Cell::get);
        for _ in 0..2_000 {
            e.pousser(&bloc);
            e.pousser(&bloc);
            m.remplir(&mut sortie, 2);
        }
        let apres = ALLOCATIONS.with(Cell::get);
        assert_eq!(apres, avant, "{sr_e} → {sr_s} alloue");
        // Le changement de chaîne : construite avant (hors du compte), le
        // rappel la prend sans allouer.
        assert!(c.changer_chaine(autre));
        let avant = ALLOCATIONS.with(Cell::get);
        for _ in 0..100 {
            e.pousser(&bloc);
            e.pousser(&bloc);
            m.remplir(&mut sortie, 2);
        }
        assert_eq!(
            ALLOCATIONS.with(Cell::get),
            avant,
            "changement de chaîne alloue"
        );
        assert!(sortie.iter().any(|x| *x != 0.0));
    }
}
