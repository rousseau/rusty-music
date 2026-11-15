// SPDX-License-Identifier: GPL-3.0-or-later
//! E7 : [`Chaine::traiter`] tourne dans le rappel audio ; il ne doit rien
//! allouer. Un allocateur qui compte, propre à ce binaire de test.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use rusty_music_player::instrument::{Chaine, Prereglage, Reglage};

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
fn traiter_n_alloue_rien() {
    let mut tampon = vec![0.0f32; 512];
    for p in Prereglage::BASSE {
        let mut c = Chaine::new(&Reglage::de(p), 48_000.0);
        for (i, x) in tampon.iter_mut().enumerate() {
            *x = 0.5 * (i as f32 * 0.02).sin();
        }
        let avant = ALLOCATIONS.with(Cell::get);
        for _ in 0..100 {
            c.traiter(&mut tampon);
        }
        c.vider();
        let apres = ALLOCATIONS.with(Cell::get);
        assert_eq!(apres, avant, "{} alloue", p.nom());
    }
}
